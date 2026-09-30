//! HTTP routes for the Kiro gateway.

use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use super::GatewayState;
use crate::kiro::account::KiroAccount;
use crate::kiro::event_stream::FrameDecoder;
use crate::kiro::map::{
    self, anthropic_message, anthropic_sse, openai_chat_json, openai_chat_sse, prepare,
    responses_json, responses_sse, CollectedReply, WireProtocol,
};
use crate::kiro::models::{catalog_ids, DEFAULT_MODEL};
use crate::kiro::pool::{classify_upstream, FailureClass};
use crate::kiro::token::{ensure_access_token, should_force_refresh};
use crate::kiro::upstream;

pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

pub async fn list_models() -> impl IntoResponse {
    let data: Vec<Value> = catalog_ids()
        .iter()
        .map(|id| json!({"id": id, "object": "model", "owned_by": "kiro"}))
        .collect();
    Json(json!({"object": "list", "data": data}))
}

pub async fn count_tokens(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let tokens = (body.len() / 4).max(1);
    (StatusCode::OK, Json(json!({"input_tokens": tokens}))).into_response()
}

pub async fn anthropic_messages(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    handle(state, headers, body, WireProtocol::Anthropic).await
}

pub async fn openai_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    handle(state, headers, body, WireProtocol::OpenAiChat).await
}

pub async fn openai_responses(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    handle(state, headers, body, WireProtocol::OpenAiResponses).await
}

async fn handle(
    state: GatewayState,
    headers: HeaderMap,
    body: Bytes,
    protocol: WireProtocol,
) -> axum::response::Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => return error_response(protocol, StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let started = Instant::now();
    let mut refreshed = false;
    let mut retry_same: Option<KiroAccount> = None;
    let mut attempts = 0;
    let max_attempts = 3;
    loop {
        attempts += 1;
        let Some(account) = retry_same.take().or_else(|| state.pool.select()) else {
            let response = error_response(protocol, StatusCode::SERVICE_UNAVAILABLE, "没有可用的 Kiro 账号");
            log_failure(&state, &headers, &incoming, protocol, started, 503, "no_account");
            return response;
        };
        let account = match ensure_access_token(&account, false).await {
            Ok(account) => account,
            Err(error) => {
                if attempts >= max_attempts {
                    log_failure(&state, &headers, &incoming, protocol, started, 401, "auth");
                    return error_response(protocol, StatusCode::UNAUTHORIZED, &error.to_string());
                }
                state.pool.note_failure(&account.id, FailureClass::Auth);
                continue;
            }
        };
        let prepared = match prepare(protocol, &incoming, &account) {
            Ok(prepared) => prepared,
            Err(error) => {
                return error_response(protocol, StatusCode::BAD_REQUEST, &error);
            }
        };
        let reply = upstream::generate(&account, &prepared.body).await;
        if let Some(message) = reply.network_error {
            log_failure(&state, &headers, &incoming, protocol, started, 504, "network");
            state.pool.note_failure(&account.id, FailureClass::Network);
            return error_response(protocol, StatusCode::GATEWAY_TIMEOUT, &message);
        }
        let text = String::from_utf8_lossy(&reply.body);
        let class = classify_upstream(reply.status, &text, false);
        if should_force_refresh(reply.status, &text, refreshed) {
            refreshed = true;
            match ensure_access_token(&account, true).await {
                Ok(updated) => {
                    retry_same = Some(updated);
                    continue;
                }
                Err(error) => {
                    log_failure(&state, &headers, &incoming, protocol, started, 401, "auth");
                    return error_response(protocol, StatusCode::UNAUTHORIZED, &error.to_string());
                }
            }
        }
        if reply.status == 401 || class == FailureClass::Auth {
            log_failure(&state, &headers, &incoming, protocol, started, 401, "auth");
            return error_response(
                protocol,
                StatusCode::UNAUTHORIZED,
                "Kiro 鉴权失败",
            );
        }
        if class == FailureClass::Client {
            log_failure(&state, &headers, &incoming, protocol, started, reply.status as i64, "client");
            return error_response(protocol, StatusCode::BAD_REQUEST, &text.chars().take(400).collect::<String>());
        }
        if matches!(class, FailureClass::Rotate | FailureClass::CoolAndRotate | FailureClass::Disable)
            && attempts < max_attempts
        {
            state.pool.note_failure(&account.id, class);
            continue;
        }
        if !(200..300).contains(&reply.status) {
            let status = if reply.status == 429 {
                StatusCode::TOO_MANY_REQUESTS
            } else if reply.status == 504 {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };
            state.pool.note_failure(&account.id, class);
            log_failure(&state, &headers, &incoming, protocol, started, status.as_u16() as i64, "upstream");
            let mut response = error_response(protocol, status, &text.chars().take(400).collect::<String>());
            if status == StatusCode::TOO_MANY_REQUESTS {
                response.headers_mut().insert(header::RETRY_AFTER, "5".parse().unwrap());
            }
            return response;
        }
        let mut decoder = FrameDecoder::new();
        let frames = decoder.push(&reply.body).unwrap_or_default();
        let mut collected = CollectedReply::default();
        if frames.is_empty() {
            collected.text = text.to_string();
        } else {
            for frame in &frames {
                collected.push_frame(frame);
            }
        }
        let client_model = client_model(&headers, &prepared.client_model);
        if map::should_log_usage("/v1/messages") {
            if let Some(id) = crate::kiro::usage_log::insert_request(
                &state.db,
                Some(&account.id),
                &client_model,
                Some(200),
                started,
                protocol,
                prepared.stream,
                None,
                Some(&headers),
            ) {
                crate::kiro::usage_log::write_usage(
                    &state.db,
                    &id,
                    collected.input_tokens,
                    collected.output_tokens,
                    collected.cache_read,
                    collected.cache_write,
                );
            }
        }
        return success_response(protocol, prepared.stream, &client_model, &collected);
    }
}

