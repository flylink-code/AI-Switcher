
#[tauri::command]
pub async fn reorder_providers(
    ordered_ids: Vec<String>,
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    state.db.with_conn(|conn| dao::reorder_providers(conn, &ordered_ids, target))?;
    if gateway_catalog_on(&state, target) {
        if let Some(first_id) = ordered_ids.first() {
            let _ = state.db.with_conn(|conn| dao::set_current_provider(conn, first_id));
        }
        sync_live_providers(&state, target, Some(&app)).await?;
    }
    Ok(())
}

/// Import a live third-party configuration into its matching application list.
#[tauri::command]
pub async fn import_live_config(
    target: ProviderTarget,
    app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    spawn_blocking_result(move || {
        let state = app.state::<AppState>();
        import_live_config_sync(target, &state)
    })
    .await
}

fn import_live_config_sync(target: ProviderTarget, state: &AppState) -> AppResult<()> {
    if target == ProviderTarget::OpenCode {
        return import_opencode_live_providers(&state);
    }
    if target == ProviderTarget::Pi {
        return sync_pi_providers_to_live(&state);
    }
    if target == ProviderTarget::Dsh {
        return sync_dsh_providers_to_live(&state);
    }
    if target == ProviderTarget::Cline {
        return sync_cline_providers_to_live(&state);
    }
    let live = match target {
        ProviderTarget::ClaudeCode => claude_code::read_current_live_provider()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::read_current_live_provider()?,
        ProviderTarget::Codex => codex::read_current_live_provider()?,
        ProviderTarget::OpenCode | ProviderTarget::Pi | ProviderTarget::Dsh | ProviderTarget::Cline => unreachable!(),
    };
    let Some(live) = live else {
        return Ok(());
    };
    import_live_provider(live, target, &state)
}

/// OpenCode 配置可携带多个自有供应商（provider 段 + 顶层 model 引用当前项）。
/// 全部同步：base_url 已存在则更新名称/模型/密钥/协议；否则新建。
/// OpenCode 无激活切换，导入不标记 `is_current`。
fn import_opencode_live_providers(state: &AppState) -> AppResult<()> {
    let live_providers = opencode::read_live_providers()?;
    if live_providers.is_empty() {
        let config_path = crate::config::get_opencode_config_path();
        return Err(AppError::Config(format!(
            "未在 OpenCode 配置中找到可导入的供应商（{}）。请确认 provider 段含 baseURL 或 baseUrl；托管项 aisw-* / ai-switcher 不会导入。若同时存在 opencode.json 与 opencode.jsonc，将优先读取含 provider 的文件。也可检查旧版 config.json 与 auth.json。",
            config_path.display()
        )));
    }
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, ProviderTarget::OpenCode))?;
    for live in &live_providers {
        let normalized_base_url = normalize_base_url(&live.base_url)?;
        let default_model = live
            .current_model
            .clone()
            .or_else(|| live.models.first().cloned())
            .unwrap_or_default();
        if default_model.trim().is_empty() {
            continue;
        }
        let failover_models: Vec<String> = live
            .models
            .iter()
            .filter(|model| model.as_str() != default_model)
            .cloned()
            .collect();
        let matched = existing.iter().find(|p| p.base_url == normalized_base_url);
        state.db.with_conn(|conn| {
            dao::upsert_provider(
                conn,
                &ProviderInput {
                    id: matched.map(|p| p.id.clone()),
                    name: live.name.clone(),
                    base_url: normalized_base_url,
                    api_key: live.auth_token.clone(),
                    clear_api_key: false,
                    model: default_model,
                    model_context_window: matched.and_then(|p| p.model_context_window),
                    auto_review_model_override: None,
                    web_search_enabled: matched.and_then(|p| p.web_search_enabled),
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: live.protocol_type,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::OpenCode,
                    notes: matched
                        .map(|p| p.notes.clone())
                        .filter(|notes| !notes.trim().is_empty())
                        .unwrap_or_else(|| {
                            format!("从 OpenCode 配置同步（provider: {}）", live.id)
                        }),
                    failover_group: matched.map(|p| p.failover_group).unwrap_or(0),
                    failover_models,
                    hidden_models: matched
                        .map(|p| p.hidden_models.clone())
                        .unwrap_or_default(),
                    thinking_config: matched.and_then(|p| p.thinking_config.clone()),
                    custom_headers: matched.and_then(|p| p.custom_headers.clone()),
                },
            )
        })?;
    }
    // 导入只更新 DB；OpenCode 侧用户自有项已存在，再把 AI-Switcher 托管项同步出去。
    sync_opencode_providers_to_live(state)?;
    Ok(())
}

/// 把 DB 中全部 OpenCode 供应商写入 `opencode.json`（多供应商并存，无需切换）。
pub(crate) fn sync_opencode_providers_to_live(state: &AppState) -> AppResult<()> {
    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::OpenCode))?;
    let mut entries: Vec<(Provider, Vec<String>)> = Vec::with_capacity(providers.len());
    for provider in providers {
        entries.push(hydrate_catalog_runtime(state, provider)?);
    }
    opencode::apply_all_providers(&entries)
}

