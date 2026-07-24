//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::config::{claude_code, claude_desktop};
use crate::database::dao;
use crate::database::dao::settings::get_setting;
use crate::error::{AppError, AppResult};
use crate::provider::{LiveProviderInfo, Provider, ProviderInput, ProviderTarget, ProtocolType};
use crate::store::AppState;


#[tauri::command]
pub fn list_providers(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state.db.with_conn(|conn| dao::list_providers(conn, target))
}

#[tauri::command]
pub fn get_current_provider(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Option<Provider>> {
    state.db.with_conn(|conn| dao::get_current_provider(conn, target))
}

#[tauri::command]
pub fn create_provider(input: ProviderInput, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}

#[tauri::command]
pub async fn update_provider(input: ProviderInput, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    if input.id.is_none() {
        return Err(AppError::Config("更新供应商时缺少 id".to_string()));
    }
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if provider.is_current {
        apply_target_provider(&provider, &state).await?;
    }
    Ok(provider)
}

#[tauri::command]
pub fn delete_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::delete_provider(conn, &id))
}

/// Activate a provider only for the application that owns it.
#[tauri::command]
pub async fn switch_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    apply_target_provider(&provider, &state).await?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &id))?;
    Ok(provider)
}

#[tauri::command]
pub fn switch_to_official(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    match target {
        ProviderTarget::ClaudeCode => claude_code::clear_provider_from_settings()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::clear_provider()?,
    }
    state.db.with_conn(|conn| dao::clear_current_provider(conn, target))
}

#[tauri::command]
pub fn reorder_providers(ordered_ids: Vec<String>, target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::reorder_providers(conn, &ordered_ids, target))
}

/// Import a live third-party configuration into its matching application list.
#[tauri::command]
pub fn import_live_config(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let live = match target {
        ProviderTarget::ClaudeCode => claude_code::read_current_live_provider()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::read_current_live_provider()?,
    };
    let Some(live) = live else {
        return Ok(());
    };
    import_live_provider(live, target, &state)
}

async fn apply_target_provider(provider: &Provider, state: &AppState) -> AppResult<()> {
    let proxy_port = get_saved_proxy_port(state);
    match provider.target_app {
        ProviderTarget::ClaudeCode => {
            if provider.protocol_type == ProtocolType::Proxy {
                state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeCode).await?;
                claude_code::apply_provider_to_settings_via_proxy(provider, proxy_port)
            } else {
                claude_code::apply_provider_to_settings(provider)
            }
        }
        ProviderTarget::ClaudeDesktop => {
            if provider.protocol_type == ProtocolType::Proxy {
                state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeDesktop).await?;
            }
            claude_desktop::apply_provider(provider, proxy_port)
        }
    }
}

fn import_live_provider(live: LiveProviderInfo, target: ProviderTarget, state: &AppState) -> AppResult<()> {
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    if let Some(provider) = existing.iter().find(|p| p.base_url == live.base_url) {
        state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        return Ok(());
    }
    let input = ProviderInput {
        id: None,
        name: "当前配置（已导入）".to_string(),
        base_url: live.base_url,
        api_key: live.auth_token,
        model: live.model,
        protocol_type: ProtocolType::Anthropic,
        target_app: target,
        notes: "从当前 Claude Code 配置导入".to_string(),
    };
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))
}

fn get_saved_proxy_port(state: &AppState) -> u16 {
    state.db.with_conn(|conn| get_setting(conn, "proxy_port"))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(15821)
}