fn client_model(headers: &HeaderMap, prepared: &str) -> String {
    headers
        .get(crate::gateway::correlation::CLIENT_MODEL_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(prepared)
        .to_string()
}

fn authorize(state: &GatewayState, headers: &HeaderMap) -> Result<(), axum::response::Response> {
    let expected = state
        .api_key
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-api-key").and_then(|value| value.to_str().ok()))
        .unwrap_or("")
        .trim();
    if provided == expected {
        Ok(())
    } else {
        Err(error_response(
            WireProtocol::Anthropic,
            StatusCode::UNAUTHORIZED,
            "API Key 不正确",
        ))
    }
}

fn success_response(
    protocol: WireProtocol,
    stream: bool,
    client_model: &str,
    reply: &CollectedReply,
) -> axum::response::Response {
    let model = if client_model.is_empty() { DEFAULT_MODEL } else { client_model };
    match (protocol, stream) {
        (WireProtocol::Anthropic, true) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            anthropic_sse(model, reply),
        )
            .into_response(),
        (WireProtocol::Anthropic, false) => Json(anthropic_message(model, reply)).into_response(),
        (WireProtocol::OpenAiChat, true) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            openai_chat_sse(model, reply),
        )
            .into_response(),
        (WireProtocol::OpenAiChat, false) => Json(openai_chat_json(model, reply)).into_response(),
        (WireProtocol::OpenAiResponses, true) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            responses_sse(model, reply),
        )
            .into_response(),
        (WireProtocol::OpenAiResponses, false) => Json(responses_json(model, reply)).into_response(),
    }
}

fn error_response(protocol: WireProtocol, status: StatusCode, message: &str) -> axum::response::Response {
    let kind = match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::GATEWAY_TIMEOUT => "timeout_error",
        _ => "invalid_request_error",
    };
    let body = match protocol {
        WireProtocol::Anthropic => json!({"type": "error", "error": {"type": kind, "message": message}}),
        WireProtocol::OpenAiChat | WireProtocol::OpenAiResponses => {
            json!({"error": {"message": message, "type": kind}})
        }
    };
    (status, Json(body)).into_response()
}

fn log_failure(
    state: &GatewayState,
    headers: &HeaderMap,
    incoming: &Value,
    protocol: WireProtocol,
    started: Instant,
    status: i64,
    category: &str,
) {
    let model = incoming
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(DEFAULT_MODEL);
    let _ = crate::kiro::usage_log::insert_request(
        &state.db,
        None,
        model,
        Some(status),
        started,
        protocol,
        false,
        Some(category),
        Some(headers),
    );
}