/// Export provider metadata only. API keys and keyring references are never
/// included in this payload.
#[tauri::command]
pub fn export_providers(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<String> {
    let providers = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    let bundle = ProviderExportBundle {
        version: 1,
        providers: providers.into_iter().map(|provider| ProviderExportEntry {
            name: provider.name,
            base_url: provider.base_url,
            model: provider.model,
            model_context_window: provider.model_context_window,
            web_search_enabled: provider.web_search_enabled,
            model_mapping: provider.model_mapping,
            protocol_type: provider.protocol_type,
            target_app: provider.target_app,
            notes: provider.notes,
            failover_group: provider.failover_group,
            failover_models: provider.failover_models,
            hidden_models: provider.hidden_models,
            thinking_config: provider.thinking_config,
            custom_headers: provider.custom_headers,
        }).collect(),
    };
    Ok(serde_json::to_string_pretty(&bundle)?)
}

/// Import an exported bundle non-destructively. Existing matching providers are
/// skipped and no imported record contains a credential.
#[tauri::command]
pub async fn import_providers_json(
    json: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProviderImportResult> {
    let bundle: ProviderExportBundle = serde_json::from_str(&json)
        .map_err(|_| AppError::Config("供应商导入文件无效".to_string()))?;
    if bundle.version != 1 {
        return Err(AppError::Config(format!("不支持的供应商导入版本: {}", bundle.version)));
    }
    let mut imported = 0;
    let mut skipped = 0;
    let mut touched_opencode = false;
    let mut touched_pi = false;
    let mut touched_dsh = false;
    let mut touched_code = false;
    let mut touched_codex = false;
    for entry in bundle.providers {
        let normalized_base_url = normalize_base_url(&entry.base_url)?;
        let existing = state.db.with_conn(|conn| dao::list_providers(conn, entry.target_app))?;
        if existing.iter().any(|provider| {
            provider.name == entry.name && provider.base_url == normalized_base_url
        }) {
            skipped += 1;
            continue;
        }
        let target_app = entry.target_app;
        state.db.with_conn(|conn| dao::upsert_provider(conn, &ProviderInput {
            id: None,
            name: entry.name,
            base_url: normalized_base_url,
            api_key: String::new(),
            clear_api_key: false,
            model: entry.model,
            model_context_window: entry.model_context_window,
            auto_review_model_override: None,
            web_search_enabled: entry.web_search_enabled,
            model_mapping: entry.model_mapping,
            protocol_type: entry.protocol_type,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app,
            notes: entry.notes,
            failover_group: entry.failover_group,
            failover_models: entry.failover_models,
            hidden_models: entry.hidden_models,
            thinking_config: entry.thinking_config,
            custom_headers: entry.custom_headers,
        }))?;
        if target_app == ProviderTarget::OpenCode {
            touched_opencode = true;
        }
        if target_app == ProviderTarget::Pi {
            touched_pi = true;
        }
        if target_app == ProviderTarget::Dsh {
            touched_dsh = true;
        }
        if target_app == ProviderTarget::ClaudeCode {
            touched_code = true;
        }
        if target_app == ProviderTarget::Codex {
            touched_codex = true;
        }
        imported += 1;
    }
    if touched_opencode {
        sync_opencode_providers_to_live(&state)?;
    }
    if touched_pi {
        sync_pi_providers_to_live(&state)?;
    }
    if touched_dsh {
        sync_dsh_providers_to_live(&state)?;
    }
    if touched_code && gateway_catalog_on(&state, ProviderTarget::ClaudeCode) {
        sync_gateway_catalog_target(ProviderTarget::ClaudeCode, Some(&app), &state).await?;
    }
    if touched_codex && gateway_catalog_on(&state, ProviderTarget::Codex) {
        sync_gateway_catalog_target(ProviderTarget::Codex, Some(&app), &state).await?;
    }
    Ok(ProviderImportResult { imported, skipped })
}

fn sync_catalog_target(state: &AppState, target: ProviderTarget) -> AppResult<()> {
    match target {
        ProviderTarget::OpenCode => sync_opencode_providers_to_live(state),
        ProviderTarget::Pi => sync_pi_providers_to_live(state),
        ProviderTarget::Dsh => sync_dsh_providers_to_live(state),
        ProviderTarget::Cline => sync_cline_providers_to_live(state),
        _ => Ok(()),
    }
}

async fn sync_live_providers<R: tauri::Runtime>(
    state: &AppState,
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
) -> AppResult<()> {
    if target.is_catalog_target() {
        if gateway_catalog_on(state, target) {
            let _ = ensure_smart_gateway_provider_row(state, target)?;
            if target == ProviderTarget::Cline {
                state.proxy.lock().await.stop_target(target);
            }
            return sync_catalog_target(state, target);
        }
        if target == ProviderTarget::Cline {
            let port = get_saved_proxy_port(state, target);
            let _ = state.proxy.lock().await.start(port, target).await;
        }
        return sync_catalog_target(state, target);
    }
    if matches!(target, ProviderTarget::ClaudeCode | ProviderTarget::Codex) {
        let _ = crate::wsl_direct::sync_claude_codex_files();
    }
    if gateway_catalog_on(state, target) {
        return sync_gateway_catalog_target(target, app, state).await;
    }
    Ok(())
}

pub(crate) fn load_gateway_pairs(state: &AppState, target: ProviderTarget) -> AppResult<Vec<(Provider, Vec<String>)>> {
    state.db.with_read_conn(|conn| {
        let profile = crate::database::dao::gateway::current_profile(conn, target)
            .ok()
            .flatten();
        let providers = crate::database::dao::gateway::list_upstream_providers(conn, false)?;
        let providers: Vec<Provider> = providers
            .into_iter()
            .filter(|provider| {
                !provider.is_smart_gateway()
                    && profile
                        .as_ref()
                        .map(|profile| crate::database::dao::gateway::profile_allows_upstream(profile, &provider.id))
                        .unwrap_or(true)
            })
            .collect();
        let mut entries = Vec::with_capacity(providers.len());
        for provider in providers {
            let cached = crate::database::dao::gateway::list_visible_upstream_model_ids(conn, &provider.id)
                .unwrap_or_default();
            entries.push((provider, cached));
        }
        Ok(entries)
    })
}

fn gateway_live_entry(
    state: &AppState,
    target: ProviderTarget,
    template: &Provider,
) -> AppResult<(Provider, Vec<String>)> {
    let port = state
        .db
        .with_conn(|conn| crate::database::dao::settings::get_setting(conn, crate::gateway::service::PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(crate::gateway::SMART_GATEWAY_PORT);
    let token = state
        .db
        .with_conn(|conn| {
            Ok(crate::database::dao::gateway::binding_for_target(conn, target)?
                .map(|binding| binding.entry_token)
                .filter(|token| !token.trim().is_empty())
                .or_else(|| {
                    crate::database::dao::gateway::profile_entry_token(conn, target)
                        .ok()
                        .flatten()
                })
                .unwrap_or_default())
        })?;
    let pairs = load_gateway_pairs(state, target)?;
    let profile = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::current_profile(conn, target))
        .ok()
        .flatten();
    let pairs: Vec<(Provider, Vec<String>)> = if let Some(profile) = profile.as_ref() {
        if profile.allowed_upstream_ids.is_empty() {
            pairs
        } else {
            pairs
                .into_iter()
                .filter(|(provider, _)| {
                    crate::database::dao::gateway::profile_allows_upstream(profile, &provider.id)
                })
                .collect()
        }
    } else {
        pairs
    };
    let hide = catalog::hide_official(state.db.as_ref(), target);
    let style = catalog::catalog_style_for(target);
    let profile_id = profile
        .as_ref()
        .map(|item| item.id.clone())
        .unwrap_or_else(|| crate::database::dao::gateway::SHARED_PROFILE_ID.to_string());
    let modes = state
        .db
        .with_conn(|conn| crate::gateway::modes::load_modes(conn, &profile_id))
        .unwrap_or_default();
    let catalog = catalog::with_auto_entry_from_modes(
        style,
        build_catalog_with(style, &pairs, hide),
        &modes,
    );
    let extra: Vec<String> = catalog
        .iter()
        .map(|entry| entry.public_id.clone())
        .collect();
    let mut live = template.clone();
    if let Some(auto) = catalog
        .iter()
        .find(|entry| catalog::is_auto_public_id(&entry.public_id))
    {
        live.model_context_window = Some(auto.context_window);
    }
    live.base_url = match live.protocol_type {
        ProtocolType::Anthropic => {
            if target == ProviderTarget::OpenCode {
                format!("http://127.0.0.1:{port}/v1")
            } else {
                format!("http://127.0.0.1:{port}")
            }
        }
        _ => format!("http://127.0.0.1:{port}/v1"),
    };
    live.api_key = token;
    live.model = crate::gateway::normalize_live_model_for(target, &live.model);
    Ok((live, extra))
}

fn hydrate_catalog_runtime(
    state: &AppState,
    provider: Provider,
) -> AppResult<(Provider, Vec<String>)> {
    if provider.is_smart_gateway() && gateway_catalog_on(state, provider.target_app) {
        return gateway_live_entry(state, provider.target_app, &provider);
    }
    let mut runtime = provider.clone();
    runtime.api_key = state
        .db
        .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
        .ok()
        .flatten()
        .unwrap_or_default();
    if runtime.is_antigravity() && runtime.api_key.trim().is_empty() {
        runtime.api_key = crate::antigravity::gateway::builtin_api_key();
    }
    let extra_models = match provider.target_app {
        ProviderTarget::Pi => state
            .db
            .with_conn(|conn| extra_models_for_pi_apply(conn, &runtime))
            .unwrap_or_default(),
        ProviderTarget::Cline => {
            let cached = state
                .db
                .with_conn(|conn| {
                    Ok(dao::get_provider_model_cache(conn, &provider.id)?
                        .map(|cache| cache.models)
                        .unwrap_or_default())
                })
                .unwrap_or_default();
            runtime.filter_hidden_models(cached)
        }
        _ => {
            let cached = state
                .db
                .with_conn(|conn| {
                    Ok(dao::get_provider_model_cache(conn, &provider.id)?
                        .map(|cache| cache.models)
                        .unwrap_or_default())
                })
                .unwrap_or_default();
            extra_models_for_ag_catalog_apply(&provider, cached)
        }
    };
    Ok((runtime, extra_models))
}

fn apply_native_gateway_entry(state: &AppState, target: ProviderTarget) -> AppResult<()> {
    let _ = ensure_smart_gateway_provider_row(state, target)?;
    sync_catalog_target(state, target)
}

pub(crate) async fn push_bound_gateway_catalogs(state: &AppState) -> AppResult<()> {
    let bindings = state
        .db
        .with_conn(crate::database::dao::gateway::list_bindings)?;
    for binding in bindings {
        match binding.target_app {
            ProviderTarget::OpenCode
            | ProviderTarget::Pi
            | ProviderTarget::Dsh
            | ProviderTarget::Cline => {
                let _ = apply_native_gateway_entry(state, binding.target_app);
            }
            _ => {
                if let Ok(Some(provider)) = state.db.with_conn(|conn| {
                    Ok(dao::list_providers(conn, binding.target_app)?
                        .into_iter()
                        .find(|item| item.is_smart_gateway()))
                }) {
                    let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await;
                }
            }
        }
    }
    Ok(())
}

/// Apply the gateway connection: write the local listener using the current provider as the live-config vehicle.
async fn sync_gateway_catalog_target<R: tauri::Runtime>(
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<()> {
    let bound = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::binding_for_target(conn, target))
        .ok()
        .flatten()
        .is_some();
    if !bound {
        return Ok(());
    }
    let auto = ensure_smart_gateway_provider_row(state, target)?;
    let _ = apply_target_provider(&auto, app, state).await?;
    let _ = state
        .db
        .with_conn(|conn| dao::set_current_provider(conn, &auto.id));
    crate::catalog::invalidate_view_cache();
    Ok(())
}

/// 把 DB 中全部 DeepSeek Harness 供应商写入 `settings.yaml` / `.credentials.yaml`（多供应商并存，无需切换）。
pub(crate) fn sync_dsh_providers_to_live(state: &AppState) -> AppResult<()> {
    use crate::config::dsh::sync_managed_dsh_providers;

    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::Dsh))?;
    let mut entries: Vec<(Provider, Vec<String>)> = Vec::with_capacity(providers.len());
    for provider in providers {
        entries.push(hydrate_catalog_runtime(state, provider)?);
    }
    sync_managed_dsh_providers(&entries)
}

pub(crate) fn sync_cline_providers_to_live(state: &AppState) -> AppResult<()> {
    use crate::config::cline::sync_managed_cline_providers;

    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::Cline))?;
    let mut entries: Vec<(Provider, Vec<String>)> = Vec::with_capacity(providers.len());
    for provider in providers {
        entries.push(hydrate_catalog_runtime(state, provider)?);
    }
    sync_managed_cline_providers(&entries)
}

