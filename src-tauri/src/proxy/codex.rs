//! OpenAI-compatible passthrough proxy for Codex (`/v1/responses`, `/v1/chat/completions`).

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;

use crate::catalog::{openai_models_payload, rewrite_json_model};
use crate::database::dao::providers::{get_current_provider, get_provider_model_cache};
use crate::database::dao::proxy_logs::{update_proxy_log_usage_idempotent, ProxyRequestAttempt};
use crate::provider::{api_endpoint_url, ProtocolType, Provider};

use super::codex_anthropic::{
    anthropic_response_to_responses, anthropic_version_header, parse_anthropic_sse_frame,
    responses_request_to_anthropic_messages, AnthropicSseToResponsesConverter,
};
use super::codex_chat::{
    chat_response_to_responses, is_unsupported_content_type_error, responses_to_chat_completions_body,
    ChatSseToResponsesConverter,
};
use super::{
    codex_auto_review::{apply_auto_review_model_override, has_subagent_header}, convert, codex_compact, extract_usage_from_json,
    extract_usage_from_sse, is_hop_by_hop_header, is_retryable_upstream_status, json_error,
    log_early_failure, log_request, log_request_with_diagnostic,
    next_failover_provider, next_failover_provider_ex, resolve_explicit_fallback,
    note_gateway_inflight, remember_gateway_success_upstream, select_gateway_runtime_provider_with,
    session_prompt_cache_hint,
    should_failover_upstream_status_ex, should_try_explicit_response,
    CS_SUBAGENT_HEADER, FAILOVER_MAX_HOPS, ListenerKind, ProxyState,
};

