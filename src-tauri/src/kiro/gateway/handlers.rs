//! HTTP routes for the Kiro gateway.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use super::GatewayState;
use crate::database::Database;
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

const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(75);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

struct KiroStreamGuard {
    streamer: map::KiroSseStreamer,
    db: Arc<Database>,
    log_id: Option<String>,
    context_window: u64,
    req_len: usize,
    usage_flushed: bool,
}

impl KiroStreamGuard {
    fn finish_and_flush(&mut self) -> String {
        self.finish_and_flush_with_outcome("complete", None, None)
    }

    fn finish_and_flush_with_outcome(
        &mut self,
        outcome: &'static str,
        error_category: Option<&'static str>,
        diagnostic: Option<&'static str>,
    ) -> String {
        let tail = self.streamer.finish(self.context_window, self.req_len);
        self.flush_usage_with_outcome(outcome, error_category, diagnostic);
        tail
    }

    fn flush_usage_with_outcome(
        &mut self,
        outcome: &'static str,
        error_category: Option<&'static str>,
        diagnostic: Option<&'static str>,
    ) {
        if self.usage_flushed {
            return;
        }
        self.usage_flushed = true;
        self.streamer
            .collected
            .finalize_usage(self.context_window, self.req_len);
        let effective_outcome = if self.streamer.collected.upstream_error.is_some() {
            "midstream_error"
        } else {
            outcome
        };
        if let Some(ref id) = self.log_id {
            let _ = self.db.with_conn(|conn| {
                crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                    conn,
                    id,
                    effective_outcome,
                    None,
                    error_category,
                    diagnostic,
                )
            });
            crate::kiro::usage_log::write_usage(
                &self.db,
                id,
                self.streamer.collected.input_tokens,
                self.streamer.collected.output_tokens,
                self.streamer.collected.cache_read,
                self.streamer.collected.cache_write,
            );
        }
    }
}

impl Drop for KiroStreamGuard {
    fn drop(&mut self) {
        self.flush_usage_with_outcome("cancelled", Some("cancelled"), Some("客户端断开连接"));
    }
}

pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

fn build_public_models() -> Vec<Value> {
    crate::kiro::models::model_list()
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
        .collect()
}

pub async fn list_models() -> impl IntoResponse {
    Json(json!({"object": "list", "data": build_public_models()}))
}

pub async fn get_model(axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let trimmed = id.trim();
    if let Some(found) = build_public_models().into_iter().find(|entry| {
        entry
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|model_id| model_id == trimmed)
    }) {
        return Json(found).into_response();
    }
    error_response(
        WireProtocol::OpenAiResponses,
        StatusCode::NOT_FOUND,
        &format!("Model '{trimmed}' not found"),
    )
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