/// 把 DB 中全部 Pi 供应商写入 `models.json` / `auth.json`（多供应商并存，无需切换）。
pub(crate) fn sync_pi_providers_to_live(state: &AppState) -> AppResult<()> {
    use crate::coding::pi::config::{
        read_pi_settings, sync_managed_pi_auth, sync_managed_pi_providers, update_pi_settings,
    };

    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::Pi))?;
    let mut model_entries: Vec<(String, serde_json::Value)> = Vec::with_capacity(providers.len());
    let mut auth_entries: Vec<(String, String)> = Vec::with_capacity(providers.len());
    for provider in &providers {
        let (runtime, extra_models) = hydrate_catalog_runtime(state, provider.clone())?;
        let provider_id = pi_provider_id(&runtime);
        model_entries.push((
            provider_id.clone(),
            pi_provider_config(&runtime, &extra_models)?,
        ));
        if !runtime.api_key.trim().is_empty() {
            auth_entries.push((provider_id, runtime.api_key));
        }
    }
    let retired = sync_managed_pi_providers(&model_entries)?;
    sync_managed_pi_auth(&auth_entries, &retired)?;

    let settings = read_pi_settings().unwrap_or_else(|_| serde_json::json!({}));
    let default_provider = settings
        .get("defaultProvider")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let default_missing = default_provider.is_empty()
        || retired.iter().any(|id| id == default_provider);
    if default_missing {
        if let (Some((id, _)), Some(provider)) = (model_entries.first(), providers.first()) {
            let _ = update_pi_settings(Some(id.clone()), Some(provider.model.clone()), None, None);
        }
    }
    Ok(())
}

fn pi_provider_config(provider: &Provider, extra_models: &[String]) -> AppResult<serde_json::Value> {
    use crate::provider::ProtocolType;
    use serde_json::json;

    let model_id = provider.model.trim();
    if model_id.is_empty() {
        return Err(AppError::Config("Pi 默认模型不能为空".to_string()));
    }
    let mut provider_cfg = json!({
        "baseUrl": normalize_pi_base_url(&provider.base_url, provider.protocol_type),
        "api": pi_api_for_protocol(provider.protocol_type),
        "models": build_pi_model_entries(provider, extra_models),
    });
    if matches!(provider.protocol_type, ProtocolType::Anthropic) {
        provider_cfg["compat"] = json!({
            "supportsEagerToolInputStreaming": false,
        });
    }
    Ok(provider_cfg)
}

fn pi_provider_id(provider: &Provider) -> String {
    if provider.is_antigravity() {
        return "antigravity".to_string();
    }
    let slug: String = provider
        .name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "custom".to_string()
    } else {
        slug
    }
}

fn pi_api_for_protocol(protocol: crate::provider::ProtocolType) -> &'static str {
    use crate::provider::ProtocolType;
    match protocol {
        ProtocolType::Anthropic => "anthropic-messages",
        ProtocolType::OpenAiResponses => "openai-responses",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => "openai-completions",
    }
}

