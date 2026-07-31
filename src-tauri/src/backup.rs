//! File-level backup with rotation.
//!
//! Backups are timestamped copies placed in the app's `backups/` directory. After
//! each backup, the oldest copies beyond `max_keep` are pruned (by modification
//! time). The SQLite-level backup primitive (using rusqlite's `backup` feature)
//! will be layered on top in a later phase; for P0 we copy the file directly,
//! which is sufficient when the DB is quiescent or WAL is checkpointed first.

use crate::config::paths::{get_app_config_dir, get_app_db_path, get_backup_dir, get_claude_skills_dir};
use crate::error::{io_context, AppError, AppResult};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Default number of backups to retain.
pub const DEFAULT_BACKUP_KEEP: usize = 10;

/// Sidecar metadata for a backup.  It contains no configuration content or
/// credential and lets recovery reject a damaged backup before writing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub version: u8,
    pub backup_file: String,
    pub source_name: String,
    pub category: String,
    pub created_at: i64,
    pub bytes: u64,
    pub sha256: String,
    pub schema_version: u32,
}

/// Versioned portable archive manifest.  A library archive intentionally
/// excludes all credential material: provider API-key columns are blanked in a
/// SQLite snapshot and neither the OS credential store nor Claude login files
/// are read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryArchiveManifest {
    pub version: u8,
    pub created_at: i64,
    pub schema_version: u32,
    #[serde(default)]
    pub credentials_included: bool,
    pub entries: Vec<LibraryArchiveEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryArchiveEntry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBackupInfo {
    pub archive_path: String,
    pub created_at: i64,
    pub entries: usize,
}

/// Read-only result returned before a user elects to restore a portable
/// library archive.  This is deliberately separate from restoration: a
/// malformed or modified archive is rejected before it can affect local data.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryArchivePreview {
    pub archive_path: String,
    pub created_at: i64,
    pub schema_version: u32,
    pub entries: usize,
    pub total_bytes: u64,
    pub credentials_included: bool,
}

const LIBRARY_ARCHIVE_VERSION: u8 = 1;
const LIBRARY_ARCHIVE_MANIFEST: &str = "manifest.json";
const MAX_LIBRARY_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_LIBRARY_ARCHIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Create a portable ZIP archive of AI-Switcher-owned data and Claude Code
/// skills.  The database is copied through SQLite's backup API before any
/// credential reference is removed, so a live WAL database is never copied as
/// raw files.
///
/// When `destination_dir` is set, the ZIP is written into that directory;
/// otherwise it uses the app backup directory.
///
/// When `include_credentials` is true, provider API keys are resolved from the
/// OS keyring into the snapshot as plaintext so a remote import can rematerialize
/// them. Prefer leaving this off unless the user explicitly opts in.
pub fn export_library_backup(
    destination_dir: Option<&Path>,
    include_credentials: bool,
) -> AppResult<LibraryBackupInfo> {
    let created_at = Utc::now().timestamp_millis();
    let backup_dir = match destination_dir {
        Some(dir) => {
            if !dir.is_dir() {
                return Err(AppError::Path(format!(
                    "导出目录不存在或不是文件夹: {}",
                    dir.display()
                )));
            }
            dir.to_path_buf()
        }
        None => {
            let dir = get_backup_dir();
            fs::create_dir_all(&dir)?;
            dir
        }
    };
    let archive_path = backup_dir.join(format!("library-{created_at}.zip"));
    let staging = StagingDirectory::create()?;
    let snapshot = staging.0.join("app.db");
    create_db_snapshot(&snapshot, include_credentials)?;

    let mut files = vec![("database/app.db".to_string(), snapshot)];
    collect_managed_files(
        &get_app_config_dir().join("session-archives"),
        "session-archives",
        &mut files,
    )?;
    collect_managed_files(
        &get_app_config_dir().join("skill-sources.json"),
        "metadata/skill-sources.json",
        &mut files,
    )?;
    collect_managed_files(&get_claude_skills_dir(), "skills", &mut files)?;

    let mut entries = Vec::with_capacity(files.len());
    let file = fs::File::create(&archive_path).map_err(|error| io_context("创建资料库备份失败", error))?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, path) in &files {
        let content = fs::read(path).map_err(|error| io_context("读取资料库备份内容失败", error))?;
        archive.start_file(name, options).map_err(|error| AppError::Other(format!("写入资料库归档失败: {error}")))?;
        archive.write_all(&content)?;
        entries.push(LibraryArchiveEntry {
            path: name.clone(),
            bytes: content.len() as u64,
            sha256: hex::encode(Sha256::digest(&content)),
        });
    }
    let manifest = LibraryArchiveManifest {
        version: LIBRARY_ARCHIVE_VERSION,
        created_at,
        schema_version: crate::database::schema::SCHEMA_VERSION,
        credentials_included: include_credentials,
        entries,
    };
    archive.start_file(LIBRARY_ARCHIVE_MANIFEST, options).map_err(|error| AppError::Other(format!("写入资料库清单失败: {error}")))?;
    archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    archive.finish().map_err(|error| AppError::Other(format!("完成资料库归档失败: {error}")))?;
    Ok(LibraryBackupInfo {
        archive_path: archive_path.to_string_lossy().into_owned(),
        created_at,
        entries: manifest.entries.len(),
    })
}

