//! Gateway HTTP handlers.

use std::convert::Infallible;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::GatewayState;
use crate::antigravity::account::store as account_store;
use crate::antigravity::map::anthropic::{
    anthropic_sse_content_block_start, anthropic_sse_message_start, anthropic_to_gemini_request,
    gemini_to_anthropic_response, gemini_to_anthropic_sse_chunk,
};
use crate::antigravity::map::openai::{
    gemini_to_openai_response, gemini_to_openai_sse_chunk, openai_to_gemini_request,
};
use crate::antigravity::map::{list_public_models};
use crate::antigravity::upstream::{unwrap_v1internal, wrap_v1internal};

const MAX_FAILOVER_HOPS: usize = 3;

pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "ai-switcher-antigravity" }))
}

pub async fn list_models(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
        "object": "list",
        "data": list_public_models(),
    }))
    .into_response()
}

pub async fn anthropic_messages(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &format!("invalid json: {error}")),
    };
    let mapped = match anthropic_to_gemini_request(&payload) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &error),
    };
    dispatch_generation(&state, &headers, mapped.model, mapped.request, mapped.stream, Protocol::Anthropic)
        .await
}

pub async fn openai_chat_completions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &format!("invalid json: {error}")),
    };
    let mapped = match openai_to_gemini_request(&payload) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &error),
    };
    dispatch_generation(&state, &headers, mapped.model, mapped.request, mapped.stream, Protocol::OpenAi)
        .await
}

#[derive(Clone, Copy)]
enum Protocol {
    Anthropic,
    OpenAi,
}

async fn dispatch_generation(
    state: &GatewayState,
    headers: &HeaderMap,
    model: String,
    request: Value,
    stream: bool,
    protocol: Protocol,
) -> Response {
    let session_key = headers
        .get("x-session-id")
        .or_else(|| headers.get("x-claude-session-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut exclude: Vec<String> = Vec::new();
    let mut last_error = String::from("upstream failed");

    for hop in 0..MAX_FAILOVER_HOPS {
        let selected = if hop == 0 {
            state.pool.select(None, session_key.as_deref())
        } else {
            let failed = exclude
                .last()
                .cloned()
                .unwrap_or_else(|| String::new());
            state
                .pool
                .rotate_after_failure(&failed, 429, session_key.as_deref(), &exclude)
        };
        let (access_token, account) = match selected {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                break;
            }
        };
        exclude.push(account.id.clone());

        let project_id = match ensure_project_id(state, &access_token, &account.id, account.token.project_id.as_deref()).await {
            Ok(value) => value,
            Err(error) => {
                last_error = error;
                let _ = account_store().mark_cooldown(&account.id, 120, &last_error);
                continue;
            }
        };

        let wrapped = wrap_v1internal(&project_id, &model, request.clone());
        let upstream = match state
            .upstream
            .generate(&access_token, &wrapped, stream)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = error.to_string();
                let _ = account_store().mark_cooldown(&account.id, 60, &last_error);
                continue;
            }
        };

        let status = upstream.status();
        if matches!(status.as_u16(), 401 | 403 | 429) {
            last_error = format!("upstream {status}");
            let _ = state.pool.rotate_after_failure(
                &account.id,
                status.as_u16(),
                session_key.as_deref(),
                &exclude,
            );
            continue;
        }
        if !status.is_success() {
            let text = upstream.text().await.unwrap_or_default();
            last_error = format!("upstream {status}: {text}");
            let _ = account_store().mark_cooldown(&account.id, 30, &last_error);
            continue;
        }

        state.pool.note_success(&account.id);
        if stream {
            return stream_response(upstream, model.clone(), protocol).await;
        }
        return match upstream.json::<Value>().await {
            Ok(value) => {
                let gemini = unwrap_v1internal(&value);
                match protocol {
                    Protocol::Anthropic => {
                        Json(gemini_to_anthropic_response(&model, &gemini)).into_response()
                    }
                    Protocol::OpenAi => {
                        Json(gemini_to_openai_response(&model, &gemini)).into_response()
                    }
                }
            }
            Err(error) => error_json(
                StatusCode::BAD_GATEWAY,
                &format!("invalid upstream json: {error}"),
            ),
        };
    }

    error_json(StatusCode::BAD_GATEWAY, &last_error)
}