fn pi_proxy_base_url(port: u16, protocol: crate::provider::ProtocolType) -> String {
    use crate::provider::ProtocolType;
    match protocol {
        // Anthropic SDK posts `/v1/messages` onto baseURL.
        ProtocolType::Anthropic => format!("http://127.0.0.1:{port}"),
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy => {
            format!("http://127.0.0.1:{port}/v1")
        }
    }
}

fn normalize_pi_base_url(base: &str, protocol: crate::provider::ProtocolType) -> String {
    use crate::provider::{ensure_openai_v1_suffix, ProtocolType};
    let mut url = base.trim().trim_end_matches('/').to_string();
    match protocol {
        ProtocolType::Anthropic => {
            // Official Anthropic base is `https://api.anthropic.com` (no `/v1`).
            // A trailing `/v1` becomes `/v1/v1/messages` → 404.
            if url.ends_with("/v1") {
                url.truncate(url.len() - 3);
                url = url.trim_end_matches('/').to_string();
            }
            url
        }
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy => {
            // Only append `/v1` when the path is empty (host root). Keep `/v4`,
            // `/compatible-mode/v1`, etc. as the vendor published them.
            ensure_openai_v1_suffix(&url).unwrap_or(url)
        }
    }
}

fn build_pi_model_entries(provider: &Provider, extra_models: &[String]) -> Vec<serde_json::Value> {
    use serde_json::json;

    let mut ids = catalog_models_from_provider(provider);
    for model in extra_models {
        let model = model.trim();
        if !model.is_empty() && !ids.iter().any(|id| id == model) {
            ids.push(model.to_string());
        }
    }
    if provider.is_antigravity() {
        for id in crate::antigravity::model_catalog::provider_suggestion_ids(24) {
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
    }
    ids = provider.filter_hidden_models(ids);

    let openai_compatible = matches!(
        provider.protocol_type,
        crate::provider::ProtocolType::OpenAiChat
            | crate::provider::ProtocolType::OpenAiResponses
            | crate::provider::ProtocolType::Proxy
    );
    ids.into_iter()
        .map(|id| {
            let context_window = catalog::advertised_context_window(provider, &id);
            let mut entry = json!({
                "id": id,
                "reasoning": true,
                "input": ["text", "image"],
                "contextWindow": context_window,
            });
            if openai_compatible {
                entry["thinkingLevelMap"] = json!({
                    "off": "off",
                    "minimal": "minimal",
                    "low": "low",
                    "medium": "medium",
                    "high": "high",
                    "xhigh": "xhigh",
                    "max": "max"
                });
            }
            entry
        })
        .collect()
}

fn extra_models_for_ag_catalog_apply(provider: &Provider, mut extra_models: Vec<String>) -> Vec<String> {
    extend_unique_models(&mut extra_models, provider.failover_models.clone());
    if uses_antigravity_model_catalog(provider) {
        extend_unique_models(&mut extra_models, crate::antigravity::list_model_ids());
        extend_unique_models(
            &mut extra_models,
            crate::antigravity::model_catalog::provider_suggestion_ids(24),
        );
        extra_models.retain(|id| {
            let trimmed = id.trim();
            crate::antigravity::model_catalog::is_agent_facing_model(trimmed)
                && !crate::antigravity::model_catalog::is_retired_model(trimmed)
                && !crate::antigravity::model_catalog::should_remap_legacy_gemini(trimmed)
        });
    }
    provider.filter_hidden_models(extra_models)
}

fn extra_models_for_pi_apply(
    conn: &rusqlite::Connection,
    provider: &Provider,
) -> AppResult<Vec<String>> {
    let mut ids = Vec::new();
    if let Some(cache) = dao::get_provider_model_cache(conn, &provider.id)? {
        extend_unique_models(&mut ids, cache.models);
    }
    if ids.len() <= 1 {
        let source_url =
            normalize_base_url(&provider.base_url).unwrap_or_else(|_| provider.base_url.clone());
        for target in [
            ProviderTarget::ClaudeCode,
            ProviderTarget::ClaudeDesktop,
            ProviderTarget::Codex,
            ProviderTarget::OpenCode,
            ProviderTarget::Pi,
        ] {
            for sibling in dao::list_providers(conn, target)? {
                if sibling.id == provider.id {
                    continue;
                }
                let sibling_url = normalize_base_url(&sibling.base_url)
                    .unwrap_or_else(|_| sibling.base_url.clone());
                if sibling_url != source_url {
                    continue;
                }
                extend_unique_models(&mut ids, catalog_models_from_provider(&sibling));
                if let Some(cache) = dao::get_provider_model_cache(conn, &sibling.id)? {
                    extend_unique_models(&mut ids, cache.models);
                }
            }
        }
    }
    Ok(provider.filter_hidden_models(ids))
}

fn extend_unique_models(ids: &mut Vec<String>, extra: Vec<String>) {
    for model in extra {
        let model = model.trim();
        if !model.is_empty() && !ids.iter().any(|id| id == model) {
            ids.push(model.to_string());
        }
    }
}

async fn apply_target_provider<R: tauri::Runtime>(
    provider: &Provider,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<(SwitchSnapshot, Option<CodexProviderSyncResult>, Option<&'static str>)> {
    // Provider rows carry only a keyring reference. Hydrate a short-lived clone
    // for config writing; it is never serialized or persisted.
    let mut runtime_provider = provider.clone();
    if runtime_provider.is_antigravity() {
        crate::commands::antigravity::ensure_gateway_running_for_provider(&runtime_provider).await?;
        let gateway = crate::antigravity::gateway_status()?;
        if runtime_provider.api_key.trim().is_empty()
            || state
                .db
                .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
                .ok()
                .flatten()
                .is_none()
        {
            // Persist gateway key so subsequent resolves succeed.
            let _ = state.db.with_conn(|conn| {
                dao::upsert_provider(
                    conn,
                    &ProviderInput {
                        id: Some(provider.id.clone()),
                        name: provider.name.clone(),
                        base_url: gateway.base_url.clone(),
                        api_key: gateway.api_key.clone(),
                        clear_api_key: false,
                        model: provider.model.clone(),
                        model_context_window: provider.model_context_window,
                        auto_review_model_override: provider.auto_review_model_override.clone(),
                        web_search_enabled: provider.web_search_enabled,
                        model_mapping: provider.model_mapping.clone(),
                        protocol_type: provider.protocol_type,
                        provider_kind: provider.provider_kind,
                        auth_binding: provider.auth_binding.clone(),
                        target_app: provider.target_app,
                        notes: provider.notes.clone(),
                        failover_group: provider.failover_group,
                        failover_models: provider.failover_models.clone(),
                        hidden_models: provider.hidden_models.clone(),
                        thinking_config: provider.thinking_config.clone(),
                        custom_headers: provider.custom_headers.clone(),
                    },
                )
            });
            runtime_provider.base_url = gateway.base_url;
        }
    }
    runtime_provider.api_key = if provider.is_codex_oauth() {
        "PROXY_MANAGED".to_string()
    } else if provider.is_antigravity() {
        state
            .db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| crate::antigravity::gateway::builtin_api_key())
    } else if gateway_catalog_on(state, provider.target_app) {
        state
            .db
            .with_conn(|conn| {
                Ok(
                    crate::database::dao::gateway::profile_entry_token(conn, provider.target_app)?
                        .and_then(|token| {
                            crate::gateway::resolved_gateway_token(&token).map(str::to_string)
                        })
                        .or_else(|| {
                            dao::resolve_api_key(conn, &provider.id)
                                .ok()
                                .flatten()
                                .and_then(|key| {
                                    crate::gateway::resolved_gateway_token(&key).map(str::to_string)
                                })
                        })
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    } else {
        state.db.with_conn(|conn| {
            dao::resolve_api_key(conn, &provider.id)?.ok_or_else(|| {
                AppError::Config("供应商未配置 API Key，无法切换".to_string())
            })
        })?
    };
    if runtime_provider.is_smart_gateway() {
        runtime_provider.model = crate::gateway::normalize_live_model_for(
            runtime_provider.target_app,
            &runtime_provider.model,
        );
        runtime_provider.api_key = state
            .db
            .with_conn(|conn| crate::database::dao::gateway::profile_entry_token(conn, provider.target_app))
            .ok()
            .flatten()
            .and_then(|token| crate::gateway::resolved_gateway_token(&token).map(str::to_string))
            .unwrap_or(runtime_provider.api_key);
    }
    if crate::gateway::resolved_gateway_token(&runtime_provider.api_key).is_none()
        && gateway_catalog_on(state, runtime_provider.target_app)
        && runtime_provider.is_smart_gateway()
        && !runtime_provider.is_codex_oauth()
    {
        return Err(AppError::Config(
            "智能网关入口凭据缺失，请重新绑定智能网关".to_string(),
        ));
    }
    let proxy_port = get_saved_proxy_port(state, runtime_provider.target_app);
    let _codex_switch_guard = if runtime_provider.target_app == ProviderTarget::Codex {
        Some(codex_switch_lock().lock().await)
    } else {
        None
    };
    let mut snapshot = SwitchSnapshot::capture(state, runtime_provider.target_app).await?;
    if runtime_provider.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空，请先编辑供应商配置".to_string()));
    }
    let gateway_catalog = live_uses_gateway_catalog(state, &runtime_provider);
    let uses_proxy = target_starts_agent_proxy(
        runtime_provider.target_app,
        gateway_catalog,
        &runtime_provider,
    );
    let result: AppResult<(Option<CodexProviderSyncResult>, Option<&'static str>)> = async {
        match runtime_provider.target_app {
            ProviderTarget::ClaudeCode => {
                let write_port = if gateway_catalog {
                    saved_smart_gateway_port(state)
                } else {
                    proxy_port
                };
                let field_proxy = uses_proxy || gateway_catalog;
                let mut ownership = prepare_code_ownership(
                    &runtime_provider,
                    state,
                    field_proxy,
                    gateway_catalog,
                    write_port,
                )?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeCode).await?;
                }
                let provider = runtime_provider.clone();
                let subagent = if gateway_catalog {
                    catalog_subagent_model(state, ProviderTarget::ClaudeCode)
                } else {
                    None
                };
                tauri::async_runtime::spawn_blocking(move || {
                    if gateway_catalog {
                        claude_code::apply_provider_to_settings_via_catalog_proxy(
                            &provider,
                            write_port,
                            subagent.as_deref(),
                            false,
                        )
                    } else if uses_proxy {
                        claude_code::apply_provider_to_settings_via_proxy(&provider, proxy_port)
                    } else {
                        claude_code::apply_provider_to_settings(&provider)
                    }
                })
                .await
                .map_err(|error| AppError::Tauri(format!("Claude Code 配置写入任务失败: {error}")))??;
                // Persist the actual on-disk managed fields so later compares
                // match Claude Code / JSON normalization instead of our preview.
                ownership.written = code_managed_fields()?;
                commit_code_ownership(state, ownership)?;
                if gateway_catalog {
                    let _ = crate::wsl_direct::sync_claude_codex_files();
                }
                Ok((None, None))
            }
            ProviderTarget::ClaudeDesktop => {
                let original_applied_id = prepare_desktop_ownership(state)?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeDesktop).await?;
                }
                let provider = runtime_provider.clone();
                let catalog_models = if gateway_catalog {
                    catalog_public_ids_for(state, ProviderTarget::ClaudeDesktop).unwrap_or_default()
                } else {
                    Vec::new()
                };
                tauri::async_runtime::spawn_blocking(move || {
                    claude_desktop::apply_provider(&provider, proxy_port, &catalog_models)
                })
                .await
                .map_err(|error| AppError::Tauri(format!("Claude Desktop 配置写入任务失败: {error}")))??;
                commit_desktop_ownership(state, original_applied_id)?;
                Ok((None, None))
            }
            ProviderTarget::Codex => {
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::Codex).await?;
                }
                let provider = runtime_provider.clone();
                let api_key = runtime_provider.api_key.clone();
                let apply_info = if gateway_catalog {
                    let pairs = load_gateway_pairs(state, ProviderTarget::Codex)?;
                    let hide_official =
                        catalog::hide_official(state.db.as_ref(), ProviderTarget::Codex);
                    let modes = state
                        .db
                        .with_conn(|conn| {
                            let profile_id =
                                crate::database::dao::gateway::profile_id_for_target(
                                    conn,
                                    ProviderTarget::Codex,
                                )
                                .unwrap_or_else(|_| {
                                    crate::database::dao::gateway::SHARED_PROFILE_ID.to_string()
                                });
                            Ok(crate::database::dao::gateway::list_route_modes(conn, &profile_id)
                                .unwrap_or_default())
                        })
                        .unwrap_or_default();
                    let catalog = catalog::with_auto_entry_from_modes(
                        CatalogStyle::Codex,
                        build_catalog_with(CatalogStyle::Codex, &pairs, hide_official),
                        &modes,
                    );
                    tauri::async_runtime::spawn_blocking(move || {
                        codex::apply_provider_with_catalog(
                            &provider,
                            &api_key,
                            None,
                            &catalog,
                        )
                    })
                    .await
                    .map_err(|error| AppError::Tauri(format!("Codex 配置写入任务失败: {error}")))??
                } else {
                    let extra_models = runtime_provider.filter_hidden_models(
                        state
                            .db
                            .with_conn(|conn| {
                                Ok(dao::get_provider_model_cache(conn, &runtime_provider.id)?
                                    .map(|cache| cache.models)
                                    .unwrap_or_default())
                            })
                            .unwrap_or_default(),
                    );
                    tauri::async_runtime::spawn_blocking(move || {
                        codex::apply_provider(
                            &provider,
                            &api_key,
                            if uses_proxy { Some(proxy_port) } else { None },
                            &extra_models,
                        )
                    })
                    .await
                    .map_err(|error| AppError::Tauri(format!("Codex 配置写入任务失败: {error}")))??
                };
                let codex_notice = if apply_info.preserved_official_login {
                    Some("preserved_official_login")
                } else {
                    Some("official_login_required")
                };
                // Config write succeeded. Session rewrite failures must not roll
                // back the provider switch; surface them as a warning instead.
                let session_sync = match codex_provider_sync::sync_to_managed_provider() {
                    Ok(result) => {
                        log::info!(
                            "Codex 历史会话同步完成: changed={} sqlite={} status={}",
                            result.changed_session_files,
                            result.sqlite_rows_updated,
                            result.status
                        );
                        result
                    }
                    Err(error) => {
                        log::warn!("Codex 历史会话同步失败（配置已切换）: {error}");
                        CodexProviderSyncResult {
                            status: "warning".into(),
                            message: format!("供应商已切换，但历史会话同步失败：{error}"),
                            target_provider: codex::managed_provider_id().into(),
                            backup_dir: None,
                            changed_session_files: 0,
                            sqlite_rows_updated: 0,
                            skipped_locked_files: Vec::new(),
                        }
                    }
                };
                Ok((Some(session_sync), codex_notice))
            }
            ProviderTarget::OpenCode => {
                sync_opencode_providers_to_live(state)?;
                Ok((None, None))
            }
            ProviderTarget::Pi => {
                sync_pi_providers_to_live(state)?;
                Ok((None, None))
            }
            ProviderTarget::Dsh => {
                sync_dsh_providers_to_live(state)?;
                Ok((None, None))
            }
            ProviderTarget::Cline => {
                if uses_proxy {
                    state
                        .proxy
                        .lock()
                        .await
                        .start(proxy_port, ProviderTarget::Cline)
                        .await?;
                }
                sync_cline_providers_to_live(state)?;
                Ok((None, None))
            }
        }
    }.await;
    let (session_sync, codex_notice) = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Err(mark_error) = snapshot.capture_last_written_files() {
                return rollback_switch(snapshot, state, AppError::Config(format!("{error}；无法安全确认配置写入状态：{mark_error}"))).await;
            }
            return rollback_switch(snapshot, state, error).await;
        }
    };
    if let Err(error) = snapshot.capture_last_written_files() {
        return rollback_switch(snapshot, state, error).await;
    }
    if !uses_proxy {
        state.proxy.lock().await.stop_target(runtime_provider.target_app);
    }
    if let Some(app) = app {
        crate::commands::proxy::publish_target_status(app, state, runtime_provider.target_app).await;
    }
    Ok((snapshot, session_sync, codex_notice))
}

