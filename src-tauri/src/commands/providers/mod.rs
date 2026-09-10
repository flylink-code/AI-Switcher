//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::catalog::{self, build_catalog_with, CatalogStyle};
use crate::config::{claude_code, claude_desktop, codex, codex_provider_sync, opencode};
use crate::config::codex_provider_sync::CodexProviderSyncResult;
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::process_util::spawn_blocking_result;
use crate::provider::{
    api_endpoint_url, catalog_models_from_provider, copied_provider_input, normalize_base_url,
    protocol_endpoint_path, should_activate_copied_provider, strip_anthropic_compat_path,
    ClaudeModelMapping, ConnectionTestResult, EndpointSpeedtestResult, LiveProviderInfo,
    ModelDiscoveryResult, Provider, ProviderExportBundle, ProviderExportEntry,
    normalized_model_mapping, normalized_auto_review_model_override, validate_target_protocol,
    ProviderImportResult, ProviderInput, ProviderKind, ProviderTarget, ProtocolType,
};
use crate::store::AppState;
use chrono::Utc;
use reqwest::header;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tauri::{Emitter, Manager};

const MODEL_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_DISCOVERED_MODELS: usize = 1_000;
const MAX_MODEL_NAME_CHARS: usize = 256;
const CODEX_OWNERSHIP_KEY: &str = "v040.codex_managed";
const OPENCODE_OWNERSHIP_KEY: &str = "v131.opencode_managed";
const PI_OWNERSHIP_KEY: &str = "v136.pi_managed";

/// A Codex switch updates config.toml, auth.json, and the model catalog as one
/// logical operation. Startup repair and a user click must not interleave their
/// snapshots, writes, or rollback decisions after an updater relaunch.
fn codex_switch_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tauri::command]
pub fn list_providers(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state.db.with_read_conn(|conn| dao::list_providers(conn, target))
}

fn gateway_catalog_on(state: &AppState, target: ProviderTarget) -> bool {
    catalog::enabled(state.db.as_ref(), target)
}

fn patch_default_profile_field<F>(state: &AppState, target: ProviderTarget, mutate: F) -> AppResult<()>
where
    F: FnOnce(&mut crate::database::dao::gateway::GatewayProfilePatch),
{
    state.db.with_conn(|conn| {
        let profile = crate::database::dao::gateway::ensure_profile_for_target(conn, target)?;
        let mut patch = crate::database::dao::gateway::GatewayProfilePatch::default();
        mutate(&mut patch);
        crate::database::dao::gateway::patch_profile(conn, &profile.id, &patch)?;
        Ok(())
    })
}

/// Compatibility shim: reads the current Agent connection (legacy catalog flag).
#[tauri::command]
pub fn get_gateway_catalog_enabled(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    if !target.supports_gateway_catalog() {
        return Ok(false);
    }
    Ok(gateway_catalog_on(&state, target))
}

/// Compatibility shim: writes `agent_connections` + default profile.
#[tauri::command]
pub async fn set_gateway_catalog_enabled(
    target: ProviderTarget,
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    if !target.supports_gateway_catalog() {
        return Err(AppError::Config("此 Agent 切换请使用连接选择（外部供应商 / 网关档案）".to_string()));
    };
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::ensure_profile_for_target(conn, target)?;
        crate::database::dao::gateway::set_current_connection_type(
            conn,
            target.as_str(),
            if enabled {
                crate::database::dao::gateway::ConnectionType::Gateway
            } else {
                crate::database::dao::gateway::ConnectionType::External
            },
        )
    })?;
    crate::catalog::invalidate_view_cache();
    if enabled {
        sync_gateway_catalog_target(target, Some(&app), &state).await?;
    } else if let Some(provider) = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, target))?
    {
        let _ = apply_target_provider(&provider, Some(&app), &state).await?;
        if target == ProviderTarget::ClaudeCode {
            let _ = claude_code::apply_opusplan_model(false);
            let _ = crate::wsl_direct::sync_claude_codex_files();
        }
    } else {
        restore_official_for_target(target, Some(&app), &state, true).await?;
    }
    crate::commands::proxy::publish_target_status(&app, &state, target).await;
    Ok(enabled)
}

