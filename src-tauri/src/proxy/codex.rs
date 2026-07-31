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

use crate::database::dao::providers::{get_current_provider, resolve_api_key};
use crate::database::dao::proxy_logs::update_proxy_log_usage_idempotent;
use crate::provider::{api_endpoint_url, ProtocolType, ProviderTarget};

use super::codex_anthropic::{
    anthropic_response_to_responses, anthropic_version_header, parse_anthropic_sse_frame,
    responses_request_to_anthropic_messages, AnthropicSseToResponsesConverter,
};
use super::{
    codex_auto_review::apply_auto_review_model_override, extract_usage_from_json, extract_usage_from_sse,
    is_hop_by_hop_header, json_error, log_early_failure, log_request, ProxyState,
};

pub async fn codex_models_handler(State(state): State<ProxyState>) -> Response {
    match state
        .db
        .with_conn(|conn| get_current_provider(conn, ProviderTarget::Codex))
    {
        Ok(Some(provider)) => {
            let body = serde_json::json!({
                "object": "list",
                "data": [{
                    "id": provider.model,
                    "object": "model",
                    "owned_by": "ai-switcher"
                }]
            });
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

    let provider = match state
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
    };

    let api_key = match state
        .db
        .with_conn(|conn| resolve_api_key(conn, &provider.id))
    {
        Ok(Some(key)) if !key.trim().is_empty() => key,
        _ => {
            log_early_failure(
                &state,
                &route,
                "credential",
                Some(401),
                started.elapsed().as_millis() as i64,
            );
            return json_error(StatusCode::UNAUTHORIZED, "Codex 供应商未配置 API Key");
        }
    };

    let is_anthropic_upstream = provider.protocol_type == ProtocolType::Anthropic;
    if is_anthropic_upstream && route.contains("chat/completions") {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Anthropic 上游 Codex 供应商仅支持 /v1/responses",
        );
    }

    let path = if is_anthropic_upstream {
        "/v1/messages"
    } else if route.contains("chat/completions") {
        "/v1/chat/completions"
    } else {
        "/v1/responses"
    };
    let upstream_url = match api_endpoint_url(&provider.base_url, path) {
        Ok(url) => url,
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
    };

    let body = apply_auto_review_model_override(
        &headers,
        &body,
        provider.auto_review_model_override.as_deref(),
    );

    let (request_body, is_stream) = if is_anthropic_upstream {
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                log_early_failure(
                    &state,
                    &route,
                    "request",
                    Some(400),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON");
            }
        };
        let requested_model = parsed
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(provider.model.trim());
        let anthropic_body = match responses_request_to_anthropic_messages(&parsed, requested_model) {
            Ok(value) => value,
            Err(error) => {
                log_early_failure(
                    &state,
                    &route,
                    "request",
                    Some(400),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::BAD_REQUEST, error.to_string());
            }
        };
        let stream = parsed
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let encoded = serde_json::to_vec(&anthropic_body).unwrap_or_default();
        (encoded, stream)
    } else {
        let stream = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| value.get("stream").and_then(Value::as_bool))
            .unwrap_or(false);
        (body.to_vec(), stream)
    };

    let mut request = state.client.request(reqwest::Method::POST, &upstream_url);
    if is_anthropic_upstream {
        request = request
            .header("x-api-key", &api_key)
            .header("anthropic-version", anthropic_version_header());
    } else {
        request = request.header(header::AUTHORIZATION, format!("Bearer {api_key}"));
    }
    request = request.header(header::CONTENT_TYPE, "application/json");
    for (name, value) in headers.iter() {
        let key = name.as_str();
        if is_hop_by_hop_header(key)
            || key.eq_ignore_ascii_case("authorization")
            || key.eq_ignore_ascii_case("host")
            || key.eq_ignore_ascii_case("content-length")
            || key.eq_ignore_ascii_case("x-api-key")
            || key.eq_ignore_ascii_case("anthropic-version")
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            request = request.header(key, value);
        }
    }

    let upstream = match request.body(request_body).send().await {
        Ok(response) => response,
        Err(error) => {
            let _ = log_request(
                &state,
                &provider,
                None,
                started.elapsed().as_millis() as i64,
                &route,
                is_stream,
                Some("network"),
            );
            return json_error(StatusCode::BAD_GATEWAY, format!("上游连接失败: {error}"));
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let log_id = log_request(
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
    );

    let is_streaming = is_stream
        || upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));

    if is_anthropic_upstream {
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
            .body(Body::from(response_bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let db = Arc::clone(&state.db);
    let mut sse_buffer = Vec::new();
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let stream = upstream.bytes_stream().map(move |chunk| match chunk {
        Ok(bytes) => {
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

async fn forward_anthropic_upstream(
    state: ProxyState,
    provider: crate::provider::Provider,
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
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let fallback_model = provider.model.clone();
    let stream = futures_util::stream::unfold(
        (
            upstream.bytes_stream(),
            Vec::new(),
            AnthropicSseToResponsesConverter::new(&fallback_model),
            false,
        ),
        move |(mut upstream_stream, mut buffer, mut converter, done)| {
            let db = Arc::clone(&db);
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
                            let Ok(frame) = std::str::from_utf8(&frame) else { continue; };
                            if let Some((event_type, data)) = parse_anthropic_sse_frame(frame) {
                                output.extend(converter.push_event(event_type, &data));
                            }
                        }
                        (output, false)
                    }
                    Some(Err(_)) => (converter.error_event("上游流式响应中断"), true),
                    None => (converter.finish_stream(), true),
                };
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
                    (upstream_stream, buffer, converter, done),
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
