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
}

const LIBRARY_ARCHIVE_VERSION: u8 = 1;
const LIBRARY_ARCHIVE_MANIFEST: &str = "manifest.json";
const MAX_LIBRARY_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_LIBRARY_ARCHIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Create a portable ZIP archive of AI-Switcher-owned data and Claude Code
/// skills.  The database is copied through SQLite's backup API before any
/// credential reference is removed, so a live WAL database is never copied as
/// raw files.
pub fn export_library_backup() -> AppResult<LibraryBackupInfo> {
    let created_at = Utc::now().timestamp_millis();
    let backup_dir = get_backup_dir();
    fs::create_dir_all(&backup_dir)?;
    let archive_path = backup_dir.join(format!("library-{created_at}.zip"));
    let staging = StagingDirectory::create()?;
    let snapshot = staging.0.join("app.db");
    create_sanitized_db_snapshot(&snapshot)?;

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

/// Validate a portable library archive without extracting or writing it.  A
/// future restore flow must call this first, then stage validated files before
/// asking the user for a separate replacement confirmation.
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
    })
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

fn create_sanitized_db_snapshot(snapshot: &Path) -> AppResult<()> {
    let source_path = get_app_db_path();
    if !source_path.is_file() {
        return Err(AppError::Config(format!("数据库文件不存在，无法归档: {}", source_path.display())));
    }
    let source = rusqlite::Connection::open(source_path)?;
    let mut destination = rusqlite::Connection::open(snapshot)?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
    backup.run_to_completion(100, std::time::Duration::from_millis(5), None)?;
    drop(backup);
    // Both legacy plaintext keys and current OS-keyring references are excluded.
    destination.execute("UPDATE providers SET api_key = ''", [])?;
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
