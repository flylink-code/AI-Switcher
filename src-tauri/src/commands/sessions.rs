//! Tauri commands for the read-only local session manager.

use crate::error::{AppError, AppResult};
use crate::session_manager::{
    self, SessionMessage, SessionProvider, SessionScanResult,
};

#[tauri::command]
pub async fn scan_sessions(
    provider: Option<SessionProvider>,
) -> AppResult<SessionScanResult> {
    tauri::async_runtime::spawn_blocking(move || session_manager::scan_sessions(provider))
        .await
        .map_err(|error| AppError::Tauri(format!("会话扫描任务失败: {error}")))?
}

#[tauri::command]
pub async fn search_session_contents(
    query: String,
    provider: Option<SessionProvider>,
    limit: Option<usize>,
) -> AppResult<SessionScanResult> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::search_session_contents(&query, provider, limit.unwrap_or(200))
    })
    .await
    .map_err(|error| AppError::Tauri(format!("会话搜索任务失败: {error}")))?
}

#[tauri::command]
pub async fn load_session_messages(
    provider: SessionProvider,
    source_path: String,
) -> AppResult<Vec<SessionMessage>> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_session_messages(provider, &source_path)
    })
    .await
    .map_err(|error| AppError::Tauri(format!("会话加载任务失败: {error}")))?
}
