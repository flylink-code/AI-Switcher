use crate::claude_plugins::{
    self, ClaudeMarketplaceListResult, ClaudePluginCatalog, ClaudePluginCommandResult,
    ClaudePluginUpdateStatus, ClaudePluginsSnapshot,
};
use crate::error::AppResult;
use crate::process_util::spawn_blocking_result;

#[tauri::command]
pub fn list_claude_plugins() -> AppResult<ClaudePluginsSnapshot> {
    claude_plugins::list_plugins_snapshot()
}

#[tauri::command]
pub fn set_claude_plugin_enabled(plugin_id: String, enabled: bool) -> AppResult<()> {
    claude_plugins::set_plugin_enabled(&plugin_id, enabled)
}

#[tauri::command]
pub fn list_claude_plugin_marketplaces() -> AppResult<ClaudeMarketplaceListResult> {
    claude_plugins::list_marketplaces()
}

#[tauri::command]
pub fn list_claude_plugin_catalog() -> AppResult<ClaudePluginCatalog> {
    claude_plugins::list_plugin_catalog()
}

#[tauri::command]
pub async fn add_claude_plugin_marketplace(source: String) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::add_marketplace(&executable, &source)
    })
    .await
}

#[tauri::command]
pub async fn remove_claude_plugin_marketplace(name: String) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::remove_marketplace(&executable, &name)
    })
    .await
}

#[tauri::command]
pub async fn update_claude_plugin_marketplace(
    name: Option<String>,
) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::update_marketplace(&executable, name.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn uninstall_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::uninstall_plugin(&executable, &plugin_id)
    })
    .await
}

#[tauri::command]
pub async fn install_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::install_plugin(&executable, &plugin_id)
    })
    .await
}

#[tauri::command]
pub async fn update_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::update_plugin(&executable, &plugin_id)
    })
    .await
}

#[tauri::command]
pub async fn check_claude_plugin_update(plugin_id: String) -> AppResult<ClaudePluginUpdateStatus> {
    spawn_blocking_result(move || {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::check_plugin_update(&executable, &plugin_id)
    })
    .await
}

#[tauri::command]
pub async fn check_claude_plugin_updates() -> AppResult<Vec<ClaudePluginUpdateStatus>> {
    spawn_blocking_result(|| {
        let executable = claude_plugins::resolve_claude_executable()?;
        claude_plugins::check_plugin_updates(&executable)
    })
    .await
}
