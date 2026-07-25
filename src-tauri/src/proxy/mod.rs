//! Local HTTP proxy that exposes an Anthropic-compatible `/v1/messages` endpoint.
//!
//! Claude Desktop is pointed at `http://127.0.0.1:<port>`; the proxy forwards
//! requests to the active third-party provider after mapping the model name and
//! injecting the real API key. Request summaries are written to the SQLite log
//! table for the usage dashboard (P4).

mod convert;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use std::convert::Infallible;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

use crate::database::dao::proxy_logs::{insert_proxy_log, maintain_proxy_logs as maintain_logs, update_proxy_log_usage};
use crate::database::dao::providers::{get_current_provider, resolve_api_key};
use crate::database::dao::settings::get_setting;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, protocol_endpoint_path, ProtocolType, Provider, ProviderTarget,
};

const DEFAULT_PORT: u16 = 15821;
const LOG_RETENTION_DAYS_KEY: &str = "proxy_log_retention_days";
const LOG_MAX_ROWS_KEY: &str = "proxy_log_max_rows";
const LOG_AUTO_MAINTAIN_KEY: &str = "proxy_log_auto_maintain";

/// Runtime handle for the local proxy.
pub struct ProxyManager {
    db: Arc<Database>,
    code: Option<ProxyRuntime>,
    desktop: Option<ProxyRuntime>,
}

struct ProxyRuntime {
    handle: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
    port: u16,
}

impl ProxyManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            code: None,
            desktop: None,
        }
    }

    /// Whether the server is currently running.
    pub fn status(&self) -> ProxyStatus {
        let mut active = Vec::new();
        for (target, runtime) in [(ProviderTarget::ClaudeCode, &self.code), (ProviderTarget::ClaudeDesktop, &self.desktop)] {
            if let Some(runtime) = runtime.as_ref().filter(|runtime| !runtime.handle.is_finished()) {
                let provider = self.db.with_conn(|conn| get_current_provider(conn, target))
                    .ok()
                    .flatten()
                    .map(|provider| provider.name);
                active.push((runtime.port, provider));
            }
        }
        let running = !active.is_empty();
        let port = active.first().map(|(port, _)| *port).unwrap_or(DEFAULT_PORT);
        let target_provider = active.into_iter()
            .filter_map(|(_, provider)| provider)
            .collect::<Vec<_>>();
        let target_provider = (!target_provider.is_empty()).then(|| target_provider.join(" / "));
        ProxyStatus {
            running,
            port,
            target_provider,
        }
    }

    pub fn status_for(&self, target: ProviderTarget) -> ProxyStatus {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.as_ref(),
            ProviderTarget::ClaudeDesktop => self.desktop.as_ref(),
        };
        let running = runtime.is_some_and(|runtime| !runtime.handle.is_finished());
        ProxyStatus {
            running,
            port: runtime.map(|runtime| runtime.port).unwrap_or(match target {
                ProviderTarget::ClaudeCode => DEFAULT_PORT,
                ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1,
            }),
            target_provider: if running {
                self.db.with_conn(|conn| get_current_provider(conn, target)).ok().flatten().map(|provider| provider.name)
            } else { None },
        }
    }

    /// Start or replace one app's proxy without interrupting the other app.
    pub async fn start(&mut self, port: u16, target: ProviderTarget) -> AppResult<()> {
        let current = match target {
            ProviderTarget::ClaudeCode => self.code.as_ref(),
            ProviderTarget::ClaudeDesktop => self.desktop.as_ref(),
        };
        if current.is_some_and(|runtime| runtime.port == port && !runtime.handle.is_finished()) {
            return Ok(());
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AppError::Io(format!("无法绑定代理端口 {port}: {e}")))?;

        let state = ProxyState {
            db: Arc::clone(&self.db),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|e| AppError::Other(format!("创建 HTTP 客户端失败: {e}")))?,
            target,
            port,
            started_at: Instant::now(),
        };

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/messages", any(proxy_handler))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });

        let handle = tokio::spawn(async move {
            if let Err(e) = server.await {
                log::error!("本地代理服务异常退出: {e}");
            }
        });

        let runtime = ProxyRuntime { handle, shutdown_tx, port };
        let previous = match target {
            ProviderTarget::ClaudeCode => self.code.replace(runtime),
            ProviderTarget::ClaudeDesktop => self.desktop.replace(runtime),
        };
        if let Some(previous) = previous {
            let _ = previous.shutdown_tx.send(());
            previous.handle.abort();
        }
        log::info!("本地代理已启动: {target:?} http://127.0.0.1:{port}");
        self.schedule_automatic_log_maintenance();
        Ok(())
    }

    /// Signal the running server to shut down.
    pub fn stop(&mut self) {
        self.stop_target(ProviderTarget::ClaudeCode);
        self.stop_target(ProviderTarget::ClaudeDesktop);
        log::info!("本地代理已停止");
    }

    pub fn stop_target(&mut self, target: ProviderTarget) {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.take(),
            ProviderTarget::ClaudeDesktop => self.desktop.take(),
        };
        if let Some(runtime) = runtime {
            let _ = runtime.shutdown_tx.send(());
            runtime.handle.abort();
        }
    }

    fn schedule_automatic_log_maintenance(&self) {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let result = db.with_conn(|conn| {
                let auto_maintain = get_setting(conn, LOG_AUTO_MAINTAIN_KEY)?.as_deref() == Some("true");
                if !auto_maintain {
                    return Ok(None);
                }
                let retention_days = get_setting(conn, LOG_RETENTION_DAYS_KEY)?
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(90_u32)
                    .clamp(1, 3650);
                let max_rows = get_setting(conn, LOG_MAX_ROWS_KEY)?
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(100_000_u32)
                    .clamp(100, 5_000_000);
                maintain_logs(conn, retention_days, max_rows, false).map(Some)
            });
            match result {
                Ok(Some(result)) => log::info!("代理启动后自动维护日志：清理 {} 条", result.deleted),
                Ok(None) => {}
                Err(error) => log::warn!("代理启动后自动维护日志失败: {error}"),
            }
        });
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub target_provider: Option<String>,
}

