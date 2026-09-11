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
    opencode: Option<ProxyRuntime>,
    pi: Option<ProxyRuntime>,
    dsh: Option<ProxyRuntime>,
    cline: Option<ProxyRuntime>,
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
            opencode: None,
            pi: None,
            dsh: None,
            cline: None,
        }
    }

    pub fn status_for(&self, target: ProviderTarget) -> ProxyStatus {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.as_ref(),
            ProviderTarget::ClaudeDesktop => self.desktop.as_ref(),
            ProviderTarget::Codex => self.codex.as_ref(),
            ProviderTarget::OpenCode => self.opencode.as_ref(),
            ProviderTarget::Pi => self.pi.as_ref(),
            ProviderTarget::Dsh => self.dsh.as_ref(),
            ProviderTarget::Cline => self.cline.as_ref(),
        };
        let running = runtime.is_some_and(|runtime| !runtime.handle.is_finished());
        ProxyStatus {
            running,
            port: runtime.map(|runtime| runtime.port).unwrap_or(match target {
                ProviderTarget::ClaudeCode => DEFAULT_PORT,
                ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1,
                ProviderTarget::Codex => DEFAULT_PORT + 2,
                ProviderTarget::OpenCode => DEFAULT_PORT + 3,
                ProviderTarget::Pi => DEFAULT_PORT + 4,
                ProviderTarget::Dsh => DEFAULT_PORT + 5,
                ProviderTarget::Cline => DEFAULT_PORT + 6,
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
            ProviderTarget::OpenCode => self.opencode.as_ref(),
            ProviderTarget::Pi => self.pi.as_ref(),
            ProviderTarget::Dsh => self.dsh.as_ref(),
            ProviderTarget::Cline => self.cline.as_ref(),
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
            listener_kind: ListenerKind::Agent,
            port,
            started_at: Instant::now(),
            correlation: None,
            request_path: String::new(),
        };

        let app = if matches!(
            target,
            ProviderTarget::Codex | ProviderTarget::Cline | ProviderTarget::OpenCode | ProviderTarget::Dsh
        ) {
            let mut app = Router::new()
                .route("/health", get(health_handler))
                .route("/v1/models", get(codex::codex_models_handler))
                .route("/v1/responses", any(codex::codex_proxy_handler))
                .route("/v1/responses/compact", any(codex::codex_proxy_handler))
                .route("/responses/compact", any(codex::codex_proxy_handler))
                .route("/v1/chat/completions", any(codex::codex_proxy_handler));
            if matches!(target, ProviderTarget::OpenCode | ProviderTarget::Dsh) {
                app = app.route("/v1/messages", any(proxy_handler));
            }
            app
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
            ProviderTarget::OpenCode => self.opencode.replace(runtime),
            ProviderTarget::Pi => self.pi.replace(runtime),
            ProviderTarget::Dsh => self.dsh.replace(runtime),
            ProviderTarget::Cline => self.cline.replace(runtime),
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
        self.stop_target(ProviderTarget::OpenCode);
        self.stop_target(ProviderTarget::Pi);
        self.stop_target(ProviderTarget::Dsh);
        self.stop_target(ProviderTarget::Cline);
        log::info!("本地代理已停止");
    }

    /// Stop all proxies and wait briefly so sockets can release before process exit.
    ///
    /// Used by the Windows updater path, which hard-exits after launching NSIS `/R`.
    pub async fn stop_graceful(&mut self) {
        self.stop_target_graceful(ProviderTarget::ClaudeCode).await;
        self.stop_target_graceful(ProviderTarget::ClaudeDesktop).await;
        self.stop_target_graceful(ProviderTarget::Codex).await;
        self.stop_target_graceful(ProviderTarget::OpenCode).await;
        self.stop_target_graceful(ProviderTarget::Pi).await;
        self.stop_target_graceful(ProviderTarget::Dsh).await;
        self.stop_target_graceful(ProviderTarget::Cline).await;
        log::info!("本地代理已优雅停止");
    }

    pub fn stop_target(&mut self, target: ProviderTarget) {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.take(),
            ProviderTarget::ClaudeDesktop => self.desktop.take(),
            ProviderTarget::Codex => self.codex.take(),
            ProviderTarget::OpenCode => self.opencode.take(),
            ProviderTarget::Pi => self.pi.take(),
            ProviderTarget::Dsh => self.dsh.take(),
            ProviderTarget::Cline => self.cline.take(),
        };
        if let Some(runtime) = runtime {
            let _ = runtime.shutdown_tx.send(());
            runtime.handle.abort();
        }
    }

    async fn stop_target_graceful(&mut self, target: ProviderTarget) {
        let runtime = match target {
            ProviderTarget::ClaudeCode => self.code.take(),
            ProviderTarget::ClaudeDesktop => self.desktop.take(),
            ProviderTarget::Codex => self.codex.take(),
            ProviderTarget::OpenCode => self.opencode.take(),
            ProviderTarget::Pi => self.pi.take(),
            ProviderTarget::Dsh => self.dsh.take(),
            ProviderTarget::Cline => self.cline.take(),
        };
        let Some(runtime) = runtime else {
            return;
        };
        let _ = runtime.shutdown_tx.send(());
        let abort = runtime.handle.abort_handle();
        match tokio::time::timeout(Duration::from_millis(1_500), runtime.handle).await {
            Ok(_) => {
                // Even after graceful shutdown, Windows may keep the port in TIME_WAIT.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(_) => {
                abort.abort();
                // Give the OS a moment to reclaim the listen port after abort.
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
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

pub fn smart_gateway_router(db: Arc<Database>, port: u16) -> Router {
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .no_proxy()
        .build()
        .unwrap_or_else(|_| Client::new());
    let state = ProxyState {
        db,
        client,
        circuits: Arc::new(Mutex::new(std::collections::HashMap::new())),
        codex_history: Arc::new(codex_history::CodexHistoryStore::default()),
        target: ProviderTarget::ClaudeCode,
        listener_kind: ListenerKind::SmartGateway,
        port,
        started_at: Instant::now(),
        correlation: None,
        request_path: String::new(),
    };
    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/messages", any(proxy_handler))
        .route("/v1/chat/completions", any(smart_gateway_openai_handler))
        .route("/v1/responses", any(smart_gateway_openai_handler))
        .route("/v1/responses/compact", any(smart_gateway_openai_handler))
        .route("/responses/compact", any(smart_gateway_openai_handler))
        .route("/v1/images/generations", any(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// OpenAI Chat / Responses on the smart gateway: require the public API key (or a
/// bound-app token) and honor `x-ai-switcher-target` the same way `/v1/messages` does.
async fn smart_gateway_openai_handler(
    State(mut state): State<ProxyState>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(error) = validate_listener_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    if let Some(target) = resolve_binding_target(&state, &headers) {
        state.target = target;
    }
    codex::codex_proxy_handler(State(state), uri, method, headers, body).await
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerKind {
    Agent,
    SmartGateway,
}

#[derive(Clone)]
pub(crate) struct ProxyState {
    pub(crate) db: Arc<Database>,
    pub(crate) client: Client,
    circuits: Arc<Mutex<std::collections::HashMap<String, ProviderCircuit>>>,
    pub(crate) codex_history: Arc<codex_history::CodexHistoryStore>,
    pub(crate) target: ProviderTarget,
    pub(crate) listener_kind: ListenerKind,
    port: u16,
    started_at: Instant,
    pub(crate) correlation: Option<crate::gateway::correlation::Correlation>,
    request_path: String,
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

fn apply_catalog_subagent_signal(
    builder: reqwest::RequestBuilder,
    is_catalog_subagent: bool,
) -> reqwest::RequestBuilder {
    if is_catalog_subagent {
        builder.header(CS_SUBAGENT_HEADER, "1")
    } else {
        builder
    }
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

pub(crate) fn next_failover_provider(
    state: &ProxyState,
    exclude_ids: &[String],
    requested_model: &str,
) -> AppResult<Option<Provider>> {
    next_failover_provider_ex(state, exclude_ids, requested_model, false)
}

pub(crate) fn next_failover_provider_ex(
    state: &ProxyState,
    exclude_ids: &[String],
    requested_model: &str,
    ignore_model_filter: bool,
) -> AppResult<Option<Provider>> {
    let enabled = if gateway_catalog_enabled(state) {
        let mode = state
            .db
            .with_conn(|conn| {
                Ok(crate::database::dao::gateway::current_profile(conn, state.target)?
                    .map(|profile| profile.fallback_mode)
                    .unwrap_or_else(|| "off".to_string()))
            })
            .unwrap_or_else(|_| "off".to_string());
        mode == "retry" || mode == "model_chain"
    } else {
        state
            .db
            .with_conn(|conn| get_setting(conn, PROXY_FAILOVER_ENABLED_KEY))?
            .as_deref()
            == Some("true")
    };
    if !enabled {
        return Ok(None);
    }
    let mut candidates = state.db.with_conn(|conn| list_providers(conn, state.target))?;
    if gateway_catalog_enabled(state) {
        if let Ok(Some(profile)) = state
            .db
            .with_conn(|conn| crate::database::dao::gateway::current_profile(conn, state.target))
        {
            candidates.retain(|candidate| {
                crate::database::dao::gateway::profile_allows_upstream(&profile, &candidate.id)
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.failover_group
            .cmp(&right.failover_group)
            .then(left.sort_index.cmp(&right.sort_index))
            .then(left.created_at.cmp(&right.created_at))
            .then(left.id.cmp(&right.id))
    });
    for mut candidate in candidates {
        if exclude_ids.iter().any(|id| id == &candidate.id)
            || candidate.base_url.trim().is_empty()
            || circuit_is_open(state, &candidate.id)
            || (!ignore_model_filter && !candidate.allows_failover_for_request(requested_model))
        {
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

pub(crate) fn load_gateway_catalog(
    state: &ProxyState,
    style: CatalogStyle,
) -> AppResult<(Vec<Provider>, Vec<crate::catalog::CatalogEntry>)> {
    state.db.with_conn(|conn| {
        let mut providers = crate::database::dao::gateway::list_upstream_providers(conn, false)?;
        providers.retain(|provider| !provider.is_smart_gateway());
        let profile = crate::database::dao::gateway::current_profile(conn, state.target)
            .ok()
            .flatten();
        if let Some(profile) = profile.as_ref() {
            if !profile.allowed_upstream_ids.is_empty() {
                providers.retain(|provider| {
                    crate::database::dao::gateway::profile_allows_upstream(profile, &provider.id)
                });
            }
        }
        let hide_official = crate::catalog::hide_official_for_conn(conn, state.target);
        let modes = crate::database::dao::gateway::list_route_modes(
            conn,
            crate::database::dao::gateway::SHARED_PROFILE_ID,
        )
        .unwrap_or_default();
        let mut pairs = Vec::with_capacity(providers.len());
        for provider in &providers {
            let cached =
                crate::database::dao::gateway::list_visible_upstream_model_ids(conn, &provider.id)
                    .unwrap_or_default();
            pairs.push((provider.clone(), cached));
        }
        let entries = crate::catalog::with_auto_entry_from_modes(
            style,
            crate::catalog::build_catalog_with(style, &pairs, hide_official),
            &modes,
        );
        Ok((providers, entries))
    })
}

fn presented_listener_token(headers: &HeaderMap) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
                .unwrap_or(value)
                .trim()
                .to_string()
        })
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.trim().to_string())
        })
        .unwrap_or_default()
}

fn resolve_binding_target(state: &ProxyState, headers: &HeaderMap) -> Option<ProviderTarget> {
    let token = presented_listener_token(headers);
    let requested = headers
        .get("x-ai-switcher-target")
        .and_then(|value| value.to_str().ok())
        .map(ProviderTarget::from_str_lossy);
    state
        .db
        .with_conn(|conn| {
            if crate::gateway::service::api_key_matches(conn, &token) {
                return Ok(requested.or(Some(ProviderTarget::ClaudeCode)));
            }
            Ok(crate::database::dao::gateway::binding_by_token(conn, &token)?
                .map(|binding| binding.target_app)
                .or(requested))
        })
        .ok()
        .flatten()
}

pub(crate) fn gateway_catalog_enabled(state: &ProxyState) -> bool {
    state.listener_kind == ListenerKind::SmartGateway
}

fn hydrate_provider_credential(state: &ProxyState, mut provider: Provider) -> AppResult<Option<Provider>> {
    if provider.is_codex_oauth() {
        match crate::codex_oauth::manager().get_valid_token(Some(&provider.auth_binding)) {
            Ok((token, account_id)) => {
                provider.api_key = token;
                provider.auth_binding = account_id;
                provider.base_url = crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string();
                provider.protocol_type = ProtocolType::OpenAiResponses;
                return Ok(Some(provider));
            }
            Err(error) => {
                log::error!("代理读取 ChatGPT OAuth 凭据失败: {error}");
                return Ok(None);
            }
        }
    }
    provider.api_key = match state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)) {
        Ok(Some(key)) => key,
        Ok(None) => return Ok(None),
        Err(error) => {
            log::error!("代理读取供应商凭据失败: {error}");
            return Ok(None);
        }
    };
    if provider.is_antigravity() && provider.api_key.trim().is_empty() {
        provider.api_key = crate::antigravity::gateway::builtin_api_key();
    }
    Ok(Some(provider))
}

pub(crate) const CS_SUBAGENT_HEADER: &str = "x-cs-subagent";

pub(crate) fn is_claude_code_subagent_request(headers: &HeaderMap, requested_model: &str) -> bool {
    headers.contains_key(CS_SUBAGENT_HEADER)
        || crate::provider::classify_claude_model_role(requested_model)
            == Some(crate::provider::ClaudeModelRole::Haiku)
}