async fn ensure_project_id(
    state: &GatewayState,
    access_token: &str,
    account_id: &str,
    existing: Option<&str>,
) -> Result<String, String> {
    if let Some(project) = existing.filter(|value| !value.trim().is_empty()) {
        return Ok(project.to_string());
    }
    let project = state
        .upstream
        .fetch_project_id(access_token)
        .await
        .map_err(|error| error.to_string())?;
    let _ = account_store().update_project_id(account_id, &project);
    Ok(project)
}

async fn stream_response(
    upstream: reqwest::Response,
    model: String,
    protocol: Protocol,
) -> Response {
    let byte_stream = upstream.bytes_stream().boxed();
    let stream = futures_util::stream::unfold(
        StreamState {
            upstream: byte_stream,
            buffer: String::new(),
            model,
            protocol,
            started: false,
            finished: false,
        },
        |mut state| async move {
            if state.finished {
                return None;
            }
            loop {
                if let Some(event) = state.take_line_event() {
                    return Some((Ok::<Bytes, Infallible>(event), state));
                }
                match state.upstream.next().await {
                    Some(Ok(bytes)) => {
                        state.buffer.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(_)) | None => {
                        let trailing = state.finish_events();
                        state.finished = true;
                        if trailing.is_empty() {
                            return None;
                        }
                        return Some((Ok::<Bytes, Infallible>(trailing), state));
                    }
                }
            }
        },
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error_json(StatusCode::INTERNAL_SERVER_ERROR, "stream build failed"))
}

struct StreamState {
    upstream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
    buffer: String,
    model: String,
    protocol: Protocol,
    started: bool,
    finished: bool,
}

impl StreamState {
    fn take_line_event(&mut self) -> Option<Bytes> {
        let idx = self.buffer.find('\n')?;
        let line = self.buffer[..idx].trim_end_matches('\r').to_string();
        self.buffer.drain(..=idx);
        if line.is_empty() {
            return None;
        }
        let data = line
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(line.as_str());
        if data.is_empty() || data == "[DONE]" {
            return None;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return None;
        };
        let gemini = unwrap_v1internal(&value);
        match self.protocol {
            Protocol::Anthropic => {
                let mut out = Vec::new();
                if !self.started {
                    self.started = true;
                    out.extend_from_slice(&sse_data(&anthropic_sse_message_start(&self.model)));
                    out.extend_from_slice(&sse_data(&anthropic_sse_content_block_start()));
                }
                for event in gemini_to_anthropic_sse_chunk(&self.model, &gemini) {
                    out.extend_from_slice(&sse_data(&event));
                }
                if out.is_empty() {
                    None
                } else {
                    Some(Bytes::from(out))
                }
            }
            Protocol::OpenAi => Some(sse_data(&gemini_to_openai_sse_chunk(&self.model, &gemini))),
        }
    }

    fn finish_events(&mut self) -> Bytes {
        match self.protocol {
            Protocol::OpenAi => Bytes::from("data: [DONE]\n\n"),
            Protocol::Anthropic => {
                if self.started {
                    return Bytes::new();
                }
                self.started = true;
                let mut out = Vec::new();
                out.extend_from_slice(&sse_data(&anthropic_sse_message_start(&self.model)));
                out.extend_from_slice(&sse_data(&anthropic_sse_content_block_start()));
                out.extend_from_slice(&sse_data(&json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": "" }
                })));
                out.extend_from_slice(&sse_data(&json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                    "usage": { "output_tokens": 0 }
                })));
                out.extend_from_slice(&sse_data(&json!({ "type": "message_stop" })));
                Bytes::from(out)
            }
        }
    }
}

fn sse_data(value: &Value) -> Bytes {
    Bytes::from(format!("data: {}\n\n", value))
}

fn authorize(state: &GatewayState, headers: &HeaderMap) -> Result<(), Response> {
    let expected = state
        .api_key
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    if expected.is_empty() {
        return Ok(());
    }
    let provided = headers
        .get("x-api-key")
        .or_else(|| headers.get(header::AUTHORIZATION))
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .strip_prefix("Bearer ")
                .unwrap_or(value)
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    if provided != expected {
        return Err(error_json(StatusCode::UNAUTHORIZED, "invalid api key"));
    }
    Ok(())
}

fn error_json(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": "antigravity_gateway_error",
            }
        })),
    )
        .into_response()
}