#[derive(Clone)]
struct ProxyState {
    db: Arc<Database>,
    client: Client,
    target: ProviderTarget,
    port: u16,
    started_at: Instant,
}

async fn health_handler(State(state): State<ProxyState>) -> impl IntoResponse {
    let provider = state.db.with_conn(|conn| get_current_provider(conn, state.target));
    let (status, provider_id, protocol, credential_ready, upstream_status, checked_at) = match provider {
        Ok(Some(provider)) => {
            let credential_ready = matches!(
                state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)),
                Ok(Some(key)) if !key.trim().is_empty()
            );
            let status = if credential_ready { "ok" } else { "degraded" };
            (
                status,
                Some(provider.id),
                Some(provider.protocol_type.as_str()),
                credential_ready,
                provider.health_status,
                provider.health_checked_at,
            )
        }
        Ok(None) => ("degraded", None, None, false, None, None),
        Err(error) => {
            log::error!("健康检查读取当前供应商失败: {error}");
            ("degraded", None, None, false, None, None)
        }
    };
    axum::Json(serde_json::json!({
        "status": status,
        "proxyListening": true,
        "targetApp": state.target.as_str(),
        "port": state.port,
        "uptimeSeconds": state.started_at.elapsed().as_secs(),
        "providerId": provider_id,
        "protocol": protocol,
        "credentialReady": credential_ready,
        "lastUpstreamCheck": {"status": upstream_status, "checkedAt": checked_at},
    }))
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let started = Instant::now();

    // Resolve the active provider synchronously.
    let provider: Option<Provider> = match state
        .db
        .with_conn(|conn| get_current_provider(conn, state.target)) {
        Ok(p) => p,
        Err(e) => {
            log::error!("代理读取当前供应商失败: {e}");
            None
        }
    };
    let Some(mut provider) = provider else {
        log_early_failure(&state, uri.path(), "provider", Some(503), started.elapsed().as_millis() as i64);
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "没有激活的第三方供应商");
    };
    provider.api_key = match state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)) {
        Ok(Some(key)) => key,
        Ok(None) => {
            log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("credential"));
            return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商未配置 API Key");
        }
        Err(e) => {
            log::error!("代理读取供应商凭据失败: {e}");
            log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("credential"));
            return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商凭据不可用");
        }
    };
    if provider.base_url.trim().is_empty() {
        log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("configuration"));
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商未配置 Base URL");
    }

    // Read and optionally rewrite the request body.
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            log_request(&state, &provider, Some(400), started.elapsed().as_millis() as i64, uri.path(), false, Some("request"));
            return json_error(StatusCode::BAD_REQUEST, format!("读取请求体失败: {e}"));
        }
    };

    let incoming: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        Err(_) => {
            log_request(&state, &provider, Some(400), started.elapsed().as_millis() as i64, uri.path(), false, Some("request"));
            return json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON");
        }
    };
    let incoming_stream = convert::wants_stream(&incoming);
    let upstream_request: AppResult<(String, Bytes, bool)> = match provider.protocol_type {
        ProtocolType::OpenAiChat | ProtocolType::Proxy => {
            let request = convert::anthropic_to_openai_chat(&incoming, provider.model.trim(), incoming_stream);
            api_endpoint_url(
                &provider.base_url,
                protocol_endpoint_path(provider.protocol_type),
            )
                .map(|url| {
                    (
                        url,
                        Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                        true,
                    )
                })
        }
        ProtocolType::OpenAiResponses => {
            let request = convert::anthropic_to_openai_responses(&incoming, provider.model.trim(), incoming_stream);
            api_endpoint_url(
                &provider.base_url,
                protocol_endpoint_path(provider.protocol_type),
            )
                .map(|url| {
                    (
                        url,
                        Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                        true,
                    )
                })
        }
        ProtocolType::Anthropic => api_endpoint_url(
            &provider.base_url,
            protocol_endpoint_path(provider.protocol_type),
        )
            .map(|url| (url, rewrite_body(&provider, &body_bytes), false)),
    };
    let (target_url, outgoing_body, translated) = match upstream_request {
        Ok(request) => request,
        Err(error) => {
            log_request(
                &state,
                &provider,
                Some(400),
                started.elapsed().as_millis() as i64,
                uri.path(),
                incoming_stream,
                Some("configuration"),
            );
            return json_error(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    // Forward the request.
    let mut req_builder = state
        .client
        .request(method.clone(), &target_url)
        .header(header::CONTENT_TYPE, "application/json");

    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if is_hop_by_hop_header(name_str)
            || name_str.eq_ignore_ascii_case("host")
            || name_str.eq_ignore_ascii_case("content-length")
            || name_str.eq_ignore_ascii_case("content-type")
            || name_str.eq_ignore_ascii_case("authorization")
            || name_str.eq_ignore_ascii_case("x-api-key")
        {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }

    if !provider.api_key.trim().is_empty() {
        let key = provider.api_key.trim();
        req_builder = req_builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
        req_builder = req_builder.header("x-api-key", key);
    }

    let upstream_resp = match req_builder.body(outgoing_body).send().await {
        Ok(r) => r,
        Err(e) => {
            log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), incoming_stream, Some("network"));
            if translated {
                log::warn!("转发到 OpenAI 兼容上游失败: {e}");
                return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
            }
            return json_error(StatusCode::BAD_GATEWAY, format!("转发到上游失败: {e}"));
        }
    };

    let status = upstream_resp.status();
    let duration_ms = started.elapsed().as_millis() as i64;
    let error_category = (!status.is_success()).then(|| "upstream");
    let log_id = log_request(&state, &provider, Some(status.as_u16() as i64), duration_ms, uri.path(), incoming_stream, error_category);

    // OpenAI upstreams are normalized into Anthropic JSON. For an Anthropic
    // streaming request, keep the OpenAI upstream stream open and translate each
    // SSE frame as it arrives rather than waiting for a completed response.
    if translated {
        if incoming_stream && status.is_success() {
            let protocol = match provider.protocol_type {
                ProtocolType::OpenAiResponses => convert::OpenAiStreamProtocol::Responses,
                _ => convert::OpenAiStreamProtocol::Chat,
            };
            let db = Arc::clone(&state.db);
            let mut decoder = UpstreamSseDecoder::default();
            let mut converter = convert::OpenAiSseConverter::new(protocol, provider.model.trim());
            let stream_log_id = log_id.clone();
            let stream = upstream_resp.bytes_stream().map(move |chunk| {
                let output = match chunk {
                    Ok(bytes) => {
                        let mut output = Vec::new();
                        for item in decoder.push(&bytes) {
                            match item {
                                UpstreamSseItem::Json(event) => output.extend(converter.push_event(&event)),
                                UpstreamSseItem::Done => output.extend(converter.finish_stream()),
                            }
                        }
                        output
                    }
                    Err(_) => converter.error_event("上游流式响应中断"),
                };
                if let (Some(id), Some((input_tokens, output_tokens))) = (stream_log_id.as_deref(), converter.usage()) {
                    if let Err(error) = db.with_conn(|conn| update_proxy_log_usage(conn, id, input_tokens, output_tokens)) {
                        log::error!("更新代理请求 Token 用量失败: {error}");
                    }
                }
                Ok::<Bytes, Infallible>(Bytes::from(output))
            });
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header("x-accel-buffering", "no")
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"));
        }
        let response_bytes = match upstream_resp.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502)),
        };
        if !status.is_success() {
            return anthropic_error(status, convert::openai_error_to_anthropic(status.as_u16()));
        }
        let upstream: Value = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502)),
        };
        let anthropic = match provider.protocol_type {
            ProtocolType::OpenAiResponses => convert::openai_responses_to_anthropic(&upstream, provider.model.trim()),
            _ => convert::openai_chat_to_anthropic(&upstream, provider.model.trim()),
        };
        if let Some(id) = log_id.as_deref() {
            update_log_usage(&state, id, extract_usage_from_json(&serde_json::to_vec(&anthropic).unwrap_or_default()));
        }
        if incoming_stream {
            return Response::builder().status(status).header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(convert::anthropic_message_to_sse(&anthropic)))
                .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"));
        }
        return Response::builder().status(status).header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&anthropic).unwrap_or_default()))
            .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造响应失败"));
    }

    // Build the response, preserving upstream headers. Non-streaming responses are
    // inspected directly; streaming responses update usage when the final SSE
    // message_delta event arrives.
    let mut resp_builder = Response::builder().status(status);
    for (name, value) in upstream_resp.headers() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }
        resp_builder = resp_builder.header(name, value);
    }

    let is_streaming = upstream_resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));

    if !is_streaming {
        let response_bytes = match upstream_resp.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {e}")),
        };
        if let Some(id) = log_id.as_deref() {
            update_log_usage(&state, id, extract_usage_from_json(&response_bytes));
        }
        return resp_builder
            .body(Body::from(response_bytes))
            .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")));
    }

    let db = Arc::clone(&state.db);
    let mut sse_buffer = Vec::new();
    let stream = upstream_resp.bytes_stream().map(move |chunk| {
        if let (Some(id), Ok(bytes)) = (log_id.as_deref(), &chunk) {
            sse_buffer.extend_from_slice(bytes);
            while let Some(end) = sse_buffer.windows(2).position(|window| window == b"\n\n") {
                let event = sse_buffer.drain(..end + 2).collect::<Vec<_>>();
                if let Some(usage) = extract_usage_from_sse(&event) {
                    if let Err(e) = db.with_conn(|conn| update_proxy_log_usage(conn, id, usage.0, usage.1)) {
                        log::error!("更新代理请求 Token 用量失败: {e}");
                    }
                }
            }
        }
        chunk
    });
    let body = Body::from_stream(stream);

    resp_builder
        .body(body)
        .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")))
}