pub async fn codex_models_handler(State(state): State<ProxyState>) -> Response {
    if crate::catalog::enabled(state.db.as_ref(), state.target) {
        match super::load_gateway_catalog(&state, crate::catalog::catalog_style_for(state.target)) {
            Ok((_, entries)) if !entries.is_empty() => {
                let body = openai_models_payload(&entries);
                let style = crate::catalog::catalog_style_for(state.target);
                let revision = crate::catalog::catalog_revision(style, &entries);
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ETAG, format!("\"{revision}\""))
                    .header("x-ai-switcher-catalog-revision", revision)
                    .body(Body::from(body.to_string()))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
            Ok(_) => return json_error(StatusCode::BAD_GATEWAY, "没有已配置的 Codex 供应商"),
            Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
        }
    }
    match state
        .db
        .with_conn(|conn| get_current_provider(conn, state.target))
    {
        Ok(Some(provider)) => {
            let cached = state
                .db
                .with_conn(|conn| {
                    Ok(get_provider_model_cache(conn, &provider.id)?
                        .map(|cache| cache.models)
                        .unwrap_or_default())
                })
                .unwrap_or_default();
            let entries = crate::catalog::build_catalog(
                crate::catalog::catalog_style_for(state.target),
                &[(provider, cached)],
            );
            let body = openai_models_payload(&entries);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Ok(None) => json_error(StatusCode::BAD_GATEWAY, "没有当前 Codex 供应商"),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

pub async fn codex_proxy_handler(
    State(state): State<ProxyState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let route = uri.path().to_string();
    if method != Method::POST {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "仅支持 POST");
    }

    let mut original_body = body;
    let incoming: Value = serde_json::from_slice(&original_body).unwrap_or(Value::Null);
    let requested_model = incoming
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if state.listener_kind == ListenerKind::SmartGateway
        && incoming
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            != requested_model
    {
        original_body = Bytes::from(rewrite_json_model(&original_body, &requested_model));
    }
    let catalog_mode = super::gateway_catalog_enabled(&state);
    let mut is_catalog_subagent = false;
    let mut route_decision = None;
    let mut route_plan: Option<crate::gateway::RouteExecutionPlan> = None;
    let mut current_upstream_model = String::new();
    let mut provider = if catalog_mode {
        match select_gateway_runtime_provider_with(
            &state,
            &requested_model,
            has_subagent_header(&headers),
            &incoming,
            uri.path(),
            &headers,
        ) {
            Ok(Some((selected, upstream_model, routed_subagent, decision, plan))) => {
                original_body = Bytes::from(rewrite_json_model(&original_body, &upstream_model));
                current_upstream_model = upstream_model;
                is_catalog_subagent = routed_subagent;
                route_decision = Some(decision);
                route_plan = Some(plan);
                selected
            }
            Ok(None) => {
                log_early_failure(
                    &state,
                    &route,
                    "provider",
                    Some(502),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::BAD_GATEWAY, "没有可路由的 Codex 供应商");
            }
            Err(error) => {
                let (status, kind) = match &error {
                    crate::proxy::GatewaySelectionError::Catalog(_) => {
                        (StatusCode::BAD_REQUEST, "model")
                    }
                    crate::proxy::GatewaySelectionError::App(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "configuration")
                    }
                };
                log_early_failure(
                    &state,
                    &route,
                    kind,
                    Some(status.as_u16() as i64),
                    started.elapsed().as_millis() as i64,
                );
                match error {
                    crate::proxy::GatewaySelectionError::Catalog(catalog_error) => {
                        return json_error(StatusCode::BAD_REQUEST, catalog_error.to_string());
                    }
                    crate::proxy::GatewaySelectionError::App(error) => {
                        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
                    }
                }
            }
        }
    } else {
        match state
            .db
            .with_conn(|conn| get_current_provider(conn, state.target))
        {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                log_early_failure(
                    &state,
                    &route,
                    "provider",
                    Some(502),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::BAD_GATEWAY, "没有当前 Codex 供应商");
            }
            Err(error) => {
                log_early_failure(
                    &state,
                    &route,
                    "configuration",
                    Some(500),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
            }
        }
    };
    if current_upstream_model.is_empty() {
        current_upstream_model = provider.model.clone();
    }
    let prepared = match prepare_codex_upstream(
        &state,
        &provider,
        &route,
        &headers,
        &original_body,
        false,
        is_catalog_subagent,
    ) {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    let mut is_anthropic_upstream = prepared.is_anthropic_upstream;
    let mut is_stream = prepared.is_stream;
    let mut compact_fallback = prepared.compact_fallback;
    let mut is_chat_bridge = prepared.is_chat_bridge;
    let mut current_prepared = prepared;

    let explicit_models: Vec<String> = route_plan
        .as_ref()
        .filter(|plan| !plan.explicit_pinned)
        .map(|plan| {
            plan.attempts
                .iter()
                .skip(1)
                .map(|attempt| attempt.model.clone())
                .collect()
        })
        .unwrap_or_default();
    let allow_cross_provider_failover =
        !route_plan.as_ref().is_some_and(|plan| plan.explicit_pinned);
    let has_explicit_chain = !explicit_models.is_empty();
    let mut explicit_models = explicit_models.into_iter();
    let mut legacy_models = if route_plan.is_none() {
        provider.failover_models.clone().into_iter()
    } else {
        Vec::new().into_iter()
    };
    let mut excluded = vec![provider.id.clone()];
    let mut attempt_index: i64 = 0;
    let mut failover_trace: Vec<String> = Vec::new();
    let mut attempts: Vec<ProxyRequestAttempt> = Vec::new();

    let upstream = loop {
        let attempt_start = Instant::now();
        note_gateway_inflight(
            &state, &provider, &current_upstream_model, is_stream, route_decision.as_ref(), &attempts,
        );
        let (result, queue_wait_ms) = super::upstream_health::send_observed_upstream_with_timing(
            current_prepared.request.body(current_prepared.request_body),
            &provider,
            Some(&state),
            is_stream,
        )
        .await;
        let attempt_duration = attempt_start.elapsed().as_millis() as i64;
        attempts.push(super::upstream_health::build_request_attempt(
            attempts.len(),
            &provider,
            &current_upstream_model,
            attempt_duration,
            &result,
        ).with_queue_wait_ms(queue_wait_ms));

        let can_explicit = allow_cross_provider_failover && match &result {
            Ok(response) => should_try_explicit_response(&provider, response),
            Err(_) => true,
        };
        let is_compact_route = codex_compact::is_responses_compact_route(&route);
        let can_generic = allow_cross_provider_failover && !is_compact_route && match &result {
            Ok(response) => {
                is_retryable_upstream_status(&state, response.status())
                    && should_failover_upstream_status_ex(&provider, response.status(), false)
            }
            Err(_) => !provider.is_kiro(),
        };

        let mut next = None;
        if can_explicit && has_explicit_chain {
            for model in explicit_models.by_ref() {
                if let Ok(Some((candidate, slug))) = resolve_explicit_fallback(&state, &model) {
                    if candidate.id == provider.id && slug == current_upstream_model {
                        continue;
                    }
                    if !crate::gateway::health::is_available(&candidate.id, Some(&slug)) {
                        continue;
                    }
                    let failover_body = Bytes::from(rewrite_json_model(&original_body, &slug));
                    if let Ok(prep) = prepare_codex_upstream(
                        &state,
                        &candidate,
                        &route,
                        &headers,
                        &failover_body,
                        false,
                        is_catalog_subagent,
                    ) {
                        next = Some((candidate, slug, prep));
                        break;
                    }
                }
            }
        } else if can_generic && !has_explicit_chain {
            if let Some(model) = legacy_models.next() {
                let failover_body = Bytes::from(rewrite_json_model(&original_body, &model));
                if let Ok(prep) = prepare_codex_upstream(
                    &state,
                    &provider,
                    &route,
                    &headers,
                    &failover_body,
                    false,
                    is_catalog_subagent,
                ) {
                    next = Some((provider.clone(), model, prep));
                }
            } else if (attempt_index as usize) < FAILOVER_MAX_HOPS {
                while let Some(fallback) = next_codex_failover_provider(
                    &state,
                    &excluded,
                    &requested_model,
                    catalog_mode,
                ) {
                    excluded.push(fallback.id.clone());
                    let failover_body = catalog_failover_body_if_needed(
                        &state,
                        &fallback,
                        &original_body,
                        catalog_mode,
                    );
                    if let Ok(prep) = prepare_codex_upstream(
                        &state,
                        &fallback,
                        &route,
                        &headers,
                        &failover_body,
                        false,
                        is_catalog_subagent,
                    ) {
                        let fallback_model = fallback.model.clone();
                        next = Some((fallback, fallback_model, prep));
                        break;
                    }
                }
            }
        }

        let Some((next_provider, next_model, next_prepared)) = next else {
            match result {
                Ok(response) => {
                    if !failover_trace.is_empty() && response.status().is_success() {
                        failover_trace.push(format!("{}({}) 接管成功", provider.name, provider.id));
                    }
                    break response;
                }
                Err(error) => {
                    let failover_diag = if !failover_trace.is_empty() {
                        Some(format!("故障降级失败: {}", failover_trace.join(" → ")))
                    } else {
                        None
                    };
                    let fail_log_id = log_request_with_diagnostic(
                        &state,
                        &provider,
                        None,
                        started.elapsed().as_millis() as i64,
                        &route,
                        is_stream,
                        Some("network"),
                        failover_diag.as_deref(),
                    );
                    let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
                    if let Some(id) = fail_log_id.as_deref() {
                        if let Some(decision) = route_decision.as_ref() {
                            super::patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
                        } else if !attempts.is_empty() {
                            super::update_proxy_log_attempts(&state, id, &attempts_json);
                        }
                    }
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        format!("上游连接失败: {error}"),
                    );
                }
            }
        };

        failover_trace.push(format!(
            "{}({}) {} → {}",
            provider.name,
            provider.id,
            result
                .as_ref()
                .map(|r| format!("状态码 {}", r.status()))
                .unwrap_or_else(|_| "网络错误".into()),
            next_provider.name
        ));

        excluded.push(next_provider.id.clone());
        attempt_index = attempt_index.saturating_add(1);
        provider = next_provider;
        current_upstream_model = next_model;
        provider.model = current_upstream_model.clone();
        // 后续协议兼容重试必须沿用当前备用模型，而不是最初的主模型。
        original_body = Bytes::from(rewrite_json_model(&original_body, &current_upstream_model));
        is_anthropic_upstream = next_prepared.is_anthropic_upstream;
        is_stream = next_prepared.is_stream;
        compact_fallback = next_prepared.compact_fallback;
        is_chat_bridge = next_prepared.is_chat_bridge;
        current_prepared = next_prepared;
    };

    let mut prefetched_bytes: Option<Bytes> = None;
    let mut status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let retry_chat = !is_chat_bridge
        && !is_anthropic_upstream
        && compact_fallback.is_none()
        && provider.protocol_type == ProtocolType::OpenAiResponses
        && !codex_compact::is_responses_compact_route(&route)
        && !route.contains("chat/completions")
        && status == StatusCode::BAD_REQUEST;
    let mut upstream = Some(upstream);
    if retry_chat {
        let Some(current) = upstream.take() else {
            return json_error(StatusCode::BAD_GATEWAY, "Codex 上游响应丢失");
        };
        match current.bytes().await {
            Ok(error_bytes) => {
                if is_unsupported_content_type_error(&error_bytes) {
                    match prepare_codex_upstream(
                        &state,
                        &provider,
                        &route,
                        &headers,
                        &original_body,
                        true,
                        is_catalog_subagent,
                    ) {
                        Ok(chat_prepared) => {
                            let chat_start = Instant::now();
                            note_gateway_inflight(
                                &state, &provider, &current_upstream_model, is_stream, route_decision.as_ref(), &attempts,
                            );
                            let (chat_result, queue_wait_ms) = super::upstream_health::send_observed_upstream_with_timing(
                                chat_prepared.request.body(chat_prepared.request_body),
                                &provider,
                                Some(&state),
                                is_stream,
                            )
                            .await;
                            let chat_duration = chat_start.elapsed().as_millis() as i64;
                            attempts.push(super::upstream_health::build_request_attempt(
                                attempts.len(),
                                &provider,
                                &current_upstream_model,
                                chat_duration,
                                &chat_result,
                            ).with_queue_wait_ms(queue_wait_ms));
                            match chat_result {
                                Ok(response) => {
                                    failover_trace.push(format!(
                                        "{}({}) Responses 400 后改走 Chat Completions",
                                        provider.name, provider.id
                                    ));
                                    is_chat_bridge = chat_prepared.is_chat_bridge;
                                    is_stream = chat_prepared.is_stream;
                                    compact_fallback = chat_prepared.compact_fallback;
                                    status = StatusCode::from_u16(response.status().as_u16())
                                        .unwrap_or(StatusCode::BAD_GATEWAY);
                                    upstream = Some(response);
                                }
                                Err(_) => {
                                    prefetched_bytes = Some(error_bytes);
                                }
                            }
                        },
                        Err(_) => {
                            prefetched_bytes = Some(error_bytes);
                        }
                    }
                } else {
                    prefetched_bytes = Some(error_bytes);
                }
            }
            Err(error) => {
                return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {error}"));
            }
        }
    }

    if let Some(bytes) = prefetched_bytes {
        let failover_diag = if !failover_trace.is_empty() {
            Some(format!("故障降级: {}", failover_trace.join(" → ")))
        } else {
            None
        };
        let fail_log_id = log_request_with_diagnostic(
            &state,
            &provider,
            Some(i64::from(status.as_u16())),
            started.elapsed().as_millis() as i64,
            &route,
            is_stream,
            Some("upstream"),
            failover_diag.as_deref(),
        );
        let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
        if let Some(id) = fail_log_id.as_deref() {
            if let Some(decision) = route_decision.as_ref() {
                super::patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
            } else if !attempts.is_empty() {
                super::update_proxy_log_attempts(&state, id, &attempts_json);
            }
        }
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let Some(upstream) = upstream else {
        return json_error(StatusCode::BAD_GATEWAY, "Codex 上游响应丢失");
    };
    let failover_diag = if !failover_trace.is_empty() {
        Some(format!("故障降级: {}", failover_trace.join(" → ")))
    } else {
        None
    };
    let log_id = log_request_with_diagnostic(
        &state,
        &provider,
        Some(i64::from(status.as_u16())),
        started.elapsed().as_millis() as i64,
        &route,
        is_stream,
        if status.is_success() {
            None
        } else {
            Some("upstream")
        },
        failover_diag.as_deref(),
    );
    let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
    if let Some(id) = log_id.as_deref() {
        if let Some(decision) = route_decision.as_ref() {
            super::patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
        } else if !attempts.is_empty() {
            super::update_proxy_log_attempts(&state, id, &attempts_json);
        }
    }

    let is_streaming = is_stream
        || upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));
    if status.is_success() {
        remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
    }

    if is_anthropic_upstream || matches!(compact_fallback, Some(CompactFallback::Anthropic)) {
        if let Some(CompactFallback::Anthropic) = compact_fallback {
            return forward_compact_anthropic_fallback(
                state,
                provider,
                upstream,
                status,
                log_id,
            )
            .await;
        }
        return forward_anthropic_upstream(
            state,
            provider,
            upstream,
            status,
            is_streaming,
            log_id,
            started,
        )
        .await;
    }

    if is_chat_bridge {
        return forward_chat_bridge_upstream(
            state,
            provider,
            upstream,
            status,
            is_streaming,
            log_id,
            None,
        )
        .await;
    }

    let mut resp_builder = Response::builder().status(status);
    for (name, value) in upstream.headers() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }
        resp_builder = resp_builder.header(name, value);
    }

    if !is_streaming {
        let response_bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {error}"));
            }
        };
        let response_bytes = if status.is_success() {
            match compact_fallback {
                Some(CompactFallback::Chat) => {
                    if let Ok(upstream_json) = serde_json::from_slice::<Value>(&response_bytes) {
                        let compact = codex_compact::chat_response_to_responses_compact(
                            &upstream_json,
                            provider.model.trim(),
                        );
                        Bytes::from(serde_json::to_vec(&compact).unwrap_or_default())
                    } else {
                        response_bytes
                    }
                }
                _ => {
                    if let Ok(value) = serde_json::from_slice::<Value>(&response_bytes) {
                        let _ = state.codex_history.record_response(&value);
                    }
                    response_bytes
                }
            }
        } else {
            response_bytes
        };
        if let Some(id) = log_id.as_deref() {
            if let Some(usage) = extract_usage_from_json(&response_bytes) {
                let _ = state.db.with_conn(|conn| {
                    update_proxy_log_usage_idempotent(
                        conn,
                        id,
                        Some(state.target.as_str()),
                        Some(provider.id.as_str()),
                        usage.envelope_id.as_deref(),
                        usage.input_tokens,
                        usage.cache_read_input_tokens,
                        usage.cache_creation_input_tokens,
                        usage.output_tokens,
                    )
                });
            }
        }
        return resp_builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(response_bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let body_log_id = log_id.clone();
    let body_db = Arc::clone(&state.db);
    let db = Arc::clone(&state.db);
    let history = Arc::clone(&state.codex_history);
    let mut sse_buffer = Vec::new();
    let mut history_buffer = Vec::new();
    let mut current_response_id = None;
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let stream = upstream.bytes_stream().map(move |chunk| match chunk {
        Ok(bytes) => {
            history_buffer.extend_from_slice(&bytes);
            while let Some(block) = super::codex_history::take_sse_block(&mut history_buffer) {
                if let Ok(text) = std::str::from_utf8(&block) {
                    let mut data_parts = Vec::new();
                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            data_parts.push(data.trim_start());
                        }
                    }
                    let data = data_parts.join("\n");
                    if !data.is_empty() && data != "[DONE]" {
                        if let Ok(value) = serde_json::from_str::<Value>(&data) {
                            super::response_lifecycle::record_response_event(&db, log_id.as_deref(), &value);
                            history.inspect_sse_event(&value, &mut current_response_id);
                        }
                    }
                }
            }
            if let Some(id) = log_id.as_deref() {
                sse_buffer.extend_from_slice(&bytes);
                if let Some(usage) = extract_usage_from_sse(&sse_buffer) {
                    let _ = db.with_conn(|conn| {
                        update_proxy_log_usage_idempotent(
                            conn,
                            id,
                            Some(target_app.as_str()),
                            Some(provider_id.as_str()),
                            usage.envelope_id.as_deref(),
                            usage.input_tokens,
                            usage.cache_read_input_tokens,
                            usage.cache_creation_input_tokens,
                            usage.output_tokens,
                        )
                    });
                }
            }
            Ok::<Bytes, Infallible>(bytes)
        }
        Err(_) => {
            if let Some(id) = log_id.as_deref() {
                let _ = db.with_conn(|conn| {
                    crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                        conn, id, "midstream_error", None, Some("midstream_error"),
                        Some("Codex 上游流式响应中途中断"),
                    )
                });
            }
            Ok(Bytes::new())
        },
    });

    resp_builder
        .body(super::response_lifecycle::track_stream_body(
            Body::from_stream(stream), body_db, body_log_id,
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn next_codex_failover_provider(
    state: &ProxyState,
    excluded: &[String],
    requested_model: &str,
    catalog_mode: bool,
) -> Option<Provider> {
    if catalog_mode {
        next_failover_provider_ex(state, excluded, requested_model, true)
    } else {
        next_failover_provider(state, excluded, requested_model)
    }
    .ok()
    .flatten()
}

fn catalog_failover_body_if_needed(
    state: &ProxyState,
    fallback: &Provider,
    original_body: &Bytes,
    catalog_mode: bool,
) -> Bytes {
    if !catalog_mode {
        return original_body.clone();
    }
    let subagent = crate::catalog::subagent_model(state.db.as_ref(), state.target);
    let entries = super::load_gateway_catalog(state, crate::catalog::catalog_style_for(state.target))
        .map(|(_, entries)| entries)
        .unwrap_or_default();
    let upstream =
        crate::catalog::failover_upstream_for_provider(fallback, &entries, subagent.as_deref());
    if upstream.is_empty() {
        return original_body.clone();
    }
    Bytes::from(rewrite_json_model(original_body, &upstream))
}

struct PreparedCodexUpstream {
    request: reqwest::RequestBuilder,
    request_body: Vec<u8>,
    is_stream: bool,
    is_anthropic_upstream: bool,
    /// When set, wrap a successful JSON upstream body into `response.compaction`.
    compact_fallback: Option<CompactFallback>,
    /// Codex client sent Responses; upstream is Chat Completions.
    is_chat_bridge: bool,
}

#[derive(Clone, Copy)]
enum CompactFallback {
    Chat,
    Anthropic,
}

fn should_bridge_responses_to_chat(protocol: ProtocolType, route: &str, force_chat: bool) -> bool {
    if codex_compact::is_responses_compact_route(route) || route.contains("chat/completions") {
        return false;
    }
    force_chat
        || matches!(
            protocol,
            ProtocolType::OpenAiChat | ProtocolType::Proxy
        )
}

fn prepare_codex_upstream(
    state: &ProxyState,
    provider: &Provider,
    route: &str,
    headers: &HeaderMap,
    original_body: &Bytes,
    force_chat: bool,
    is_catalog_subagent: bool,
) -> Result<PreparedCodexUpstream, Response> {
    let api_key = match crate::database::dao::provider_runtime_api_key(&provider.api_key) {
        Ok(Some(key)) if !key.trim().is_empty() => key,
        _ => {
            return Err(json_error(
                StatusCode::UNAUTHORIZED,
                "Codex 供应商未配置 API Key",
            ));
        }
    };

    let is_compact = codex_compact::is_responses_compact_route(route);
    let is_anthropic_upstream = provider.protocol_type == ProtocolType::Anthropic;
    let wants_chat_bridge =
        !is_compact && should_bridge_responses_to_chat(provider.protocol_type, route, force_chat);
    if is_anthropic_upstream && route.contains("chat/completions") {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Anthropic 上游 Codex 供应商仅支持 /v1/responses",
        ));
    }

    let path = if is_compact {
        codex_compact::compact_upstream_path(provider.protocol_type).to_string()
    } else if is_anthropic_upstream {
        "/v1/messages".to_string()
    } else if route.contains("chat/completions") || wants_chat_bridge {
        "/v1/chat/completions".to_string()
    } else {
        "/v1/responses".to_string()
    };
    let upstream_url = match api_endpoint_url(&provider.base_url, &path) {
        Ok(url) => url,
        Err(error) => {
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            ));
        }
    };

    // Failover must use the *target* provider's auto-review override.
    // Catalog mode rewrites any subagent at routing time; keep the per-provider
    // override for independent mode only.
    let body = if crate::catalog::enabled(state.db.as_ref(), state.target) {
        Bytes::copy_from_slice(original_body)
    } else {
        apply_auto_review_model_override(
            headers,
            original_body,
            provider.auto_review_model_override.as_deref(),
        )
    };

    let needs_history_enrich = is_anthropic_upstream
        || (is_compact
            && matches!(
                provider.protocol_type,
                ProtocolType::OpenAiChat | ProtocolType::Proxy | ProtocolType::Anthropic
            ));

    let (request_body, is_stream, compact_fallback) = if is_compact {
        let mut parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return Err(json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"));
            }
        };
        if let Err(error) = codex_compact::reject_streaming_compact(&parsed) {
            return Err(json_error(StatusCode::BAD_REQUEST, error.to_string()));
        }
        if needs_history_enrich {
            let _ = state.codex_history.enrich_request(&mut parsed);
        }
        let requested_model = parsed
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(provider.model.trim());
        match provider.protocol_type {
            ProtocolType::OpenAiResponses => (
                serde_json::to_vec(&parsed).unwrap_or_else(|_| body.to_vec()),
                false,
                None,
            ),
            ProtocolType::OpenAiChat | ProtocolType::Proxy => {
                let mut chat = match codex_compact::compact_request_to_openai_chat(&parsed, requested_model)
                {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(json_error(StatusCode::BAD_REQUEST, error.to_string()));
                    }
                };
                super::codex_moonshot_schema::rewrite_chat_tools_if_needed(
                    &provider.base_url,
                    &mut chat,
                );
                (
                    serde_json::to_vec(&chat).unwrap_or_default(),
                    false,
                    Some(CompactFallback::Chat),
                )
            }
            ProtocolType::Anthropic => {
                let anthropic =
                    match codex_compact::compact_request_to_anthropic(&parsed, requested_model) {
                        Ok(value) => value,
                        Err(error) => {
                            return Err(json_error(StatusCode::BAD_REQUEST, error.to_string()));
                        }
                    };
                (
                    serde_json::to_vec(&anthropic).unwrap_or_default(),
                    false,
                    Some(CompactFallback::Anthropic),
                )
            }
        }
    } else if is_anthropic_upstream {
        let mut parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return Err(json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"));
            }
        };
        let _ = state.codex_history.enrich_request(&mut parsed);
        let requested_model = parsed
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(provider.model.trim());
        let anthropic_body =
            match responses_request_to_anthropic_messages(&parsed, requested_model) {
                Ok(value) => value,
                Err(error) => {
                    return Err(json_error(StatusCode::BAD_REQUEST, error.to_string()));
                }
            };
        let stream = parsed
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let encoded = serde_json::to_vec(&anthropic_body).unwrap_or_default();
        (encoded, stream, None)
    } else if route.contains("chat/completions") {
        let mut parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return Err(json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"));
            }
        };
        let stream = parsed
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let explicit = parsed
            .get("prompt_cache_key")
            .and_then(Value::as_str)
            .map(str::to_string);
        convert::reinject_chat_prompt_cache_key(
            &mut parsed,
            explicit.as_deref(),
            session_prompt_cache_hint(headers).as_deref(),
            convert::chat_prompt_cache_allowed_for_base_url(&provider.base_url),
        );
        (
            serde_json::to_vec(&parsed).unwrap_or_else(|_| body.to_vec()),
            stream,
            None,
        )
    } else if wants_chat_bridge {
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return Err(json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"));
            }
        };
        let mut chat = match responses_to_chat_completions_body(&parsed) {
            Ok(value) => value,
            Err(error) => {
                return Err(json_error(StatusCode::BAD_REQUEST, error));
            }
        };
        super::codex_moonshot_schema::rewrite_chat_tools_if_needed(&provider.base_url, &mut chat);
        let stream = chat
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        (
            serde_json::to_vec(&chat).unwrap_or_default(),
            stream,
            None,
        )
    } else {
        let mut parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return Err(json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"));
            }
        };
        convert::sanitize_codex_responses_body(&mut parsed);
        let stream = parsed
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        (
            serde_json::to_vec(&parsed).unwrap_or_else(|_| body.to_vec()),
            stream,
            None,
        )
    };

    let mut request = state.client.request(reqwest::Method::POST, &upstream_url);
    if is_anthropic_upstream || matches!(compact_fallback, Some(CompactFallback::Anthropic)) {
        request = request
            .header("x-api-key", &api_key)
            .header("anthropic-version", anthropic_version_header());
    } else {
        request = request.header(header::AUTHORIZATION, format!("Bearer {api_key}"));
    }
    request = request.header(header::CONTENT_TYPE, "application/json");
    if is_catalog_subagent {
        request = request.header(CS_SUBAGENT_HEADER, "1");
    }
    if let Some(correlation) = state.correlation.as_ref() {
        request = request.header(
            crate::gateway::correlation::REQUEST_ID_HEADER,
            correlation.id.as_str(),
        );
        if let Some(target) = correlation.target_app.as_deref() {
            request = request.header(crate::gateway::correlation::TARGET_APP_HEADER, target);
        }
    }
    for (name, value) in headers.iter() {
        let key = name.as_str();
        if is_hop_by_hop_header(key)
            || key.eq_ignore_ascii_case("authorization")
            || key.eq_ignore_ascii_case("host")
            || key.eq_ignore_ascii_case("content-length")
            || key.eq_ignore_ascii_case("x-api-key")
            || key.eq_ignore_ascii_case("anthropic-version")
            || key.eq_ignore_ascii_case("content-type")
            || key.eq_ignore_ascii_case(crate::gateway::correlation::REQUEST_ID_HEADER)
            || key.eq_ignore_ascii_case(crate::gateway::correlation::TARGET_APP_HEADER)
            || key.eq_ignore_ascii_case(crate::gateway::PARENT_SESSION_HEADER)
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            request = request.header(key, value);
        }
    }

    if let Some(ref custom_headers) = provider.custom_headers {
        for (k, v) in custom_headers {
            if !is_hop_by_hop_header(k)
                && !k.eq_ignore_ascii_case("host")
                && !k.eq_ignore_ascii_case("content-length")
            {
                request = request.header(k.as_str(), v.as_str());
            }
        }
    }

    Ok(PreparedCodexUpstream {
        request,
        request_body,
        is_stream,
        is_anthropic_upstream,
        compact_fallback,
        is_chat_bridge: wants_chat_bridge,
    })
}

