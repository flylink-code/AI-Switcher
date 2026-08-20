//! Gateway HTTP handlers.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
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
use crate::antigravity::model_catalog;
use crate::antigravity::limiter::{AcquireOutcome, LimiterPermit};
use crate::antigravity::upstream::{classify_rate_limit_body, unwrap_v1internal, wrap_v1internal};
use crate::antigravity::usage_log::{self, WireProtocol};
use crate::database::Database;
use crate::error::{AppError, AppResult};

const MAX_FAILOVER_HOPS: usize = 3;
/// Remaining-quota 429 is usually a Cloud Code SKU/RPM limit. Try this account
/// plus one other; do not walk the rest of the pool (that cools every number).
const MAX_SKU_RATE_LIMIT_ACCOUNT_HOPS: usize = 2;
/// Local token-bucket / concurrency busy: try one other account, then 429.
const MAX_LOCAL_LIMITER_HOPS: usize = 2;
const CS_SUBAGENT_HEADER: &str = "x-cs-subagent";
const MAX_MODEL_LEVEL_ATTEMPTS: usize = 3;
const SUBAGENT_NON_STREAM_RETRY_DEADLINE: Duration = Duration::from_secs(8);
const SUBAGENT_STREAM_RETRY_DEADLINE: Duration = Duration::from_secs(15);
const MAIN_NON_STREAM_RETRY_DEADLINE: Duration = Duration::from_secs(25);
const MAIN_STREAM_RETRY_DEADLINE: Duration = Duration::from_secs(45);
const SHORT_PAYLOAD_MAX_BYTES: usize = 12_000;
const UPSTREAM_500_RETRY_BACKOFF: Duration = Duration::from_millis(250);

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
    let session_key = session_key_from_headers(&headers);
    let mapped = match openai_to_gemini_request(&payload, session_key.as_deref()) {
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
    let session_key = session_key_from_headers(&headers);
    let mapped = match responses_to_gemini_request(&payload, session_key.as_deref()) {
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
    let is_subagent = should_treat_as_subagent(headers, &model, &request, stream);
    let retry_deadline = retry_deadline_for(stream, is_subagent, started);
    let session_key = session_key_from_headers(headers);
    let model_chain: Vec<String> = model_catalog::gemini_level_fallback_chain(&model)
        .into_iter()
        .take(MAX_MODEL_LEVEL_ATTEMPTS)
        .collect();
    log::info!(
        "Antigravity dispatch model={model} chain={} stream={stream}",
        model_chain.join(">")
    );
    let mut exclude: Vec<String> = Vec::new();
    let mut exclude_labels: Vec<String> = Vec::new();
    let mut last_error = String::from("upstream failed");
    let mut last_fail_status: u16 = 0;
    let mut last_attempted_model = model.clone();
    let mut upstream_calls: u32 = 0;
    let mut models_tried: Vec<String> = Vec::new();
    let mut limiter_hops: usize = 0;

    for hop in 0..MAX_FAILOVER_HOPS {
        if past_retry_deadline(retry_deadline) {
            last_error = format!(
                "retry deadline exceeded after {} upstream calls",
                upstream_calls
            );
            last_fail_status = 429;
            break;
        }
        let selected = if hop == 0 {
            state.pool.select_async(None, session_key.as_deref()).await
        } else {
            let failed = exclude.last().cloned().unwrap_or_default();
            if failed.is_empty() {
                break;
            }
            let status = if last_fail_status == 0 {
                500
            } else {
                last_fail_status
            };
            state
                .pool
                .rotate_after_failure_async(&failed, status, session_key.as_deref(), &exclude)
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
        let account_email = if account.email.trim().is_empty() {
            account.id.clone()
        } else {
            account.email.clone()
        };
        log::info!("Antigravity hop={hop} account={account_email}");
        exclude_labels.push(account_email.clone());

        let acquire = state
            .limiter
            .acquire_until(&account.id, is_subagent, retry_deadline)
            .await;
        let limiter_permit = match acquire {
            AcquireOutcome::Granted(permit) => permit,
            AcquireOutcome::Busy => {
                limiter_hops += 1;
                last_fail_status = 429;
                last_error = "local rate limit: account busy".into();
                log::warn!(
                    "Antigravity limiter {last_error} on {account_email}; hop={limiter_hops}/{MAX_LOCAL_LIMITER_HOPS}"
                );
                if limiter_hops >= MAX_LOCAL_LIMITER_HOPS || past_retry_deadline(retry_deadline) {
                    break;
                }
                continue;
            }
            AcquireOutcome::RateLimited => {
                limiter_hops += 1;
                last_fail_status = 429;
                last_error = "local rate limit: account hot".into();
                log::warn!(
                    "Antigravity limiter {last_error} on {account_email}; hop={limiter_hops}/{MAX_LOCAL_LIMITER_HOPS}"
                );
                if limiter_hops >= MAX_LOCAL_LIMITER_HOPS || past_retry_deadline(retry_deadline) {
                    break;
                }
                continue;
            }
        };

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
                last_error = error.to_string();
                last_fail_status = 500;
                if !should_cool_account_on_generate_error(&error) {
                    log::warn!(
                        "Antigravity project lookup network error on {account_email}; not cooling account: {last_error}"
                    );
                    break;
                }
                let _ = account_store().mark_cooldown(&account.id, 45, &last_error);
                continue;
            }
        };

        let mut current_model = model.clone();
        let mut model_idx = 0usize;
        let upstream = 'levels: loop {
            if past_retry_deadline(retry_deadline) {
                last_error = format!(
                    "retry deadline exceeded after {} upstream calls",
                    upstream_calls
                );
                last_fail_status = 429;
                break 'levels Err(());
            }
            current_model = model_chain
                .get(model_idx)
                .cloned()
                .unwrap_or_else(|| model.clone());
            if !models_tried.iter().any(|item| item == &current_model) {
                models_tried.push(current_model.clone());
            }
            last_attempted_model = current_model.clone();
            let wrapped = wrap_v1internal(&project_id, &current_model, request.clone());
            // 500/502/504 are usually transient blips: retry the same account with
            // a short backoff before burning a rotation (503/529 already got
            // per-host backoff inside `generate`).
            let mut server_error_retry = 0u32;
            let attempt = loop {
                if past_retry_deadline(retry_deadline) {
                    break Err(AppError::Other("retry deadline exceeded".into()));
                }
                let Some(budget) = remaining_until(retry_deadline) else {
                    break Err(AppError::Other("retry deadline exceeded".into()));
                };
                upstream_calls += 1;
                match tokio::time::timeout(
                    budget,
                    state.upstream.generate(&access_token, &wrapped, stream),
                )
                .await
                {
                    Ok(Ok(response)) => {
                        let status = response.status();
                        if matches!(status.as_u16(), 500 | 502 | 504) && server_error_retry < 1 {
                            server_error_retry += 1;
                            log::warn!(
                                "Antigravity upstream {status}; same-account retry {server_error_retry}/1"
                            );
                            tokio::time::sleep(UPSTREAM_500_RETRY_BACKOFF).await;
                            continue;
                        }
                        break Ok(response);
                    }
                    Ok(Err(error)) => break Err(error),
                    Err(_) => break Err(AppError::Other("retry deadline exceeded".into())),
                }
            };
            let response = match attempt {
                Ok(response) => response,
                Err(error) => {
                    last_error = error.to_string();
                    last_fail_status = if last_error.contains("deadline") {
                        429
                    } else if last_error.starts_with("network/") {
                        504
                    } else {
                        502
                    };
                    if !should_cool_account_on_generate_error(&error) {
                        log::warn!(
                            "Antigravity generate network error on {account_email}; not cooling account: {last_error}"
                        );
                        break 'levels Err(());
                    }
                    if last_fail_status == 502 {
                        let _ = account_store().mark_cooldown(&account.id, 20, &last_error);
                    }
                    break 'levels Err(());
                }
            };
            if response.status().as_u16() == 429 && model_idx + 1 < model_chain.len() {
                if remaining_until(retry_deadline).is_none() {
                    last_error = format!(
                        "retry deadline exceeded after {} upstream calls",
                        upstream_calls
                    );
                    last_fail_status = 429;
                    break 'levels Err(());
                }
                let next = &model_chain[model_idx + 1];
                let text = response.text().await.unwrap_or_default();
                last_error = format!(
                    "upstream 429: {}",
                    text.trim().chars().take(240).collect::<String>()
                );
                log::warn!(
                    "Antigravity {current_model} → 429 RESOURCE_EXHAUSTED; retrying {next} on the same account"
                );
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                model_idx += 1;
                continue 'levels;
            }
            break 'levels Ok(response);
        };
        let upstream = match upstream {
            Ok(response) => response,
            Err(()) => {
                if last_error.starts_with("network/") {
                    last_fail_status = 504;
                    break;
                }
                continue;
            }
        };

        let status = upstream.status();
        if matches!(status.as_u16(), 400 | 422) {
            let text = upstream.text().await.unwrap_or_default();
            let clipped: String = text.trim().chars().take(400).collect();
            last_error = if clipped.is_empty() {
                format!("upstream {status}")
            } else {
                format!("upstream {status}: {clipped}")
            };
            last_fail_status = status.as_u16();
            log::warn!(
                "Antigravity request-body error {status} on {account_email} model={last_attempted_model}; not rotating: {last_error}"
            );
            let _ = usage_log::insert_request(
                &state.db,
                exclude_labels.last().map(String::as_str),
                &last_attempted_model,
                Some(status.as_u16() as i64),
                started,
                protocol,
                stream,
                Some("upstream"),
                Some(&last_error),
            );
            return error_json(status, &last_error);
        }
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
            last_fail_status = status.as_u16();
            if status.as_u16() == 429 {
                state.limiter.note_upstream_rate_limited(&account.id);
                let kind = classify_rate_limit_body(&text);
                let rotate_pool = crate::antigravity::pool::should_rotate_pool_on_429(&account);
                let cooldown = if rotate_pool {
                    crate::antigravity::pool::rate_limit_cooldown_secs(retry_after)
                } else {
                    crate::antigravity::pool::sku_rate_limit_cooldown_secs(retry_after)
                };
                let _ = account_store().mark_cooldown(&account.id, cooldown, &last_error);
                if rotate_pool {
                    log::warn!(
                        "Antigravity 429 class={} hop={hop} account={account_email} model={last_attempted_model} with empty quota snapshot; rotating",
                        kind.label()
                    );
                } else if hop + 1 >= MAX_SKU_RATE_LIMIT_ACCOUNT_HOPS {
                    // Same-account model fallback already ran. One extra account
                    // covers SKU capacity on another number; more hops cool the pool.
                    log::warn!(
                        "Antigravity 429 class={} hop={hop} account={account_email} model={last_attempted_model} with remaining quota; not walking the rest of the pool",
                        kind.label()
                    );
                    break;
                } else {
                    log::warn!(
                        "Antigravity 429 class={} hop={hop} account={account_email} model={last_attempted_model} with remaining quota; trying one more account",
                        kind.label()
                    );
                }
            } else if status.as_u16() == 403 {
                let _ = account_store().mark_forbidden_403(&account.id, &last_error);
            } else {
                let _ = account_store().mark_cooldown(&account.id, 180, &last_error);
            }
            continue;
        }
        if is_request_body_status(status.as_u16()) {
            let text = upstream.text().await.unwrap_or_default();
            let clipped: String = text.trim().chars().take(400).collect();
            last_error = if clipped.is_empty() {
                format!("upstream {status}")
            } else {
                format!("upstream {status}: {clipped}")
            };
            log::warn!(
                "Antigravity request-body error {status} on {account_email} model={last_attempted_model}: {last_error}"
            );
            let _ = usage_log::insert_request(
                &state.db,
                exclude_labels.last().map(String::as_str),
                &last_attempted_model,
                Some(status.as_u16() as i64),
                started,
                protocol,
                stream,
                Some("upstream"),
                Some(&last_error),
            );
            let client_status =
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_REQUEST);
            return error_json(client_status, &last_error);
        }
        if !status.is_success() {
            let text = upstream.text().await.unwrap_or_default();
            last_error = format!("upstream {status}: {text}");
            last_fail_status = status.as_u16();
            let _ = account_store().mark_cooldown(&account.id, 15, &last_error);
            continue;
        }

        state.pool.note_success(&account.id);
        state.limiter.note_upstream_success(&account.id);
        let account_label = if account.email.trim().is_empty() {
            account.id.as_str()
        } else {
            account.email.as_str()
        };
        let log_id = usage_log::insert_request(
            &state.db,
            Some(account_label),
            &current_model,
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
                current_model.clone(),
                protocol,
                state.db.clone(),
                log_id,
                Some(account_label.to_string()),
                session_key.clone(),
                thoughts_allowed,
                tool_params,
                limiter_permit,
            )
            .await;
        }
        drop(limiter_permit);
        return match upstream.json::<Value>().await {
            Ok(value) => {
                let gemini = unwrap_v1internal(&value);
                if let Some(id) = log_id.as_deref() {
                    usage_log::update_usage_from_gemini(&state.db, id, Some(account_label), &gemini);
                }
                match protocol {
                    WireProtocol::Anthropic => {
                        Json(gemini_to_anthropic_response(
                            &current_model,
                            &gemini,
                            session_key.as_deref(),
                            thoughts_allowed,
                            &tool_params,
                        ))
                        .into_response()
                    }
                    WireProtocol::OpenAiChat => {
                        Json(gemini_to_openai_response(
                            &current_model,
                            &gemini,
                            session_key.as_deref(),
                            &tool_params,
                        ))
                            .into_response()
                    }
                    WireProtocol::OpenAiResponses => {
                        Json(gemini_to_responses_response(
                            &current_model,
                            &gemini,
                            session_key.as_deref(),
                            &tool_params,
                        ))
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
    let status = client_status_from_dispatch(last_fail_status, &clipped_error);
    let error_category = dispatch_error_category(&clipped_error);
    if status == StatusCode::TOO_MANY_REQUESTS {
        log::warn!(
            "Antigravity dispatch failed 429 after {upstream_calls} upstream calls in {}ms; accounts=[{}] models=[{}]",
            started.elapsed().as_millis(),
            exclude_labels.join(","),
            models_tried.join(",")
        );
    } else if error_category == "network" {
        log::warn!(
            "Antigravity dispatch failed network after {upstream_calls} upstream calls in {}ms; accounts=[{}] models=[{}]: {clipped_error}",
            started.elapsed().as_millis(),
            exclude_labels.join(","),
            models_tried.join(",")
        );
    }
    let _ = usage_log::insert_request(
        &state.db,
        exclude_labels.last().map(String::as_str),
        &last_attempted_model,
        Some(status.as_u16() as i64),
        started,
        protocol,
        stream,
        Some(error_category),
        Some(&clipped_error),
    );
    error_json(status, &clipped_error)
}

async fn ensure_project_id(
    state: &GatewayState,
    access_token: &str,
    account_id: &str,
    existing: Option<&str>,
) -> AppResult<String> {
    if let Some(project) = existing.filter(|value| !value.trim().is_empty()) {
        return Ok(project.to_string());
    }
    let project = state.upstream.fetch_project_id(access_token).await?;
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
    limiter_permit: LimiterPermit,
) -> Response {
    let byte_stream = upstream.bytes_stream().boxed();
    let responses_encoder = matches!(protocol, WireProtocol::OpenAiResponses)
        .then(|| ResponsesStreamEncoder::new(&model, tool_params.clone(), session_key.clone()));
    let stream = futures_util::stream::unfold(
        StreamState {
            upstream: byte_stream,
            buffer: SseLineBuffer::new(),
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
            _limiter_permit: limiter_permit,
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
                        state.buffer.push(&bytes);
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

/// SSE line buffer that never decodes a TCP chunk with `from_utf8_lossy`.
/// Gemini Chinese is 3-byte UTF-8; splitting a character across chunks and
/// replacing it with U+FFFD is the usual “中文乱码” seen only on this path.
struct SseLineBuffer {
    bytes: Vec<u8>,
}

impl SseLineBuffer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
    }

    fn take_line(&mut self) -> Option<String> {
        let idx = self.bytes.iter().position(|&b| b == b'\n')?;
        let mut line: Vec<u8> = self.bytes.drain(..=idx).collect();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        match String::from_utf8(line) {
            Ok(text) => Some(text),
            Err(_) => Some(String::new()),
        }
    }

    fn finish(&mut self) {
        if !self.bytes.is_empty() && self.bytes.last() != Some(&b'\n') {
            self.bytes.push(b'\n');
        }
    }
}

struct StreamState {
    upstream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
    buffer: SseLineBuffer,
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
    _limiter_permit: LimiterPermit,
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
        loop {
            let line = self.buffer.take_line()?;
            if line.is_empty() {
                continue;
            }
            let data = line
                .strip_prefix("data:")
                .map(str::trim)
                .unwrap_or(line.as_str());
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let gemini = unwrap_v1internal(&value);
            self.note_usage(&gemini);
            let mapped = match self.protocol {
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
                        out.extend_from_slice(&anthropic_sse(&event));
                    }
                    out
                }
                WireProtocol::OpenAiChat => sse_data(&gemini_to_openai_sse_chunk(
                    &self.model,
                    &gemini,
                    self.session_key.as_deref(),
                    &self.tool_params,
                ))
                .to_vec(),
                WireProtocol::OpenAiResponses => {
                    let Some(encoder) = self.responses_encoder.as_mut() else {
                        continue;
                    };
                    encoder.encode_gemini_chunk(&gemini)
                }
            };
            if mapped.is_empty() {
                continue;
            }
            return Some(Bytes::from(mapped));
        }
    }

    fn finish_events(&mut self) -> Bytes {
        self.buffer.finish();
        let mut out = Vec::new();
        while let Some(event) = self.take_line_event() {
            out.extend_from_slice(&event);
        }
        match self.protocol {
            WireProtocol::OpenAiChat => {
                let mut trailer = String::new();
                if self.last_input > 0 || self.last_output > 0 {
                    let chunk = crate::antigravity::map::openai::openai_usage_sse_chunk(
                        &self.model,
                        self.last_input,
                        self.last_output,
                    );
                    trailer.push_str(&format!("data: {chunk}\n\n"));
                }
                trailer.push_str("data: [DONE]\n\n");
                out.extend_from_slice(trailer.as_bytes());
            }
            WireProtocol::OpenAiResponses => {
                if let Some(encoder) = self.responses_encoder.as_mut() {
                    out.extend_from_slice(&encoder.finish());
                }
            }
            WireProtocol::Anthropic => {
                let mut anthropic = self.anthropic.take().unwrap_or_default();
                let events = anthropic.finish_events(&self.model);
                self.anthropic = Some(anthropic);
                for event in events {
                    out.extend_from_slice(&anthropic_sse(&event));
                }
            }
        }
        Bytes::from(out)
    }
}

fn sse_data(value: &Value) -> Bytes {
    Bytes::from(format!("data: {}\n\n", value))
}

/// Anthropic clients (Pi, official SDK) dispatch on the SSE `event:` field,
/// not `data.type`. Omitting it makes Pi skip every frame and fail with
/// "stream ended without a stop reason".
fn anthropic_sse(value: &Value) -> Bytes {
    let event = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    Bytes::from(format!("event: {event}\ndata: {value}\n\n"))
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

fn is_subagent_request(headers: &HeaderMap) -> bool {
    headers
        .get(CS_SUBAGENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let trimmed = value.trim();
            !trimmed.is_empty() && trimmed != "0" && !trimmed.eq_ignore_ascii_case("false")
        })
        .unwrap_or(false)
}

fn retry_deadline_for(stream: bool, is_subagent: bool, started: Instant) -> Instant {
    let budget = match (is_subagent, stream) {
        (true, false) => SUBAGENT_NON_STREAM_RETRY_DEADLINE,
        (true, true) => SUBAGENT_STREAM_RETRY_DEADLINE,
        (false, false) => MAIN_NON_STREAM_RETRY_DEADLINE,
        (false, true) => MAIN_STREAM_RETRY_DEADLINE,
    };
    started + budget
}

fn past_retry_deadline(deadline: Instant) -> bool {
    Instant::now() >= deadline
}

fn remaining_until(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| *duration > Duration::from_millis(50))
}

