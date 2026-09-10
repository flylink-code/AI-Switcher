fn catalog_subagent_model(state: &AppState, target: ProviderTarget) -> Option<String> {
    catalog::subagent_model(state.db.as_ref(), target)
}

#[tauri::command]
pub fn get_current_provider(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Option<Provider>> {
    state.db.with_conn(|conn| dao::get_current_provider(conn, target))
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

async fn speedtest_base_url(base_url: &str) -> AppResult<EndpointSpeedtestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let url = normalize_base_url(base_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
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
            message: format!("RTT {} · {}", format_latency(latency_ms), sanitize_network_error(&error)),
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

fn sanitize_network_error(error: &reqwest::Error) -> String {
    let text = error.to_string();
    if text.len() > 180 {
        format!("{}…", &text[..180])
    } else {
        text
    }
}
