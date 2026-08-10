//! Gateway HTTP handlers.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

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
    anthropic_to_gemini_request, effort_mapping_diagnostic, gemini_to_anthropic_response,
    gemini_to_anthropic_sse_chunk, AnthropicStreamState,
};
use crate::antigravity::map::args_fix::ToolParamKeys;
use crate::antigravity::map::openai::{
    gemini_to_openai_response, gemini_to_openai_sse_chunk, openai_to_gemini_request,
};
use crate::antigravity::map::responses::{
    gemini_to_responses_response, responses_compact_stub, responses_to_gemini_request,
    ResponsesStreamEncoder,
};
use crate::antigravity::map::list_public_models;
use crate::antigravity::upstream::{unwrap_v1internal, wrap_v1internal};
use crate::antigravity::usage_log::{self, WireProtocol};
use crate::database::Database;

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
    let session_key = session_key_from_headers(&headers);
    let sticky = session_key
        .as_deref()
        .and_then(crate::antigravity::session_effort::get);
    let mapped = match anthropic_to_gemini_request(&payload, sticky, session_key.as_deref()) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &error),
    };
    if let (Some(session), Some(level)) = (session_key.as_deref(), mapped.remember_effort) {
        crate::antigravity::session_effort::set(session, level);
    }
    let diagnostic = effort_mapping_diagnostic(&payload, &mapped.model);
    dispatch_generation(
        &state,
        &headers,
        mapped.model,
        mapped.request,
        mapped.stream,
        WireProtocol::Anthropic,
        Some(diagnostic),
        mapped.thoughts_allowed,
        mapped.tool_params,
    )
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
    dispatch_generation(
        &state,
        &headers,
        mapped.model,
        mapped.request,
        mapped.stream,
        WireProtocol::OpenAiChat,
        None,
        false,
        mapped.tool_params,
    )
    .await
}

pub async fn openai_responses(
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
    let mapped = match responses_to_gemini_request(&payload) {
        Ok(value) => value,
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &error),
    };
    dispatch_generation(
        &state,
        &headers,
        mapped.model,
        mapped.request,
        mapped.stream,
        WireProtocol::OpenAiResponses,
        None,
        false,
        mapped.tool_params,
    )
    .await
}

/// Minimal compact endpoint so Codex does not 404 on `/v1/responses/compact`.
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
        Err(error) => return error_json(StatusCode::BAD_REQUEST, &format!("invalid json: {error}")),
    };
    if payload.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        return error_json(
            StatusCode::BAD_REQUEST,
            "Codex /responses/compact 不支持显式 stream=true",
        );
    }
    let compact = responses_compact_stub(&payload);
    let model = compact
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let _ = usage_log::insert_request(
        &state.db,
        None,
        &model,
        Some(StatusCode::OK.as_u16() as i64),
        Instant::now(),
        WireProtocol::OpenAiResponses,
        false,
        None,
        Some("compact=stub"),
    );
    Json(compact).into_response()
}