pub(crate) async fn sync_live_after_connection_change<R: tauri::Runtime>(
    target: ProviderTarget,
    gateway: bool,
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> AppResult<()> {
    if gateway {
        if target.supports_gateway_catalog() {
            sync_gateway_catalog_target(target, Some(app), state).await?;
        } else {
            if target == ProviderTarget::Cline && !gateway {
                let port = get_saved_proxy_port(state, target);
                state.proxy.lock().await.start(port, target).await?;
            }
            sync_live_providers(state, target, Some(app)).await?;
        }
    } else if target.supports_gateway_catalog() {
        if let Some(provider) = state
            .db
            .with_conn(|conn| dao::get_current_provider(conn, target))?
        {
            let _ = apply_target_provider(&provider, Some(app), state).await?;
            if target == ProviderTarget::ClaudeCode {
                let _ = claude_code::apply_opusplan_model(false);
                let _ = crate::wsl_direct::sync_claude_codex_files();
            }
        } else {
            restore_official_for_target(target, Some(app), state, true).await?;
        }
    } else {
        state.proxy.lock().await.stop_target(target);
        sync_live_providers(state, target, Some(app)).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_gateway_catalog_subagent(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    Ok(catalog_subagent_model(&state, target).unwrap_or_default())
}

#[tauri::command]
pub async fn set_gateway_catalog_subagent(
    target: ProviderTarget,
    model: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let Some(key) = catalog::subagent_setting_key(target) else {
        return Err(AppError::Config("此 Agent 不支持目录子代理".to_string()));
    };
    let trimmed = model.trim().to_string();
    state.db.with_conn(|conn| set_setting(conn, key, &trimmed))?;
    patch_default_profile_field(&state, target, |profile| {
        profile.subagent_model = Some(trimmed.clone());
    })?;
    crate::catalog::invalidate_view_cache();
    if gateway_catalog_on(&state, target) {
        if let Some(provider) = state
            .db
            .with_conn(|conn| dao::get_current_provider(conn, target))?
        {
            let _ = apply_target_provider(&provider, Some(&app), &state).await?;
        }
    }
    Ok(trimmed)
}

#[tauri::command]
pub fn get_gateway_catalog_hide_official(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    if !target.supports_gateway_catalog() {
        return Ok(false);
    }
    Ok(catalog::hide_official(state.db.as_ref(), target))
}

#[tauri::command]
pub async fn set_gateway_catalog_hide_official(
    target: ProviderTarget,
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    let Some(key) = catalog::hide_official_setting_key(target) else {
        return Err(AppError::Config("此 Agent 不支持隐藏官方内置模型".to_string()));
    };
    state
        .db
        .with_conn(|conn| set_setting(conn, key, if enabled { "true" } else { "false" }))?;
    patch_default_profile_field(&state, target, |profile| {
        profile.hide_official = Some(enabled);
    })?;
    crate::catalog::invalidate_view_cache();
    if gateway_catalog_on(&state, target) {
        sync_gateway_catalog_target(target, Some(&app), &state).await?;
    }
    crate::commands::proxy::publish_target_status(&app, &state, target).await;
    Ok(enabled)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCatalogModelOption {
    pub public_id: String,
    pub display_name: String,
    pub provider_name: String,
    #[serde(default)]
    pub context_window: u64,
    #[serde(default)]
    pub web_search_enabled: bool,
    #[serde(default)]
    pub vision_enabled: bool,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
}

fn require_claude_code_catalog(target: ProviderTarget) -> AppResult<()> {
    if target != ProviderTarget::ClaudeCode {
        return Err(AppError::Config("仅 Claude Code 统一目录支持 Opus Plan".to_string()));
    }
    Ok(())
}

async fn persist_claude_code_catalog_setting<R: tauri::Runtime>(
    target: ProviderTarget,
    key: &str,
    value: &str,
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> AppResult<()> {
    require_claude_code_catalog(target)?;
    state.db.with_conn(|conn| set_setting(conn, key, value))?;
    crate::catalog::invalidate_view_cache();
    if key == catalog::GATEWAY_CATALOG_CODE_OPUSPLAN_KEY {
        patch_default_profile_field(state, target, |profile| {
            profile.role_routing_enabled = Some(false);
        })?;
    } else if key == catalog::GATEWAY_CATALOG_CODE_PLAN_KEY {
        patch_default_profile_field(state, target, |profile| {
            profile.plan_model = Some(value.to_string());
        })?;
    } else if key == catalog::GATEWAY_CATALOG_CODE_EXECUTE_KEY {
        patch_default_profile_field(state, target, |profile| {
            profile.execute_model = Some(value.to_string());
        })?;
    }
    if gateway_catalog_on(state, target) {
        if let Some(provider) = state
            .db
            .with_conn(|conn| dao::get_current_provider(conn, target))?
        {
            let _ = apply_target_provider(&provider, Some(app), state).await?;
        }
    }
    if key == catalog::GATEWAY_CATALOG_CODE_OPUSPLAN_KEY && value != "true" {
        let _ = claude_code::apply_opusplan_model(false);
        let _ = crate::wsl_direct::sync_claude_codex_files();
    }
    Ok(())
}

#[tauri::command]
pub fn get_gateway_catalog_opusplan(
    _target: ProviderTarget,
    _state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    Ok(false)
}

#[tauri::command]
pub async fn set_gateway_catalog_opusplan(
    target: ProviderTarget,
    _enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
    let Some(key) = catalog::opusplan_setting_key(target) else {
        return Err(AppError::Config("仅 Claude Code 曾支持 Opus Plan".to_string()));
    };
    persist_claude_code_catalog_setting(target, key, "false", &app, &state).await?;
    let _ = claude_code::apply_opusplan_model(false);
    let _ = crate::wsl_direct::sync_claude_codex_files();
    Ok(false)
}

#[tauri::command]
pub fn get_gateway_catalog_plan(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    Ok(catalog::plan_model(state.db.as_ref(), target).unwrap_or_default())
}

#[tauri::command]
pub async fn set_gateway_catalog_plan(
    target: ProviderTarget,
    model: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let Some(key) = catalog::plan_setting_key(target) else {
        return Err(AppError::Config("仅 Claude Code 统一目录支持规划模型".to_string()));
    };
    let trimmed = model.trim().to_string();
    persist_claude_code_catalog_setting(target, key, &trimmed, &app, &state).await?;
    Ok(trimmed)
}

#[tauri::command]
pub fn get_gateway_catalog_execute(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    Ok(catalog::execute_model(state.db.as_ref(), target).unwrap_or_default())
}

#[tauri::command]
pub async fn set_gateway_catalog_execute(
    target: ProviderTarget,
    model: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let Some(key) = catalog::execute_setting_key(target) else {
        return Err(AppError::Config("仅 Claude Code 统一目录支持执行模型".to_string()));
    };
    let trimmed = model.trim().to_string();
    persist_claude_code_catalog_setting(target, key, &trimmed, &app, &state).await?;
    Ok(trimmed)
}

#[tauri::command]
pub fn get_claude_code_default_permission_mode() -> AppResult<String> {
    claude_code::read_permission_default_mode()
}

#[tauri::command]
pub fn set_claude_code_default_permission_mode(mode: String) -> AppResult<String> {
    let written = claude_code::apply_permission_default_mode(&mode)?;
    let _ = crate::wsl_direct::sync_claude_codex_files();
    Ok(written)
}

#[tauri::command]
pub fn list_gateway_catalog_models(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<String>> {
    catalog_public_ids_for(&state, target)
}

fn catalog_public_ids_for(state: &AppState, target: ProviderTarget) -> AppResult<Vec<String>> {
    let style = catalog::catalog_style_for(target);
    let pairs = load_gateway_pairs(state, target)?;
    let hide_official = catalog::hide_official(state.db.as_ref(), target);
    Ok(catalog::with_auto_public_ids(
        style,
        build_catalog_with(style, &pairs, hide_official)
            .into_iter()
            .map(|entry| entry.public_id)
            .collect(),
    ))
}

fn saved_smart_gateway_port(state: &AppState) -> u16 {
    state
        .db
        .with_conn(|conn| crate::database::dao::settings::get_setting(conn, crate::gateway::service::PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(crate::gateway::SMART_GATEWAY_PORT)
}

/// Whether applying this provider should start the per-agent local proxy.
/// Smart-gateway catalog for Code/Codex/Cline hits 15828 directly; Desktop still uses 15822.
fn target_starts_agent_proxy(
    target: ProviderTarget,
    gateway_catalog: bool,
    provider: &Provider,
) -> bool {
    match target {
        ProviderTarget::OpenCode | ProviderTarget::Pi | ProviderTarget::Dsh => false,
        ProviderTarget::ClaudeCode | ProviderTarget::Codex | ProviderTarget::Cline
            if gateway_catalog =>
        {
            false
        }
        _ => gateway_catalog || provider.is_codex_oauth() || provider.requires_local_proxy(),
    }
}

fn live_uses_gateway_catalog(state: &AppState, provider: &Provider) -> bool {
    if provider.target_app.is_catalog_target() {
        gateway_catalog_on(state, provider.target_app)
    } else {
        provider.is_smart_gateway()
    }
}

#[tauri::command]
pub fn list_gateway_catalog_entries(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayCatalogModelOption>> {
    let style = catalog::catalog_style_for(target);
    let pairs = load_gateway_pairs(&state, target)?;
    let hide_official = catalog::hide_official(state.db.as_ref(), target);
    let names: BTreeMap<String, String> = pairs
        .iter()
        .map(|(provider, _)| (provider.id.clone(), provider.name.clone()))
        .collect();
    let mut options: Vec<GatewayCatalogModelOption> = vec![GatewayCatalogModelOption {
        provider_name: "Auto".to_string(),
        public_id: "auto".to_string(),
        display_name: "Auto".to_string(),
        context_window: 0,
        web_search_enabled: false,
        vision_enabled: false,
        reasoning_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
    }];
    options.extend(build_catalog_with(style, &pairs, hide_official).into_iter().map(|entry| {
        let inferred = crate::gateway::metadata::infer(&entry.public_id);
        GatewayCatalogModelOption {
            provider_name: names
                .get(&entry.provider_id)
                .cloned()
                .unwrap_or_else(|| entry.display_name.clone()),
            public_id: entry.public_id,
            display_name: entry.display_name,
            context_window: entry.context_window,
            web_search_enabled: entry.web_search_enabled
                || inferred.capabilities["web_search"].as_bool().unwrap_or(false),
            vision_enabled: inferred.capabilities["vision"].as_bool().unwrap_or(false),
            reasoning_levels: if inferred.reasoning_levels.is_empty() {
                vec!["off".into(), "low".into(), "medium".into(), "high".into()]
            } else {
                inferred.reasoning_levels
            },
        }
    }));
    Ok(options)
}


include!("crud.rs");
include!("apply.rs");
include!("live_sync.rs");
include!("gateway_binding.rs");
include!("copy_across_agents.rs");
