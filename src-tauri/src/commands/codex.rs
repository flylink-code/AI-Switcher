//! Safe, credential-free Codex status commands.

use crate::config::codex::{
    auth_status, get_output_profile, get_web_search_mode, set_output_profile, set_web_search_mode,
    CodexAuthStatus, CodexOutputProfile, CodexOutputProfileSnapshot, CodexWebSearchMode,
    CodexWebSearchSnapshot,
};
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

#[tauri::command]
pub fn get_codex_web_search_mode() -> AppResult<CodexWebSearchSnapshot> {
    get_web_search_mode()
}

#[tauri::command]
pub fn set_codex_web_search_mode(mode: CodexWebSearchMode) -> AppResult<CodexWebSearchSnapshot> {
    set_web_search_mode(mode)
}

#[tauri::command]
pub fn get_codex_output_profile() -> AppResult<CodexOutputProfileSnapshot> {
    get_output_profile()
}

#[tauri::command]
pub fn set_codex_output_profile(
    profile: CodexOutputProfile,
    show_raw_reasoning: bool,
) -> AppResult<CodexOutputProfileSnapshot> {
    let snapshot = set_output_profile(profile, show_raw_reasoning)?;
    let _ = crate::wsl_direct::sync_claude_codex_files();
    Ok(snapshot)
}
