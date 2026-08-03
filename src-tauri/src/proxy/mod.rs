//! Local HTTP proxy that exposes an Anthropic-compatible `/v1/messages` endpoint.
//!
//! Claude Desktop is pointed at `http://127.0.0.1:<port>`; the proxy forwards
//! requests to the active third-party provider after mapping the model name and
//! injecting the real API key. Request summaries are written to the SQLite log
//! table for the usage dashboard (P4).

mod convert;
mod codex;
mod codex_anthropic;
mod codex_auto_review;
mod codex_compact;
mod codex_history;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

use crate::database::dao::proxy_logs::{
    insert_proxy_log, maintain_proxy_logs as maintain_logs, update_proxy_log_diagnostic,
    update_proxy_log_usage_idempotent, extract_usage_envelope_id,
};
use crate::database::dao::providers::{get_current_provider, list_providers, resolve_api_key};
use crate::database::dao::settings::get_setting;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, protocol_endpoint_path_for_provider, resolve_upstream_model, ProtocolType,
    Provider, ProviderTarget,
};

const DEFAULT_PORT: u16 = 15821;
const LOG_RETENTION_DAYS_KEY: &str = "proxy_log_retention_days";
const LOG_MAX_ROWS_KEY: &str = "proxy_log_max_rows";
const LOG_AUTO_MAINTAIN_KEY: &str = "proxy_log_auto_maintain";
pub const PROXY_FAILOVER_ENABLED_KEY: &str = "proxy_failover_enabled";
pub const PROXY_RETRYABLE_STATUS_CODES_KEY: &str = "proxy_retryable_status_codes";
pub const PROXY_STREAMING_IDLE_TIMEOUT_KEY: &str = "proxy_streaming_idle_timeout_secs";
const DEFAULT_STREAMING_IDLE_TIMEOUT_SECS: u64 = 180;
const MAX_UPSTREAM_ERROR_BYTES: usize = 16 * 1024;
const CIRCUIT_FAILURE_THRESHOLD: u8 = 2;
const CIRCUIT_OPEN_SECONDS: u64 = 60;

#[derive(Debug, Clone, Default)]
pub(crate) struct UsageCounts {
    pub(crate) input_tokens: i64,
    pub(crate) cache_read_input_tokens: i64,
    pub(crate) cache_creation_input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) envelope_id: Option<String>,
}

/// Runtime handle for the local proxy.
pub struct ProxyManager {
    db: Arc<Database>,
    lifecycle_tx: UnboundedSender<ProxyLifecycleEvent>,
    code: Option<ProxyRuntime>,
    desktop: Option<ProxyRuntime>,
    codex: Option<ProxyRuntime>,
}

struct ProxyRuntime {
    handle: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
    port: u16,
}

impl ProxyManager {
    pub fn new(
        db: Arc<Database>,
        lifecycle_tx: UnboundedSender<ProxyLifecycleEvent>,
    ) -> Self {
        Self {
            db,
            lifecycle_tx,
            code: None,
            desktop: None,
            codex: None,
        }
    }

    pub fn status_for(&self, target: ProviderTarget) -> ProxyStatus {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.as_ref(),
            ProviderTarget::ClaudeDesktop => self.desktop.as_ref(),
            ProviderTarget::Codex => self.codex.as_ref(),
        };
        let running = runtime.is_some_and(|runtime| !runtime.handle.is_finished());
        ProxyStatus {
            running,
            port: runtime.map(|runtime| runtime.port).unwrap_or(match target {
                ProviderTarget::ClaudeCode => DEFAULT_PORT,
                ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1,
                ProviderTarget::Codex => DEFAULT_PORT + 2,
            }),
            target_provider: if running {
                self.db.with_conn(|conn| get_current_provider(conn, target)).ok().flatten().map(|provider| provider.name)
            } else { None },
            phase: if running { "running" } else { "stopped" }.to_string(),
            last_error: None,
            checked_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Start or replace one app's proxy without interrupting the other app.
    pub async fn start(&mut self, port: u16, target: ProviderTarget) -> AppResult<()> {
        let current = match target {
            ProviderTarget::ClaudeCode => self.code.as_ref(),
            ProviderTarget::ClaudeDesktop => self.desktop.as_ref(),
            ProviderTarget::Codex => self.codex.as_ref(),
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
                // Never inherit HTTP(S)_PROXY for upstream calls: the local listener is an
                // API gateway on 127.0.0.1, not a system proxy. Following env proxies can
                // create loops if HTTP_PROXY accidentally points at our own ports.
                .no_proxy()
                .build()
                .map_err(|e| AppError::Other(format!("创建 HTTP 客户端失败: {e}")))?,
            circuits: Arc::new(Mutex::new(std::collections::HashMap::new())),
            codex_history: Arc::new(codex_history::CodexHistoryStore::default()),
            target,
            port,
            started_at: Instant::now(),
        };

        let app = if target == ProviderTarget::Codex {
            Router::new()
                .route("/health", get(health_handler))
                .route("/v1/models", get(codex::codex_models_handler))
                .route("/v1/responses", any(codex::codex_proxy_handler))
                .route("/v1/responses/compact", any(codex::codex_proxy_handler))
                .route("/responses/compact", any(codex::codex_proxy_handler))
                .route("/v1/chat/completions", any(codex::codex_proxy_handler))
        } else {
            let mut app = Router::new()
                .route("/health", get(health_handler))
                .route("/v1/models", get(models_handler))
                .route("/v1/messages", any(proxy_handler));
            if target == ProviderTarget::ClaudeDesktop {
                app = app
                    .route(
                        &format!("{}/v1/models", crate::config::claude_desktop::CLAUDE_DESKTOP_PROXY_PREFIX),
                        get(models_handler),
                    )
                    .route(
                        &format!("{}/v1/messages", crate::config::claude_desktop::CLAUDE_DESKTOP_PROXY_PREFIX),
                        any(proxy_handler),
                    );
            }
            app
        };

        let app = app
            .layer(CorsLayer::permissive())
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });

        let lifecycle_tx = self.lifecycle_tx.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = server.await {
                log::error!("本地代理服务异常退出: {e}");
                let _ = lifecycle_tx.send(ProxyLifecycleEvent {
                    target,
                    error: e.to_string(),
                });
            }
        });

        let runtime = ProxyRuntime { handle, shutdown_tx, port };
        let previous = match target {
            ProviderTarget::ClaudeCode => self.code.replace(runtime),
            ProviderTarget::ClaudeDesktop => self.desktop.replace(runtime),
            ProviderTarget::Codex => self.codex.replace(runtime),
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
        self.stop_target(ProviderTarget::Codex);
        log::info!("本地代理已停止");
    }

    pub fn stop_target(&mut self, target: ProviderTarget) {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.take(),
            ProviderTarget::ClaudeDesktop => self.desktop.take(),
            ProviderTarget::Codex => self.codex.take(),
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
    pub phase: String,
    pub last_error: Option<String>,
    pub checked_at: i64,
}

#[derive(Debug)]
pub struct ProxyLifecycleEvent {
    pub target: ProviderTarget,
    pub error: String,
}

#[derive(Clone)]
pub(crate) struct ProxyState {
    pub(crate) db: Arc<Database>,
    pub(crate) client: Client,
    circuits: Arc<Mutex<std::collections::HashMap<String, ProviderCircuit>>>,
    pub(crate) codex_history: Arc<codex_history::CodexHistoryStore>,
    pub(crate) target: ProviderTarget,
    port: u16,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct ProviderCircuit {
    failures: u8,
    open_until: Option<Instant>,
}

struct PreparedUpstreamRequest {
    builder: reqwest::RequestBuilder,
    outgoing_body: Bytes,
    translated: bool,
}

fn circuit_is_open(state: &ProxyState, provider_id: &str) -> bool {
    let Ok(mut circuits) = state.circuits.lock() else { return false; };
    let Some(circuit) = circuits.get(provider_id) else { return false; };
    match circuit.open_until {
        Some(until) if until > Instant::now() => true,
        Some(_) => {
            circuits.remove(provider_id);
            false
        }
        None => false,
    }
}

pub(crate) fn record_provider_success(state: &ProxyState, provider_id: &str) {
    if let Ok(mut circuits) = state.circuits.lock() {
        circuits.remove(provider_id);
    }
}

pub(crate) fn record_provider_failure(state: &ProxyState, provider_id: &str) {
    if let Ok(mut circuits) = state.circuits.lock() {
        let circuit = circuits.entry(provider_id.to_string()).or_insert(ProviderCircuit {
            failures: 0,
            open_until: None,
        });
        circuit.failures = circuit.failures.saturating_add(1);
        if circuit.failures >= CIRCUIT_FAILURE_THRESHOLD {
            circuit.open_until = Some(Instant::now() + std::time::Duration::from_secs(CIRCUIT_OPEN_SECONDS));
        }
    }
}

pub(crate) fn next_failover_provider(state: &ProxyState, current_id: &str) -> AppResult<Option<Provider>> {
    let enabled = state.db.with_conn(|conn| get_setting(conn, PROXY_FAILOVER_ENABLED_KEY))?
        .as_deref() == Some("true");
    if !enabled { return Ok(None); }
    let candidates = state.db.with_conn(|conn| list_providers(conn, state.target))?;
    for mut candidate in candidates {
        if candidate.id == current_id || candidate.base_url.trim().is_empty() || circuit_is_open(state, &candidate.id) {
            continue;
        }
        if candidate.is_codex_oauth() {
            if let Ok((token, account_id)) =
                crate::codex_oauth::manager().get_valid_token(Some(&candidate.auth_binding))
            {
                candidate.api_key = token;
                candidate.auth_binding = account_id;
                candidate.base_url = crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string();
                candidate.protocol_type = ProtocolType::OpenAiResponses;
                return Ok(Some(candidate));
            }
            continue;
        }
        match state.db.with_conn(|conn| resolve_api_key(conn, &candidate.id)) {
            Ok(Some(key)) if !key.trim().is_empty() => {
                candidate.api_key = key;
                return Ok(Some(candidate));
            }
            Ok(_) | Err(_) => continue,
        }
    }
    Ok(None)
}

fn prepare_upstream_request(
    state: &ProxyState,
    provider: &mut Provider,
    method: &Method,
    headers: &HeaderMap,
    incoming: &Value,
    body_bytes: &Bytes,
    incoming_stream: bool,
) -> AppResult<PreparedUpstreamRequest> {
    let requested_model = incoming.get("model").and_then(Value::as_str).unwrap_or("");
    provider.model = resolve_upstream_model(provider, requested_model);
    let target_url = api_endpoint_url(
        &provider.base_url,
        protocol_endpoint_path_for_provider(provider),
    )?;
    let (outgoing_body, translated) =
        encode_upstream_request(provider, incoming, body_bytes, incoming_stream, headers);
    let mut builder = state.client.request(method.clone(), target_url).header(header::CONTENT_TYPE, "application/json");
    if !provider.is_codex_oauth() {
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
            builder = builder.header(name, value);
        }
    }
    let key = provider.api_key.trim();
    builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    if provider.is_codex_oauth() {
        builder = builder
            .header("originator", crate::codex_oauth::ORIGINATOR)
            .header("version", crate::codex_oauth::CLIENT_VERSION)
            .header("Chatgpt-Account-Id", provider.auth_binding.trim());
    } else {
        builder = builder.header("x-api-key", key);
    }
    Ok(PreparedUpstreamRequest { builder, outgoing_body, translated })
}

