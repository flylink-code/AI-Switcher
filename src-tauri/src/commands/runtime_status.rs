//! Runtime status of managed client apps (Claude Code / Desktop / Codex).

use crate::error::AppResult;
use crate::runtime_status::{self, ManagedAppRuntimeStatus};

#[tauri::command]
pub fn get_managed_apps_runtime_status() -> AppResult<ManagedAppRuntimeStatus> {
    Ok(runtime_status::get_managed_apps_runtime_status())
}
