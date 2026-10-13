//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::catalog::{self, build_catalog_with, CatalogStyle};
use crate::config::codex_provider_sync::CodexProviderSyncResult;
use crate::config::{claude_code, claude_desktop, codex, codex_provider_sync, opencode};
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, catalog_models_from_provider, normalize_base_url,
    normalized_auto_review_model_override, normalized_model_mapping, protocol_endpoint_path,
    strip_anthropic_compat_path, validate_target_protocol, ClaudeModelMapping,
    ConnectionTestResult, EndpointSpeedtestResult, LiveProviderInfo, ModelDiscoveryResult,
    ProtocolType, Provider, ProviderExportBundle, ProviderExportEntry, ProviderImportResult,
    ProviderInput, ProviderKind, ProviderTarget,
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
use tauri::Emitter;

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
pub fn list_providers(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<Provider>> {
    state
        .db
        .with_read_conn(|conn| dao::list_providers(conn, target))
}

fn gateway_catalog_on(state: &AppState, target: ProviderTarget) -> bool {
    catalog::enabled(state.db.as_ref(), target)
}

fn patch_default_profile_field<F>(
    state: &AppState,
    target: ProviderTarget,
    mutate: F,
) -> AppResult<()>
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
        return Err(AppError::Config(
            "此 Agent 不支持隐藏官方内置模型".to_string(),
        ));
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
    /// Providers that publish this model, highest priority first.
    #[serde(default)]
    pub source_names: Vec<String>,
    #[serde(default)]
    pub context_window: u64,
    #[serde(default)]
    pub web_search_enabled: bool,
    #[serde(default)]
    pub vision_enabled: bool,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
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
pub fn get_claude_desktop_auto_mode() -> AppResult<bool> {
    Ok(claude_desktop::read_auto_mode_enabled())
}

#[tauri::command]
pub fn set_claude_desktop_auto_mode(enabled: bool) -> AppResult<bool> {
    claude_desktop::apply_auto_mode_enabled(enabled)
}

#[tauri::command]
pub fn get_claude_code_agent_settings() -> AppResult<claude_code::ClaudeCodeAgentSettings> {
    claude_code::read_agent_settings()
}

#[tauri::command]
pub fn set_claude_code_agent_settings(
    settings: claude_code::ClaudeCodeAgentSettings,
) -> AppResult<claude_code::ClaudeCodeAgentSettings> {
    let written = claude_code::apply_agent_settings(&settings)?;
    let _ = crate::wsl_direct::sync_claude_codex_files();
    Ok(written)
}

#[tauri::command]
pub fn get_opencode_permission_mode() -> AppResult<String> {
    opencode::read_permission_mode()
}

#[tauri::command]
pub fn set_opencode_permission_mode(mode: String) -> AppResult<String> {
    opencode::apply_permission_mode(&mode)
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
    Ok(build_catalog_with(style, &pairs, hide_official)
        .into_iter()
        .map(|entry| entry.public_id)
        .collect())
}

fn saved_smart_gateway_port(state: &AppState) -> u16 {
    state
        .db
        .with_conn(|conn| {
            crate::database::dao::settings::get_setting(conn, crate::gateway::service::PORT_SETTING)
        })
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
    let options = build_catalog_with(style, &pairs, hide_official)
        .into_iter()
        .map(|entry| {
            let inferred = crate::gateway::metadata::infer(&entry.public_id);
            let source_names: Vec<String> =
                names.get(&entry.provider_id).cloned().into_iter().collect();
            GatewayCatalogModelOption {
                provider_name: names
                    .get(&entry.provider_id)
                    .cloned()
                    .unwrap_or_else(|| entry.display_name.clone()),
                source_names,
                public_id: entry.public_id,
                display_name: entry.display_name,
                context_window: entry.context_window,
                web_search_enabled: entry.web_search_enabled
                    || inferred.capabilities["web_search"]
                        .as_bool()
                        .unwrap_or(false),
                vision_enabled: inferred.capabilities["vision"].as_bool().unwrap_or(false),
                reasoning_levels: if inferred.reasoning_levels.is_empty() {
                    vec!["off".into(), "low".into(), "medium".into(), "high".into()]
                } else {
                    inferred.reasoning_levels
                },
            }
        });
    Ok(options.collect())
}

include!("crud.rs");
include!("apply.rs");
include!("live_sync.rs");
include!("gateway_binding.rs");
include!("copy_across_agents.rs");
include!("unified.rs");
include!("config_drift.rs");