fn compatible_stream_retry(provider: &Provider, prepared: &PreparedUpstreamRequest, incoming_stream: bool) -> Option<reqwest::RequestBuilder> {
    if !prepared.translated || !incoming_stream || !matches!(provider.protocol_type, ProtocolType::OpenAiChat | ProtocolType::Proxy) {
        return None;
    }
    serde_json::from_slice::<Value>(&prepared.outgoing_body)
        .ok()
        .and_then(|mut value| {
            value.as_object_mut()?.remove("stream_options")?;
            serde_json::to_vec(&value).ok()
        })
        .and_then(|body| prepared.builder.try_clone().map(|builder| builder.body(body)))
}

pub(crate) fn is_retryable_upstream_status(state: &ProxyState, status: StatusCode) -> bool {
    let codes = state
        .db
        .with_conn(|conn| load_retryable_status_codes(conn))
        .unwrap_or_else(|_| default_retryable_status_codes());
    codes.contains(&status.as_u16())
}

pub fn default_retryable_status_codes() -> Vec<u16> {
    let mut codes = Vec::new();
    codes.extend(400..=404);
    codes.push(408);
    codes.push(429);
    codes.extend(500..=599);
    codes
}

pub fn load_retryable_status_codes(conn: &rusqlite::Connection) -> AppResult<Vec<u16>> {
    let Some(raw) = get_setting(conn, PROXY_RETRYABLE_STATUS_CODES_KEY)? else {
        return Ok(default_retryable_status_codes());
    };
    parse_retryable_status_codes(&raw)
}

pub fn parse_retryable_status_codes(raw: &str) -> AppResult<Vec<u16>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(default_retryable_status_codes());
    }
    let mut codes = Vec::new();
    for part in trimmed.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: u16 = start
                .trim()
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码范围: {part}")))?;
            let end: u16 = end
                .trim()
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码范围: {part}")))?;
            if start > end || start < 100 || end > 599 {
                return Err(AppError::Config(format!("无效的重试状态码范围: {part}")));
            }
            codes.extend(start..=end);
        } else {
            let code: u16 = part
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码: {part}")))?;
            if !(100..=599).contains(&code) {
                return Err(AppError::Config(format!("无效的重试状态码: {part}")));
            }
            codes.push(code);
        }
    }
    codes.sort_unstable();
    codes.dedup();
    if codes.is_empty() {
        Ok(default_retryable_status_codes())
    } else {
        Ok(codes)
    }
}

pub fn format_retryable_status_codes(codes: &[u16]) -> String {
    if codes.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut start = codes[0];
    let mut prev = codes[0];
    for &code in &codes[1..] {
        if code == prev + 1 {
            prev = code;
            continue;
        }
        if start == prev {
            parts.push(start.to_string());
        } else {
            parts.push(format!("{start}-{prev}"));
        }
        start = code;
        prev = code;
    }
    if start == prev {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}-{prev}"));
    }
    parts.join(",")
}

