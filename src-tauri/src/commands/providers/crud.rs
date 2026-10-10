fn catalog_subagent_model(state: &AppState, target: ProviderTarget) -> Option<String> {
    catalog::subagent_model(state.db.as_ref(), target)
}

#[tauri::command]
pub fn get_current_provider(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Option<Provider>> {
    state.db.with_read_conn(|conn| {
        if let Some(binding) = crate::database::dao::gateway::binding_for_target(conn, target)? {
            if binding.mode == "direct" {
                return Ok(Some(crate::database::dao::gateway::provider_from_upstream(conn, &binding.direct_upstream_id, target)?));
            }
        }
        dao::get_current_provider(conn, target)
    })
}

#[tauri::command]
pub async fn create_provider(
    input: ProviderInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.provider_kind == ProviderKind::SmartGateway {
        return Err(AppError::Config(
            "智能网关由托管 Auto 卡提供，请用「新增供应商 → 智能网关」接入".to_string(),
        ));
    }
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    sync_live_providers(&state, provider.target_app, Some(&app)).await?;
    Ok(provider)
}

/// Copy a configured provider (including API key) onto another Agent, adapting
/// protocol / Base URL / Claude role mapping for the destination.
#[tauri::command]
pub async fn copy_provider_to_target(
    id: String,
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let (source, api_key, existing_names) = state.db.with_conn(|conn| {
        let source = dao::get_provider(conn, &id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
        let api_key = dao::resolve_api_key(conn, &id)?.unwrap_or_default();
        let existing_names = dao::list_providers(conn, target)?
            .into_iter()
            .map(|provider| provider.name)
            .collect::<Vec<_>>();
        Ok((source, api_key, existing_names))
    })?;
    let input = copied_provider_input(&source, target, &existing_names, api_key)?;
    let cache = state
        .db
        .with_conn(|conn| dao::get_provider_model_cache(conn, &id))?;
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if let Some(cache) = cache {
        if !cache.models.is_empty() {
            state.db.with_conn(|conn| {
                dao::save_provider_model_cache(conn, &provider.id, &cache.models, cache.checked_at)
            })?;
        }
    }
    if should_activate_copied_provider(provider.target_app) {
        state
            .db
            .with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        let current = state
            .db
            .with_conn(|conn| dao::get_provider(conn, &provider.id))?
            .ok_or_else(|| AppError::Config("复制后未能读回供应商".into()))?;
        let _ = apply_target_provider(&current, Some(&app), &state).await?;
        return Ok(current);
    }
    sync_live_providers(&state, provider.target_app, Some(&app)).await?;
    Ok(provider)
}

#[tauri::command]
pub async fn update_provider(
    input: ProviderInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.id.is_none() {
        return Err(AppError::Config("更新供应商时缺少 id".to_string()));
    }
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if provider.target_app.hides_provider_switch()
        || gateway_catalog_on(&state, provider.target_app)
    {
        sync_live_providers(&state, provider.target_app, Some(&app)).await?;
    } else if provider.is_current {
        let _ = apply_target_provider(&provider, Some(&app), &state).await?;
    }
    Ok(provider)
}

#[tauri::command]
pub async fn delete_provider(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?
            .map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    state.db.with_conn(|conn| dao::delete_provider(conn, &id))?;
    sync_live_providers(&state, target, Some(&app)).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchProviderResult {
    pub provider: Provider,
    pub session_sync: Option<CodexProviderSyncResult>,
    /// Codex-only hint for the frontend: `preserved_official_login` when the
    /// vendor key went into config.toml and auth.json kept its ChatGPT login,
    /// `official_login_required` when no official login exists and the Codex
    /// desktop app will hide custom models until the user logs in once.
    pub codex_notice: Option<&'static str>,
}

/// Activate a provider only for the application that owns it.
#[tauri::command]
pub async fn switch_provider(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<SwitchProviderResult> {
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let result = switch_provider_for_target(&id, target, Some(&app), &state).await?;
    schedule_provider_health_check(app, result.provider.clone(), Arc::clone(&state.db));
    Ok(result)
}

/// Shared provider switching service used by both IPC and tray actions.
/// Live configuration is switched locally first. Connectivity is checked in the
/// background so network latency never blocks the user's explicit selection.
pub async fn switch_provider_for_target<R: tauri::Runtime>(
    id: &str,
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<SwitchProviderResult> {
    let mut provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    if provider.target_app != target {
        return Err(AppError::Config("供应商不属于此应用".to_string()));
    }
    let connection = if provider.is_smart_gateway() {
        crate::database::dao::gateway::ConnectionType::Gateway
    } else {
        crate::database::dao::gateway::ConnectionType::External
    };
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::ensure_profile_for_target(conn, target)?;
        crate::database::dao::gateway::set_current_connection_type(conn, target.as_str(), connection)
    })?;
    crate::catalog::invalidate_view_cache();
    if provider.is_smart_gateway() {
        provider = ensure_smart_gateway_provider_row(state, target)?;
    }
    if provider.is_current {
        if target == ProviderTarget::ClaudeCode {
            let _ = repair_current_code_model_fields(state).await;
        } else if target == ProviderTarget::Codex {
            let _ = repair_codex_managed_proxy_endpoint(state).await;
        }
        let needs_proxy = target_starts_agent_proxy(
            target,
            live_uses_gateway_catalog(state, &provider),
            &provider,
        );
        let proxy_running = state.proxy.lock().await.status_for(target).running;
        if needs_proxy == proxy_running {
            log::debug!(
                "跳过重复供应商切换: target={} provider={}",
                target.as_str(),
                id
            );
            return Ok(SwitchProviderResult {
                provider,
                session_sync: None,
                codex_notice: None,
            });
        }
        log::info!(
            "当前供应商代理状态不一致，执行修复式重应用: target={} provider={} needs_proxy={} running={}",
            target.as_str(),
            id,
            needs_proxy,
            proxy_running
        );
        let (mut snapshot, session_sync, codex_notice) =
            apply_target_provider(&provider, app, state).await?;
        let _ = snapshot.capture_last_written_files();
        return Ok(SwitchProviderResult {
            provider,
            session_sync,
            codex_notice,
        });
    }
    let started = Instant::now();
    let (snapshot, session_sync, codex_notice) = apply_target_provider(&provider, app, state).await?;
    let applied_ms = started.elapsed().as_millis();
    if let Err(error) = state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id)) {
        return rollback_switch(snapshot, state, error).await;
    }
    crate::catalog::invalidate_view_cache();
    log::info!(
        "供应商快速切换完成: target={} provider={} apply={}ms total={}ms",
        target.as_str(),
        provider.id,
        applied_ms,
        started.elapsed().as_millis()
    );
    Ok(SwitchProviderResult {
        provider,
        session_sync,
        codex_notice,
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderHealthUpdated {
    provider_id: String,
    target_app: ProviderTarget,
    ok: bool,
    category: String,
    message: String,
    checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
}

pub fn schedule_provider_health_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    provider: Provider,
    db: Arc<crate::database::Database>,
) {
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let key = db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        let result = test_provider_with_key(&provider, key, db.as_ref(), true).await;
        let result = match result {
            Ok(result) => result,
            Err(error) => ConnectionTestResult {
                ok: false,
                category: "internal".to_string(),
                message: error.to_string(),
                checked_at: Utc::now().timestamp_millis(),
                latency_ms: None,
            },
        };
        log::info!(
            "供应商后台验证完成: target={} provider={} ok={} duration={}ms",
            provider.target_app.as_str(),
            provider.id,
            result.ok,
            started.elapsed().as_millis()
        );
        let _ = app.emit(
            "provider-health-updated",
            ProviderHealthUpdated {
                provider_id: provider.id,
                target_app: provider.target_app,
                ok: result.ok,
                category: result.category,
                message: result.message,
                checked_at: result.checked_at,
                latency_ms: result.latency_ms,
            },
        );
    });
}

/// Test an already-stored provider without exposing its credential to the UI.
#[tauri::command]
pub async fn test_provider_connection(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    test_provider_impl(&provider, &state).await
}

/// Test values currently entered in the form.  The supplied API key is kept
/// only in this request and is never written to SQLite, the keyring or logs.
#[tauri::command]
pub async fn test_provider_input(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = temporary_provider(&input, &state)?;
    test_provider_with_key(&provider, provider.api_key.clone(), state.db.as_ref(), false).await
}

/// Measure RTT to the provider Base URL with a lightweight HTTP request (no API key).
#[tauri::command]
pub async fn speedtest_provider_endpoint(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<EndpointSpeedtestResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    speedtest_base_url(&provider.base_url).await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorReport {
    pub provider_id: String,
    pub provider_name: String,
    pub target_app: ProviderTarget,
    pub ok: bool,
    pub category: String,
    pub message: String,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub quarantined: bool,
}

#[tauri::command]
pub async fn batch_diagnose_providers(
    target: Option<ProviderTarget>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ProviderDoctorReport>> {
    let targets = match target {
        Some(t) => vec![t],
        None => vec![
            ProviderTarget::ClaudeCode,
            ProviderTarget::Codex,
            ProviderTarget::OpenCode,
            ProviderTarget::Pi,
        ],
    };

    let mut providers = Vec::new();
    state.db.with_conn(|conn| {
        for t in targets {
            let mut list = dao::list_providers(conn, t)?;
            providers.append(&mut list);
        }
        Ok::<(), AppError>(())
    })?;

    let mut reports = Vec::with_capacity(providers.len());
    for provider in providers {
        let test_res = test_provider_impl(&provider, &state).await;
        let (ok, category, message, latency_ms, status_code) = match test_res {
            Ok(res) => {
                let code = if res.category == "authentication" {
                    Some(401u16)
                } else if res.ok {
                    Some(200u16)
                } else {
                    None
                };
                (res.ok, res.category, res.message, res.latency_ms, code)
            }
            Err(e) => (false, "system".to_string(), e.to_string(), None, None),
        };
        let quarantined = !ok
            && (category == "authentication"
                || status_code == Some(401)
                || status_code == Some(403)
                || provider.failover_group == 0);
        reports.push(ProviderDoctorReport {
            provider_id: provider.id,
            provider_name: provider.name,
            target_app: provider.target_app,
            ok,
            category,
            message,
            latency_ms,
            status_code,
            quarantined,
        });
    }

    Ok(reports)
}

#[tauri::command]
pub async fn quarantine_failed_providers(
    provider_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<usize> {
    let mut count = 0;
    for id in provider_ids {
        let res = state.db.with_conn(|conn| {
            if let Some(mut provider) = dao::get_provider(conn, &id)? {
                provider.failover_group = 0;
                if !provider.notes.contains("[已隔离]") {
                    if !provider.notes.is_empty() {
                        provider.notes.push_str(" ");
                    }
                    provider.notes.push_str("[已隔离: 401/403 鉴权异常]");
                }
                let input = ProviderInput {
                    id: Some(provider.id.clone()),
                    name: provider.name,
                    base_url: provider.base_url,
                    api_key: String::new(),
                    clear_api_key: false,
                    model: provider.model,
                    model_context_window: provider.model_context_window,
                    auto_review_model_override: provider.auto_review_model_override,
                    web_search_enabled: provider.web_search_enabled,
                    model_mapping: provider.model_mapping,
                    protocol_type: provider.protocol_type,
                    provider_kind: provider.provider_kind,
                    auth_binding: provider.auth_binding,
                    target_app: provider.target_app,
                    notes: provider.notes,
                    failover_group: 0,
                    failover_models: provider.failover_models,
                    hidden_models: provider.hidden_models,
                    thinking_config: provider.thinking_config,
                    custom_headers: provider.custom_headers,
                };
                let _ = dao::upsert_provider(conn, &input)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        if res {
            count += 1;
        }
    }
    Ok(count)
}

fn is_loopback_url(url_str: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url_str) {
        match parsed.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            None => false,
        }
    } else {
        false
    }
}

async fn speedtest_base_url(base_url: &str) -> AppResult<EndpointSpeedtestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let url = normalize_base_url(base_url)?;
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3));

    // 仅本地回环地址禁用系统代理，避免宿主系统代理劫持 127.0.0.1 导致本地测试或测速失败；公网地址保持继承系统代理
    if is_loopback_url(&url) {
        builder = builder.no_proxy();
    }

    let client = builder
        .build()
        .map_err(|error| AppError::Other(format!("创建测速客户端失败: {error}")))?;
    let started = Instant::now();
    let response = client.get(&url).send().await;
    let latency_ms = Some(started.elapsed().as_millis() as u64);
    match response {
        Ok(response) => {
            let status = response.status();
            Ok(EndpointSpeedtestResult {
                ok: true,
                latency_ms,
                message: format!("RTT {} · HTTP {}", format_latency(latency_ms), status.as_u16()),
                checked_at,
                url,
            })
        }
        Err(error) => Ok(EndpointSpeedtestResult {
            ok: false,
            latency_ms,
            message: format!("RTT {} · {}", format_latency(latency_ms), sanitize_network_error(error)),
            checked_at,
            url,
        }),
    }
}

fn format_latency(latency_ms: Option<u64>) -> String {
    match latency_ms {
        Some(ms) => format!("{ms} ms"),
        None => "—".to_string(),
    }
}

fn sanitize_network_error(error: reqwest::Error) -> String {
    let text = error.without_url().to_string();
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > 180 {
        let truncated: String = chars[..180].iter().collect();
        format!("{truncated}…")
    } else {
        text
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSpeedtestItemResult {
    pub id: String,
    pub name: String,
    pub result: EndpointSpeedtestResult,
    pub cancelled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSpeedtestResult {
    pub batch_id: String,
    pub total: usize,
    pub completed: usize,
    pub cancelled: bool,
    pub items: Vec<BatchSpeedtestItemResult>,
}

#[derive(Debug, Clone)]
pub struct TargetEndpoint {
    pub id: String,
    pub name: String,
    pub base_url: String,
}

type SpeedtestCancelSender = tokio::sync::watch::Sender<bool>;

struct CancelRegistry {
    active: std::collections::HashMap<String, SpeedtestCancelSender>,
    tombstones: std::collections::HashMap<String, std::time::Instant>,
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            active: std::collections::HashMap::new(),
            tombstones: std::collections::HashMap::new(),
        }
    }

    fn clean_expired_tombstones(&mut self) {
        let now = std::time::Instant::now();
        self.tombstones
            .retain(|_, time| now.duration_since(*time) < std::time::Duration::from_secs(60));
    }
}

fn speedtest_cancels() -> &'static std::sync::Mutex<CancelRegistry> {
    static REGISTRY: OnceLock<std::sync::Mutex<CancelRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(CancelRegistry::new()))
}

fn register_speedtest_cancel(
    batch_id: &str,
) -> AppResult<(SpeedtestCancelSender, tokio::sync::watch::Receiver<bool>)> {
    let mut reg = speedtest_cancels()
        .lock()
        .map_err(|_| AppError::Other("取消注册表锁损坏".to_string()))?;
    reg.clean_expired_tombstones();

    if reg.active.contains_key(batch_id) {
        return Err(AppError::Config(format!("批次 ID 已在进行中: {batch_id}")));
    }

    // 检查是否有提前到达的取消墓碑
    let pre_cancelled = reg.tombstones.remove(batch_id).is_some();
    let (tx, rx) = tokio::sync::watch::channel(pre_cancelled);
    reg.active.insert(batch_id.to_string(), tx.clone());
    Ok((tx, rx))
}

fn unregister_speedtest_cancel(batch_id: &str) {
    if let Ok(mut reg) = speedtest_cancels().lock() {
        reg.active.remove(batch_id);
    }
}

struct SpeedtestCancelGuard(String);

impl Drop for SpeedtestCancelGuard {
    fn drop(&mut self) {
        unregister_speedtest_cancel(&self.0);
    }
}

const MAX_BATCH_SPEEDTEST_SIZE: usize = 50;

/// 从数据库仅查询 id, name, base_url 三个字段，避免全量 Provider 读取可能触碰凭据
fn query_target_endpoints(
    conn: &rusqlite::Connection,
    ids: &[String],
) -> AppResult<Vec<TargetEndpoint>> {
    let mut targets = Vec::with_capacity(ids.len());
    let mut stmt_upstream = conn
        .prepare_cached("SELECT id, name, base_url FROM upstreams WHERE id = ?1")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let mut stmt_provider = conn
        .prepare_cached("SELECT id, name, base_url FROM providers WHERE id = ?1")
        .map_err(|e| AppError::Database(e.to_string()))?;

    for id in ids {
        let mut row_opt = match stmt_upstream.query_row([id.as_str()], |row| {
            Ok(TargetEndpoint {
                id: row.get(0)?,
                name: row.get(1)?,
                base_url: row.get(2)?,
            })
        }) {
            Ok(t) => Some(t),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(AppError::Database(e.to_string())),
        };

        if row_opt.is_none() {
            row_opt = match stmt_provider.query_row([id.as_str()], |row| {
                Ok(TargetEndpoint {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                })
            }) {
                Ok(t) => Some(t),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => return Err(AppError::Database(e.to_string())),
            };
        }

        if let Some(target) = row_opt {
            targets.push(target);
        } else {
            targets.push(TargetEndpoint {
                id: id.clone(),
                name: id.clone(),
                base_url: String::new(),
            });
        }
    }
    Ok(targets)
}

/// 生产批次测速核心执行函数（支持传入自定义 probe 闭包以便真实 HTTP 隔离回归）
async fn execute_batch_speedtest_internal<F, Fut>(
    targets: Vec<TargetEndpoint>,
    concurrency: Option<usize>,
    batch_id: String,
    rx: tokio::sync::watch::Receiver<bool>,
    probe: F,
) -> AppResult<BatchSpeedtestResult>
where
    F: Fn(String) -> Fut + Send + Sync + 'static + Clone,
    Fut: std::future::Future<Output = AppResult<EndpointSpeedtestResult>> + Send + 'static,
{
    let total = targets.len();
    let limit = concurrency.unwrap_or(4).clamp(1, 8);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(limit));

    let mut join_set = tokio::task::JoinSet::new();

    for (idx, target) in targets.into_iter().enumerate() {
        let sem = semaphore.clone();
        let rx = rx.clone();
        let probe = probe.clone();

        join_set.spawn(async move {
            // 排队前检查是否已取消
            if *rx.borrow() {
                return (
                    idx,
                    BatchSpeedtestItemResult {
                        id: target.id,
                        name: target.name,
                        result: EndpointSpeedtestResult {
                            ok: false,
                            latency_ms: None,
                            message: "已取消 (任务未开始)".to_string(),
                            checked_at: Utc::now().timestamp_millis(),
                            url: target.base_url,
                        },
                        cancelled: true,
                    },
                );
            }

            let permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    return (
                        idx,
                        BatchSpeedtestItemResult {
                            id: target.id,
                            name: target.name,
                            result: EndpointSpeedtestResult {
                                ok: false,
                                latency_ms: None,
                                message: "信号量获取失败".to_string(),
                                checked_at: Utc::now().timestamp_millis(),
                                url: target.base_url,
                            },
                            cancelled: false,
                        },
                    );
                }
            };

            // 获取信号量后再次检查排队期间是否已触发取消
            if *rx.borrow() {
                drop(permit);
                return (
                    idx,
                    BatchSpeedtestItemResult {
                        id: target.id,
                        name: target.name,
                        result: EndpointSpeedtestResult {
                            ok: false,
                            latency_ms: None,
                            message: "已取消 (任务未开始)".to_string(),
                            checked_at: Utc::now().timestamp_millis(),
                            url: target.base_url,
                        },
                        cancelled: true,
                    },
                );
            }

            if target.base_url.trim().is_empty() {
                drop(permit);
                return (
                    idx,
                    BatchSpeedtestItemResult {
                        id: target.id,
                        name: target.name,
                        result: EndpointSpeedtestResult {
                            ok: false,
                            latency_ms: None,
                            message: "缺少有效 Base URL 或上游不存在".to_string(),
                            checked_at: Utc::now().timestamp_millis(),
                            url: String::new(),
                        },
                        cancelled: false,
                    },
                );
            }

            let mut rx_change = rx.clone();
            let url_for_probe = target.base_url.clone();
            let item_res = tokio::select! {
                _ = async {
                    while rx_change.changed().await.is_ok() {
                        if *rx_change.borrow() {
                            return;
                        }
                    }
                    std::future::pending::<()>().await;
                } => {
                    BatchSpeedtestItemResult {
                        id: target.id,
                        name: target.name,
                        result: EndpointSpeedtestResult {
                            ok: false,
                            latency_ms: None,
                            message: "已取消 (测速中断)".to_string(),
                            checked_at: Utc::now().timestamp_millis(),
                            url: target.base_url,
                        },
                        cancelled: true,
                    }
                }
                res = probe(url_for_probe) => {
                    let (ok, latency_ms, message, checked_at, url) = match res {
                        Ok(r) => (r.ok, r.latency_ms, r.message, r.checked_at, r.url),
                        Err(e) => (false, None, e.to_string(), Utc::now().timestamp_millis(), target.base_url),
                    };
                    BatchSpeedtestItemResult {
                        id: target.id,
                        name: target.name,
                        result: EndpointSpeedtestResult {
                            ok,
                            latency_ms,
                            message,
                            checked_at,
                            url,
                        },
                        cancelled: false,
                    }
                }
            };
            drop(permit);
            (idx, item_res)
        });
    }

    let mut indexed_items: Vec<(usize, BatchSpeedtestItemResult)> = Vec::with_capacity(total);
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok((idx, item)) => indexed_items.push((idx, item)),
            Err(e) => {
                log::error!("测速子任务执行失败: {e}");
                return Err(AppError::Other(format!("测速子任务执行失败: {e}")));
            }
        }
    }
    indexed_items.sort_by_key(|(idx, _)| *idx);
    let items: Vec<BatchSpeedtestItemResult> = indexed_items.into_iter().map(|(_, item)| item).collect();

    let completed = items.iter().filter(|i| !i.cancelled).count();
    let cancelled = *rx.borrow() || items.iter().any(|i| i.cancelled);

    Ok(BatchSpeedtestResult {
        batch_id,
        total,
        completed,
        cancelled,
        items,
    })
}