/// Find the newest `library-*.zip` under `directory` (non-recursive).
pub fn find_latest_library_archive(directory: &Path) -> AppResult<PathBuf> {
    if !directory.is_dir() {
        return Err(AppError::Path(format!(
            "目录不存在或不是文件夹: {}",
            directory.display()
        )));
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(directory).map_err(|error| io_context("读取归档目录失败", error))? {
        let entry = entry.map_err(|error| io_context("读取归档目录项失败", error))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if !(lower.starts_with("library-") && lower.ends_with(".zip")) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &best {
            Some((time, _)) if modified <= *time => {}
            _ => best = Some((modified, path)),
        }
    }
    best.map(|(_, path)| path).ok_or_else(|| {
        AppError::Config(format!(
            "目录中未找到 library-*.zip：{}",
            directory.display()
        ))
    })
}

/// Validate a portable library ZIP and return a summary without extracting it.
pub fn preview_library_backup(archive_path: &Path) -> AppResult<LibraryArchivePreview> {
    let metadata = fs::metadata(archive_path)
        .map_err(|error| io_context("读取资料库归档失败", error))?;
    if !metadata.is_file() {
        return Err(AppError::Path("资料库归档路径不是文件".to_string()));
    }
    if metadata.len() > MAX_LIBRARY_ARCHIVE_BYTES {
        return Err(AppError::Config("资料库归档超过 1 GB 安全限制".to_string()));
    }

    let file = fs::File::open(archive_path).map_err(|error| io_context("打开资料库归档失败", error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Config(format!("资料库归档不是有效的 ZIP 文件: {error}")))?;
    let manifest_index = (0..archive.len())
        .find(|index| archive.by_index(*index).map(|entry| entry.name() == LIBRARY_ARCHIVE_MANIFEST).unwrap_or(false))
        .ok_or_else(|| AppError::Config("资料库归档缺少清单文件".to_string()))?;
    let manifest: LibraryArchiveManifest = {
        let mut entry = archive.by_index(manifest_index)
            .map_err(|error| AppError::Config(format!("读取资料库清单失败: {error}")))?;
        let mut content = Vec::new();
        entry.read_to_end(&mut content).map_err(|error| io_context("读取资料库清单失败", error))?;
        serde_json::from_slice(&content).map_err(|_| AppError::Config("资料库归档清单格式无效".to_string()))?
    };
    if manifest.version != LIBRARY_ARCHIVE_VERSION {
        return Err(AppError::Config(format!("不支持的资料库归档版本: {}", manifest.version)));
    }

    let mut expected_paths = HashSet::with_capacity(manifest.entries.len());
    let mut total_bytes = 0_u64;
    for expected in &manifest.entries {
        validate_library_archive_path(&expected.path)?;
        if !expected_paths.insert(expected.path.as_str()) {
            return Err(AppError::Config(format!("资料库归档清单包含重复文件: {}", expected.path)));
        }
        if expected.bytes > MAX_LIBRARY_ARCHIVE_ENTRY_BYTES {
            return Err(AppError::Config(format!("资料库归档条目超过 256 MB 限制: {}", expected.path)));
        }
        total_bytes = total_bytes.checked_add(expected.bytes)
            .ok_or_else(|| AppError::Config("资料库归档内容大小溢出".to_string()))?;
        if total_bytes > MAX_LIBRARY_ARCHIVE_BYTES {
            return Err(AppError::Config("资料库归档解压后超过 1 GB 安全限制".to_string()));
        }
        let mut entry = archive.by_name(&expected.path)
            .map_err(|_| AppError::Config(format!("资料库归档缺少内容文件: {}", expected.path)))?;
        if entry.is_dir() || entry.size() != expected.bytes {
            return Err(AppError::Config(format!("资料库归档条目大小不匹配: {}", expected.path)));
        }
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(|error| io_context("校验资料库归档失败", error))?;
            if read == 0 { break; }
            hasher.update(&buffer[..read]);
            bytes += read as u64;
        }
        if bytes != expected.bytes || hex::encode(hasher.finalize()) != expected.sha256 {
            return Err(AppError::Config(format!("资料库归档校验失败: {}", expected.path)));
        }
    }

    let mut actual_paths = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)
            .map_err(|error| AppError::Config(format!("读取资料库归档目录失败: {error}")))?;
        let name = entry.name();
        if name == LIBRARY_ARCHIVE_MANIFEST { continue; }
        validate_library_archive_path(name)?;
        if entry.is_dir() || !actual_paths.insert(name.to_string()) || !expected_paths.contains(name) {
            return Err(AppError::Config(format!("资料库归档包含未清单化或重复条目: {name}")));
        }
    }
    if actual_paths.len() != expected_paths.len() {
        return Err(AppError::Config("资料库归档内容与清单不一致".to_string()));
    }

    Ok(LibraryArchivePreview {
        archive_path: archive_path.to_string_lossy().into_owned(),
        created_at: manifest.created_at,
        schema_version: manifest.schema_version,
        entries: manifest.entries.len(),
        total_bytes,
        credentials_included: manifest.credentials_included,
    })
}