pub fn load_streaming_idle_timeout_secs(conn: &rusqlite::Connection) -> AppResult<u64> {
    let Some(raw) = get_setting(conn, PROXY_STREAMING_IDLE_TIMEOUT_KEY)? else {
        return Ok(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS);
    };
    let value = raw
        .trim()
        .parse::<u64>()
        .map_err(|_| AppError::Config("流式空闲超时必须是正整数秒".to_string()))?;
    Ok(value.clamp(5, 3600))
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

async fn models_handler(State(state): State<ProxyState>, headers: HeaderMap) -> Response {
    if let Err(error) = validate_desktop_gateway_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    match state
        .db
        .with_conn(|conn| get_current_provider(conn, state.target))
    {
        Ok(Some(provider)) => {
            axum::Json(crate::config::claude_desktop::model_list_response(&provider))
                .into_response()
        }
        Ok(None) => json_error(StatusCode::SERVICE_UNAVAILABLE, "没有激活的第三方供应商"),
        Err(error) => {
            log::error!("读取模型目录失败: {error}");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录")
        }
    }
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if let Err(error) = validate_desktop_gateway_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
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
    if provider.is_codex_oauth() {
        match crate::codex_oauth::manager().get_valid_token(Some(&provider.auth_binding)) {
            Ok((token, account_id)) => {
                provider.api_key = token;
                provider.auth_binding = account_id;
                provider.base_url = crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string();
                provider.protocol_type = ProtocolType::OpenAiResponses;
            }
            Err(error) => {
                log::error!("代理读取 ChatGPT OAuth 凭据失败: {error}");
                return json_error(StatusCode::SERVICE_UNAVAILABLE, "ChatGPT 登录已失效");
            }
        }
    } else {
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
    }
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
    let prepared = match prepare_upstream_request(&state, &mut provider, &method, &headers, &incoming, &body_bytes, incoming_stream) {
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
    let mut translated = prepared.translated;
    let mut retry_without_stream_options = compatible_stream_retry(&provider, &prepared, incoming_stream);
    let mut upstream_resp = match prepared.builder.body(prepared.outgoing_body).send().await {
        Ok(r) => r,
        Err(e) => {
            record_provider_failure(&state, &provider.id);
            let fallback = next_failover_provider(&state, &provider.id).ok().flatten();
            let Some(mut fallback) = fallback else {
                log_request(&state, &provider, Some(502), started.elapsed().as_millis() as i64, uri.path(), incoming_stream, Some("network"));
                if translated {
                    log::warn!("转发到 OpenAI 兼容上游失败: {e}");
                    return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
                }
                return json_error(StatusCode::BAD_GATEWAY, format!("转发到上游失败: {e}"));
            };
            log::warn!("供应商 {} 网络请求失败，尝试故障切换到 {}", provider.id, fallback.id);
            let fallback_prepared = match prepare_upstream_request(&state, &mut fallback, &method, &headers, &incoming, &body_bytes, incoming_stream) {
                Ok(request) => request,
                Err(_) => {
                    log_request(&state, &provider, Some(502), started.elapsed().as_millis() as i64, uri.path(), incoming_stream, Some("network"));
                    return json_error(StatusCode::BAD_GATEWAY, "首选供应商不可用，备用供应商配置无效");
                }
            };
            translated = fallback_prepared.translated;
            retry_without_stream_options = compatible_stream_retry(&fallback, &fallback_prepared, incoming_stream);
            match fallback_prepared.builder.body(fallback_prepared.outgoing_body).send().await {
                Ok(response) => {
                    provider = fallback;
                    response
                }
                Err(fallback_error) => {
                    record_provider_failure(&state, &fallback.id);
                    log_request(&state, &fallback, Some(502), started.elapsed().as_millis() as i64, uri.path(), incoming_stream, Some("network"));
                    if translated {
                        return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
                    }
                    return json_error(StatusCode::BAD_GATEWAY, format!("首选与备用供应商均无法连接: {fallback_error}"));
                }
            }
        }
    };

    if is_retryable_upstream_status(&state, upstream_resp.status()) {
        record_provider_failure(&state, &provider.id);
        if let Ok(Some(mut fallback)) = next_failover_provider(&state, &provider.id) {
            log::warn!("供应商 {} 返回 {}，尝试故障切换到 {}", provider.id, upstream_resp.status(), fallback.id);
            if let Ok(fallback_prepared) = prepare_upstream_request(&state, &mut fallback, &method, &headers, &incoming, &body_bytes, incoming_stream) {
                let fallback_translated = fallback_prepared.translated;
                let fallback_retry = compatible_stream_retry(&fallback, &fallback_prepared, incoming_stream);
                match fallback_prepared.builder.body(fallback_prepared.outgoing_body).send().await {
                    Ok(response) => {
                        provider = fallback;
                        translated = fallback_translated;
                        retry_without_stream_options = fallback_retry;
                        upstream_resp = response;
                    }
                    Err(error) => {
                        record_provider_failure(&state, &fallback.id);
                        log::warn!("备用供应商 {} 连接失败: {error}", fallback.id);
                    }
                }
            }
        }
    }

    if let Some(retry_request) = retry_without_stream_options {
        let rejected_status = upstream_resp.status();
        if matches!(
            rejected_status,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ) {
            let rejected_body = match upstream_resp.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let log_id = log_request(
                        &state,
                        &provider,
                        Some(502),
                        started.elapsed().as_millis() as i64,
                        uri.path(),
                        incoming_stream,
                        Some("network"),
                    );
                    update_log_diagnostic(
                        &state,
                        log_id.as_deref(),
                        "network",
                        "读取 stream_options 兼容性错误响应失败",
                    );
                    log::warn!("读取 OpenAI stream_options 兼容性错误失败: {error}");
                    return anthropic_error(
                        StatusCode::BAD_GATEWAY,
                        convert::openai_error_to_anthropic(502),
                    );
                }
            };
            if explicitly_rejects_stream_options(&rejected_body) {
                log::info!(
                    "上游明确不支持 stream_options.include_usage，移除该字段后兼容重试一次"
                );
                upstream_resp = match retry_request.send().await {
                    Ok(response) => response,
                    Err(error) => {
                        let log_id = log_request(
                            &state,
                            &provider,
                            Some(502),
                            started.elapsed().as_millis() as i64,
                            uri.path(),
                            incoming_stream,
                            Some("network"),
                        );
                        update_log_diagnostic(
                            &state,
                            log_id.as_deref(),
                            "network",
                            "移除 stream_options 后的兼容重试连接失败",
                        );
                        log::warn!("OpenAI stream_options 兼容重试失败: {error}");
                        return anthropic_error(
                            StatusCode::BAD_GATEWAY,
                            convert::openai_error_to_anthropic(502),
                        );
                    }
                };
            } else {
                let log_id = log_request(
                    &state,
                    &provider,
                    Some(rejected_status.as_u16() as i64),
                    started.elapsed().as_millis() as i64,
                    uri.path(),
                    incoming_stream,
                    Some("upstream"),
                );
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "upstream",
                    &sanitized_upstream_diagnostic(rejected_status, &rejected_body),
                );
                return anthropic_error(
                    rejected_status,
                    convert::openai_error_to_anthropic(rejected_status.as_u16()),
                );
            }
        }
    }

    let status = upstream_resp.status();
    if status.is_success() {
        record_provider_success(&state, &provider.id);
    }
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
            let decoder = UpstreamSseDecoder::default();
            let converter = convert::OpenAiSseConverter::new(protocol, provider.model.trim());
            let stream_log_id = log_id.clone();
            let target_app = state.target.as_str().to_string();
            let provider_id = provider.id.clone();
            let idle = Duration::from_secs(
                state
                    .db
                    .with_conn(load_streaming_idle_timeout_secs)
                    .unwrap_or(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS),
            );
            let upstream_stream = upstream_resp.bytes_stream();
            let stream = futures_util::stream::unfold(
                (upstream_stream, decoder, converter, false),
                move |(mut upstream_stream, mut decoder, mut converter, done)| {
                    let db = Arc::clone(&db);
                    let stream_log_id = stream_log_id.clone();
                    let target_app = target_app.clone();
                    let provider_id = provider_id.clone();
                    async move {
                        if done {
                            return None;
                        }
                        let next = tokio::time::timeout(idle, upstream_stream.next()).await;
                        let (output, done) = match next {
                            Ok(Some(Ok(bytes))) => {
                                let mut output = Vec::new();
                                for item in decoder.push(&bytes) {
                                    match item {
                                        UpstreamSseItem::Json(event) => {
                                            output.extend(converter.push_event(&event))
                                        }
                                        UpstreamSseItem::Done => {
                                            output.extend(converter.finish_stream())
                                        }
                                    }
                                }
                                (output, false)
                            }
                            Ok(Some(Err(_))) => {
                                (converter.error_event("上游流式响应中断"), true)
                            }
                            Ok(None) => {
                                let output = converter.finish_stream();
                                (output, true)
                            }
                            Err(_) => (converter.error_event("流式响应空闲超时"), true),
                        };
                        if let (Some(id), Some(usage)) =
                            (stream_log_id.as_deref(), converter.usage())
                        {
                            if let Err(error) = db.with_conn(|conn| {
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
                            }) {
                                log::error!("更新代理请求 Token 用量失败: {error}");
                            }
                        }
                        if output.is_empty() && done {
                            return None;
                        }
                        Some((
                            Ok::<Bytes, Infallible>(Bytes::from(output)),
                            (upstream_stream, decoder, converter, done),
                        ))
                    }
                },
            );
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
            Err(_) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "network",
                    "读取 OpenAI 上游响应失败",
                );
                return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
            }
        };
        if !status.is_success() {
            update_log_diagnostic(
                &state,
                log_id.as_deref(),
                "upstream",
                &sanitized_upstream_diagnostic(status, &response_bytes),
            );
            return anthropic_error(status, convert::openai_error_to_anthropic(status.as_u16()));
        }
        let upstream: Value = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "conversion",
                    "OpenAI 上游返回了无法转换的非 JSON 成功响应",
                );
                return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
            }
        };
        if provider.protocol_type == ProtocolType::OpenAiResponses {
            if let Some(failed) = convert::responses_failed_anthropic_error(&upstream) {
                record_provider_failure(&state, &provider.id);
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "upstream",
                    failed
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Responses status=failed"),
                );
                if let Ok(Some(mut fallback)) = next_failover_provider(&state, &provider.id) {
                    log::warn!(
                        "供应商 {} 返回 Responses failed envelope，尝试故障切换到 {}",
                        provider.id,
                        fallback.id
                    );
                    if let Ok(fallback_prepared) = prepare_upstream_request(
                        &state,
                        &mut fallback,
                        &method,
                        &headers,
                        &incoming,
                        &body_bytes,
                        incoming_stream,
                    ) {
                        let fallback_translated = fallback_prepared.translated;
                        match fallback_prepared
                            .builder
                            .body(fallback_prepared.outgoing_body)
                            .send()
                            .await
                        {
                            Ok(response) if response.status().is_success() => {
                                let fallback_bytes = match response.bytes().await {
                                    Ok(bytes) => bytes,
                                    Err(_) => {
                                        record_provider_failure(&state, &fallback.id);
                                        return anthropic_error(
                                            StatusCode::BAD_GATEWAY,
                                            failed,
                                        );
                                    }
                                };
                                let fallback_json: Value = match serde_json::from_slice(&fallback_bytes)
                                {
                                    Ok(value) => value,
                                    Err(_) => {
                                        record_provider_failure(&state, &fallback.id);
                                        return anthropic_error(
                                            StatusCode::BAD_GATEWAY,
                                            failed,
                                        );
                                    }
                                };
                                if fallback.protocol_type == ProtocolType::OpenAiResponses
                                    && convert::responses_failed_anthropic_error(&fallback_json)
                                        .is_some()
                                {
                                    record_provider_failure(&state, &fallback.id);
                                    return anthropic_error(StatusCode::BAD_GATEWAY, failed);
                                }
                                provider = fallback;
                                let _ = fallback_translated;
                                let anthropic = match provider.protocol_type {
                                    ProtocolType::OpenAiResponses => {
                                        convert::openai_responses_to_anthropic(
                                            &fallback_json,
                                            provider.model.trim(),
                                        )
                                    }
                                    _ => convert::openai_chat_to_anthropic(
                                        &fallback_json,
                                        provider.model.trim(),
                                    ),
                                };
                                record_provider_success(&state, &provider.id);
                                if let Some(id) = log_id.as_deref() {
                                    update_log_usage(
                                        &state,
                                        &provider,
                                        id,
                                        extract_usage_from_json(
                                            &serde_json::to_vec(&anthropic).unwrap_or_default(),
                                        ),
                                    );
                                }
                                if incoming_stream {
                                    return Response::builder()
                                        .status(StatusCode::OK)
                                        .header(header::CONTENT_TYPE, "text/event-stream")
                                        .header(header::CACHE_CONTROL, "no-cache")
                                        .body(Body::from(convert::anthropic_message_to_sse(&anthropic)))
                                        .unwrap_or_else(|_| {
                                            json_error(
                                                StatusCode::INTERNAL_SERVER_ERROR,
                                                "构造流式响应失败",
                                            )
                                        });
                                }
                                return Response::builder()
                                    .status(StatusCode::OK)
                                    .header(header::CONTENT_TYPE, "application/json")
                                    .body(Body::from(
                                        serde_json::to_vec(&anthropic).unwrap_or_default(),
                                    ))
                                    .unwrap_or_else(|_| {
                                        json_error(
                                            StatusCode::INTERNAL_SERVER_ERROR,
                                            "构造响应失败",
                                        )
                                    });
                            }
                            Ok(_) | Err(_) => {
                                record_provider_failure(&state, &fallback.id);
                            }
                        }
                    }
                }
                return anthropic_error(StatusCode::BAD_GATEWAY, failed);
            }
        }
        let anthropic = match provider.protocol_type {
            ProtocolType::OpenAiResponses => convert::openai_responses_to_anthropic(&upstream, provider.model.trim()),
            _ => convert::openai_chat_to_anthropic(&upstream, provider.model.trim()),
        };
        if let Some(id) = log_id.as_deref() {
            update_log_usage(
                &state,
                &provider,
                id,
                extract_usage_from_json(&serde_json::to_vec(&anthropic).unwrap_or_default()),
            );
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
            Err(e) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "network",
                    "读取 Anthropic 上游响应失败",
                );
                return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {e}"));
            }
        };
        if !status.is_success() {
            update_log_diagnostic(
                &state,
                log_id.as_deref(),
                "upstream",
                &sanitized_upstream_diagnostic(status, &response_bytes),
            );
        }
        if let Some(id) = log_id.as_deref() {
            update_log_usage(&state, &provider, id, extract_usage_from_json(&response_bytes));
        }
        return resp_builder
            .body(Body::from(response_bytes))
            .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")));
    }

    let db = Arc::clone(&state.db);
    let sse_buffer = Vec::new();
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let idle = Duration::from_secs(
        state
            .db
            .with_conn(load_streaming_idle_timeout_secs)
            .unwrap_or(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS),
    );
    let upstream_stream = upstream_resp.bytes_stream();
    let stream_log_id = log_id.clone();
    let stream = futures_util::stream::unfold(
        (upstream_stream, sse_buffer, false),
        move |(mut upstream_stream, mut sse_buffer, done)| {
            let db = Arc::clone(&db);
            let target_app = target_app.clone();
            let provider_id = provider_id.clone();
            let stream_log_id = stream_log_id.clone();
            async move {
                if done {
                    return None;
                }
                match tokio::time::timeout(idle, upstream_stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        if let Some(id) = stream_log_id.as_deref() {
                            sse_buffer.extend_from_slice(&bytes);
                            while let Some(end) =
                                sse_buffer.windows(2).position(|window| window == b"\n\n")
                            {
                                let event = sse_buffer.drain(..end + 2).collect::<Vec<_>>();
                                if let Some(usage) = extract_usage_from_sse(&event) {
                                    if let Err(e) = db.with_conn(|conn| {
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
                                    }) {
                                        log::error!("更新代理请求 Token 用量失败: {e}");
                                    }
                                }
                            }
                        }
                        Some((
                            Ok::<Bytes, reqwest::Error>(bytes),
                            (upstream_stream, sse_buffer, false),
                        ))
                    }
                    Ok(Some(Err(error))) => Some((Err(error), (upstream_stream, sse_buffer, true))),
                    Ok(None) => None,
                    Err(_) => {
                        let message = convert::OpenAiSseConverter::new(
                            convert::OpenAiStreamProtocol::Chat,
                            "proxy",
                        )
                        .error_event("流式响应空闲超时");
                        Some((
                            Ok(Bytes::from(message)),
                            (upstream_stream, sse_buffer, true),
                        ))
                    }
                }
            }
        },
    );
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
    if provider.model.trim().is_empty() {
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

pub(crate) fn session_prompt_cache_hint(headers: &HeaderMap) -> Option<String> {
    for name in [
        "x-session-id",
        "x-chatgpt-session-id",
        "x-conversation-id",
        "session_id",
    ] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn encode_upstream_request(
    provider: &Provider,
    incoming: &Value,
    original: &Bytes,
    stream: bool,
    headers: &HeaderMap,
) -> (Bytes, bool) {
    match provider.protocol_type {
        ProtocolType::OpenAiChat | ProtocolType::Proxy => {
            let mut request =
                convert::anthropic_to_openai_chat(incoming, provider.model.trim(), stream);
            convert::reinject_chat_prompt_cache_key(
                &mut request,
                incoming
                    .get("prompt_cache_key")
                    .and_then(Value::as_str),
                session_prompt_cache_hint(headers).as_deref(),
                convert::chat_prompt_cache_allowed_for_base_url(&provider.base_url),
            );
            (
                Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                true,
            )
        }
        ProtocolType::OpenAiResponses => {
            let mut request =
                convert::anthropic_to_openai_responses(incoming, provider.model.trim(), stream);
            if provider.is_codex_oauth() {
                convert::apply_codex_oauth_response_body(&mut request);
            }
            (
                Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                true,
            )
        }
        ProtocolType::Anthropic => (rewrite_body(provider, original), false),
    }
}

pub(crate) fn log_request(
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

fn validate_desktop_gateway_auth(state: &ProxyState, headers: &HeaderMap) -> AppResult<()> {
    if state.target != ProviderTarget::ClaudeDesktop {
        return Ok(());
    }
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    crate::config::claude_desktop::validate_gateway_auth_header(auth)
}

fn gateway_auth_error(error: AppError) -> Response {
    json_error(StatusCode::UNAUTHORIZED, error.to_string())
}

pub(crate) fn log_early_failure(
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
        "conversion" => "upstream response conversion failed",
        "provider" => "no active provider",
        _ => "proxy request failed",
    }
}

fn update_log_diagnostic(
    state: &ProxyState,
    id: Option<&str>,
    category: &str,
    diagnostic: &str,
) {
    let Some(id) = id else {
        return;
    };
    if let Err(error) = state.db.with_conn(|conn| {
        update_proxy_log_diagnostic(conn, id, category, diagnostic)
    }) {
        log::error!("更新代理错误诊断失败: {error}");
    }
}

fn sanitized_upstream_diagnostic(status: StatusCode, bytes: &[u8]) -> String {
    let limited = &bytes[..bytes.len().min(MAX_UPSTREAM_ERROR_BYTES)];
    let value = serde_json::from_slice::<Value>(limited).ok();
    let mut fields = Vec::new();
    if let Some(value) = value.as_ref() {
        let error = value.get("error").unwrap_or(value);
        for (label, candidate) in [
            ("type", error.get("type")),
            ("code", error.get("code")),
            ("message", error.get("message").or_else(|| value.get("message"))),
            (
                "request_id",
                value
                    .get("request_id")
                    .or_else(|| value.get("requestId"))
                    .or_else(|| error.get("request_id")),
            ),
        ] {
            let text = candidate.and_then(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            });
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                fields.push(format!("{label}={}", sanitize_diagnostic_text(&text)));
            }
        }
    }
    if fields.is_empty() {
        format!("上游返回 HTTP {}，未提供可安全展示的错误摘要", status.as_u16())
    } else {
        format!("上游 HTTP {}；{}", status.as_u16(), fields.join("；"))
    }
}