struct SwitchSnapshot {
    target: ProviderTarget,
    files: Vec<FileSnapshot>,
    ownership_key: &'static str,
    ownership_value: Option<String>,
    proxy: ProxySnapshot,
}

struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
    last_written: Option<Option<Vec<u8>>>,
}

struct ProxySnapshot {
    running: bool,
    port: u16,
}

impl SwitchSnapshot {
    async fn capture(state: &AppState, target: ProviderTarget) -> AppResult<Self> {
        let paths = match target {
            ProviderTarget::ClaudeCode => vec![crate::config::get_claude_settings_path()],
            ProviderTarget::ClaudeDesktop => {
                let paths = claude_desktop::detect_claude_desktop();
                let mut files = Vec::new();
                if let Some(config_library) = &paths.config_library {
                    files.push(config_library.join(format!("{}.json", claude_desktop::PROFILE_ID)));
                    files.push(config_library.join(format!(
                        "{}.json",
                        claude_desktop::LEGACY_PROFILE_ID
                    )));
                }
                if let Some(meta_path) = &paths.meta_path {
                    files.push(meta_path.clone());
                }
                if let Some(normal_config_path) = &paths.normal_config_path {
                    files.push(normal_config_path.clone());
                }
                if let Some(threep_config_path) = &paths.threep_config_path {
                    files.push(threep_config_path.clone());
                }
                files
            }
            ProviderTarget::Codex => vec![
                crate::config::get_codex_config_path(),
                crate::config::get_codex_auth_path(),
                crate::config::get_codex_config_dir().join("ai-switcher-model-catalog.json"),
            ],
            ProviderTarget::OpenCode => vec![crate::config::get_opencode_config_path()],
            ProviderTarget::Pi => vec![
                crate::coding::pi::config::get_pi_settings_path(),
                crate::coding::pi::config::get_pi_auth_path(),
                crate::coding::pi::config::get_pi_models_path(),
            ],
            ProviderTarget::Dsh => vec![
                crate::config::get_dsh_settings_path(),
                crate::config::get_dsh_credentials_path(),
            ],
            ProviderTarget::Cline => vec![crate::config::cline::cline_config_dir().join("ai-switcher.json")],
        };
        let files = paths.into_iter().map(FileSnapshot::capture).collect::<AppResult<Vec<_>>>()?;
        let ownership_key = match target {
            ProviderTarget::ClaudeCode => CODE_OWNERSHIP_KEY,
            ProviderTarget::ClaudeDesktop => DESKTOP_OWNERSHIP_KEY,
            ProviderTarget::Codex => CODEX_OWNERSHIP_KEY,
            ProviderTarget::OpenCode => OPENCODE_OWNERSHIP_KEY,
            ProviderTarget::Pi => PI_OWNERSHIP_KEY,
            ProviderTarget::Dsh => "v1310.dsh_managed",
            ProviderTarget::Cline => "v1323.cline_managed",
        };
        let ownership_value = state.db.with_conn(|conn| get_setting(conn, ownership_key))?;
        let proxy = {
            let proxy = state.proxy.lock().await;
            let status = proxy.status_for(target);
            ProxySnapshot { running: status.running, port: status.port }
        };
        Ok(Self { target, files, ownership_key, ownership_value, proxy })
    }

