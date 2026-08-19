use crate::codex_plugins::{
    self, CodexMarketplaceListResult, CodexPluginCatalog, CodexPluginCommandResult,
    CodexPluginUpdateStatus, CodexPluginsSnapshot,
};
use crate::error::AppResult;
use crate::process_util::spawn_blocking_result;

#[tauri::command]
pub fn list_codex_plugins() -> AppResult<CodexPluginsSnapshot> {
    codex_plugins::list_plugins_snapshot()
}

#[tauri::command]
pub fn set_codex_plugin_enabled(plugin_id: String, enabled: bool) -> AppResult<()> {
    codex_plugins::set_plugin_enabled(&plugin_id, enabled)
}

#[tauri::command]
pub fn list_codex_plugin_marketplaces() -> AppResult<CodexMarketplaceListResult> {
    codex_plugins::list_marketplaces()
}

#[tauri::command]
pub fn list_codex_plugin_catalog() -> AppResult<CodexPluginCatalog> {
    codex_plugins::list_plugin_catalog()
}

#[tauri::command]
pub async fn add_codex_plugin_marketplace(source: String) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::add_marketplace(&source)).await
}

#[tauri::command]
pub async fn remove_codex_plugin_marketplace(name: String) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::remove_marketplace(&name)).await
}

#[tauri::command]
pub async fn upgrade_codex_plugin_marketplace(
    name: Option<String>,
) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::upgrade_marketplace(name.as_deref())).await
}

#[tauri::command]
pub async fn uninstall_codex_plugin(plugin_id: String) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::uninstall_plugin(&plugin_id)).await
}

#[tauri::command]
pub async fn install_codex_plugin(plugin_id: String) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::install_plugin(&plugin_id)).await
}

#[tauri::command]
pub async fn update_codex_plugin(plugin_id: String) -> AppResult<CodexPluginCommandResult> {
    spawn_blocking_result(move || codex_plugins::update_plugin(&plugin_id)).await
}

#[tauri::command]
pub async fn check_codex_plugin_update(plugin_id: String) -> AppResult<CodexPluginUpdateStatus> {
    spawn_blocking_result(move || codex_plugins::check_plugin_update(&plugin_id)).await
}

#[tauri::command]
pub async fn check_codex_plugin_updates() -> AppResult<Vec<CodexPluginUpdateStatus>> {
    spawn_blocking_result(codex_plugins::check_plugin_updates).await
}
