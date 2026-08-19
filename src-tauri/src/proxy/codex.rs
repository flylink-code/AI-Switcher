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

use crate::catalog::{openai_models_payload, rewrite_json_model, CatalogStyle};
use crate::database::dao::providers::{get_current_provider, get_provider_model_cache, resolve_api_key};
use crate::database::dao::proxy_logs::update_proxy_log_usage_idempotent;
use crate::provider::{api_endpoint_url, ProtocolType, Provider, ProviderTarget};

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
    log_early_failure, log_request, log_request_with_diagnostic, next_failover_provider,
    next_failover_provider_ex, record_provider_failure,
    record_provider_success, select_gateway_runtime_provider_with, CS_SUBAGENT_HEADER,
    session_prompt_cache_hint, should_failover_upstream_status_ex,
    FAILOVER_MAX_HOPS, ProxyState,
};

pub async fn codex_models_handler(State(state): State<ProxyState>) -> Response {
    if crate::catalog::enabled(state.db.as_ref(), ProviderTarget::Codex) {
        match super::load_gateway_catalog(&state, CatalogStyle::Codex) {
            Ok((_, entries)) if !entries.is_empty() => {
                let body = openai_models_payload(&entries);
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
            Ok(_) => return json_error(StatusCode::BAD_GATEWAY, "没有已配置的 Codex 供应商"),
            Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
        }
    }
    match state
        .db
        .with_conn(|conn| get_current_provider(conn, ProviderTarget::Codex))
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
            let entries = crate::catalog::build_catalog(CatalogStyle::Codex, &[(provider, cached)]);
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
    let requested_model = serde_json::from_slice::<Value>(&original_body)
        .ok()
        .and_then(|value| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let catalog_mode = crate::catalog::enabled(state.db.as_ref(), ProviderTarget::Codex);
    let mut is_catalog_subagent = false;
    let mut provider = if catalog_mode {
        match select_gateway_runtime_provider_with(
            &state,
            &requested_model,
            has_subagent_header(&headers),
        ) {
            Ok(Some((selected, upstream, routed_subagent))) => {
                original_body = Bytes::from(rewrite_json_model(&original_body, &upstream));
                is_catalog_subagent = routed_subagent;
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
    } else {
        match state
            .db
            .with_conn(|conn| get_current_provider(conn, ProviderTarget::Codex))
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
    let mut failover_trace: Vec<String> = Vec::new();
    let mut upstream = match prepared.request.body(prepared.request_body).send().await {
        Ok(response) => response,
        Err(error) => {
            record_provider_failure(&state, &provider.id);
            failover_trace.push(format!("{}({}) 网络错误", provider.name, provider.id));
            let mut excluded = vec![provider.id.clone()];
            let mut last_error = error.to_string();
            let mut recovered = None::<reqwest::Response>;
            for _ in 0..FAILOVER_MAX_HOPS {
                let Some(fallback) = next_codex_failover_provider(
                    &state,
                    &excluded,
                    &requested_model,
                    catalog_mode,
                ) else {
                    break;
                };
                excluded.push(fallback.id.clone());
                log::warn!(
                    "Codex 供应商 {} 网络请求失败，尝试故障切换到 {}",
                    provider.id,
                    fallback.id
                );
                let failover_body = catalog_failover_body_if_needed(
                    &state,
                    &fallback,
                    &original_body,
                    catalog_mode,
                );
                match prepare_codex_upstream(
                    &state,
                    &fallback,
                    &route,
                    &headers,
                    &failover_body,
                    false,
                    is_catalog_subagent,
                ) {
                    Ok(fallback_prepared) => {
                        match fallback_prepared
                            .request
                            .body(fallback_prepared.request_body)
                            .send()
                            .await
                        {
                            Ok(response) => {
                                failover_trace.push(format!("{}({}) 接管", fallback.name, fallback.id));
                                provider = fallback;
                                is_anthropic_upstream = fallback_prepared.is_anthropic_upstream;
                                is_stream = fallback_prepared.is_stream;
                                compact_fallback = fallback_prepared.compact_fallback;
                                is_chat_bridge = fallback_prepared.is_chat_bridge;
                                recovered = Some(response);
                                break;
                            }
                            Err(fallback_error) => {
                                record_provider_failure(&state, &fallback.id);
                                failover_trace.push(format!("{}({}) 失败: {fallback_error}", fallback.name, fallback.id));
                                last_error = fallback_error.to_string();
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
            match recovered {
                Some(response) => response,
                None => {
                    let failover_diag = if !failover_trace.is_empty() {
                        Some(format!("故障降级失败: {}", failover_trace.join(" → ")))
                    } else {
                        None
                    };
                    let _ = log_request_with_diagnostic(
                        &state,
                        &provider,
                        None,
                        started.elapsed().as_millis() as i64,
                        &route,
                        is_stream,
                        Some("network"),
                        failover_diag.as_deref(),
                    );
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        format!("上游连接失败: {last_error}"),
                    );
                }
            }
        }
    };

    if is_retryable_upstream_status(&state, upstream.status())
        && should_failover_upstream_status_ex(&provider, upstream.status(), catalog_mode)
    {
        record_provider_failure(&state, &provider.id);
        failover_trace.push(format!("{}({}) 状态码 {}", provider.name, provider.id, upstream.status()));
        let mut excluded = vec![provider.id.clone()];
        for _ in 0..FAILOVER_MAX_HOPS {
            let Some(fallback) = next_codex_failover_provider(
                &state,
                &excluded,
                &requested_model,
                catalog_mode,
            ) else {
                break;
            };
            excluded.push(fallback.id.clone());
            log::warn!(
                "Codex 供应商 {} 返回 {}，尝试故障切换到 {}",
                provider.id,
                upstream.status(),
                fallback.id
            );
            let failover_body = catalog_failover_body_if_needed(
                &state,
                &fallback,
                &original_body,
                catalog_mode,
            );
            if let Ok(fallback_prepared) = prepare_codex_upstream(
                &state,
                &fallback,
                &route,
                &headers,
                &failover_body,
                false,
                is_catalog_subagent,
            ) {
                match fallback_prepared
                    .request
                    .body(fallback_prepared.request_body)
                    .send()
                    .await
                {
                    Ok(response) => {
                        provider = fallback;
                        is_anthropic_upstream = fallback_prepared.is_anthropic_upstream;
                        is_stream = fallback_prepared.is_stream;
                        compact_fallback = fallback_prepared.compact_fallback;
                        is_chat_bridge = fallback_prepared.is_chat_bridge;
                        upstream = response;
                        if !is_retryable_upstream_status(&state, upstream.status()) {
                            failover_trace.push(format!("{}({}) 接管成功", provider.name, provider.id));
                            break;
                        }
                        record_provider_failure(&state, &provider.id);
                        failover_trace.push(format!("{}({}) 状态码 {}", provider.name, provider.id, upstream.status()));
                    }
                    Err(error) => {
                        record_provider_failure(&state, &fallback.id);
                        failover_trace.push(format!("{}({}) 连接失败: {error}", fallback.name, fallback.id));
                        log::warn!("Codex 备用供应商 {} 连接失败: {error}", fallback.id);
                    }
                }
            }
        }
    }

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
        let current = upstream.take().expect("Codex upstream response");
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
                        Ok(chat_prepared) => match chat_prepared
                            .request
                            .body(chat_prepared.request_body)
                            .send()
                            .await
                        {
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
        if status.is_success() {
            record_provider_success(&state, &provider.id);
        }
        let failover_diag = if !failover_trace.is_empty() {
            Some(format!("故障降级: {}", failover_trace.join(" → ")))
        } else {
            None
        };
        let _ = log_request_with_diagnostic(
            &state,
            &provider,
            Some(i64::from(status.as_u16())),
            started.elapsed().as_millis() as i64,
            &route,
            is_stream,
            Some("upstream"),
            failover_diag.as_deref(),
        );
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let upstream = upstream.expect("Codex upstream response");
    if status.is_success() {
        record_provider_success(&state, &provider.id);
    }
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

    let is_streaming = is_stream
        || upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));

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
        Err(_) => Ok(Bytes::new()),
    });

    resp_builder
        .body(Body::from_stream(stream))
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
    let subagent = crate::catalog::subagent_model(state.db.as_ref(), ProviderTarget::Codex);
    let entries = super::load_gateway_catalog(state, CatalogStyle::Codex)
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
    let api_key = match state
        .db
        .with_conn(|conn| resolve_api_key(conn, &provider.id))
    {
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
    let body = if crate::catalog::enabled(state.db.as_ref(), ProviderTarget::Codex) {
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
                let chat = match codex_compact::compact_request_to_openai_chat(&parsed, requested_model)
                {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(json_error(StatusCode::BAD_REQUEST, error.to_string()));
                    }
                };
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
        let chat = match responses_to_chat_completions_body(&parsed) {
            Ok(value) => value,
            Err(error) => {
                return Err(json_error(StatusCode::BAD_REQUEST, error));
            }
        };
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
        let stream = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| value.get("stream").and_then(Value::as_bool))
            .unwrap_or(false);
        (body.to_vec(), stream, None)
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
    for (name, value) in headers.iter() {
        let key = name.as_str();
        if is_hop_by_hop_header(key)
            || key.eq_ignore_ascii_case("authorization")
            || key.eq_ignore_ascii_case("host")
            || key.eq_ignore_ascii_case("content-length")
            || key.eq_ignore_ascii_case("x-api-key")
            || key.eq_ignore_ascii_case("anthropic-version")
            || key.eq_ignore_ascii_case("content-type")
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            request = request.header(key, value);
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
                    Some(Err(_)) => (converter.finish_done(), true),
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
        .body(Body::from_stream(stream))
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
                    Some(Err(_)) => (converter.error_event("上游流式响应中断"), true),
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
        .body(Body::from_stream(stream))
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
}