fn explicitly_rejects_stream_options(bytes: &[u8]) -> bool {
    let limited = &bytes[..bytes.len().min(MAX_UPSTREAM_ERROR_BYTES)];
    let text = String::from_utf8_lossy(limited).to_lowercase();
    text.contains("stream_options")
        && [
            "unknown",
            "unsupported",
            "unrecognized",
            "not allowed",
            "extra field",
            "不支持",
            "未知",
        ]
        .iter()
        .any(|marker| text.contains(marker))
}

fn sanitize_diagnostic_text(value: &str) -> String {
    crate::log_redact::redact_secrets(value)
}

fn update_log_usage(state: &ProxyState, provider: &Provider, id: &str, usage: Option<UsageCounts>) {
    let Some(usage) = usage else {
        return;
    };
    if let Err(e) = state.db.with_conn(|conn| {
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
    }) {
        log::error!("更新代理请求 Token 用量失败: {e}");
    }
}

pub(crate) fn extract_usage_from_json(bytes: &[u8]) -> Option<UsageCounts> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    usage_from_value(&value)
}

pub(crate) fn extract_usage_from_sse(bytes: &[u8]) -> Option<UsageCounts> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|value| usage_from_value(&value))
        .last()
}

fn usage_from_value(value: &Value) -> Option<UsageCounts> {
    let usage = value.get("usage").or_else(|| value.pointer("/response/usage"))?;
    let anthropic_input = usage.get("input_tokens").and_then(Value::as_i64);
    let total_input = anthropic_input
        .or_else(|| usage.get("prompt_tokens").and_then(Value::as_i64))?;
    let cache_read = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cached_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, total_input);
    Some(UsageCounts {
        input_tokens: anthropic_input.unwrap_or_else(|| total_input.saturating_sub(cache_read)),
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        output_tokens: usage.get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        envelope_id: extract_usage_envelope_id(value),
    })
}

