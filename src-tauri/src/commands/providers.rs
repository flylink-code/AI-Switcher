//! Provider management commands.

use serde::Serialize;

use crate::config::claude_code;
use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::{Provider, ProviderInput};
use crate::store::AppState;

#[derive(Debug, Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub notes: String,
}

/// List all providers, ordered by sort index.
#[tauri::command]
pub fn list_providers(state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state.db.with_conn(|conn| dao::list_providers(conn))
}

/// The currently-active provider, if any.
#[tauri::command]
pub fn get_current_provider(state: tauri::State<'_, AppState>) -> AppResult<Option<Provider>> {
    state.db.with_conn(|conn| dao::get_current_provider(conn))
}

/// Create a new provider from frontend input. Returns the persisted provider.
#[tauri::command]
pub fn create_provider(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}

/// Update an existing provider. `input.id` must be set.
#[tauri::command]
pub fn update_provider(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.id.is_none() {
        return Err(AppError::Config("更新供应商时缺少 id".to_string()));
    }
    let updated = state
        .db
        .with_conn(|conn| dao::upsert_provider(conn, &input))?;
    // If the updated provider is the current one, re-apply it to settings.json so
    // the live config reflects the edits (e.g. rotated token).
    if updated.is_current {
        claude_code::apply_provider_to_settings(&updated)?;
    }
    Ok(updated)
}

/// Delete a provider by id.
#[tauri::command]
pub fn delete_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::delete_provider(conn, &id))
}

/// Activate a provider: write it to settings.json and mark it current.
#[tauri::command]
pub fn switch_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    let provider = state
        .db
        .with_conn(|conn| {
            dao::get_provider(conn, &id)?
                .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
        })?;
    claude_code::apply_provider_to_settings(&provider)?;
    state
        .db
        .with_conn(|conn| dao::set_current_provider(conn, &id))?;
    Ok(provider)
}

/// Switch to official login mode: clear ANTHROPIC_* keys, unset current.
#[tauri::command]
pub fn switch_to_official(state: tauri::State<'_, AppState>) -> AppResult<()> {
    claude_code::clear_provider_from_settings()?;
    state.db.with_conn(|conn| dao::clear_current_provider(conn))?;
    Ok(())
}

/// Reorder providers by id sequence.
#[tauri::command]
pub fn reorder_providers(
    ordered_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| dao::reorder_providers(conn, &ordered_ids))
}

/// Re-run the live-config import (pulls current settings.json into the DB).
#[tauri::command]
pub fn import_live_config(state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| crate::database::seed::run_seed(conn))
}

/// The bundled preset catalog (for the "add from preset" picker).
#[tauri::command]
pub fn list_presets() -> Vec<PresetInfo> {
    crate::provider_presets::presets()
        .iter()
        .map(|p| PresetInfo {
            name: p.name.to_string(),
            base_url: p.base_url.to_string(),
            model: p.model.to_string(),
            notes: p.notes.to_string(),
        })
        .collect()
}