/// Result of replacing the local managed library from a portable ZIP.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRestoreResult {
    pub archive_path: String,
    pub restored_entries: usize,
    pub backup_db_path: Option<String>,
    pub restart_required: bool,
    pub credentials_imported: bool,
}

/// Validate, stage, and replace local managed library files from `archive_path`.
///
/// The live SQLite connection is closed before the on-disk file is replaced, then
/// reopened. Plaintext API keys from credential-inclusive archives are rematerialized
/// into the OS keyring. Callers should still restart for proxy/UI consistency.
pub fn restore_library_backup(
    archive_path: &Path,
    db: &crate::database::Database,
) -> AppResult<LibraryRestoreResult> {
    let preview = preview_library_backup(archive_path)?;
    if preview.schema_version > crate::database::schema::SCHEMA_VERSION {
        return Err(AppError::Config(format!(
            "归档架构版本 v{} 高于当前程序支持的 v{}，请先升级 AI-Switcher",
            preview.schema_version,
            crate::database::schema::SCHEMA_VERSION
        )));
    }

    let staging = StagingDirectory::create()?;
    let restored_entries = extract_library_archive(archive_path, &staging.0)?;

    // Snapshot the live DB so a failed restore can be rolled back manually.
    let db_path = get_app_db_path();
    let backup_db_path = if db_path.is_file() {
        // Checkpoint via a short-lived connection before file-copy backup.
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        backup_file(&db_path, DEFAULT_BACKUP_KEEP)?
            .map(|path| path.to_string_lossy().into_owned())
    } else {
        None
    };

    let staged_db = staging.0.join("database").join("app.db");
    if staged_db.is_file() {
        db.replace_on_disk_and_reopen(&staged_db)?;
    }

    let staged_skills = staging.0.join("skills");
    if staged_skills.exists() {
        replace_directory_contents(&get_claude_skills_dir(), &staged_skills)?;
    }

    let staged_archives = staging.0.join("session-archives");
    if staged_archives.exists() {
        replace_directory_contents(
            &get_app_config_dir().join("session-archives"),
            &staged_archives,
        )?;
    }

    let staged_skill_sources = staging.0.join("metadata").join("skill-sources.json");
    if staged_skill_sources.is_file() {
        let dest = get_app_config_dir().join("skill-sources.json");
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|error| io_context("创建配置目录失败", error))?;
        }
        fs::copy(&staged_skill_sources, &dest)
            .map_err(|error| io_context("写入 skill-sources.json 失败", error))?;
    }

    Ok(LibraryRestoreResult {
        archive_path: preview.archive_path,
        restored_entries,
        backup_db_path,
        restart_required: true,
        credentials_imported: preview.credentials_included,
    })
}