    async fn restore(self, state: &AppState) -> AppResult<()> {
        let mut failures = Vec::new();
        for file in self.files {
            if let Err(error) = file.restore() {
                failures.push(format!("恢复配置文件失败: {error}"));
            }
        }
        if let Err(error) = state.db.with_conn(|conn| {
            set_setting(conn, self.ownership_key, self.ownership_value.as_deref().unwrap_or(""))
        }) {
            failures.push(format!("恢复配置所有权失败: {error}"));
        }
        let proxy_result = {
            let mut proxy = state.proxy.lock().await;
            if self.proxy.running {
                proxy.start(self.proxy.port, self.target).await
            } else {
                proxy.stop_target(self.target);
                Ok(())
            }
        };
        if let Err(error) = proxy_result {
            failures.push(format!("恢复本地代理失败: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Config(failures.join("；")))
        }
    }

    fn capture_last_written_files(&mut self) -> AppResult<()> {
        for file in &mut self.files {
            file.capture_last_written()?;
        }
        Ok(())
    }
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> AppResult<Self> {
        let contents = if path.exists() { Some(std::fs::read(&path)?) } else { None };
        Ok(Self { path, contents, last_written: None })
    }

    fn capture_last_written(&mut self) -> AppResult<()> {
        self.last_written = Some(Self::read_contents(&self.path)?);
        Ok(())
    }

    fn restore(self) -> AppResult<()> {
        let Some(last_written) = self.last_written else {
            return Ok(());
        };
        if Self::read_contents(&self.path)? != last_written {
            return Err(AppError::Config(format!(
                "检测到配置文件已被外部修改，已拒绝覆盖: {}",
                self.path.display()
            )));
        }
        match self.contents {
            Some(contents) => crate::config::atomic_write(&self.path, &contents),
            None if self.path.exists() => {
                std::fs::remove_file(&self.path)?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn read_contents(path: &std::path::Path) -> AppResult<Option<Vec<u8>>> {
        if path.exists() {
            Ok(Some(std::fs::read(path)?))
        } else {
            Ok(None)
        }
    }
}

fn switch_failure_message(error: &AppError) -> String {
    match error {
        AppError::Config(message) => message.clone(),
        other => other.to_string(),
    }
}

async fn rollback_switch<T>(snapshot: SwitchSnapshot, state: &AppState, error: AppError) -> AppResult<T> {
    match snapshot.restore(state).await {
        Ok(()) => Err(AppError::Config(format!(
            "{}（已回滚到切换前配置）",
            switch_failure_message(&error)
        ))),
        Err(rollback_error) => Err(AppError::Config(format!(
            "{}；已尝试回滚，但部分恢复失败：{}",
            switch_failure_message(&error),
            switch_failure_message(&rollback_error)
        ))),
    }
}

const CODE_OWNERSHIP_KEY: &str = "p7.code_config_ownership";
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CodeOwnership {
    before: BTreeMap<String, Option<Value>>,
    written: BTreeMap<String, Option<Value>>,
}

fn code_managed_fields() -> AppResult<BTreeMap<String, Option<Value>>> {
    let path = crate::config::get_claude_settings_path();
    let settings = if path.exists() {
        serde_json::from_slice::<Value>(&std::fs::read(path)?)?
    } else {
        Value::Object(Default::default())
    };
    let env = settings.get("env").and_then(Value::as_object);
    Ok(claude_code::MANAGED_ENV_KEYS.into_iter().map(|key| {
        (
            key.to_string(),
            normalize_managed_value(env.and_then(|map| map.get(key)).cloned()),
        )
    }).collect())
}

/// Treat missing / null / blank string as the same "absent" state for ownership.
fn normalize_managed_value(value: Option<Value>) -> Option<Value> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.trim().is_empty() => None,
        other => other,
    }
}

fn normalize_managed_value_for_key(key: &str, value: Option<Value>) -> Option<Value> {
    let value = normalize_managed_value(value);
    match (key, value) {
        ("ANTHROPIC_BASE_URL", Some(Value::String(url))) => {
            let trimmed = url.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(Value::String(trimmed))
            }
        }
        (_, other) => other,
    }
}

fn normalize_managed_fields(
    fields: &BTreeMap<String, Option<Value>>,
) -> BTreeMap<String, Option<Value>> {
    fields
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                normalize_managed_value_for_key(key, value.clone()),
            )
        })
        .collect()
}

