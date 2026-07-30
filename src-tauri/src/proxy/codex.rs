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

use crate::database::dao::providers::{get_current_provider, resolve_api_key};
use crate::database::dao::proxy_logs::update_proxy_log_usage;
use crate::provider::{api_endpoint_url, ProviderTarget};

use super::{
    extract_usage_from_json, extract_usage_from_sse, is_hop_by_hop_header, json_error,
    log_early_failure, log_request, ProxyState,
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

    let path = if route.contains("chat/completions") {
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

    let is_stream = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("stream").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);

    let mut request = state.client.request(reqwest::Method::POST, &upstream_url);
    request = request.header(header::AUTHORIZATION, format!("Bearer {api_key}"));
    request = request.header(header::CONTENT_TYPE, "application/json");
    for (name, value) in headers.iter() {
        let key = name.as_str();
        if is_hop_by_hop_header(key)
            || key.eq_ignore_ascii_case("authorization")
            || key.eq_ignore_ascii_case("host")
            || key.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            request = request.header(key, value);
        }
    }

    let upstream = match request.body(body.to_vec()).send().await {
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

    let mut resp_builder = Response::builder().status(status);
    for (name, value) in upstream.headers() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }
        resp_builder = resp_builder.header(name, value);
    }

    let is_streaming = is_stream
        || upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));

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
                    update_proxy_log_usage(
                        conn,
                        id,
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
    let stream = upstream.bytes_stream().map(move |chunk| match chunk {
        Ok(bytes) => {
            if let Some(id) = log_id.as_deref() {
                sse_buffer.extend_from_slice(&bytes);
                if let Some(usage) = extract_usage_from_sse(&sse_buffer) {
                    let _ = db.with_conn(|conn| {
                        update_proxy_log_usage(
                            conn,
                            id,
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