pub(crate) fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
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

pub(crate) fn is_hop_by_hop_header(name: &str) -> bool {
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
    use crate::database::Database;
    use crate::provider::{ClaudeModelMapping, ProviderKind, ProviderTarget};

    fn provider(protocol_type: ProtocolType) -> Provider {
        Provider {
            id: "mapped".into(),
            name: "Mapped".into(),
            base_url: "https://api.example.test".into(),
            api_key: "secret".into(),
            api_key_set: true,
            model: "opus-upstream".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            sort_index: 0,
            is_current: true,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
        }
    }

    fn circuit_test_state() -> ProxyState {
        ProxyState {
            db: Arc::new(Database::memory().unwrap()),
            client: Client::new(),
            circuits: Arc::new(Mutex::new(std::collections::HashMap::new())),
            codex_history: Arc::new(super::codex_history::CodexHistoryStore::default()),
            target: ProviderTarget::ClaudeCode,
            port: DEFAULT_PORT,
            started_at: Instant::now(),
        }
    }

    #[test]
    fn provider_circuit_opens_after_two_failures_and_resets_on_success() {
        let state = circuit_test_state();
        record_provider_failure(&state, "provider-a");
        assert!(!circuit_is_open(&state, "provider-a"));
        record_provider_failure(&state, "provider-a");
        assert!(circuit_is_open(&state, "provider-a"));
        record_provider_success(&state, "provider-a");
        assert!(!circuit_is_open(&state, "provider-a"));
    }

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

    #[test]
    fn all_protocols_send_the_resolved_model() {
        for requested_model in [
            "claude-opus-5",
            "claude-opus-5[1m]",
            "claude-opus-4-8",
            "claude-opus-4-8[1m]",
        ] {
            let incoming = serde_json::json!({
                "model": requested_model,
                "max_tokens": 32,
                "messages": [{"role": "user", "content": "hello"}],
            });
            let original = Bytes::from(serde_json::to_vec(&incoming).unwrap());

            for protocol in [
                ProtocolType::Anthropic,
                ProtocolType::OpenAiChat,
                ProtocolType::OpenAiResponses,
            ] {
                let provider = provider(protocol);
                let (body, _) =
                    encode_upstream_request(&provider, &incoming, &original, false, &HeaderMap::new());
                let value: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(
                    value["model"], "opus-upstream",
                    "{protocol:?} / {requested_model}"
                );
            }
        }
    }

    #[test]
    fn kimi_chat_encode_reinjects_session_prompt_cache_key() {
        let mut provider = provider(ProtocolType::OpenAiChat);
        provider.base_url = "https://api.moonshot.cn/v1".into();
        let incoming = serde_json::json!({
            "model": "claude-sonnet-5",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hello"}],
        });
        let original = Bytes::from(serde_json::to_vec(&incoming).unwrap());
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", "sess-kimi".parse().unwrap());
        let (body, translated) =
            encode_upstream_request(&provider, &incoming, &original, false, &headers);
        assert!(translated);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["prompt_cache_key"], "sess-kimi");
    }

    #[test]
    fn upstream_diagnostic_keeps_safe_fields_and_redacts_tokens() {
        let body = br#"{"error":{"type":"gateway_error","code":"bad_gateway","message":"Bearer secret-token failed"},"request_id":"req_1"}"#;
        let diagnostic = sanitized_upstream_diagnostic(StatusCode::BAD_GATEWAY, body);
        assert!(diagnostic.contains("HTTP 502"));
        assert!(diagnostic.contains("gateway_error"));
        assert!(diagnostic.contains("req_1"));
        assert!(!diagnostic.contains("secret-token"));
        assert!(diagnostic.contains("[redacted]"));
    }

    #[test]
    fn stream_options_retry_requires_an_explicit_parameter_rejection() {
        assert!(explicitly_rejects_stream_options(
            br#"{"error":{"message":"Unknown field: stream_options"}}"#
        ));
        assert!(!explicitly_rejects_stream_options(
            br#"{"error":{"message":"Temporary upstream failure"}}"#
        ));
        assert!(!explicitly_rejects_stream_options(
            br#"{"error":{"message":"Unknown model"}}"#
        ));
    }

    #[test]
    fn usage_parser_preserves_anthropic_input_and_supports_openai_usage() {
        let anthropic = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 40,
                "cache_creation_input_tokens": 5,
                "output_tokens": 20
            }
        });
        let parsed = usage_from_value(&anthropic).expect("anthropic usage");
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.cache_read_input_tokens, 40);
        assert_eq!(parsed.output_tokens, 20);

        let openai = serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 40 }
            }
        });
        let parsed = usage_from_value(&openai).expect("OpenAI-compatible usage");
        assert_eq!(parsed.input_tokens, 60);
        assert_eq!(parsed.cache_read_input_tokens, 40);
        assert_eq!(parsed.output_tokens, 20);
    }

    #[test]
    fn kimi_anthropic_usage_and_final_stream_frame_are_preserved() {
        let kimi = br#"{"usage":{"input_tokens":321,"cache_read_input_tokens":12,"output_tokens":45}}"#;
        let parsed = extract_usage_from_json(kimi).expect("Kimi Anthropic usage");
        assert_eq!(parsed.input_tokens, 321);
        assert_eq!(parsed.cache_read_input_tokens, 12);
        assert_eq!(parsed.output_tokens, 45);

        let stream = br#"data: {"type":"content_block_delta"}

data: {"type":"message_delta","usage":{"input_tokens":321,"output_tokens":45}}

data: {"type":"message_delta","usage":{"input_tokens":321,"output_tokens":67}}
"#;
        let parsed = extract_usage_from_sse(stream).expect("final Kimi stream usage");
        assert_eq!(parsed.input_tokens, 321);
        assert_eq!(parsed.output_tokens, 67);
    }
}
