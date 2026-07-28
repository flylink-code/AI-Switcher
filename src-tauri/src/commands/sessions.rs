//! Tauri commands for the read-only local session manager.

use crate::error::{AppError, AppResult};
use crate::session_manager::{
    self, SessionArchiveInfo, SessionBatchBackupInfo, SessionBatchExportInfo, SessionMessage,
    SessionMeta, SessionProvider, SessionScanResult,
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

#[tauri::command]
pub async fn export_claude_code_session(source_path: String) -> AppResult<SessionArchiveInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::export_claude_code_session(&source_path))
        .await.map_err(|error| AppError::Tauri(format!("会话导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn backup_claude_code_sessions(source_paths: Vec<String>) -> AppResult<SessionBatchBackupInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::backup_claude_code_sessions(&source_paths))
        .await.map_err(|error| AppError::Tauri(format!("会话批量备份任务失败: {error}")))?
}

#[tauri::command]
pub async fn export_claude_code_sessions(source_paths: Vec<String>) -> AppResult<SessionBatchExportInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::export_claude_code_sessions(&source_paths))
        .await.map_err(|error| AppError::Tauri(format!("会话批量导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn import_claude_code_session(archive_path: String) -> AppResult<SessionMeta> {
    tauri::async_runtime::spawn_blocking(move || session_manager::import_claude_code_session(&archive_path))
        .await.map_err(|error| AppError::Tauri(format!("会话导入任务失败: {error}")))?
}

#[tauri::command]
pub async fn trash_claude_code_session(source_path: String) -> AppResult<SessionArchiveInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::trash_claude_code_session(&source_path))
        .await.map_err(|error| AppError::Tauri(format!("会话删除任务失败: {error}")))?
}

#[tauri::command]
pub async fn restore_trashed_claude_code_session(archive_path: String) -> AppResult<SessionMeta> {
    tauri::async_runtime::spawn_blocking(move || session_manager::restore_trashed_claude_code_session(&archive_path))
        .await.map_err(|error| AppError::Tauri(format!("会话恢复任务失败: {error}")))?
}

#[tauri::command]
pub async fn list_trashed_claude_code_sessions() -> AppResult<Vec<SessionArchiveInfo>> {
    tauri::async_runtime::spawn_blocking(session_manager::list_trashed_claude_code_sessions)
        .await.map_err(|error| AppError::Tauri(format!("会话回收站读取失败: {error}")))?
}