fn should_treat_as_subagent(
    headers: &HeaderMap,
    model: &str,
    request: &Value,
    stream: bool,
) -> bool {
    if is_subagent_request(headers) {
        return true;
    }
    if stream {
        return false;
    }
    let lower = model.trim().to_ascii_lowercase();
    let sonnet_role = lower.contains("sonnet") || lower == "claude-sonnet-5";
    sonnet_role && is_short_anthropic_payload(request)
}

fn is_short_anthropic_payload(request: &Value) -> bool {
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.len() > 4 {
        return false;
    }
    serde_json::to_string(request)
        .map(|serialized| serialized.len() <= SHORT_PAYLOAD_MAX_BYTES)
        .unwrap_or(false)
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

/// Map the last upstream/pool error to a client status.
///
/// Claude Desktop treats HTTP 502 as a server fault and retries immediately
/// (1–3s), which turns a Cloud Code 429 into a request storm. Preserve 429 so
/// the client (and local proxy) can back off.
fn is_request_body_status(status: u16) -> bool {
    matches!(status, 400 | 422)
}

fn should_cool_account_on_generate_error(error: &AppError) -> bool {
    !matches!(error, AppError::Network(_))
}

fn dispatch_error_category(last_error: &str) -> &'static str {
    if last_error.starts_with("network/") {
        "network"
    } else {
        "upstream"
    }
}