/// 批量测速入口 IPC
#[tauri::command]
pub async fn batch_speedtest_upstream_endpoints(
    ids: Vec<String>,
    concurrency: Option<usize>,
    batch_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<BatchSpeedtestResult> {
    let batch_id = batch_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let (_tx, rx) = register_speedtest_cancel(&batch_id)?;
    let _guard = SpeedtestCancelGuard(batch_id.clone());

    if ids.is_empty() {
        return Ok(BatchSpeedtestResult {
            batch_id,
            total: 0,
            completed: 0,
            cancelled: false,
            items: Vec::new(),
        });
    }

    // 保留顺序去重
    let mut seen = std::collections::HashSet::new();
    let mut unique_ids = Vec::with_capacity(ids.len());
    for id in ids {
        if seen.insert(id.clone()) {
            unique_ids.push(id);
        }
    }

    if unique_ids.len() > MAX_BATCH_SPEEDTEST_SIZE {
        return Err(AppError::Config(format!(
            "批量测速数量超过上限（最大 {MAX_BATCH_SPEEDTEST_SIZE}，收到 {}）",
            unique_ids.len()
        )));
    }

    let targets = state.db.with_read_conn(|conn| query_target_endpoints(conn, &unique_ids))?;

    let probe = |url: String| async move { speedtest_base_url(&url).await };
    execute_batch_speedtest_internal(targets, concurrency, batch_id, rx, probe).await
}

/// 显式取消批量测速 IPC
#[tauri::command]
pub async fn cancel_batch_speedtest(batch_id: String) -> AppResult<bool> {
    let mut reg = speedtest_cancels()
        .lock()
        .map_err(|_| AppError::Other("取消注册表锁损坏".to_string()))?;
    reg.clean_expired_tombstones();

    if let Some(tx) = reg.active.get(&batch_id) {
        let _ = tx.send(true);
        log::info!("已触发批量测速取消 handle: batch_id={batch_id}");
        Ok(true)
    } else {
        // 记录取消墓碑以防批次 IPC 正在初始化就绪前到达
        if reg.tombstones.len() > 100 {
            reg.clean_expired_tombstones();
            if reg.tombstones.len() > 100 {
                if let Some(oldest) = reg.tombstones.keys().next().cloned() {
                    reg.tombstones.remove(&oldest);
                }
            }
        }
        reg.tombstones.insert(batch_id.clone(), std::time::Instant::now());
        log::info!("记录未就绪批量测速取消墓碑: batch_id={batch_id}");
        Ok(true)
    }
}

#[cfg(test)]
mod batch_speedtest_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_production_batch_with_loopback_http_and_concurrency() {
        // 启动本地回环 HTTP 服务器
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let current_concurrent = Arc::new(AtomicUsize::new(0));
        let max_observed = Arc::new(AtomicUsize::new(0));

        let current_c = current_concurrent.clone();
        let max_c = max_observed.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let cur = current_c.clone();
                let max = max_c.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 512];
                    let _ = socket.read(&mut buf).await;
                    let c = cur.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(c, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK";
                    let _ = socket.write_all(response).await;
                    let _ = socket.flush().await;
                    cur.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });

        // 构造 6 个指向回环服务的测速目标
        let targets = (0..6)
            .map(|i| TargetEndpoint {
                id: format!("up-{i}"),
                name: format!("Upstream {i}"),
                base_url: format!("http://127.0.0.1:{port}"),
            })
            .collect();

        let (_tx, rx) = tokio::sync::watch::channel(false);
        // 并发上限设为 2
        let probe = |url: String| async move { speedtest_base_url(&url).await };
        let res = execute_batch_speedtest_internal(
            targets,
            Some(2),
            "test-loopback-batch".to_string(),
            rx,
            probe,
        )
        .await
        .unwrap();

        assert_eq!(res.total, 6);
        assert_eq!(res.completed, 6);
        assert!(!res.cancelled);
        for item in &res.items {
            assert!(
                item.result.ok,
                "回环测速项失败: id={}, url={}, message={}",
                item.id, item.result.url, item.result.message
            );
        }
        // 验证实际并发峰值受控且不超过 2
        assert!(max_observed.load(Ordering::SeqCst) <= 2);
        assert!(max_observed.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn test_production_batch_in_flight_and_queued_cancellation() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let batch_id = "test-cancel-flight".to_string();

        let targets = (0..5)
            .map(|i| TargetEndpoint {
                id: format!("up-{i}"),
                name: format!("Upstream {i}"),
                base_url: format!("http://example.com/{i}"),
            })
            .collect();

        // 模拟较慢的探针
        let probe = |_url: String| async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            Ok(EndpointSpeedtestResult {
                ok: true,
                latency_ms: Some(300),
                message: "OK".to_string(),
                checked_at: 0,
                url: "".to_string(),
            })
        };

        let handle = tokio::spawn(execute_batch_speedtest_internal(
            targets,
            Some(2),
            batch_id,
            rx,
            probe,
        ));

        // 运行 20ms 后触发取消（前两项在途，后三项排队）
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        tx.send(true).unwrap();

        let res = handle.await.unwrap().unwrap();
        assert_eq!(res.total, 5);
        assert!(res.cancelled);
        // 所有项均被及时取消
        assert!(res.items.iter().all(|i| i.cancelled));
    }

    #[tokio::test]
    async fn test_production_batch_tombstone_early_cancellation() {
        let batch_id = "test-tombstone-early";
        // 在批次启动前触发取消
        let cancelled = cancel_batch_speedtest(batch_id.to_string()).await.unwrap();
        assert!(cancelled);

        let (_tx, rx) = register_speedtest_cancel(batch_id).unwrap();
        assert!(*rx.borrow());

        let targets = vec![TargetEndpoint {
            id: "up-1".to_string(),
            name: "Upstream 1".to_string(),
            base_url: "http://127.0.0.1:80".to_string(),
        }];

        let probe = |url: String| async move { speedtest_base_url(&url).await };
        let res = execute_batch_speedtest_internal(
            targets,
            Some(4),
            batch_id.to_string(),
            rx,
            probe,
        )
        .await
        .unwrap();

        assert_eq!(res.total, 1);
        assert_eq!(res.completed, 0);
        assert!(res.cancelled);
        assert!(res.items[0].cancelled);

        unregister_speedtest_cancel(batch_id);
    }

    #[tokio::test]
    async fn test_duplicate_batch_id_rejected() {
        let batch_id = "test-dup-id";
        let _r1 = register_speedtest_cancel(batch_id).unwrap();
        let r2 = register_speedtest_cancel(batch_id);
        assert!(r2.is_err());
        unregister_speedtest_cancel(batch_id);
    }

    #[tokio::test]
    async fn test_speedtest_cancel_guard_cleanup() {
        let batch_id = "test-guard-cleanup";
        {
            let _r = register_speedtest_cancel(batch_id).unwrap();
            let _guard = SpeedtestCancelGuard(batch_id.to_string());
            assert!(speedtest_cancels().lock().unwrap().active.contains_key(batch_id));
        }
        assert!(!speedtest_cancels().lock().unwrap().active.contains_key(batch_id));
    }
}
