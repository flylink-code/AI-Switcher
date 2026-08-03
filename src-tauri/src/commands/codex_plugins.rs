//! Codex Agent Plugins commands.

use crate::codex_plugins::{self, CodexPlugin};
use crate::error::AppResult;

#[tauri::command]
pub fn list_codex_plugins() -> AppResult<Vec<CodexPlugin>> {
    codex_plugins::list_plugins()
}

#[tauri::command]
pub fn set_codex_plugin_enabled(plugin_id: String, enabled: bool) -> AppResult<()> {
    codex_plugins::set_plugin_enabled(&plugin_id, enabled)
}