fn client_status_from_dispatch(last_fail_status: u16, last_error: &str) -> StatusCode {
    let from_error = client_status_from_upstream_error(last_error);
    if last_fail_status == 429 || from_error == StatusCode::TOO_MANY_REQUESTS {
        return StatusCode::TOO_MANY_REQUESTS;
    }
    if last_error.starts_with("network/") || last_fail_status == 504 {
        return StatusCode::GATEWAY_TIMEOUT;
    }
    if matches!(last_fail_status, 400 | 401 | 403 | 422) {
        return StatusCode::from_u16(last_fail_status).unwrap_or(from_error);
    }
    from_error
}

fn client_status_from_upstream_error(last_error: &str) -> StatusCode {
    let lower = last_error.to_ascii_lowercase();
    if looks_like_rate_limit(&lower) {
        StatusCode::TOO_MANY_REQUESTS
    } else if lower.starts_with("network/") {
        StatusCode::GATEWAY_TIMEOUT
    } else if looks_like_http_status(&lower, 401) || lower.contains("unauthenticated") {
        StatusCode::UNAUTHORIZED
    } else if looks_like_http_status(&lower, 403)
        || lower.contains("forbidden")
        || lower.contains("permission denied")
    {
        StatusCode::FORBIDDEN
    } else if looks_like_http_status(&lower, 400)
        || looks_like_http_status(&lower, 422)
        || lower.contains("thought_signature")
        || lower.contains("invalid_argument")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn looks_like_rate_limit(lower: &str) -> bool {
    lower.contains("retry deadline")
        || lower.contains("local rate limit")
        || lower.contains("resource_exhausted")
        || lower.contains("resource has been exhausted")
        || lower.contains("too many requests")
        || looks_like_http_status(lower, 429)
}

/// Match `upstream 401` / ` 401:` so `34018ms` is not treated as HTTP 401.
fn looks_like_http_status(lower: &str, code: u16) -> bool {
    let token = code.to_string();
    let prefixed = format!("upstream {token}");
    if lower.contains(&prefixed) {
        return true;
    }
    let padded = format!(" {token}");
    let Some(index) = lower.find(&padded) else {
        return false;
    };
    let after = index + padded.len();
    let next = lower.as_bytes().get(after).copied();
    matches!(next, None | Some(b' ') | Some(b':') | Some(b',') | Some(b'}') | Some(b'"'))
        && lower.contains("upstream")
}

fn error_type_for_status(status: StatusCode) -> &'static str {
    match status.as_u16() {
        429 => "rate_limit_error",
        401 => "authentication_error",
        403 => "permission_error",
        400 | 422 => "invalid_request_error",
        504 => "timeout_error",
        _ => "antigravity_gateway_error",
    }
}

