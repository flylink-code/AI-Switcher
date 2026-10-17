//! HTTP routes for the Kiro gateway.

use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use super::GatewayState;
use crate::kiro::account::KiroAccount;
use crate::kiro::event_stream::FrameDecoder;
use crate::kiro::map::{
    self, anthropic_message, anthropic_sse, openai_chat_json, openai_chat_sse, prepare,
    responses_json, responses_sse, CollectedReply, WireProtocol,
};
use crate::kiro::models::DEFAULT_MODEL;
use crate::kiro::pool::{classify_upstream, FailureClass};
use crate::kiro::token::{ensure_access_token, should_force_refresh};
use crate::kiro::upstream;

pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

pub async fn list_models() -> impl IntoResponse {
    let data: Vec<Value> = crate::kiro::models::model_list()
        .models
        .into_iter()
        .map(|model| {
            let context_window = if model.context > 0 {
                model.context
            } else {
                crate::gateway::metadata::context_window_for(&model.id)
            };
            let max_output = if model.output > 0 {
                model.output
            } else {
                crate::gateway::metadata::max_output_for(&model.id, context_window)
            };
            json!({
                "id": model.id,
                "object": "model",
                "owned_by": "kiro",
                "display_name": model.name,
                "context_window": context_window,
                "max_input_tokens": context_window,
                "max_output_tokens": max_output,
                "max_tokens": max_output,
            })
        })
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
            let response = error_response(
                protocol,
                StatusCode::SERVICE_UNAVAILABLE,
                "没有可用的 Kiro 账号",
            );
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                503,
                "no_account",
            );
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
        let http_resp = match upstream::send_generate_request(&account, &prepared.body).await {
            Ok(resp) => resp,
            Err(message) => {
                state.pool.note_failure(&account.id, FailureClass::Network);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state, &headers, &incoming, protocol, started, 504, "network",
                );
                return error_response(protocol, StatusCode::GATEWAY_TIMEOUT, &message);
            }
        };
        let status_code = http_resp.status().as_u16();
        let upstream_ok = (200..300).contains(&status_code);
        if upstream_ok && prepared.stream {
            use futures_util::StreamExt;
            let client_model = client_model(&headers, &prepared.client_model);
            let mut byte_stream = http_resp.bytes_stream();
            let mut decoder = FrameDecoder::new();
            let mut streamer = map::KiroSseStreamer::new(protocol, &client_model);
            let mut initial_sse = String::new();
            let mut raw_buf = Vec::new();
            let mut first_frame_seen = false;
            let mut decode_err: Option<String> = None;

            while !first_frame_seen {
                match byte_stream.next().await {
                    Some(Ok(chunk)) => {
                        if raw_buf.len() < 4096 {
                            raw_buf.extend_from_slice(&chunk);
                        }
                        match decoder.push(&chunk) {
                            Ok(frames) => {
                                if !frames.is_empty() {
                                    first_frame_seen = true;
                                    for frame in &frames {
                                        initial_sse.push_str(&streamer.push_frame(frame));
                                    }
                                }
                            }
                            Err(err) => {
                                decode_err = Some(format!("{err:?}"));
                                break;
                            }
                        }
                    }
                    Some(Err(err)) => {
                        decode_err = Some(err.to_string());
                        break;
                    }
                    None => break,
                }
            }

            if let Some(err) = decode_err {
                log::warn!("Kiro event-stream decode error: {err}");
                state.pool.note_failure(&account.id, FailureClass::Rotate);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state, &headers, &incoming, protocol, started, 502, "upstream",
                );
                return error_response(
                    protocol,
                    StatusCode::BAD_GATEWAY,
                    &format!("Kiro 上游事件流解析失败: {err}"),
                );
            }

            if !first_frame_seen && streamer.collected.upstream_error.is_none() {
                if let Ok(json_body) = serde_json::from_slice::<Value>(&raw_buf) {
                    if let Some(err_msg) = json_body
                        .get("message")
                        .or_else(|| json_body.pointer("/error/message"))
                        .and_then(Value::as_str)
                    {
                        streamer.collected.upstream_error = Some(err_msg.to_string());
                    }
                } else {
                    streamer.collected.upstream_error =
                        Some("Kiro 上游返回了空的事件流".to_string());
                }
            }

            if let Some(err_msg) = streamer.collected.upstream_error.as_deref() {
                let stream_class = classify_upstream(502, err_msg, false);
                state.pool.note_failure(&account.id, stream_class);
                if attempts < max_attempts && stream_class != FailureClass::Client {
                    continue;
                }
                let status = if stream_class == FailureClass::Client {
                    StatusCode::BAD_REQUEST
                } else if matches!(stream_class, FailureClass::CoolAndRotate) {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::BAD_GATEWAY
                };
                log_failure(
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    status.as_u16() as i64,
                    if status == StatusCode::BAD_REQUEST {
                        "client"
                    } else {
                        "upstream"
                    },
                );
                return error_response(
                    protocol,
                    status,
                    &err_msg.chars().take(400).collect::<String>(),
                );
            }

            let log_id = if map::should_log_usage("/v1/messages") {
                crate::kiro::usage_log::insert_request(
                    &state.db,
                    Some(&account.id),
                    &client_model,
                    Some(200),
                    started,
                    protocol,
                    true,
                    None,
                    Some(&headers),
                )
            } else {
                None
            };
            let db = state.db.clone();
            let context_window = crate::kiro::models::context_window(&prepared.backend_model);
            let req_len = body.len();
            let sse_stream = futures_util::stream::unfold(
                (
                    byte_stream,
                    decoder,
                    streamer,
                    Some(initial_sse),
                    false,
                    db,
                    log_id,
                ),
                move |(
                    mut byte_stream,
                    mut decoder,
                    mut streamer,
                    mut pending,
                    done,
                    db,
                    log_id,
                )| async move {
                    if done {
                        return None;
                    }
                    if let Some(init) = pending.take() {
                        if !init.is_empty() {
                            return Some((
                                Ok::<Bytes, std::convert::Infallible>(Bytes::from(init)),
                                (byte_stream, decoder, streamer, None, false, db, log_id),
                            ));
                        }
                    }
                    loop {
                        match byte_stream.next().await {
                            Some(Ok(chunk)) => match decoder.push(&chunk) {
                                Ok(frames) => {
                                    let mut out = String::new();
                                    for frame in &frames {
                                        out.push_str(&streamer.push_frame(frame));
                                    }
                                    if !out.is_empty() {
                                        return Some((
                                            Ok(Bytes::from(out)),
                                            (
                                                byte_stream,
                                                decoder,
                                                streamer,
                                                None,
                                                false,
                                                db,
                                                log_id,
                                            ),
                                        ));
                                    }
                                }
                                Err(err) => {
                                    log::warn!("Kiro mid-stream decode error: {err:?}");
                                    let tail = streamer.finish(context_window, req_len);
                                    if let Some(ref id) = log_id {
                                        crate::kiro::usage_log::write_usage(
                                            &db,
                                            id,
                                            streamer.collected.input_tokens,
                                            streamer.collected.output_tokens,
                                            streamer.collected.cache_read,
                                            streamer.collected.cache_write,
                                        );
                                    }
                                    return Some((
                                        Ok(Bytes::from(tail)),
                                        (byte_stream, decoder, streamer, None, true, db, log_id),
                                    ));
                                }
                            },
                            Some(Err(err)) => {
                                log::warn!("Kiro mid-stream read error: {err}");
                                let tail = streamer.finish(context_window, req_len);
                                if let Some(ref id) = log_id {
                                    crate::kiro::usage_log::write_usage(
                                        &db,
                                        id,
                                        streamer.collected.input_tokens,
                                        streamer.collected.output_tokens,
                                        streamer.collected.cache_read,
                                        streamer.collected.cache_write,
                                    );
                                }
                                return Some((
                                    Ok(Bytes::from(tail)),
                                    (byte_stream, decoder, streamer, None, true, db, log_id),
                                ));
                            }
                            None => {
                                let tail = streamer.finish(context_window, req_len);
                                if let Some(ref id) = log_id {
                                    crate::kiro::usage_log::write_usage(
                                        &db,
                                        id,
                                        streamer.collected.input_tokens,
                                        streamer.collected.output_tokens,
                                        streamer.collected.cache_read,
                                        streamer.collected.cache_write,
                                    );
                                }
                                return Some((
                                    Ok(Bytes::from(tail)),
                                    (byte_stream, decoder, streamer, None, true, db, log_id),
                                ));
                            }
                        }
                    }
                },
            );

            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from_stream(sse_stream))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }

        let reply = upstream::UpstreamReply {
            status: status_code,
            body: http_resp.bytes().await.unwrap_or_default(),
            network_error: None,
        };
        let text = String::from_utf8_lossy(&reply.body);
        // 2xx bodies are binary event-stream; classifying them would turn a
        // successful reply into a client error.
        let class = if upstream_ok {
            FailureClass::Rotate
        } else {
            classify_upstream(reply.status, &text, false)
        };
        if !upstream_ok && should_force_refresh(reply.status, &text, refreshed) {
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
        if !upstream_ok && (reply.status == 401 || class == FailureClass::Auth) {
            log_failure(&state, &headers, &incoming, protocol, started, 401, "auth");
            return error_response(protocol, StatusCode::UNAUTHORIZED, "Kiro 鉴权失败");
        }
        if !upstream_ok && class == FailureClass::Client {
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                reply.status as i64,
                "client",
            );
            return error_response(
                protocol,
                StatusCode::BAD_REQUEST,
                &text.chars().take(400).collect::<String>(),
            );
        }
        if !upstream_ok
            && matches!(
                class,
                FailureClass::Rotate | FailureClass::CoolAndRotate | FailureClass::Disable
            )
            && attempts < max_attempts
        {
            state.pool.note_failure(&account.id, class);
            continue;
        }
        if !upstream_ok {
            let status = if reply.status == 429 {
                StatusCode::TOO_MANY_REQUESTS
            } else if reply.status == 504 {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };
            state.pool.note_failure(&account.id, class);
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                status.as_u16() as i64,
                "upstream",
            );
            let mut response = error_response(
                protocol,
                status,
                &text.chars().take(400).collect::<String>(),
            );
            if status == StatusCode::TOO_MANY_REQUESTS {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, "5".parse().unwrap());
            }
            return response;
        }
        let mut decoder = FrameDecoder::new();
        let frames = match decoder.push(&reply.body) {
            Ok(frames) => frames,
            Err(err) => {
                log::warn!("Kiro event-stream decode error: {err:?}");
                state.pool.note_failure(&account.id, FailureClass::Rotate);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state, &headers, &incoming, protocol, started, 502, "upstream",
                );
                return error_response(
                    protocol,
                    StatusCode::BAD_GATEWAY,
                    &format!("Kiro 上游事件流解析失败: {err:?}"),
                );
            }
        };
        let mut collected = CollectedReply::default();
        if frames.is_empty() {
            if let Ok(json_body) = serde_json::from_slice::<Value>(&reply.body) {
                if let Some(err_msg) = json_body
                    .get("message")
                    .or_else(|| json_body.pointer("/error/message"))
                    .and_then(Value::as_str)
                {
                    collected.upstream_error = Some(err_msg.to_string());
                } else {
                    collected.text = text.to_string();
                }
            } else {
                collected.upstream_error = Some("Kiro 上游返回了空的事件流".to_string());
            }
        } else {
            for frame in &frames {
                collected.push_frame(frame);
            }
        }
        if let Some(err_msg) = collected.upstream_error.as_deref() {
            let stream_class = classify_upstream(502, err_msg, false);
            state.pool.note_failure(&account.id, stream_class);
            if attempts < max_attempts && stream_class != FailureClass::Client {
                continue;
            }
            let status = if stream_class == FailureClass::Client {
                StatusCode::BAD_REQUEST
            } else if matches!(stream_class, FailureClass::CoolAndRotate) {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_GATEWAY
            };
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                status.as_u16() as i64,
                if status == StatusCode::BAD_REQUEST {
                    "client"
                } else {
                    "upstream"
                },
            );
            return error_response(
                protocol,
                status,
                &err_msg.chars().take(400).collect::<String>(),
            );
        }
        let client_model = client_model(&headers, &prepared.client_model);
        collected.finalize_usage(
            crate::kiro::models::context_window(&prepared.backend_model),
            body.len(),
        );
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
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
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
    let model = if client_model.is_empty() {
        DEFAULT_MODEL
    } else {
        client_model
    };
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
        (WireProtocol::OpenAiResponses, false) => {
            Json(responses_json(model, reply)).into_response()
        }
    }
}