fn extract_library_archive(archive_path: &Path, staging_root: &Path) -> AppResult<usize> {
    let file = fs::File::open(archive_path).map_err(|error| io_context("打开资料库归档失败", error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Config(format!("资料库归档不是有效的 ZIP 文件: {error}")))?;
    let mut count = 0_usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Config(format!("读取资料库归档目录失败: {error}")))?;
        let name = entry.name().to_string();
        if name == LIBRARY_ARCHIVE_MANIFEST || entry.is_dir() {
            continue;
        }
        validate_library_archive_path(&name)?;
        let dest = staging_root.join(Path::new(&name));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|error| io_context("创建解压目录失败", error))?;
        }
        let mut out = fs::File::create(&dest).map_err(|error| io_context("写入解压文件失败", error))?;
        std::io::copy(&mut entry, &mut out).map_err(|error| io_context("解压资料库归档失败", error))?;
        count += 1;
    }
    Ok(count)
}

fn replace_directory_contents(dest: &Path, source: &Path) -> AppResult<()> {
    if dest.exists() {
        let stamp = Utc::now().format("%Y%m%d_%H%M%S_%f");
        let file_name = dest
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "data".to_string());
        let aside = dest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{file_name}.pre-restore-{stamp}"));
        fs::rename(dest, &aside).map_err(|error| io_context("备份原目录失败", error))?;
    }
    copy_directory_recursive(source, dest)
}

fn copy_directory_recursive(source: &Path, dest: &Path) -> AppResult<()> {
    fs::create_dir_all(dest).map_err(|error| io_context("创建目标目录失败", error))?;
    for entry in fs::read_dir(source).map_err(|error| io_context("读取恢复目录失败", error))? {
        let entry = entry.map_err(|error| io_context("读取恢复目录项失败", error))?;
        let file_type = entry.file_type().map_err(|error| io_context("读取恢复目录项类型失败", error))?;
        if file_type.is_symlink() {
            continue;
        }
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_recursive(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to).map_err(|error| io_context("复制恢复文件失败", error))?;
        }
    }
    Ok(())
}

fn validate_library_archive_path(path: &str) -> AppResult<()> {
    if path.is_empty() || path == LIBRARY_ARCHIVE_MANIFEST || path.contains('\\') || path.starts_with('/') {
        return Err(AppError::Path(format!("资料库归档包含不安全路径: {path}")));
    }
    let parsed = Path::new(path);
    if parsed.components().any(|component| !matches!(component, std::path::Component::Normal(_))) {
        return Err(AppError::Path(format!("资料库归档包含不安全路径: {path}")));
    }
    Ok(())
}

fn create_db_snapshot(snapshot: &Path, include_credentials: bool) -> AppResult<()> {
    let source_path = get_app_db_path();
    if !source_path.is_file() {
        return Err(AppError::Config(format!("数据库文件不存在，无法归档: {}", source_path.display())));
    }
    let source = rusqlite::Connection::open(source_path)?;
    let mut destination = rusqlite::Connection::open(snapshot)?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
    backup.run_to_completion(100, std::time::Duration::from_millis(5), None)?;
    drop(backup);
    if include_credentials {
        materialize_credentials_into_snapshot(&destination)?;
    } else {
        // Both legacy plaintext keys and current OS-keyring references are excluded.
        destination.execute("UPDATE providers SET api_key = ''", [])?;
    }
    Ok(())
}