fn retry_after_header(status: StatusCode, message: &str) -> Option<&'static str> {
    match status.as_u16() {
        429 if looks_like_rate_limit(&message.to_ascii_lowercase())
            && (message.contains("local rate") || message.contains("deadline")) =>
        {
            Some("8")
        }
        429 => Some("45"),
        504 => Some("5"),
        _ => None,
    }
}

fn error_json(status: StatusCode, message: &str) -> Response {
    let mut response = (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "message": message,
                "type": error_type_for_status(status),
            }
        })),
    )
        .into_response();
    if let Some(secs) = retry_after_header(status, message) {
        if let Ok(value) = HeaderValue::from_str(secs) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_sse_includes_event_field_for_pi() {
        let event = json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 1 }
        });
        let encoded = String::from_utf8(anthropic_sse(&event).to_vec()).unwrap();
        assert!(encoded.starts_with("event: message_delta\n"));
        assert!(encoded.contains("data: {"));
        assert!(encoded.contains("\"stop_reason\":\"end_turn\""));
        assert!(encoded.ends_with("\n\n"));
    }

    #[test]
    fn client_status_preserves_upstream_429() {
        assert_eq!(
            client_status_from_upstream_error(
                "upstream 429 Too Many Requests: Resource has been exhausted"
            ),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            client_status_from_upstream_error("RESOURCE_EXHAUSTED"),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            client_status_from_upstream_error("retry deadline exceeded after 1 upstream calls"),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            client_status_from_upstream_error("local rate limit: account hot"),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            client_status_from_upstream_error("upstream 403: permission denied"),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            client_status_from_upstream_error("invalid upstream json"),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            client_status_from_upstream_error(
                "upstream 400 Bad Request: Function call is missing a thought_signature"
            ),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            client_status_from_upstream_error("network/connect: 3 hosts unreachable in 1200ms"),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            client_status_from_upstream_error(
                "network/connect: 3 hosts unreachable in 34018ms via http://127.0.0.1:17891"
            ),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            client_status_from_dispatch(
                429,
                "retry deadline exceeded after 4 upstream calls"
            ),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            client_status_from_dispatch(504, "network/connect: 3 hosts unreachable in 15005ms"),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn main_session_retry_budget_is_longer_than_subagent() {
        let started = Instant::now();
        let main = retry_deadline_for(true, false, started);
        let sub = retry_deadline_for(true, true, started);
        assert!(main > sub);
        assert_eq!(main.duration_since(started), MAIN_STREAM_RETRY_DEADLINE);
        assert_eq!(sub.duration_since(started), SUBAGENT_STREAM_RETRY_DEADLINE);
    }

    #[test]
    fn network_errors_are_not_account_faults() {
        assert!(!should_cool_account_on_generate_error(&AppError::Network(
            "network/connect: 3 hosts unreachable".into()
        )));
        assert!(should_cool_account_on_generate_error(&AppError::Other(
            "upstream 502: backend unavailable".into()
        )));
        assert_eq!(
            dispatch_error_category("network/timeout: 3 hosts unreachable in 800ms"),
            "network"
        );
        assert_eq!(
            dispatch_error_category("upstream 502: backend unavailable"),
            "upstream"
        );
        assert!(is_request_body_status(400));
        assert!(is_request_body_status(422));
        assert!(!is_request_body_status(502));
    }

    #[test]
    fn sse_line_buffer_reassembles_split_chinese_utf8() {
        let line = "data: {\"text\":\"你好世界\"}\n";
        let bytes = line.as_bytes();
        let 你 = "你".len();
        let split_at = "data: {\"text\":\"".len() + 1; // mid-character of 你
        assert!(split_at < "data: {\"text\":\"你好".len());
        assert!(!bytes[..split_at].is_empty());
        assert_ne!(你, 1);

        let mut buffer = SseLineBuffer::new();
        buffer.push(&bytes[..split_at]);
        assert!(buffer.take_line().is_none(), "incomplete line must wait");
        buffer.push(&bytes[split_at..]);
        let got = buffer.take_line().expect("complete line");
        assert_eq!(got, "data: {\"text\":\"你好世界\"}");
        assert!(!got.contains('\u{FFFD}'));
    }

    #[test]
    fn sse_line_buffer_skips_empty_separator_and_keeps_next_frame() {
        let mut buffer = SseLineBuffer::new();
        buffer.push(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n");
        assert_eq!(buffer.take_line().as_deref(), Some("data: {\"a\":1}"));
        assert_eq!(buffer.take_line().as_deref(), Some(""));
        assert_eq!(buffer.take_line().as_deref(), Some("data: {\"b\":2}"));
    }

    #[test]
    fn lossy_utf8_on_split_chinese_would_corrupt() {
        let text = "你好";
        let bytes = text.as_bytes();
        let split = 1; // inside 你
        let corrupted = format!(
            "{}{}",
            String::from_utf8_lossy(&bytes[..split]),
            String::from_utf8_lossy(&bytes[split..])
        );
        assert_ne!(corrupted, text);
        assert!(corrupted.contains('\u{FFFD}'));
    }
}