/// Keys we expected to be absent (`None`) but which now exist (e.g. Claude Code
/// rewrote `ANTHROPIC_API_KEY`) are adopted so the next apply can clear them
/// without blocking the user. Real edits to values we previously wrote still
/// fail the ownership check.
fn adopt_absent_key_drift(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    for (key, current_value) in current {
        let written =
            normalize_managed_value_for_key(key, ownership.written.get(key).cloned().unwrap_or(None));
        let current_value = normalize_managed_value_for_key(key, current_value.clone());
        if written.is_none() && current_value.is_some() {
            ownership
                .before
                .entry(key.clone())
                .or_insert_with(|| current_value.clone());
            ownership.written.insert(key.clone(), current_value);
        }
    }
}

/// Claude Code sometimes relocates the same secret between `ANTHROPIC_AUTH_TOKEN`
/// and `ANTHROPIC_API_KEY`. Treat that migration as compatible ownership drift.
fn adopt_credential_key_migration(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    const AUTH: &str = "ANTHROPIC_AUTH_TOKEN";
    const API: &str = "ANTHROPIC_API_KEY";

    let written_auth =
        normalize_managed_value(ownership.written.get(AUTH).cloned().unwrap_or(None));
    let written_api = normalize_managed_value(ownership.written.get(API).cloned().unwrap_or(None));
    let current_auth = normalize_managed_value(current.get(AUTH).cloned().unwrap_or(None));
    let current_api = normalize_managed_value(current.get(API).cloned().unwrap_or(None));

    if written_auth.is_some()
        && current_auth.is_none()
        && current_api == written_auth
        && (written_api.is_none() || written_api == written_auth)
    {
        ownership.written.insert(AUTH.to_string(), None);
        ownership.written.insert(API.to_string(), current_api.clone());
        ownership
            .before
            .entry(API.to_string())
            .or_insert_with(|| current_api);
        return;
    }

    if written_api.is_some()
        && current_api.is_none()
        && current_auth == written_api
        && (written_auth.is_none() || written_auth == written_api)
    {
        ownership.written.insert(API.to_string(), None);
        ownership
            .written
            .insert(AUTH.to_string(), current_auth.clone());
        ownership
            .before
            .entry(AUTH.to_string())
            .or_insert_with(|| current_auth);
    }
}

fn reconcile_code_ownership(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    upgrade_code_ownership_fields(ownership, current);
    adopt_absent_key_drift(ownership, current);
    adopt_credential_key_migration(ownership, current);
}

fn managed_fields_match(
    left: &BTreeMap<String, Option<Value>>,
    right: &BTreeMap<String, Option<Value>>,
) -> bool {
    normalize_managed_fields(left) == normalize_managed_fields(right)
}