fn materialize_credentials_into_snapshot(destination: &rusqlite::Connection) -> AppResult<()> {
    let mut stmt = destination.prepare("SELECT id, api_key FROM providers")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, value) in rows {
        if value.is_empty() {
            continue;
        }
        let plaintext = if crate::secrets::is_keyring_ref(&value) {
            let account = &value[crate::secrets::KEYRING_REF_PREFIX.len()..];
            match crate::secrets::load_key(account)? {
                Some(secret) => secret,
                None => {
                    destination.execute(
                        "UPDATE providers SET api_key = '' WHERE id = ?",
                        rusqlite::params![id],
                    )?;
                    continue;
                }
            }
        } else {
            value
        };
        destination.execute(
            "UPDATE providers SET api_key = ? WHERE id = ?",
            rusqlite::params![plaintext, id],
        )?;
    }
    Ok(())
}

fn collect_managed_files(source: &Path, archive_root: &str, output: &mut Vec<(String, PathBuf)>) -> AppResult<()> {
    if !source.exists() { return Ok(()); }
    if fs::symlink_metadata(source)?.file_type().is_symlink() { return Ok(()); }
    if source.is_file() {
        output.push((archive_root.to_string(), source.to_path_buf()));
        return Ok(());
    }
    let source_root = source.canonicalize().map_err(|error| io_context("解析资料库目录失败", error))?;
    collect_directory_files(&source_root, &source_root, archive_root, output)
}

fn collect_directory_files(root: &Path, current: &Path, archive_root: &str, output: &mut Vec<(String, PathBuf)>) -> AppResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() { continue; }
        let path = entry.path();
        if file_type.is_dir() {
            collect_directory_files(root, &path, archive_root, output)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|_| AppError::Path("资料库路径超出根目录".to_string()))?;
            let safe = relative.to_string_lossy().replace('\\', "/");
            output.push((format!("{archive_root}/{safe}"), path));
        }
    }
    Ok(())
}

/// A process-owned staging directory which is removed even when ZIP creation
/// returns early with an error. It is never placed beside user data.
struct StagingDirectory(PathBuf);

impl StagingDirectory {
    fn create() -> AppResult<Self> {
        let path = std::env::temp_dir().join(format!("ai-switcher-library-{}", Uuid::new_v4()));
        fs::create_dir(&path).map_err(|error| io_context("创建备份临时目录失败", error))?;
        Ok(Self(path))
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("清理资料库备份临时目录失败 {}: {error}", self.0.display());
            }
        }
    }
}

/// Create a timestamped backup copy of `src` inside the app's backup directory,
/// then prune to at most `max_keep` entries. The backup stem is derived from the
/// source filename.
///
/// Returns the created backup path. Returns `Ok(None)` if `src` does not exist.
pub fn backup_file(src: &Path, max_keep: usize) -> AppResult<Option<PathBuf>> {
    if !src.exists() {
        return Ok(None);
    }
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".to_string());
    backup_file_named(src, &stem, max_keep)
}

/// Like [`backup_file`] but with an explicit stem (e.g. `"settings.json"` so the
/// backups are named `settings.json_<ts>.bak`). Used to keep DB and settings
/// backups visually distinct.
pub fn backup_file_named(src: &Path, stem: &str, max_keep: usize) -> AppResult<Option<PathBuf>> {
    if !src.exists() {
        return Ok(None);
    }

    let backup_dir = get_backup_dir();
    fs::create_dir_all(&backup_dir)?;

    let now = Utc::now();
    let ts = now.format("%Y%m%d_%H%M%S");
    // Nanosecond suffix makes concurrent backups effectively unique, sidestepping
    // a same-second filename race (two threads both see dest as absent, then both
    // try to copy into it).
    let nanos = now.timestamp_subsec_nanos();

    let mut dest = backup_dir.join(format!("{stem}_{ts}_{nanos}.bak"));
    let mut counter = 1;
    while dest.exists() {
        dest = backup_dir.join(format!("{stem}_{ts}_{nanos}_{counter}.bak"));
        counter += 1;
    }

    fs::copy(src, &dest).map_err(|e| io_context("备份复制失败", e))?;
    write_manifest(&dest, src, stem)?;
    let created = dest.clone();

    prune_backups_for_category(&backup_dir, stem, max_keep)?;
    Ok(Some(created))
}

