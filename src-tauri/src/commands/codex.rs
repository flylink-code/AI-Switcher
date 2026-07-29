//! Safe, credential-free Codex status commands.

use crate::config::codex::{auth_status, CodexAuthStatus};
use crate::error::AppResult;

#[tauri::command]
pub fn get_codex_auth_status() -> AppResult<CodexAuthStatus> {
    Ok(auth_status())
}