fn error_response(
    protocol: WireProtocol,
    status: StatusCode,
    message: &str,
) -> axum::response::Response {
    let kind = match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::GATEWAY_TIMEOUT => "timeout_error",
        _ => "invalid_request_error",
    };
    let body = match protocol {
        WireProtocol::Anthropic => {
            json!({"type": "error", "error": {"type": kind, "message": message}})
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    #[ignore]
    async fn live_kiro_claude_sonnet_55_incremental_streaming_smoke() {
        let proxy_url = crate::system_proxy::outbound_proxy_url();
        println!("System outbound_proxy_url: {:?}", proxy_url);
        crate::kiro::outbound::apply_loaded(&crate::kiro::outbound::OutboundSettings {
            mode: crate::kiro::outbound::OutboundMode::System,
            proxy_url: String::new(),
            effective_proxy_url: proxy_url,
        });
        let account = tokio::task::spawn_blocking(|| {
            crate::kiro::pool::AccountPool::new().select()
        })
        .await
        .unwrap()
        .expect("requires at least one active Kiro account in ~/.claude-switcher/kiro_accounts.json");
        let account = ensure_access_token(&account, false)
            .await
            .expect("ensure_access_token for Kiro account");

        for round in 1..=2 {
            let incoming = serde_json::json!({
                "model": "claude-sonnet-5.5",
                "max_tokens": 256,
                "stream": true,
                "messages": [
                    {
                        "role": "user",
                        "content": "请用中文写一首五言绝句并简短赏析（控制在80字以内）。"
                    }
                ]
            });
            let prepared = map::prepare(WireProtocol::Anthropic, &incoming, &account)
                .expect("prepare Kiro request");
            let context_window = crate::kiro::models::context_window(&prepared.backend_model);
            let req_len = incoming.to_string().len();
            let t0 = Instant::now();
            let resp = upstream::send_generate_request(&account, &prepared.body)
                .await
                .expect("send_generate_request to Kiro");
            let status = resp.status().as_u16();
            println!(
                "[Round {round}] HTTP {status} response headers at +{}ms",
                t0.elapsed().as_millis()
            );
            if status != 200 {
                let err_body = resp.text().await.unwrap_or_default();
                panic!("Expected 200 OK, got {status}: {err_body}");
            }

            let mut streamer = map::KiroSseStreamer::new(
                WireProtocol::Anthropic,
                &prepared.client_model,
            );

            let mut byte_stream = resp.bytes_stream();
            let mut decoder = FrameDecoder::new();
            let mut delta_chunks = 0usize;
            let mut first_delta_ms = None;
            let mut last_delta_ms = 0u128;

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.expect("stream chunk");
                let frames = decoder.push(&bytes).expect("decode frames");
                for frame in &frames {
                    let out = streamer.push_frame(frame);
                    if !out.is_empty() {
                        if out.contains("text_delta") || out.contains("thinking_delta") {
                            let ms = t0.elapsed().as_millis();
                            if first_delta_ms.is_none() {
                                first_delta_ms = Some(ms);
                            }
                            last_delta_ms = ms;
                            delta_chunks += 1;
                            if delta_chunks <= 8 {
                                println!(
                                    "[Round {round}] [+{ms}ms] chunk #{delta_chunks}: {}",
                                    out.lines().find(|l| l.starts_with("data:")).unwrap_or("")
                                );
                            }
                        }
                    }
                }
            }
            let fin = streamer.finish(context_window, req_len);
            let total_ms = t0.elapsed().as_millis();
            println!(
                "[Round {round}] [+{total_ms}ms] stream finished: delta_chunks={delta_chunks}, first_delta={:?}ms, last_delta={last_delta_ms}ms",
                first_delta_ms
            );
            assert!(delta_chunks > 1, "expected multiple incremental SSE delta chunks");
            assert!(fin.contains("event: message_stop"));
        }
    }
}