#[derive(Default)]
struct UpstreamSseDecoder {
    buffer: Vec<u8>,
}

enum UpstreamSseItem {
    Json(Value),
    Done,
}

impl UpstreamSseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Vec<UpstreamSseItem> {
        self.buffer.extend_from_slice(bytes);
        let mut items = Vec::new();
        while let Some((end, delimiter_len)) = find_sse_frame_end(&self.buffer) {
            let frame = self.buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
            let Ok(frame) = std::str::from_utf8(&frame) else { continue; };
            let data = frame.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() { continue; }
            if data == "[DONE]" {
                items.push(UpstreamSseItem::Done);
            } else if let Ok(value) = serde_json::from_str::<Value>(&data) {
                items.push(UpstreamSseItem::Json(value));
            }
        }
        items
    }
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

fn rewrite_body(provider: &Provider, original: &Bytes) -> Bytes {
    if !provider.protocol_type.uses_proxy() || provider.model.trim().is_empty() {
        return original.clone();
    }

    let mut value: Value = match serde_json::from_slice(original) {
        Ok(v) => v,
        Err(_) => return original.clone(),
    };

    if value.get("model").is_some() {
        value["model"] = Value::String(provider.model.trim().to_string());
    }

    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| original.clone())
}

fn log_request(
    state: &ProxyState,
    provider: &Provider,
    status: Option<i64>,
    duration_ms: i64,
    route: &str,
    is_stream: bool,
    error_category: Option<&str>,
) -> Option<String> {
    let model = if provider.model.trim().is_empty() {
        None
    } else {
        Some(provider.model.trim())
    };
    match state.db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            Some(&provider.id),
            Some(&provider.name),
            model,
            status,
            duration_ms,
            Some(state.target.as_str()),
            Some(provider.protocol_type.as_str()),
            Some(route),
            is_stream,
            error_category,
            error_category.map(error_diagnostic),
        )
    }) {
        Ok(id) => Some(id),
        Err(e) => {
            log::error!("写入代理请求日志失败: {e}");
            None
        }
    }
}

