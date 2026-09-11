//! POST /v1/messages/count_tokens — Anthropic passthrough or local estimate.

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::body::Body;
use bytes::Bytes;
use serde_json::Value;

use crate::error::AppResult;
use crate::provider::{api_endpoint_url, ProtocolType};
use crate::proxy::{hydrate_provider_credential, json_error, select_gateway_runtime_provider_with, ProxyState};

pub async fn handle(state: ProxyState, headers: HeaderMap, body: Bytes) -> Response {
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON"),
    };
    let requested = incoming
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let routed = match select_gateway_runtime_provider_with(
        &state,
        &requested,
        false,
        &incoming,
        "/v1/messages/count_tokens",
    ) {
        Ok(Some((provider, _, _, _, _))) => provider,
        Ok(None) => return json_error(StatusCode::BAD_GATEWAY, "没有可路由的上游"),
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let Some(provider) = (match hydrate_provider_credential(&state, routed) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }) else {
        return json_error(StatusCode::BAD_GATEWAY, "上游凭据不可用");
    };

    if provider.protocol_type == ProtocolType::Anthropic {
        match forward_anthropic(&state, &provider, &headers, body).await {
            Ok(response) => return response,
            Err(error) => {
                log::warn!("count_tokens 上游透传失败，改用本地估算: {error}");
            }
        }
    }

    let tokens = crate::gateway::estimate_request_tokens(&incoming);
    let payload = serde_json::json!({
        "input_tokens": tokens,
        "estimated": true,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn forward_anthropic(
    state: &ProxyState,
    provider: &crate::provider::Provider,
    headers: &HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    let url = api_endpoint_url(&provider.base_url, "/v1/messages/count_tokens")?;
    let mut request = state
        .client
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-api-key", provider.api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .body(body);
    if let Some(version) = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
    {
        request = request.header("anthropic-version", version);
    }
    let response = request.send().await.map_err(|error| {
        crate::error::AppError::Network(format!("count_tokens 上游失败: {error}"))
    })?;
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = response.bytes().await.map_err(|error| {
        crate::error::AppError::Network(format!("读取 count_tokens 响应失败: {error}"))
    })?;
    Ok(Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()))
}