async fn dispatch_generation(
    state: &GatewayState,
    headers: &HeaderMap,
    model: String,
    request: Value,
    stream: bool,
    protocol: WireProtocol,
    diagnostic: Option<String>,
    thoughts_allowed: bool,
    tool_params: ToolParamKeys,
) -> Response {
    let started = Instant::now();
    let session_key = session_key_from_headers(headers);
    let mut exclude: Vec<String> = Vec::new();
    let mut last_error = String::from("upstream failed");

    for hop in 0..MAX_FAILOVER_HOPS {
        let selected = if hop == 0 {
            state.pool.select_async(None, session_key.as_deref()).await
        } else {
            let failed = exclude.last().cloned().unwrap_or_default();
            state
                .pool
                .rotate_after_failure_async(&failed, 429, session_key.as_deref(), &exclude)
                .await
        };
        let (access_token, account) = match selected {
            Ok(value) => value,
            Err(error) => {
                let message = error.to_string();
                // Keep the real upstream/token error when failover only ran out of accounts.
                if hop == 0 || last_error == "upstream failed" {
                    last_error = message;
                } else {
                    last_error = format!("{last_error}；{message}");
                }
                break;
            }
        };
        exclude.push(account.id.clone());

        let project_id = match ensure_project_id(
            state,
            &access_token,
            &account.id,
            account.token.project_id.as_deref(),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                last_error = error;
                let _ = account_store().mark_cooldown(&account.id, 45, &last_error);
                continue;
            }
        };

        let wrapped = wrap_v1internal(&project_id, &model, request.clone());
        // 500/502/504 are usually transient blips: retry the same account with
        // a short backoff before burning a rotation (503/529 already got
        // per-host backoff inside `generate`).
        let mut server_error_retry = 0u32;
        let upstream = loop {
            match state
                .upstream
                .generate(&access_token, &wrapped, stream)
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if matches!(status.as_u16(), 500 | 502 | 504) && server_error_retry < 2 {
                        server_error_retry += 1;
                        log::warn!(
                            "Antigravity upstream {status}; same-account retry {server_error_retry}/2"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(
                            2 << server_error_retry,
                        ))
                        .await;
                        continue;
                    }
                    break Ok(response);
                }
                Err(error) => break Err(error),
            }
        };
        let upstream = match upstream {
            Ok(response) => response,
            Err(error) => {
                last_error = error.to_string();
                let _ = account_store().mark_cooldown(&account.id, 20, &last_error);
                continue;
            }
        };

        let status = upstream.status();
        if matches!(status.as_u16(), 401 | 403 | 429) {
            let retry_after = crate::antigravity::upstream::retry_after_secs(&upstream);
            let text = upstream.text().await.unwrap_or_default();
            let detail = text.trim();
            last_error = if detail.is_empty() {
                format!("upstream {status}")
            } else {
                let clipped: String = detail.chars().take(240).collect();
                format!("upstream {status}: {clipped}")
            };
            let _ = state
                .pool
                .rotate_after_failure_async(
                    &account.id,
                    status.as_u16(),
                    session_key.as_deref(),
                    &exclude,
                )
                .await;
            // Honor the upstream Retry-After as the cooldown window for 429.
            if status.as_u16() == 429 {
                if let Some(seconds) = retry_after {
                    let _ = account_store().adjust_cooldown_secs(&account.id, seconds as i64);
                }
            }
            continue;
        }
        if !status.is_success() {
            let text = upstream.text().await.unwrap_or_default();
            last_error = format!("upstream {status}: {text}");
            let _ = account_store().mark_cooldown(&account.id, 15, &last_error);
            continue;
        }

        state.pool.note_success(&account.id);
        let log_id = usage_log::insert_request(
            &state.db,
            Some(&account.id),
            &model,
            Some(status.as_u16() as i64),
            started,
            protocol,
            stream,
            None,
            diagnostic.as_deref(),
        );
        if stream {
            return stream_response(
                upstream,
                model.clone(),
                protocol,
                state.db.clone(),
                log_id,
                Some(account.id.clone()),
                session_key.clone(),
                thoughts_allowed,
                tool_params,
            )
            .await;
        }
        return match upstream.json::<Value>().await {
            Ok(value) => {
                let gemini = unwrap_v1internal(&value);
                if let Some(id) = log_id.as_deref() {
                    usage_log::update_usage_from_gemini(&state.db, id, Some(&account.id), &gemini);
                }
                match protocol {
                    WireProtocol::Anthropic => {
                        Json(gemini_to_anthropic_response(
                            &model,
                            &gemini,
                            session_key.as_deref(),
                            thoughts_allowed,
                            &tool_params,
                        ))
                        .into_response()
                    }
                    WireProtocol::OpenAiChat => {
                        Json(gemini_to_openai_response(&model, &gemini, &tool_params))
                            .into_response()
                    }
                    WireProtocol::OpenAiResponses => {
                        Json(gemini_to_responses_response(&model, &gemini, &tool_params))
                            .into_response()
                    }
                }
            }
            Err(error) => error_json(
                StatusCode::BAD_GATEWAY,
                &format!("invalid upstream json: {error}"),
            ),
        };
    }

    let clipped_error: String = last_error.chars().take(400).collect();
    let _ = usage_log::insert_request(
        &state.db,
        exclude.last().map(String::as_str),
        &model,
        Some(StatusCode::BAD_GATEWAY.as_u16() as i64),
        started,
        protocol,
        stream,
        Some("upstream"),
        Some(&clipped_error),
    );
    error_json(StatusCode::BAD_GATEWAY, &clipped_error)
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
    protocol: WireProtocol,
    db: Arc<Database>,
    log_id: Option<String>,
    account_id: Option<String>,
    session_key: Option<String>,
    thoughts_allowed: bool,
    tool_params: ToolParamKeys,
) -> Response {
    let byte_stream = upstream.bytes_stream().boxed();
    let responses_encoder = matches!(protocol, WireProtocol::OpenAiResponses)
        .then(|| ResponsesStreamEncoder::new(&model, tool_params.clone()));
    let stream = futures_util::stream::unfold(
        StreamState {
            upstream: byte_stream,
            buffer: String::new(),
            model,
            protocol,
            anthropic: matches!(protocol, WireProtocol::Anthropic)
                .then(AnthropicStreamState::default),
            finished: false,
            db,
            log_id,
            account_id,
            session_key,
            thoughts_allowed,
            tool_params,
            last_input: 0,
            last_output: 0,
            responses_encoder,
        },
        |mut state| async move {
            if state.finished {
                return None;
            }
            loop {
                if let Some(event) = state.take_line_event() {
                    return Some((Ok::<Bytes, Infallible>(event), state));
                }
                // Idle-timeout each upstream frame (mirrors Antigravity-Manager's
                // 300s per-frame guard) so a stalled upstream cannot hang the
                // client connection forever.
                let next_frame = tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    state.upstream.next(),
                )
                .await;
                match next_frame {
                    Ok(Some(Ok(bytes))) => {
                        state.buffer.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Ok(Some(Err(_))) | Ok(None) => {
                        let trailing = state.finish_events();
                        state.flush_usage();
                        state.finished = true;
                        if trailing.is_empty() {
                            return None;
                        }
                        return Some((Ok::<Bytes, Infallible>(trailing), state));
                    }
                    Err(_) => {
                        log::error!(
                            "Antigravity stream idle for 300s; closing (model {})",
                            state.model
                        );
                        let trailing = state.finish_events();
                        state.flush_usage();
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
    protocol: WireProtocol,
    finished: bool,
    /// Anthropic 协议的跨 chunk 块状态（thinking/text 块开闭、message 生命周期）。
    anthropic: Option<AnthropicStreamState>,
    db: Arc<Database>,
    log_id: Option<String>,
    account_id: Option<String>,
    session_key: Option<String>,
    thoughts_allowed: bool,
    /// 本次请求的工具声明参数键名，响应映射时纠偏 args key。
    tool_params: ToolParamKeys,
    last_input: i64,
    last_output: i64,
    responses_encoder: Option<ResponsesStreamEncoder>,
}

impl StreamState {
    fn note_usage(&mut self, gemini: &Value) {
        let (input, output) = usage_log::tokens_from_gemini(gemini);
        if input > 0 {
            self.last_input = input;
        }
        if output > 0 {
            self.last_output = output;
        }
    }

    fn flush_usage(&self) {
        let Some(log_id) = self.log_id.as_deref() else {
            return;
        };
        usage_log::update_usage_tokens(
            &self.db,
            log_id,
            self.account_id.as_deref(),
            self.last_input,
            self.last_output,
        );
    }

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
        self.note_usage(&gemini);
        match self.protocol {
            WireProtocol::Anthropic => {
                let model = self.model.clone();
                let session = self.session_key.clone();
                let mut anthropic = self.anthropic.take().unwrap_or_default();
                let events = gemini_to_anthropic_sse_chunk(
                    &mut anthropic,
                    &model,
                    &gemini,
                    session.as_deref(),
                    self.thoughts_allowed,
                    &self.tool_params,
                );
                self.anthropic = Some(anthropic);
                let mut out = Vec::new();
                for event in events {
                    out.extend_from_slice(&sse_data(&event));
                }
                if out.is_empty() {
                    None
                } else {
                    Some(Bytes::from(out))
                }
            }
            WireProtocol::OpenAiChat => Some(sse_data(&gemini_to_openai_sse_chunk(
                &self.model,
                &gemini,
                &self.tool_params,
            ))),
            WireProtocol::OpenAiResponses => {
                let Some(encoder) = self.responses_encoder.as_mut() else {
                    return None;
                };
                let bytes = encoder.encode_gemini_chunk(&gemini);
                if bytes.is_empty() {
                    None
                } else {
                    Some(Bytes::from(bytes))
                }
            }
        }
    }

    fn finish_events(&mut self) -> Bytes {
        match self.protocol {
            WireProtocol::OpenAiChat => Bytes::from("data: [DONE]\n\n"),
            WireProtocol::OpenAiResponses => {
                if let Some(encoder) = self.responses_encoder.as_mut() {
                    Bytes::from(encoder.finish())
                } else {
                    Bytes::new()
                }
            }
            WireProtocol::Anthropic => {
                let mut anthropic = self.anthropic.take().unwrap_or_default();
                let events = anthropic.finish_events(&self.model, self.last_output);
                self.anthropic = Some(anthropic);
                let mut out = Vec::new();
                for event in events {
                    out.extend_from_slice(&sse_data(&event));
                }
                Bytes::from(out)
            }
        }
    }
}

fn sse_data(value: &Value) -> Bytes {
    Bytes::from(format!("data: {}\n\n", value))
}

fn session_key_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-session-id")
        .or_else(|| headers.get("x-claude-session-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