fn log_early_failure(
    state: &ProxyState,
    route: &str,
    error_category: &str,
    status: Option<i64>,
    duration_ms: i64,
) {
    if let Err(error) = state.db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            None,
            None,
            None,
            status,
            duration_ms,
            Some(state.target.as_str()),
            None,
            Some(route),
            false,
            Some(error_category),
            Some(error_diagnostic(error_category)),
        )
        .map(|_| ())
    }) {
        log::error!("写入代理早期失败日志失败: {error}");
    }
}

fn error_diagnostic(category: &str) -> &'static str {
    match category {
        "credential" => "credential unavailable",
        "configuration" => "provider configuration invalid",
        "request" => "request could not be parsed",
        "network" => "upstream connection failed",
        "upstream" => "upstream returned an error status",
        "provider" => "no active provider",
        _ => "proxy request failed",
    }
}

fn update_log_usage(state: &ProxyState, id: &str, usage: Option<(i64, i64)>) {
    let Some((input_tokens, output_tokens)) = usage else {
        return;
    };
    if let Err(e) = state.db.with_conn(|conn| {
        update_proxy_log_usage(conn, id, input_tokens, output_tokens)
    }) {
        log::error!("更新代理请求 Token 用量失败: {e}");
    }
}

fn extract_usage_from_json(bytes: &[u8]) -> Option<(i64, i64)> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    usage_from_value(&value)
}