pub async fn openai_responses_compact(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                WireProtocol::OpenAiResponses,
                StatusCode::BAD_REQUEST,
                &format!("invalid json: {error}"),
            )
        }
    };
    if payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return error_response(
            WireProtocol::OpenAiResponses,
            StatusCode::BAD_REQUEST,
            "Codex /responses/compact 不支持显式 stream=true",
        );
    }
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string();
    let compact = crate::antigravity::map::responses::responses_compact_stub(&payload);
    let _ = crate::kiro::usage_log::insert_request(
        &state.db,
        None,
        &model,
        Some(StatusCode::OK.as_u16() as i64),
        Instant::now(),
        WireProtocol::OpenAiResponses,
        false,
        None,
        Some("compact=stub"),
        Some(&headers),
    );
    Json(compact).into_response()
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
    let mut excluded_ids: Vec<String> = Vec::new();
    let mut last_account_id: Option<String> = None;
    let mut attempts = 0;
    let max_attempts = 3;
    loop {
        attempts += 1;
        let Some(account) = retry_same
            .take()
            .or_else(|| state.pool.select_excluding(&excluded_ids))
        else {
            let msg = "没有可用的 Kiro 账号";
            let response = error_response(protocol, StatusCode::SERVICE_UNAVAILABLE, msg);
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                last_account_id.as_deref(),
                503,
                "no_account",
                Some(msg),
            );
            return response;
        };
        last_account_id = Some(account.id.clone());
        if !excluded_ids.iter().any(|id| id == &account.id) {
            excluded_ids.push(account.id.clone());
        }
        let account = match ensure_access_token(&account, false).await {
            Ok(account) => account,
            Err(error) => {
                let err_text = error.to_string();
                if attempts >= max_attempts {
                    log_failure(
                        &state,
                        &headers,
                        &incoming,
                        protocol,
                        started,
                        last_account_id.as_deref(),
                        401,
                        "auth",
                        Some(&err_text),
                    );
                    return error_response(protocol, StatusCode::UNAUTHORIZED, &err_text);
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
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    last_account_id.as_deref(),
                    504,
                    "network",
                    Some(&message),
                );
                return error_response(protocol, StatusCode::GATEWAY_TIMEOUT, &message);
            }
        };
        let status_code = http_resp.status().as_u16();
        let upstream_ok = (200..300).contains(&status_code);
        if upstream_ok && prepared.stream {
            use futures_util::StreamExt;
            let client_model = client_model(&headers, &prepared.client_model);
            let log_model = if crate::proxy::desktop_alias::is_desktop_alias(&client_model) {
                prepared.client_model.clone()
            } else {
                client_model.clone()
            };
            let effort_diag = crate::database::dao::proxy_logs::extract_request_reasoning_effort(
                &incoming,
                Some(&log_model),
                None,
            )
            .map(|e| format!("effort={e}"));
            let mut byte_stream = http_resp.bytes_stream();
            let mut decoder = FrameDecoder::new();
            let mut streamer = map::KiroSseStreamer::new(protocol, &client_model);
            let mut initial_sse = String::new();
            let mut raw_buf = Vec::new();
            let mut first_frame_seen = false;
            let mut decode_err: Option<String> = None;
            let mut first_frame_timed_out = false;
            let first_frame_deadline = tokio::time::Instant::now() + FIRST_FRAME_TIMEOUT;

            while !first_frame_seen {
                match tokio::time::timeout_at(first_frame_deadline, byte_stream.next()).await {
                    Ok(Some(Ok(chunk))) => {
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
                    Ok(Some(Err(err))) => {
                        decode_err = Some(err.to_string());
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        first_frame_timed_out = true;
                        break;
                    }
                }
            }

            if first_frame_timed_out {
                let msg = format!(
                    "Kiro 流式首帧超时（>{}s）",
                    FIRST_FRAME_TIMEOUT.as_secs()
                );
                log::warn!("{msg} account={}", account.id);
                state.pool.note_failure(&account.id, FailureClass::Rotate);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    last_account_id.as_deref(),
                    504,
                    "timeout",
                    Some(&msg),
                );
                return error_response(protocol, StatusCode::GATEWAY_TIMEOUT, &msg);
            }

            if let Some(err) = decode_err {
                let msg = format!("Kiro 上游事件流解析失败: {err}");
                log::warn!("Kiro event-stream decode error: {err}");
                state.pool.note_failure(&account.id, FailureClass::Rotate);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    last_account_id.as_deref(),
                    502,
                    "upstream",
                    Some(&msg),
                );
                return error_response(protocol, StatusCode::BAD_GATEWAY, &msg);
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
                let truncated: String = err_msg.chars().take(400).collect();
                log_failure(
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    last_account_id.as_deref(),
                    status.as_u16() as i64,
                    if status == StatusCode::BAD_REQUEST {
                        "client"
                    } else {
                        "upstream"
                    },
                    Some(&truncated),
                );
                return error_response(protocol, status, &truncated);
            }

            let log_id = if map::should_log_usage("/v1/messages") {
                crate::kiro::usage_log::insert_request(
                    &state.db,
                    Some(&account.id),
                    &log_model,
                    Some(200),
                    started,
                    protocol,
                    true,
                    None,
                    effort_diag.as_deref(),
                    Some(&headers),
                )
            } else {
                None
            };
            let context_window = crate::kiro::models::context_window(&prepared.backend_model);
            let req_len = body.len();
            let guard = KiroStreamGuard {
                streamer,
                db: state.db.clone(),
                log_id,
                context_window,
                req_len,
                usage_flushed: false,
            };
            let sse_stream = futures_util::stream::unfold(
                (byte_stream, decoder, guard, Some(initial_sse), false),
                move |(mut byte_stream, mut decoder, mut guard, mut pending, done)| async move {
                    if done {
                        return None;
                    }
                    if let Some(init) = pending.take() {
                        if !init.is_empty() {
                            return Some((
                                Ok::<Bytes, std::convert::Infallible>(Bytes::from(init)),
                                (byte_stream, decoder, guard, None, false),
                            ));
                        }
                    }
                    loop {
                        match tokio::time::timeout(STREAM_IDLE_TIMEOUT, byte_stream.next()).await {
                            Ok(Some(Ok(chunk))) => match decoder.push(&chunk) {
                                Ok(frames) => {
                                    let mut out = String::new();
                                    for frame in &frames {
                                        out.push_str(&guard.streamer.push_frame(frame));
                                    }
                                    if !out.is_empty() {
                                        return Some((
                                            Ok(Bytes::from(out)),
                                            (byte_stream, decoder, guard, None, false),
                                        ));
                                    }
                                }
                                Err(err) => {
                                    log::warn!("Kiro mid-stream decode error: {err:?}");
                                    let tail = guard.finish_and_flush_with_outcome(
                                        "midstream_error",
                                        Some("midstream_error"),
                                        Some("Kiro 上游事件流解析中断"),
                                    );
                                    return Some((
                                        Ok(Bytes::from(tail)),
                                        (byte_stream, decoder, guard, None, true),
                                    ));
                                }
                            },
                            Ok(Some(Err(err))) => {
                                log::warn!("Kiro mid-stream read error: {err}");
                                let tail = guard.finish_and_flush_with_outcome(
                                    "midstream_error",
                                    Some("midstream_error"),
                                    Some("Kiro 上游流式响应中途中断"),
                                );
                                return Some((
                                    Ok(Bytes::from(tail)),
                                    (byte_stream, decoder, guard, None, true),
                                ));
                            }
                            Ok(None) => {
                                let tail = guard.finish_and_flush();
                                return Some((
                                    Ok(Bytes::from(tail)),
                                    (byte_stream, decoder, guard, None, true),
                                ));
                            }
                            Err(_) => {
                                log::warn!(
                                    "Kiro mid-stream idle timeout after {}s",
                                    STREAM_IDLE_TIMEOUT.as_secs()
                                );
                                let tail = guard.finish_and_flush_with_outcome(
                                    "midstream_error",
                                    Some("timeout"),
                                    Some("Kiro 流式响应空闲超时"),
                                );
                                return Some((
                                    Ok(Bytes::from(tail)),
                                    (byte_stream, decoder, guard, None, true),
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
                    let err_text = error.to_string();
                    log_failure(
                        &state,
                        &headers,
                        &incoming,
                        protocol,
                        started,
                        last_account_id.as_deref(),
                        401,
                        "auth",
                        Some(&err_text),
                    );
                    return error_response(protocol, StatusCode::UNAUTHORIZED, &err_text);
                }
            }
        }
        if !upstream_ok && (reply.status == 401 || class == FailureClass::Auth) {
            let msg = "Kiro 鉴权失败";
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                last_account_id.as_deref(),
                401,
                "auth",
                Some(msg),
            );
            return error_response(protocol, StatusCode::UNAUTHORIZED, msg);
        }
        if !upstream_ok && class == FailureClass::Client {
            let truncated: String = text.chars().take(400).collect();
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                last_account_id.as_deref(),
                reply.status as i64,
                "client",
                Some(&truncated),
            );
            return error_response(protocol, StatusCode::BAD_REQUEST, &truncated);
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
            let truncated: String = text.chars().take(400).collect();
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                last_account_id.as_deref(),
                status.as_u16() as i64,
                "upstream",
                Some(&truncated),
            );
            let mut response = error_response(protocol, status, &truncated);
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
                let msg = format!("Kiro 上游事件流解析失败: {err:?}");
                log::warn!("Kiro event-stream decode error: {err:?}");
                state.pool.note_failure(&account.id, FailureClass::Rotate);
                if attempts < max_attempts {
                    continue;
                }
                log_failure(
                    &state,
                    &headers,
                    &incoming,
                    protocol,
                    started,
                    last_account_id.as_deref(),
                    502,
                    "upstream",
                    Some(&msg),
                );
                return error_response(protocol, StatusCode::BAD_GATEWAY, &msg);
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
            let truncated: String = err_msg.chars().take(400).collect();
            log_failure(
                &state,
                &headers,
                &incoming,
                protocol,
                started,
                last_account_id.as_deref(),
                status.as_u16() as i64,
                if status == StatusCode::BAD_REQUEST {
                    "client"
                } else {
                    "upstream"
                }
                ,
                Some(&truncated),
            );
            return error_response(protocol, status, &truncated);
        }
        let client_model = client_model(&headers, &prepared.client_model);
        let log_model = if crate::proxy::desktop_alias::is_desktop_alias(&client_model) {
            prepared.client_model.clone()
        } else {
            client_model.clone()
        };
        let effort_diag = crate::database::dao::proxy_logs::extract_request_reasoning_effort(
            &incoming,
            Some(&log_model),
            None,
        )
        .map(|e| format!("effort={e}"));
        collected.finalize_usage(
            crate::kiro::models::context_window(&prepared.backend_model),
            body.len(),
        );
        if map::should_log_usage("/v1/messages") {
            if let Some(id) = crate::kiro::usage_log::insert_request(
                &state.db,
                Some(&account.id),
                &log_model,
                Some(200),
                started,
                protocol,
                prepared.stream,
                None,
                effort_diag.as_deref(),
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
    account_id: Option<&str>,
    status: i64,
    category: &str,
    diagnostic: Option<&str>,
) {
    let model = incoming
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(DEFAULT_MODEL);
    let _ = crate::kiro::usage_log::insert_request(
        &state.db,
        account_id,
        model,
        Some(status),
        started,
        protocol,
        false,
        Some(category),
        diagnostic,
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
                    if !out.is_empty()
                        && (out.contains("text_delta") || out.contains("thinking_delta"))
                    {
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
