use crate::codex_plugins::{
    self, CodexMarketplaceListResult, CodexPluginCommandResult, CodexPluginsSnapshot,
};
use crate::error::AppResult;

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
pub fn add_codex_plugin_marketplace(source: String) -> AppResult<CodexPluginCommandResult> {
    codex_plugins::add_marketplace(&source)
}

#[tauri::command]
pub fn remove_codex_plugin_marketplace(name: String) -> AppResult<CodexPluginCommandResult> {
    codex_plugins::remove_marketplace(&name)
}

#[tauri::command]
pub fn uninstall_codex_plugin(plugin_id: String) -> AppResult<CodexPluginCommandResult> {
    codex_plugins::uninstall_plugin(&plugin_id)
}
