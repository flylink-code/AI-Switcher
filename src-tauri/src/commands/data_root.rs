//! Managed application-library migration. Claude's live files remain under
//! their official locations; only AI-Switcher-owned data is moved.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::paths::{configured_data_root, get_legacy_app_config_dir, write_data_root_config};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRootInfo {
    pub active_path: String,
    pub legacy_path: String,
    pub migrated: bool,
    pub restart_required: bool,
}

#[tauri::command]
pub fn get_data_root() -> DataRootInfo {
    let legacy = get_legacy_app_config_dir();
    let active = configured_data_root().unwrap_or_else(|| legacy.clone());
    DataRootInfo {
        active_path: active.to_string_lossy().into_owned(),
        legacy_path: legacy.to_string_lossy().into_owned(),
        migrated: active != legacy,
        restart_required: false,
    }
}

/// Copy the legacy AI-Switcher data directory into an empty target and make it
/// the active root for the next process launch. Existing data is never removed.
#[tauri::command]
pub fn migrate_data_root(target_path: String) -> AppResult<DataRootInfo> {
    let legacy = get_legacy_app_config_dir();
    let source = configured_data_root().unwrap_or_else(|| legacy.clone());
    let target = normalize_target(&target_path)?;
    if target == source {
        return Err(AppError::Config("所选目录已经是当前资料库目录".to_string()));
    }
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err(AppError::Config("资料库目录不能嵌套在当前资料库中".to_string()));
    }
    if target.exists() && fs::read_dir(&target)?.next().is_some() {
        return Err(AppError::Config("资料库目标目录必须为空，避免覆盖已有文件".to_string()));
    }
    fs::create_dir_all(&target)?;
    if source.exists() {
        copy_directory_except_database(&source, &target, &source)?;
        let source_database = source.join("app.db");
        if source_database.is_file() {
            snapshot_sqlite_database(&source_database, &target.join("app.db"))?;
        }
        verify_directory_copy_except_database(&source, &target)?;
        if source_database.is_file() {
            verify_sqlite_snapshot(&target.join("app.db"))?;
        }
    }
    write_data_root_config(&target)?;
    Ok(DataRootInfo {
        active_path: target.to_string_lossy().into_owned(),
        legacy_path: legacy.to_string_lossy().into_owned(),
        migrated: true,
        restart_required: true,
    })
}

/// `app.db` may have an active WAL while the UI runs.  Copy it through
/// SQLite's backup API instead of copying `app.db`, `app.db-wal`, and
/// `app.db-shm` as ordinary files.
fn snapshot_sqlite_database(source: &Path, target: &Path) -> AppResult<()> {
    let source = rusqlite::Connection::open(source)?;
    let mut target = rusqlite::Connection::open(target)?;
    let backup = rusqlite::backup::Backup::new(&source, &mut target)?;
    backup.run_to_completion(100, std::time::Duration::from_millis(5), None)?;
    Ok(())
}

fn verify_sqlite_snapshot(path: &Path) -> AppResult<()> {
    let connection = rusqlite::Connection::open(path)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(AppError::Config(format!("资料库数据库快照校验失败: {integrity}")));
    }
    Ok(())
}

fn normalize_target(value: &str) -> AppResult<PathBuf> {
    let raw = PathBuf::from(value.trim());
    if !raw.is_absolute() {
        return Err(AppError::Config("资料库目录必须是绝对路径".to_string()));
    }
    fs::create_dir_all(&raw)?;
    raw.canonicalize().map_err(Into::into)
}

fn copy_directory_except_database(source: &Path, target: &Path, root: &Path) -> AppResult<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let relative = source_path.strip_prefix(root).map_err(|_| AppError::Path("资料库路径超出根目录".to_string()))?;
        if is_database_file(relative) { continue; }
        let target_path = target.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(AppError::Config(format!("资料库不支持迁移符号链接: {}", source_path.display())));
        }
        if kind.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory_except_database(&source_path, &target_path, root)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

/// Compare every copied path and content hash before changing the immutable
/// bootstrap pointer. A failed verification leaves the old library active and
/// the target untouched for inspection/retry.
fn verify_directory_copy_except_database(source: &Path, target: &Path) -> AppResult<()> {
    let mut source_files = list_files_with_hashes_except_database(source)?;
    let mut target_files = list_files_with_hashes_except_database(target)?;
    source_files.sort();
    target_files.sort();
    if source_files != target_files {
        return Err(AppError::Config("资料库复制校验失败，旧资料库仍保持活动状态".to_string()));
    }
    Ok(())
}

fn list_files_with_hashes_except_database(root: &Path) -> AppResult<Vec<(String, String)>> {
    let mut files = Vec::new();
    list_files_recursive_except_database(root, root, &mut files)?;
    Ok(files)
}

fn list_files_recursive_except_database(root: &Path, current: &Path, files: &mut Vec<(String, String)>) -> AppResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|_| AppError::Path("资料库路径超出根目录".to_string()))?;
        if is_database_file(relative) { continue; }
        if kind.is_symlink() {
            return Err(AppError::Config(format!("资料库不支持校验符号链接: {}", path.display())));
        }
        if kind.is_dir() {
            list_files_recursive_except_database(root, &path, files)?;
        } else if kind.is_file() {
            files.push((relative.to_string_lossy().replace('\\', "/"), sha256_file(&path)?));
        }
    }
    Ok(())
}

fn is_database_file(relative: &Path) -> bool {
    relative.components().count() == 1
        && matches!(relative.to_string_lossy().as_ref(), "app.db" | "app.db-wal" | "app.db-shm")
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn relative_library_paths_are_rejected() {
        assert!(normalize_target("relative/library").is_err());
    }

    #[test]
    fn copied_library_is_verified_by_relative_path_and_content_hash() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        fs::create_dir_all(source.path().join("nested")).unwrap();
        fs::write(source.path().join("nested/item.txt"), b"contents").unwrap();
        copy_directory_except_database(source.path(), target.path(), source.path()).unwrap();
        verify_directory_copy_except_database(source.path(), target.path()).unwrap();
        fs::write(target.path().join("nested/item.txt"), b"changed").unwrap();
        assert!(verify_directory_copy_except_database(source.path(), target.path()).is_err());
    }

    #[test]
    fn sqlite_database_is_copied_as_an_integrity_checked_snapshot() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        let source_db = source.path().join("app.db");
        let connection = rusqlite::Connection::open(&source_db).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE values_table (value TEXT); INSERT INTO values_table VALUES ('present');").unwrap();
        drop(connection);

        let target_db = target.path().join("app.db");
        snapshot_sqlite_database(&source_db, &target_db).unwrap();
        verify_sqlite_snapshot(&target_db).unwrap();
        let copied = rusqlite::Connection::open(target_db).unwrap();
        let value: String = copied.query_row("SELECT value FROM values_table", [], |row| row.get(0)).unwrap();
        assert_eq!(value, "present");
    }
}
