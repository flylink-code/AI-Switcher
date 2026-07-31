//! Backup trigger command.

use crate::backup::{
    backup_file, export_library_backup as export_library,
    preview_library_backup as preview_library, restore_library_backup as restore_library,
    LibraryArchivePreview, LibraryBackupInfo, LibraryRestoreResult, DEFAULT_BACKUP_KEEP,
};
use crate::config::paths::get_app_db_path;
use crate::error::{AppError, AppResult};

/// Back up the app database now, returning the created backup path (or a message
/// if the source was missing).
#[tauri::command]
pub fn backup_now() -> AppResult<String> {
    let src = get_app_db_path();
    // Checkpoint WAL so the backup copy reflects the latest committed data.
    // (No-op if the DB has no WAL; safe to ignore the result.)
    if let Ok(conn) = rusqlite::Connection::open(&src) {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    match backup_file(&src, DEFAULT_BACKUP_KEEP)? {
        Some(p) => Ok(p.to_string_lossy().into_owned()),
        None => Err(AppError::Config(format!(
            "数据库文件不存在，无法备份: {}",
            src.display()
        ))),
    }
}

/// Export a versioned, portable managed-library ZIP. The archive excludes API
/// keys, OS credentials, Claude sign-in state, and private keys by design.
#[tauri::command]
pub fn export_library_backup() -> AppResult<LibraryBackupInfo> {
    export_library()
}

/// Verify a portable library ZIP before any restore workflow is allowed to
/// stage it.  This command never extracts or changes local files.
#[tauri::command]
pub fn preview_library_backup(archive_path: String) -> AppResult<LibraryArchivePreview> {
    preview_library(std::path::Path::new(&archive_path))
}

/// Replace the local managed library from a verified portable ZIP.
/// Requires an application restart before the restored database is used.
#[tauri::command]
pub fn restore_library_backup(archive_path: String) -> AppResult<LibraryRestoreResult> {
    restore_library(std::path::Path::new(&archive_path))
}