fn extract_usage_from_sse(bytes: &[u8]) -> Option<(i64, i64)> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find_map(|value| usage_from_value(&value))
}

fn usage_from_value(value: &Value) -> Option<(i64, i64)> {
    let usage = value.get("usage")?;
    Some((
        usage.get("input_tokens")?.as_i64()?,
        usage.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
    ))
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = serde_json::json!({"error": message.into()});
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| status.into_response())
}

fn anthropic_error(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| status.into_response())
}

fn is_hop_by_hop_header(name: &str) -> bool {
    let name = name.as_bytes();
    matches!(
        name,
        b"connection"
            | b"keep-alive"
            | b"transfer-encoding"
            | b"te"
            | b"trailer"
            | b"proxy-authorization"
            | b"proxy-authenticate"
            | b"upgrade"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_sse_decoder_reassembles_split_json_frames() {
        let mut decoder = UpstreamSseDecoder::default();
        assert!(decoder.push(b"data: {\"type\":\"response.output_text").is_empty());
        let events = decoder.push(b".delta\",\"delta\":\"hi\"}\n\n");
        assert_eq!(events.len(), 1);
        let UpstreamSseItem::Json(event) = &events[0] else { panic!("expected JSON SSE event"); };
        assert_eq!(event["type"], "response.output_text.delta");
        assert_eq!(event["delta"], "hi");
    }

    #[test]
    fn upstream_sse_decoder_accepts_crlf_and_done() {
        let mut decoder = UpstreamSseDecoder::default();
        let events = decoder.push(b"data: [DONE]\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], UpstreamSseItem::Done));
    }
}
