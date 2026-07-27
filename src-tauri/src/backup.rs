//! File-level backup with rotation.
//!
//! Backups are timestamped copies placed in the app's `backups/` directory. After
//! each backup, the oldest copies beyond `max_keep` are pruned (by modification
//! time). The SQLite-level backup primitive (using rusqlite's `backup` feature)
//! will be layered on top in a later phase; for P0 we copy the file directly,
//! which is sufficient when the DB is quiescent or WAL is checkpointed first.

use crate::config::paths::get_backup_dir;
use crate::error::{io_context, AppError, AppResult};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

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

    fn count_baks_in(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .count()
    }
}
