//! Safe, credential-free Codex status commands.

use crate::config::codex::{auth_status, CodexAuthStatus};
use crate::config::codex_provider_sync::{self, CodexProviderSyncResult};
use crate::error::AppResult;

#[tauri::command]
pub fn get_codex_auth_status() -> AppResult<CodexAuthStatus> {
    Ok(auth_status())
}

/// Rewrite Codex historical session `model_provider` values so the Codex UI
/// continues to show threads after a third-party provider switch.
#[tauri::command]
pub fn sync_codex_session_providers(
    target_provider: Option<String>,
) -> AppResult<CodexProviderSyncResult> {
    let target = target_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    codex_provider_sync::sync_sessions_to_provider(None, target)
}
