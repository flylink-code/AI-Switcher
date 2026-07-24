//! Backup trigger command.

use crate::backup::{backup_file, DEFAULT_BACKUP_KEEP};
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
