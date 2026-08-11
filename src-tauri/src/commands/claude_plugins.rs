use crate::claude_plugins::{
    self, ClaudeMarketplaceListResult, ClaudePluginCatalog, ClaudePluginCommandResult,
    ClaudePluginUpdateStatus, ClaudePluginsSnapshot,
};
use crate::error::AppResult;

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
pub fn add_claude_plugin_marketplace(source: String) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::add_marketplace(&executable, &source)
}

#[tauri::command]
pub fn remove_claude_plugin_marketplace(name: String) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::remove_marketplace(&executable, &name)
}

#[tauri::command]
pub fn update_claude_plugin_marketplace(
    name: Option<String>,
) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::update_marketplace(&executable, name.as_deref())
}

#[tauri::command]
pub fn uninstall_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::uninstall_plugin(&executable, &plugin_id)
}

#[tauri::command]
pub fn install_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::install_plugin(&executable, &plugin_id)
}

#[tauri::command]
pub fn update_claude_plugin(plugin_id: String) -> AppResult<ClaudePluginCommandResult> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::update_plugin(&executable, &plugin_id)
}

#[tauri::command]
pub fn check_claude_plugin_update(plugin_id: String) -> AppResult<ClaudePluginUpdateStatus> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::check_plugin_update(&executable, &plugin_id)
}

#[tauri::command]
pub fn check_claude_plugin_updates() -> AppResult<Vec<ClaudePluginUpdateStatus>> {
    let executable = claude_plugins::resolve_claude_executable()?;
    claude_plugins::check_plugin_updates(&executable)
}
