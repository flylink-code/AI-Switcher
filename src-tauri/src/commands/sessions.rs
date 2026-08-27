//! Tauri commands for the read-only local session manager.

use crate::error::{AppError, AppResult};
use crate::session_manager::{
    self, SessionArchiveInfo, SessionBackupArchiveInfo, SessionBatchBackupInfo,
    SessionBatchExportInfo, SessionBatchRestoreResult, SessionMessage, SessionMeta, SessionProvider,
    SessionScanResult,
};
use crate::store::AppState;

#[tauri::command]
pub fn get_session_backup_dir(state: tauri::State<'_, AppState>) -> AppResult<String> {
    let dir = state
        .db
        .with_conn(|conn| session_manager::get_configured_session_backup_dir(conn))?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn set_session_backup_dir(path: String, state: tauri::State<'_, AppState>) -> AppResult<String> {
    state
        .db
        .with_conn(|conn| session_manager::set_configured_session_backup_dir(conn, &path))
}

#[tauri::command]
pub fn reset_session_backup_dir(state: tauri::State<'_, AppState>) -> AppResult<String> {
    state
        .db
        .with_conn(|conn| session_manager::reset_configured_session_backup_dir(conn))
}

#[tauri::command]
pub fn get_session_auto_backup_settings(
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::session_backup::SessionAutoBackupSettings> {
    state
        .db
        .with_conn(crate::session_backup::load_auto_backup_settings)
}

#[tauri::command]
pub fn set_session_auto_backup_settings(
    settings: crate::session_backup::SessionAutoBackupSettings,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::session_backup::SessionAutoBackupSettings> {
    state
        .db
        .with_conn(|conn| crate::session_backup::save_auto_backup_settings(conn, &settings))
}

#[tauri::command]
pub fn get_session_mirror_dir(
    provider: SessionProvider,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let backup_dir = state
        .db
        .with_conn(|conn| session_manager::get_configured_session_backup_dir(conn))?;
    Ok(
        crate::session_backup::mirror_provider_dir(&backup_dir, provider)
            .to_string_lossy()
            .into_owned(),
    )
}

#[tauri::command]
pub async fn restore_session_mirror(
    provider: SessionProvider,
    overwrite: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> AppResult<SessionBatchRestoreResult> {
    let backup_dir = state
        .db
        .with_conn(|conn| session_manager::get_configured_session_backup_dir(conn))?;
    let overwrite = overwrite.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        crate::session_backup::restore_session_mirror(provider, &backup_dir, overwrite)
    })
    .await
    .map_err(|error| AppError::Tauri(format!("从镜像恢复会话失败: {error}")))?
}

#[tauri::command]
pub async fn backup_all_sessions(
    provider: SessionProvider,
    destination_dir: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<SessionBatchExportInfo> {
    let target_dir = match destination_dir {
        Some(dir) if !dir.trim().is_empty() => Some(dir),
        _ => {
            let configured = state
                .db
                .with_conn(|conn| session_manager::get_configured_session_backup_dir(conn))?;
            Some(configured.to_string_lossy().into_owned())
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::backup_all_sessions(provider, target_dir.as_deref())
    })
    .await
    .map_err(|error| AppError::Tauri(format!("全量会话备份任务失败: {error}")))?
}

#[tauri::command]
pub async fn list_session_backups(
    provider: Option<SessionProvider>,
    backup_dir: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<SessionBackupArchiveInfo>> {
    let target_dir = match backup_dir {
        Some(dir) if !dir.trim().is_empty() => Some(dir),
        _ => {
            let configured = state
                .db
                .with_conn(|conn| session_manager::get_configured_session_backup_dir(conn))?;
            Some(configured.to_string_lossy().into_owned())
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::list_session_backups(provider, target_dir.as_deref())
    })
    .await
    .map_err(|error| AppError::Tauri(format!("读取会话备份列表失败: {error}")))?
}

#[tauri::command]
pub async fn restore_session_backup(
    provider: SessionProvider,
    archive_path: String,
    overwrite: Option<bool>,
) -> AppResult<SessionBatchRestoreResult> {
    let overwrite = overwrite.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::restore_session_backup(provider, &archive_path, overwrite)
    })
    .await
    .map_err(|error| AppError::Tauri(format!("恢复会话备份任务失败: {error}")))?
}

#[tauri::command]
pub async fn scan_sessions(
    provider: Option<SessionProvider>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<SessionScanResult> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::scan_sessions(provider, offset, limit)
    })
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
pub async fn export_session(provider: SessionProvider, source_path: String, destination_dir: Option<String>) -> AppResult<SessionArchiveInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::export_session(provider, &source_path, destination_dir.as_deref()))
        .await.map_err(|error| AppError::Tauri(format!("会话导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn export_session_markdown(provider: SessionProvider, source_path: String, destination_dir: Option<String>) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || session_manager::export_session_markdown(provider, &source_path, destination_dir.as_deref()))
        .await.map_err(|error| AppError::Tauri(format!("会话 Markdown 导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn backup_sessions(provider: SessionProvider, source_paths: Vec<String>) -> AppResult<SessionBatchBackupInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::backup_sessions(provider, &source_paths))
        .await.map_err(|error| AppError::Tauri(format!("会话批量备份任务失败: {error}")))?
}

#[tauri::command]
pub async fn export_sessions(provider: SessionProvider, source_paths: Vec<String>, destination_dir: Option<String>) -> AppResult<SessionBatchExportInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::export_sessions(provider, &source_paths, destination_dir.as_deref()))
        .await.map_err(|error| AppError::Tauri(format!("会话批量导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn import_session(provider: SessionProvider, archive_path: String) -> AppResult<SessionMeta> {
    tauri::async_runtime::spawn_blocking(move || session_manager::import_session(provider, &archive_path))
        .await.map_err(|error| AppError::Tauri(format!("会话导入任务失败: {error}")))?
}

#[tauri::command]
pub async fn trash_session(provider: SessionProvider, source_path: String) -> AppResult<SessionArchiveInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::trash_session(provider, &source_path))
        .await.map_err(|error| AppError::Tauri(format!("会话删除任务失败: {error}")))?
}

#[tauri::command]
pub async fn restore_trashed_session(provider: SessionProvider, archive_path: String) -> AppResult<SessionMeta> {
    tauri::async_runtime::spawn_blocking(move || session_manager::restore_trashed_session(provider, &archive_path))
        .await.map_err(|error| AppError::Tauri(format!("会话恢复任务失败: {error}")))?
}

#[tauri::command]
pub async fn list_trashed_sessions(provider: SessionProvider) -> AppResult<Vec<SessionArchiveInfo>> {
    tauri::async_runtime::spawn_blocking(move || session_manager::list_trashed_sessions(provider))
        .await.map_err(|error| AppError::Tauri(format!("会话回收站读取失败: {error}")))?
}

// migrate_claude_code_session: UI/IPC disabled — proxy Responses multi-turn fix
// made same-provider resume work; keep session_manager::migrate_* for unit tests only.

// --- Legacy Claude Code-only session commands ---
// Kept for backward compatibility; the frontend now uses the generic
// provider-parameterized commands above (`export_session`, `backup_sessions`, ...).

#[tauri::command]
pub async fn export_claude_code_session(
    source_path: String,
    destination_dir: Option<String>,
) -> AppResult<SessionArchiveInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::export_claude_code_session(&source_path, destination_dir.as_deref())
    })
        .await.map_err(|error| AppError::Tauri(format!("会话导出任务失败: {error}")))?
}

#[tauri::command]
pub async fn backup_claude_code_sessions(source_paths: Vec<String>) -> AppResult<SessionBatchBackupInfo> {
    tauri::async_runtime::spawn_blocking(move || session_manager::backup_claude_code_sessions(&source_paths))
        .await.map_err(|error| AppError::Tauri(format!("会话批量备份任务失败: {error}")))?
}

#[tauri::command]
pub async fn export_claude_code_sessions(
    source_paths: Vec<String>,
    destination_dir: Option<String>,
) -> AppResult<SessionBatchExportInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::export_claude_code_sessions(&source_paths, destination_dir.as_deref())
    })
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