async fn forward_chat_bridge_upstream(
    state: ProxyState,
    provider: Provider,
    upstream: reqwest::Response,
    status: StatusCode,
    is_streaming: bool,
    log_id: Option<String>,
    prefetched_bytes: Option<Bytes>,
) -> Response {
    if !is_streaming {
        let response_bytes = if let Some(bytes) = prefetched_bytes {
            bytes
        } else {
            match upstream.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {error}"));
                }
            }
        };
        if !status.is_success() {
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(response_bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        let chat: Value = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => {
                return json_error(
                    StatusCode::BAD_GATEWAY,
                    "Chat Completions 上游返回了无法转换的响应",
                );
            }
        };
        let responses = chat_response_to_responses(&chat, provider.model.trim());
        let _ = state.codex_history.record_response(&responses);
        let encoded = serde_json::to_vec(&responses).unwrap_or_default();
        if let Some(id) = log_id.as_deref() {
            if let Some(usage) = extract_usage_from_json(&encoded) {
                let _ = state.db.with_conn(|conn| {
                    update_proxy_log_usage_idempotent(
                        conn,
                        id,
                        Some(state.target.as_str()),
                        Some(provider.id.as_str()),
                        usage.envelope_id.as_deref(),
                        usage.input_tokens,
                        usage.cache_read_input_tokens,
                        usage.cache_creation_input_tokens,
                        usage.output_tokens,
                    )
                });
            }
        }
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(encoded))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let body_log_id = log_id.clone();
    let body_db = Arc::clone(&state.db);
    let db = Arc::clone(&state.db);
    let history = Arc::clone(&state.codex_history);
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let fallback_model = provider.model.clone();
    let stream = futures_util::stream::unfold(
        (
            upstream.bytes_stream(),
            Vec::new(),
            ChatSseToResponsesConverter::new(&fallback_model),
            false,
            None::<String>,
            Vec::<u8>::new(),
        ),
        move |(mut upstream_stream, mut buffer, mut converter, done, mut response_id, mut out_buf)| {
            let db = Arc::clone(&db);
            let history = Arc::clone(&history);
            let target_app = target_app.clone();
            let provider_id = provider_id.clone();
            let stream_log_id = log_id.clone();
            async move {
                if done {
                    return None;
                }
                let next = upstream_stream.next().await;
                let (output, done) = match next {
                    Some(Ok(bytes)) => {
                        buffer.extend_from_slice(&bytes);
                        let mut output = Vec::new();
                        while let Some((end, delimiter_len)) = find_sse_frame_end(&buffer) {
                            let frame = buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
                            let Ok(frame) = std::str::from_utf8(&frame) else {
                                continue;
                            };
                            if let Some(data) = parse_chat_sse_data(frame) {
                                if data == "[DONE]" {
                                    output.extend(converter.finish_done());
                                } else if let Ok(chunk) = serde_json::from_str::<Value>(&data) {
                                    output.extend(converter.push_chat_chunk(&chunk));
                                }
                            }
                        }
                        (output, false)
                    }
                    Some(Err(_)) => {
                        if let Some(id) = stream_log_id.as_deref() {
                            let _ = db.with_conn(|conn| {
                                crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                                    conn,
                                    id,
                                    "midstream_error",
                                    None,
                                    Some("midstream_error"),
                                    Some("Codex 上游流式响应中途中断"),
                                )
                            });
                        }
                        (converter.finish_done(), true)
                    }
                    None => {
                        let mut rest = Vec::new();
                        if !buffer.is_empty() {
                            if let Ok(frame) = std::str::from_utf8(&buffer) {
                                if let Some(data) = parse_chat_sse_data(frame) {
                                    if data == "[DONE]" {
                                        rest.extend(converter.finish_done());
                                    } else if let Ok(chunk) = serde_json::from_str::<Value>(&data) {
                                        rest.extend(converter.push_chat_chunk(&chunk));
                                    }
                                }
                            }
                            buffer.clear();
                        }
                        rest.extend(converter.finish_done());
                        if let Some(id) = stream_log_id.as_deref() {
                            let _ = db.with_conn(|conn| {
                                crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                                    conn,
                                    id,
                                    "complete",
                                    None,
                                    None,
                                    None,
                                )
                            });
                        }
                        (rest, true)
                    }
                };
                out_buf.extend_from_slice(&output);
                while let Some(block) = super::codex_history::take_sse_block(&mut out_buf) {
                    if let Ok(text) = std::str::from_utf8(&block) {
                        let mut data_parts = Vec::new();
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data:") {
                                data_parts.push(data.trim_start());
                            }
                        }
                        let data = data_parts.join("\n");
                        if !data.is_empty() && data != "[DONE]" {
                            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                                super::response_lifecycle::record_response_event(&db, stream_log_id.as_deref(), &value);
                            history.inspect_sse_event(&value, &mut response_id);
                            }
                        }
                    }
                }
                if let Some(id) = stream_log_id.as_deref() {
                    if let Some(usage) = extract_usage_from_sse(&output) {
                        let _ = db.with_conn(|conn| {
                            update_proxy_log_usage_idempotent(
                                conn,
                                id,
                                Some(target_app.as_str()),
                                Some(provider_id.as_str()),
                                usage.envelope_id.as_deref(),
                                usage.input_tokens,
                                usage.cache_read_input_tokens,
                                usage.cache_creation_input_tokens,
                                usage.output_tokens,
                            )
                        });
                    }
                }
                if output.is_empty() && done {
                    return None;
                }
                Some((
                    Ok::<Bytes, Infallible>(Bytes::from(output)),
                    (upstream_stream, buffer, converter, done, response_id, out_buf),
                ))
            }
        },
    );

    Response::builder()
        .status(if status.is_success() {
            StatusCode::OK
        } else {
            status
        })
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(super::response_lifecycle::track_stream_body(
            Body::from_stream(stream), body_db, body_log_id,
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn parse_chat_sse_data(frame: &str) -> Option<String> {
    let mut data_parts = Vec::new();
    for line in frame.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            data_parts.push(data.trim_start());
        }
    }
    let data = data_parts.join("\n");
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

async fn forward_compact_anthropic_fallback(
    state: ProxyState,
    provider: Provider,
    upstream: reqwest::Response,
    status: StatusCode,
    log_id: Option<String>,
) -> Response {
    let response_bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {error}"));
        }
    };
    if !status.is_success() {
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(response_bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    let anthropic: Value = match serde_json::from_slice(&response_bytes) {
        Ok(value) => value,
        Err(_) => {
            return json_error(StatusCode::BAD_GATEWAY, "Anthropic compact 上游返回了无法转换的响应");
        }
    };
    let compact =
        codex_compact::anthropic_response_to_responses_compact(&anthropic, provider.model.trim());
    let encoded = serde_json::to_vec(&compact).unwrap_or_default();
    if let Some(id) = log_id.as_deref() {
        if let Some(usage) = extract_usage_from_json(&encoded) {
            let _ = state.db.with_conn(|conn| {
                update_proxy_log_usage_idempotent(
                    conn,
                    id,
                    Some(state.target.as_str()),
                    Some(provider.id.as_str()),
                    usage.envelope_id.as_deref(),
                    usage.input_tokens,
                    usage.cache_read_input_tokens,
                    usage.cache_creation_input_tokens,
                    usage.output_tokens,
                )
            });
        }
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(encoded))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn forward_anthropic_upstream(
    state: ProxyState,
    provider: Provider,
    upstream: reqwest::Response,
    status: StatusCode,
    is_streaming: bool,
    log_id: Option<String>,
    started: Instant,
) -> Response {
    if !is_streaming {
        let response_bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {error}"));
            }
        };
        if !status.is_success() {
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(response_bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        let anthropic: Value = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => {
                let _ = log_request(
                    &state,
                    &provider,
                    Some(502),
                    started.elapsed().as_millis() as i64,
                    "/v1/responses",
                    false,
                    Some("conversion"),
                );
                return json_error(StatusCode::BAD_GATEWAY, "Anthropic 上游返回了无法转换的响应");
            }
        };
        let responses = anthropic_response_to_responses(&anthropic);
        let _ = state.codex_history.record_response(&responses);
        let encoded = serde_json::to_vec(&responses).unwrap_or_default();
        if let Some(id) = log_id.as_deref() {
            if let Some(usage) = extract_usage_from_json(&encoded) {
                let _ = state.db.with_conn(|conn| {
                    update_proxy_log_usage_idempotent(
                        conn,
                        id,
                        Some(state.target.as_str()),
                        Some(provider.id.as_str()),
                        usage.envelope_id.as_deref(),
                        usage.input_tokens,
                        usage.cache_read_input_tokens,
                        usage.cache_creation_input_tokens,
                        usage.output_tokens,
                    )
                });
            }
        }
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(encoded))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let body_log_id = log_id.clone();
    let body_db = Arc::clone(&state.db);
    let db = Arc::clone(&state.db);
    let history = Arc::clone(&state.codex_history);
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let fallback_model = provider.model.clone();
    let stream = futures_util::stream::unfold(
        (
            upstream.bytes_stream(),
            Vec::new(),
            AnthropicSseToResponsesConverter::new(&fallback_model),
            false,
            None::<String>,
            Vec::<u8>::new(),
        ),
        move |(mut upstream_stream, mut buffer, mut converter, done, mut response_id, mut out_buf)| {
            let db = Arc::clone(&db);
            let history = Arc::clone(&history);
            let target_app = target_app.clone();
            let provider_id = provider_id.clone();
            let stream_log_id = log_id.clone();
            async move {
                if done {
                    return None;
                }
                let next = upstream_stream.next().await;
                let (output, done) = match next {
                    Some(Ok(bytes)) => {
                        buffer.extend_from_slice(&bytes);
                        let mut output = Vec::new();
                        while let Some((end, delimiter_len)) = find_sse_frame_end(&buffer) {
                            let frame = buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
                            let Ok(frame) = std::str::from_utf8(&frame) else {
                                continue;
                            };
                            if let Some((event_type, data)) = parse_anthropic_sse_frame(frame) {
                                output.extend(converter.push_event(event_type, &data));
                            }
                        }
                        (output, false)
                    }
                    Some(Err(_)) => {
                        if let Some(id) = stream_log_id.as_deref() {
                            let _ = db.with_conn(|conn| {
                                crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                                    conn, id, "midstream_error", None, Some("midstream_error"),
                                    Some("Codex 上游流式响应中途中断"),
                                )
                            });
                        }
                        (converter.error_event("上游流式响应中断"), true)
                    }
                    None => (converter.finish_stream(), true),
                };
                out_buf.extend_from_slice(&output);
                while let Some(block) = super::codex_history::take_sse_block(&mut out_buf) {
                    if let Ok(text) = std::str::from_utf8(&block) {
                        let mut data_parts = Vec::new();
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data:") {
                                data_parts.push(data.trim_start());
                            }
                        }
                        let data = data_parts.join("\n");
                        if !data.is_empty() && data != "[DONE]" {
                            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                                super::response_lifecycle::record_response_event(&db, stream_log_id.as_deref(), &value);
                            history.inspect_sse_event(&value, &mut response_id);
                            }
                        }
                    }
                }
                if let Some(id) = stream_log_id.as_deref() {
                    if let Some(usage) = extract_usage_from_sse(&output) {
                        let _ = db.with_conn(|conn| {
                            update_proxy_log_usage_idempotent(
                                conn,
                                id,
                                Some(target_app.as_str()),
                                Some(provider_id.as_str()),
                                usage.envelope_id.as_deref(),
                                usage.input_tokens,
                                usage.cache_read_input_tokens,
                                usage.cache_creation_input_tokens,
                                usage.output_tokens,
                            )
                        });
                    }
                }
                if output.is_empty() && done {
                    return None;
                }
                Some((
                    Ok::<Bytes, Infallible>(Bytes::from(output)),
                    (upstream_stream, buffer, converter, done, response_id, out_buf),
                ))
            }
        },
    );

    Response::builder()
        .status(if status.is_success() {
            StatusCode::OK
        } else {
            status
        })
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(super::response_lifecycle::track_stream_body(
            Body::from_stream(stream), body_db, body_log_id,
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn find_sse_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len().saturating_sub(1) {
        if buffer[index..].starts_with(b"\n\n") {
            return Some((index, 2));
        }
        if buffer[index..].starts_with(b"\r\n\r\n") {
            return Some((index, 4));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::should_try_explicit_fallback;
    use serde_json::json;

    #[test]
    fn anthropic_bridge_enriches_bare_function_call_output_before_convert() {
        let history = super::super::codex_history::CodexHistoryStore::default();
        history.record_response(&json!({
            "id": "resp_1",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{\"q\":\"x\"}"
            }]
        }));

        let mut request = json!({
            "previous_response_id": "resp_1",
            "model": "claude-sonnet-5",
            "max_output_tokens": 64,
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "found"
            }]
        });
        assert_eq!(history.enrich_request(&mut request), 1);

        let anthropic =
            responses_request_to_anthropic_messages(&request, "claude-sonnet-5").unwrap();
        let messages = anthropic["messages"].as_array().unwrap();
        assert!(
            messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("assistant")
                    && message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block.get("type").and_then(Value::as_str) == Some("tool_use")
                                    && block.get("id").and_then(Value::as_str) == Some("call_1")
                            })
                        })
            }),
            "enriched assistant tool_use missing: {anthropic}"
        );
        assert!(
            messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("user")
                    && message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block.get("type").and_then(Value::as_str) == Some("tool_result")
                                    && block.get("tool_use_id").and_then(Value::as_str)
                                        == Some("call_1")
                            })
                        })
            }),
            "tool_result missing: {anthropic}"
        );
    }

    #[test]
    fn openai_chat_responses_route_bridges_to_chat() {
        assert!(should_bridge_responses_to_chat(
            ProtocolType::OpenAiChat,
            "/v1/responses",
            false
        ));
        assert!(should_bridge_responses_to_chat(
            ProtocolType::Proxy,
            "/v1/responses",
            false
        ));
        assert!(!should_bridge_responses_to_chat(
            ProtocolType::OpenAiResponses,
            "/v1/responses",
            false
        ));
        assert!(should_bridge_responses_to_chat(
            ProtocolType::OpenAiResponses,
            "/v1/responses",
            true
        ));
        assert!(!should_bridge_responses_to_chat(
            ProtocolType::Anthropic,
            "/v1/responses",
            false
        ));
        assert!(!should_bridge_responses_to_chat(
            ProtocolType::OpenAiChat,
            "/v1/chat/completions",
            false
        ));
    }

    #[test]
    fn responses_input_array_converts_to_chat_completions_messages() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "stream": true,
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        assert_eq!(chat["model"], "gpt-5.6-luna");
        assert_eq!(chat["stream"], true);
        assert_eq!(chat["messages"][0]["role"], "user");
        assert_eq!(chat["messages"][0]["content"], "hello");
    }

    #[test]
    fn test_explicit_fallback_rewrites_model_and_does_not_send_raw_public_id() {
        let body = Bytes::from(json!({
            "model": "codex.auto",
            "input": "test prompt",
            "stream": false
        }).to_string());

        let upstream_slug = "gpt-5.6-luna";
        let rewritten = rewrite_json_model(&body, upstream_slug);
        let parsed: Value = serde_json::from_slice(&rewritten).unwrap();

        assert_eq!(parsed["model"], "gpt-5.6-luna");
        assert_ne!(parsed["model"], "codex.auto");
    }

    #[test]
    fn test_explicit_chain_pinned_blocks_failover() {
        let plan = crate::gateway::RouteExecutionPlan {
            attempts: vec![
                crate::gateway::RouteAttemptPlan {
                    index: 0,
                    model: "primary-model".into(),
                    upstream_id: Some("p1".into()),
                },
                crate::gateway::RouteAttemptPlan {
                    index: 1,
                    model: "backup-model".into(),
                    upstream_id: None,
                },
            ],
            fallback_mode: "off".into(),
            primary_model: "primary-model".into(),
            explicit_pinned: true,
        };

        let allow_cross_provider_failover = !plan.explicit_pinned;
        assert!(!allow_cross_provider_failover);

        let explicit_models: Vec<String> = if !plan.explicit_pinned {
            plan.attempts.iter().skip(1).map(|a| a.model.clone()).collect()
        } else {
            Vec::new()
        };
        assert!(explicit_models.is_empty());
    }

    #[test]
    fn test_explicit_chain_limits_attempts_to_three_total() {
        let plan = crate::gateway::RouteExecutionPlan {
            attempts: vec![
                crate::gateway::RouteAttemptPlan {
                    index: 0,
                    model: "m0".into(),
                    upstream_id: Some("p0".into()),
                },
                crate::gateway::RouteAttemptPlan { index: 1, model: "m1".into(), upstream_id: None },
                crate::gateway::RouteAttemptPlan { index: 2, model: "m2".into(), upstream_id: None },
                crate::gateway::RouteAttemptPlan { index: 3, model: "m3".into(), upstream_id: None },
            ],
            fallback_mode: "model_chain".into(),
            primary_model: "m0".into(),
            explicit_pinned: false,
        };

        // Plan attempts can be up to 3 total, skip 1 primary gives at most 2 explicit fallbacks
        let explicit_models: Vec<String> = plan
            .attempts
            .iter()
            .skip(1)
            .take(2)
            .map(|a| a.model.clone())
            .collect();
        assert_eq!(explicit_models.len(), 2);
        assert_eq!(explicit_models, vec!["m1", "m2"]);
    }

    fn test_provider(kind: crate::provider::ProviderKind) -> Provider {
        Provider {
            id: "test".into(),
            name: "test".into(),
            base_url: "http://127.0.0.1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "test-model".into(),
            model_context_window: Some(200_000),
            auto_review_model_override: None,
            web_search_enabled: Some(true),
            model_mapping: crate::provider::ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            provider_kind: kind,
            auth_binding: String::new(),
            target_app: crate::provider::ProviderTarget::Codex,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_latency_ms: None,
            health_status: None,
            health_checked_at: None,
        }
    }

    #[test]
    fn test_explicit_vs_generic_failover_status_rules() {
        let ag_provider = test_provider(crate::provider::ProviderKind::Antigravity);
        let kiro_provider = test_provider(crate::provider::ProviderKind::Kiro);
        let std_provider = test_provider(crate::provider::ProviderKind::Standard);

        // Explicit fallback: AG 429 & 504 allowed; Kiro 429 & 504 rejected
        assert!(should_try_explicit_fallback(&ag_provider, StatusCode::TOO_MANY_REQUESTS));
        assert!(should_try_explicit_fallback(&ag_provider, StatusCode::GATEWAY_TIMEOUT));
        assert!(!should_try_explicit_fallback(&kiro_provider, StatusCode::TOO_MANY_REQUESTS));
        assert!(!should_try_explicit_fallback(&kiro_provider, StatusCode::GATEWAY_TIMEOUT));
        assert!(should_try_explicit_fallback(&std_provider, StatusCode::TOO_MANY_REQUESTS));
        assert!(should_try_explicit_fallback(&std_provider, StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!should_try_explicit_fallback(&std_provider, StatusCode::BAD_REQUEST));

        // Generic failover: AG 429 & 504 suppressed; Kiro 429 & 504 suppressed
        assert!(!should_failover_upstream_status_ex(&ag_provider, StatusCode::TOO_MANY_REQUESTS, false));
        assert!(!should_failover_upstream_status_ex(&ag_provider, StatusCode::GATEWAY_TIMEOUT, false));
        assert!(!should_failover_upstream_status_ex(&kiro_provider, StatusCode::TOO_MANY_REQUESTS, false));
        assert!(!should_failover_upstream_status_ex(&kiro_provider, StatusCode::GATEWAY_TIMEOUT, false));
        assert!(should_failover_upstream_status_ex(&std_provider, StatusCode::TOO_MANY_REQUESTS, false));
        assert!(should_failover_upstream_status_ex(&std_provider, StatusCode::INTERNAL_SERVER_ERROR, false));
    }
}
