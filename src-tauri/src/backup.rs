//! File-level backup with rotation.
//!
//! Backups are timestamped copies placed in the app's `backups/` directory. After
//! each backup, the oldest copies beyond `max_keep` are pruned (by modification
//! time). The SQLite-level backup primitive (using rusqlite's `backup` feature)
//! will be layered on top in a later phase; for P0 we copy the file directly,
//! which is sufficient when the DB is quiescent or WAL is checkpointed first.

use crate::config::paths::get_backup_dir;
use crate::error::{io_context, AppResult};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

/// Default number of backups to retain.
pub const DEFAULT_BACKUP_KEEP: usize = 10;

/// Create a timestamped backup copy of `src` inside the app's backup directory,
/// then prune to at most `max_keep` entries.
///
/// Returns the created backup path. Returns `Ok(None)` if `src` does not exist.
pub fn backup_file(src: &Path, max_keep: usize) -> AppResult<Option<PathBuf>> {
    if !src.exists() {
        return Ok(None);
    }

    let backup_dir = get_backup_dir();
    fs::create_dir_all(&backup_dir)?;

    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".to_string());
    let ts = Utc::now().format("%Y%m%d_%H%M%S");

    // Disambiguate within the same second.
    let mut dest = backup_dir.join(format!("{stem}_{ts}.bak"));
    let mut counter = 1;
    while dest.exists() {
        dest = backup_dir.join(format!("{stem}_{ts}_{counter}.bak"));
        counter += 1;
    }

    fs::copy(src, &dest).map_err(|e| io_context("备份复制失败", e))?;
    let created = dest.clone();

    prune_backups(&backup_dir, max_keep)?;
    Ok(Some(created))
}

/// Remove the oldest files in `dir` (by mtime) until at most `keep` remain.
/// Only files whose name ends with `.bak` are considered.
pub fn prune_backups(dir: &Path, keep: usize) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
        .filter_map(|e| {
            let path = e.path();
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((path, mtime))
        })
        .collect();

    if entries.len() <= keep {
        return Ok(());
    }

    // Newest first.
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in entries.into_iter().skip(keep) {
        if let Err(e) = fs::remove_file(&path) {
            log::warn!("删除过期备份失败 {}: {e}", path.display());
        }
    }
    Ok(())
}

/// Count current `.bak` files in the backup directory (handy for diagnostics).
#[allow(dead_code)]
pub fn count_backups() -> AppResult<usize> {
    let dir = get_backup_dir();
    if !dir.exists() {
        return Ok(0);
    }
    Ok(fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::tempdir;

    /// `prune_backups` keeps the newest `keep` `.bak` files and removes the rest.
    #[test]
    fn prune_keeps_newest_n() {
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
        prune_backups(dir.path(), 10).unwrap();
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