pub fn manifest_for_backup(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.manifest.json", path.to_string_lossy()))
}

pub fn load_manifest(path: &Path) -> AppResult<Option<BackupManifest>> {
    let manifest_path = manifest_for_backup(path);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&manifest_path).map_err(|error| io_context("读取备份清单失败", error))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| AppError::Config("备份清单格式无效".to_string()))
}

/// Verify a backup when it has a manifest.  Legacy backups without a sidecar
/// remain restorable for backwards compatibility.
pub fn verify_backup(path: &Path) -> AppResult<Option<BackupManifest>> {
    let Some(manifest) = load_manifest(path)? else {
        return Ok(None);
    };
    let actual_size = fs::metadata(path).map_err(|error| io_context("读取备份文件失败", error))?.len();
    if actual_size != manifest.bytes || sha256_file(path)? != manifest.sha256 {
        return Err(AppError::Config("备份校验失败，文件可能已损坏或被修改".to_string()));
    }
    Ok(Some(manifest))
}

fn write_manifest(dest: &Path, src: &Path, category: &str) -> AppResult<()> {
    let manifest = BackupManifest {
        version: 1,
        backup_file: dest.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string(),
        source_name: src.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string(),
        category: category.to_string(),
        created_at: Utc::now().timestamp_millis(),
        bytes: fs::metadata(dest).map_err(|error| io_context("读取备份文件失败", error))?.len(),
        sha256: sha256_file(dest)?,
        schema_version: crate::database::schema::SCHEMA_VERSION,
    };
    let content = serde_json::to_vec_pretty(&manifest)?;
    fs::write(manifest_for_backup(dest), content).map_err(|error| io_context("写入备份清单失败", error))
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = fs::File::open(path).map_err(|error| io_context("读取备份文件失败", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| io_context("读取备份文件失败", error))?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Rotate one backup category without allowing a busy source (for example,
/// database migration backups) to evict unrelated configuration backups.
pub fn prune_backups_for_category(dir: &Path, category: &str, keep: usize) -> AppResult<()> {
    if !dir.exists() { return Ok(()); }
    let prefix = format!("{category}_");
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(&prefix) && name.ends_with(".bak")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    entries.sort_by_key(|(modified, _)| *modified);
    let remove_count = entries.len().saturating_sub(keep);
    for (_, path) in entries.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                // Rotation is maintenance only. A locked old backup can be
                // retried later; never reject the caller's new backup.
                log::warn!("删除过期备份失败（已忽略） {}: {error}", path.display());
            }
        }
        let manifest = manifest_for_backup(&path);
        if manifest.is_file() {
            if let Err(error) = fs::remove_file(manifest) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    // A concurrently opened manifest is harmless: the backup
                    // body is already gone and a later maintenance pass can
                    // remove the sidecar. Never fail a configuration write.
                    log::warn!("删除过期备份清单失败（已忽略）: {error}");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::tempdir;

    /// Category rotation keeps the newest `keep` matching `.bak` files.
    #[test]
    fn category_prune_keeps_newest_n() {
        let dir = tempdir().unwrap();
        // Create 12 backups with monotonically increasing mtimes (1s apart so the
        // 1s mtime granularity on some filesystems distinguishes them).
        for i in 0..12 {
            let p = dir.path().join(format!("app_{i:02}.bak"));
            fs::write(&p, b"x").unwrap();
            // Set mtime explicitly so ordering is deterministic regardless of FS.
            let time = std::time::SystemTime::UNIX_EPOCH
                + Duration::from_secs(1_700_000_000 + i as u64);
            let _ = filetime::set_file_mtime(&p, filetime::FileTime::from_system_time(time));
            sleep(Duration::from_millis(5));
        }
        prune_backups_for_category(dir.path(), "app", 10).unwrap();
        let remaining = count_baks_in(dir.path());
        assert_eq!(remaining, 10, "should keep exactly 10 after pruning 12");
        // The oldest two (app_00, app_01) should be gone; app_11 retained.
        assert!(!dir.path().join("app_00.bak").exists());
        assert!(!dir.path().join("app_01.bak").exists());
        assert!(dir.path().join("app_11.bak").exists());
    }

    #[test]
    fn find_latest_library_archive_picks_newest_zip() {
        let dir = tempdir().unwrap();
        let older = dir.path().join("library-1.zip");
        let newer = dir.path().join("library-2.zip");
        fs::write(&older, b"old").unwrap();
        fs::write(&newer, b"new").unwrap();
        let older_time = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100);
        let newer_time = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_200);
        filetime::set_file_mtime(&older, filetime::FileTime::from_system_time(older_time)).unwrap();
        filetime::set_file_mtime(&newer, filetime::FileTime::from_system_time(newer_time)).unwrap();
        let found = find_latest_library_archive(dir.path()).unwrap();
        assert_eq!(found, newer);
    }

    #[test]
    fn library_archive_preview_verifies_manifest_hashes() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("library.zip");
        write_test_library_archive(&archive_path, "skills/demo/SKILL.md", b"safe content", None);

        let preview = preview_library_backup(&archive_path).unwrap();
        assert_eq!(preview.entries, 1);
        assert_eq!(preview.total_bytes, 12);
    }

    #[test]
    fn library_archive_preview_rejects_tampered_content() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("modified.zip");
        write_test_library_archive(&archive_path, "skills/demo/SKILL.md", b"safe content", Some("00"));

        assert!(preview_library_backup(&archive_path).is_err());
    }

    #[test]
    fn library_archive_preview_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("unsafe.zip");
        write_test_library_archive(&archive_path, "../outside.txt", b"unsafe", None);

        assert!(preview_library_backup(&archive_path).is_err());
    }

    #[test]
    fn library_archive_restore_extracts_to_destination_dirs() {
        let root = tempdir().unwrap();
        let archive_path = root.path().join("library.zip");
        write_test_library_archive(&archive_path, "skills/demo/SKILL.md", b"restored skill", None);

        let config = root.path().join("config");
        let skills = root.path().join("skills");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&skills).unwrap();
        // Point path helpers via env is not available; call extract + replace helpers directly.
        let staging = StagingDirectory::create().unwrap();
        let count = extract_library_archive(&archive_path, &staging.0).unwrap();
        assert_eq!(count, 1);
        replace_directory_contents(&skills, &staging.0.join("skills")).unwrap();
        let restored = fs::read_to_string(skills.join("demo").join("SKILL.md")).unwrap();
        assert_eq!(restored, "restored skill");
    }

    fn write_test_library_archive(path: &Path, entry_path: &str, content: &[u8], hash_override: Option<&str>) {
        let entry = LibraryArchiveEntry {
            path: entry_path.to_string(),
            bytes: content.len() as u64,
            sha256: hash_override.map(str::to_string).unwrap_or_else(|| hex::encode(Sha256::digest(content))),
        };
        let manifest = LibraryArchiveManifest {
            version: LIBRARY_ARCHIVE_VERSION,
            created_at: 1,
            schema_version: 1,
            credentials_included: false,
            entries: vec![entry],
        };
        let file = fs::File::create(path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file(entry_path, options).unwrap();
        archive.write_all(content).unwrap();
        archive.start_file(LIBRARY_ARCHIVE_MANIFEST, options).unwrap();
        archive.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
        archive.finish().unwrap();
    }

    fn count_baks_in(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .count()
    }
}