fn expected_code_fields(
    provider: &Provider,
    proxy: bool,
    catalog: bool,
    port: u16,
) -> BTreeMap<String, Option<Value>> {
    use crate::provider::{
        ClaudeModelRole, CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID, CLAUDE_OPUS_ROLE_ID,
        CLAUDE_SONNET_ROLE_ID,
    };

    let mut values = claude_code::MANAGED_ENV_KEYS
        .into_iter()
        .map(|key| (key.to_string(), None))
        .collect::<BTreeMap<_, _>>();
    values.insert("ANTHROPIC_BASE_URL".to_string(), Some(Value::String(if proxy {
        format!("http://127.0.0.1:{port}")
    } else { provider.base_url.clone() })));
    values.insert("ANTHROPIC_AUTH_TOKEN".to_string(), Some(Value::String(if proxy {
        if catalog {
            crate::gateway::resolved_gateway_token(&provider.api_key)
                .unwrap_or("")
                .to_string()
        } else {
            "local-proxy-code".to_string()
        }
    } else { provider.api_key.clone() })));
    if catalog && proxy {
        values.insert(
            "ANTHROPIC_API_KEY".to_string(),
            Some(Value::String(
                crate::gateway::resolved_gateway_token(&provider.api_key)
                    .unwrap_or("")
                    .to_string(),
            )),
        );
    }
    if catalog {
        values.insert(
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".to_string(),
            Some(Value::String("1".to_string())),
        );
    }
    let roles = [
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            CLAUDE_SONNET_ROLE_ID,
            ClaudeModelRole::Sonnet,
        ),
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            CLAUDE_OPUS_ROLE_ID,
            ClaudeModelRole::Opus,
        ),
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            CLAUDE_HAIKU_ROLE_ID,
            ClaudeModelRole::Haiku,
        ),
        (
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            CLAUDE_FABLE_ROLE_ID,
            ClaudeModelRole::Fable,
        ),
    ];
    if catalog {
        values.insert(
            "ANTHROPIC_MODEL".to_string(),
            Some(Value::String(crate::gateway::normalize_live_model_for(
                ProviderTarget::ClaudeCode,
                &provider.model,
            ))),
        );
    } else if !proxy && !provider.model.trim().is_empty() {
        values.insert(
            "ANTHROPIC_MODEL".to_string(),
            Some(Value::String(provider.model.trim().to_string())),
        );
    }
    for (model_key, name_key, stable_model, role) in roles {
        let upstream = provider
            .model_mapping
            .for_role(role, provider.model.trim())
            .to_string();
        values.insert(
            model_key.to_string(),
            Some(Value::String(if proxy {
                stable_model.to_string()
            } else {
                upstream.clone()
            })),
        );
        values.insert(name_key.to_string(), Some(Value::String(upstream)));
    }
    values.insert(
        "CLAUDE_CODE_SUBAGENT_MODEL".to_string(),
        Some(Value::String(
            provider
                .model_mapping
                .for_role(ClaudeModelRole::Subagent, provider.model.trim())
                .to_string(),
        )),
    );
    values
}

fn upgrade_code_ownership_fields(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    for key in claude_code::MANAGED_ENV_KEYS {
        if !ownership.written.contains_key(key) {
            let current_value = current.get(key).cloned().unwrap_or(None);
            ownership
                .before
                .entry(key.to_string())
                .or_insert(None);
            ownership
                .written
                .insert(key.to_string(), current_value);
        } else {
            ownership.before.entry(key.to_string()).or_insert(None);
        }
    }
}

fn prepare_code_ownership(
    provider: &Provider,
    state: &AppState,
    proxy: bool,
    catalog: bool,
    port: u16,
) -> AppResult<CodeOwnership> {
    let current = code_managed_fields()?;
    let expected = expected_code_fields(provider, proxy, catalog, port);
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        let mut ownership: CodeOwnership = serde_json::from_str(&raw)
            .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全切换".to_string()))?;
        reconcile_code_ownership(&mut ownership, &current);
        if !managed_fields_match(&ownership.written, &current) {
            // Claude Code / user edits after our last write used to hard-block every
            // later switch. Rebaseline onto the live file so the user can continue.
            log::warn!(
                "Claude Code managed fields drifted from ownership record; rebasing onto current settings before switch"
            );
            return Ok(CodeOwnership {
                before: current,
                written: expected,
            });
        }
        ownership.written = expected;
        Ok(ownership)
    } else {
        Ok(CodeOwnership {
            before: current,
            written: expected,
        })
    }
}

fn commit_code_ownership(state: &AppState, ownership: CodeOwnership) -> AppResult<()> {
    state.db.with_conn(|conn| {
        set_setting(conn, CODE_OWNERSHIP_KEY, &serde_json::to_string(&ownership)?)
    })
}

fn restore_code_ownership(state: &AppState) -> AppResult<()> {
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return claude_code::clear_provider_from_settings();
    };
    let mut ownership: CodeOwnership = serde_json::from_str(&raw)
        .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全恢复".to_string()))?;
    let current = code_managed_fields()?;
    reconcile_code_ownership(&mut ownership, &current);
    if !managed_fields_match(&current, &ownership.written) {
        // Stale before-state is no longer trustworthy; clear managed keys for
        // official mode instead of refusing forever.
        log::warn!(
            "Claude Code managed fields drifted from ownership record; clearing managed fields for official restore"
        );
        claude_code::clear_provider_from_settings()?;
        return state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""));
    }
    claude_code::restore_managed_fields(&ownership.before)?;
    state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""))
}

const DESKTOP_OWNERSHIP_KEY: &str = "p7.desktop_original_applied_id";

fn desktop_switch_original_applied_id(
    stored_raw: Option<&str>,
    current_applied: Option<&str>,
) -> AppResult<Option<String>> {
    let current = current_applied
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let current_is_managed = current
        .as_deref()
        .is_some_and(claude_desktop::is_managed_profile_id);
    if let Some(raw) = stored_raw.map(str::trim).filter(|value| !value.is_empty()) {
        if current_is_managed {
            let original: Option<String> = serde_json::from_str(raw)?;
            return Ok(original.filter(|id| !claude_desktop::is_managed_profile_id(id)));
        }
        // Desktop UI (or a failed gateway session) switched away from our
        // profile. An explicit「设为当前」must take over again, same as Code
        // rebasing onto drifted settings instead of refusing forever.
        log::warn!(
            "Claude Desktop appliedId drifted from managed profile; rebasing ownership before switch"
        );
        return Ok(current.filter(|id| !claude_desktop::is_managed_profile_id(id)));
    }
    Ok(current.filter(|id| !claude_desktop::is_managed_profile_id(id)))
}

fn prepare_desktop_ownership(state: &AppState) -> AppResult<Option<String>> {
    if !claude_desktop::is_supported_platform() {
        // Stale ownership from a Windows/macOS DB copy must not block Linux users.
        let _ = state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""));
        return Err(AppError::Config(
            "当前系统不支持 Claude Desktop 配置管理（仅 Windows / macOS）".to_string(),
        ));
    }
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    let applied = claude_desktop::current_applied_id()?;
    desktop_switch_original_applied_id(raw.as_deref(), applied.as_deref())
}

fn commit_desktop_ownership(state: &AppState, original_applied_id: Option<String>) -> AppResult<()> {
    state.db.with_conn(|conn| {
        set_setting(conn, DESKTOP_OWNERSHIP_KEY, &serde_json::to_string(&original_applied_id)?)
    })
}

fn restore_desktop_ownership(state: &AppState) -> AppResult<()> {
    if !claude_desktop::is_supported_platform() {
        let _ = state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""));
        return Err(AppError::Config(
            "当前系统不支持 Claude Desktop 配置管理（仅 Windows / macOS）".to_string(),
        ));
    }
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return claude_desktop::clear_provider();
    };
    if !claude_desktop::current_applied_id()?
        .as_deref()
        .is_some_and(claude_desktop::is_managed_profile_id)
    {
        return Err(AppError::Config("检测到 Claude Desktop 配置已被外部修改，已拒绝覆盖".to_string()));
    }
    let original: Option<String> = serde_json::from_str(&raw)?;
    claude_desktop::clear_provider_restoring_applied_id(original)?;
    state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""))
}
