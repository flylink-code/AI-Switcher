//! Timed ZIP snapshots and live session-file mirroring.
//!
//! Schedule and mirror are independent toggles. Both write under the configured
//! session backup directory. OpenCode / Cline are not file-archived.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::session_manager::{
    archive_provider_slug, backup_all_sessions_auto, collect_all_session_paths_for_provider,
    file_mtime_secs, get_configured_session_backup_dir, import_target, is_within_active_window,
    prune_auto_session_backups, relative_to_root, simplified_path, validated_session,
    SessionBatchRestoreResult, SessionProvider,
};

pub const SCHEDULE_ENABLED_KEY: &str = "session_backup_schedule_enabled";
pub const INTERVAL_MINUTES_KEY: &str = "session_backup_interval_minutes";
pub const KEEP_AUTO_KEY: &str = "session_backup_keep_auto";
pub const MIRROR_ENABLED_KEY: &str = "session_backup_mirror_enabled";
pub const ACTIVE_DAYS_KEY: &str = "session_backup_active_days";

const MIRROR_DIR_NAME: &str = "mirror";
const POLL_SECS: u64 = 5;
const STARTUP_DELAY_SECS: u64 = 30;
const DEBOUNCE_SECS: u64 = 8;
const MAX_MIRROR_FILE_BYTES: u64 = 80 * 1024 * 1024;
const DEFAULT_INTERVAL_MINUTES: u32 = 60;
const DEFAULT_KEEP_AUTO: u32 = 8;
const DEFAULT_ACTIVE_DAYS: u32 = 30;

const FILE_ARCHIVE_PROVIDERS: [SessionProvider; 4] = [
    SessionProvider::ClaudeCode,
    SessionProvider::Codex,
    SessionProvider::Pi,
    SessionProvider::Dsh,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAutoBackupSettings {
    pub schedule_enabled: bool,
    pub interval_minutes: u32,
    pub keep_auto: u32,
    pub mirror_enabled: bool,
    pub active_days: u32,
}

impl Default for SessionAutoBackupSettings {
    fn default() -> Self {
        Self {
            schedule_enabled: false,
            interval_minutes: DEFAULT_INTERVAL_MINUTES,
            keep_auto: DEFAULT_KEEP_AUTO,
            mirror_enabled: false,
            active_days: DEFAULT_ACTIVE_DAYS,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    mtime_secs: i64,
    size: u64,
}

struct PendingCopy {
    stamp: FileStamp,
    first_seen: Instant,
}

fn backup_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn load_auto_backup_settings(conn: &rusqlite::Connection) -> AppResult<SessionAutoBackupSettings> {
    let mut settings = SessionAutoBackupSettings::default();
    settings.schedule_enabled = get_setting(conn, SCHEDULE_ENABLED_KEY)?.as_deref() == Some("true");
    settings.mirror_enabled = get_setting(conn, MIRROR_ENABLED_KEY)?.as_deref() == Some("true");
    if let Some(raw) = get_setting(conn, INTERVAL_MINUTES_KEY)? {
        if let Ok(value) = raw.parse::<u32>() {
            settings.interval_minutes = normalize_interval_minutes(value);
        }
    }
    if let Some(raw) = get_setting(conn, KEEP_AUTO_KEY)? {
        if let Ok(value) = raw.parse::<u32>() {
            settings.keep_auto = value.clamp(1, 32);
        }
    }
    if let Some(raw) = get_setting(conn, ACTIVE_DAYS_KEY)? {
        if let Ok(value) = raw.parse::<u32>() {
            settings.active_days = normalize_active_days(value);
        }
    }
    Ok(settings)
}

pub fn save_auto_backup_settings(
    conn: &rusqlite::Connection,
    settings: &SessionAutoBackupSettings,
) -> AppResult<SessionAutoBackupSettings> {
    let normalized = SessionAutoBackupSettings {
        schedule_enabled: settings.schedule_enabled,
        interval_minutes: normalize_interval_minutes(settings.interval_minutes),
        keep_auto: settings.keep_auto.clamp(1, 32),
        mirror_enabled: settings.mirror_enabled,
        active_days: normalize_active_days(settings.active_days),
    };
    set_setting(
        conn,
        SCHEDULE_ENABLED_KEY,
        if normalized.schedule_enabled { "true" } else { "false" },
    )?;
    set_setting(
        conn,
        MIRROR_ENABLED_KEY,
        if normalized.mirror_enabled { "true" } else { "false" },
    )?;
    set_setting(conn, INTERVAL_MINUTES_KEY, &normalized.interval_minutes.to_string())?;
    set_setting(conn, KEEP_AUTO_KEY, &normalized.keep_auto.to_string())?;
    set_setting(conn, ACTIVE_DAYS_KEY, &normalized.active_days.to_string())?;
    Ok(normalized)
}

fn normalize_interval_minutes(value: u32) -> u32 {
    match value {
        15 | 60 | 360 | 1440 => value,
        _ => DEFAULT_INTERVAL_MINUTES,
    }
}

fn normalize_active_days(value: u32) -> u32 {
    match value {
        0 | 7 | 30 | 90 => value,
        _ => DEFAULT_ACTIVE_DAYS,
    }
}

pub fn mirror_root(backup_dir: &Path) -> PathBuf {
    backup_dir.join(MIRROR_DIR_NAME)
}

pub fn mirror_provider_dir(backup_dir: &Path, provider: SessionProvider) -> PathBuf {
    mirror_root(backup_dir).join(archive_provider_slug(provider))
}

pub fn path_is_under(child: &Path, parent: &Path) -> bool {
    let child = child
        .canonicalize()
        .map(|path| simplified_path(&path))
        .unwrap_or_else(|_| simplified_path(child));
    let parent = parent
        .canonicalize()
        .map(|path| simplified_path(&path))
        .unwrap_or_else(|_| simplified_path(parent));
    child == parent || child.starts_with(&parent) || relative_to_root(&child, &parent).is_ok()
}

/// Blocking tick used by the background loop and tests.
fn run_backup_tick(
    backup_dir: &Path,
    settings: &SessionAutoBackupSettings,
    run_zip: bool,
    copied: &mut HashMap<PathBuf, FileStamp>,
    pending: &mut HashMap<PathBuf, PendingCopy>,
) {
    let _guard = backup_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if settings.mirror_enabled {
        if let Err(error) = sync_session_mirrors(backup_dir, settings, copied, pending) {
            log::warn!("会话镜像同步失败: {error}");
        }
    }
    if run_zip && settings.schedule_enabled {
        let dir_str = backup_dir.to_string_lossy().into_owned();
        for provider in FILE_ARCHIVE_PROVIDERS {
            match backup_all_sessions_auto(provider, Some(&dir_str), settings.active_days) {
                Ok(Some(info)) => {
                    log::info!(
                        "自动会话 ZIP: {:?} sessions={} path={}",
                        provider,
                        info.session_count,
                        info.archive_path
                    );
                    if let Err(error) =
                        prune_auto_session_backups(backup_dir, provider, settings.keep_auto as usize)
                    {
                        log::warn!("自动会话 ZIP 裁剪失败 {:?}: {error}", provider);
                    }
                }
                Ok(None) => {}
                Err(error) => log::warn!("自动会话 ZIP 失败 {:?}: {error}", provider),
            }
        }
    }
}

fn sync_session_mirrors(
    backup_dir: &Path,
    settings: &SessionAutoBackupSettings,
    copied: &mut HashMap<PathBuf, FileStamp>,
    pending: &mut HashMap<PathBuf, PendingCopy>,
) -> AppResult<()> {
    let now = Instant::now();
    for provider in FILE_ARCHIVE_PROVIDERS {
        let paths = match collect_all_session_paths_for_provider(provider) {
            Ok(paths) => paths,
            Err(error) => {
                log::debug!("会话镜像跳过 {provider:?}: {error}");
                continue;
            }
        };
        let dest_root = mirror_provider_dir(backup_dir, provider);
        let mut live_relatives: HashMap<PathBuf, PathBuf> = HashMap::new();
        for source_path in &paths {
            let Ok((source, relative)) = validated_session(provider, source_path) else {
                continue;
            };
            if path_is_under(&source, backup_dir) {
                continue;
            }
            live_relatives.insert(relative.clone(), source.clone());
            let Some(stamp) = file_stamp(&source) else {
                continue;
            };
            if stamp.size > MAX_MIRROR_FILE_BYTES {
                log::debug!(
                    "会话镜像跳过超大文件 {} ({} bytes)",
                    source.display(),
                    stamp.size
                );
                continue;
            }
            if !is_within_active_window(stamp.mtime_secs, settings.active_days) {
                continue;
            }
            if copied.get(&source) == Some(&stamp) {
                pending.remove(&source);
                continue;
            }
            let due = match pending.get(&source) {
                Some(pending_copy) if pending_copy.stamp == stamp => {
                    now.duration_since(pending_copy.first_seen) >= Duration::from_secs(DEBOUNCE_SECS)
                }
                _ => {
                    pending.insert(
                        source.clone(),
                        PendingCopy {
                            stamp,
                            first_seen: now,
                        },
                    );
                    false
                }
            };
            if !due {
                continue;
            }
            let dest = dest_root.join(&relative);
            if let Err(error) = copy_session_file(&source, &dest) {
                log::warn!("会话镜像写入失败 {}: {error}", dest.display());
                continue;
            }
            copied.insert(source.clone(), stamp);
            pending.remove(&source);
        }
        prune_mirror_provider(backup_dir, provider, settings.active_days, &live_relatives)?;
    }
    Ok(())
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        mtime_secs: file_mtime_secs(path)?,
        size: meta.len(),
    })
}

fn copy_session_file(source: &Path, dest: &Path) -> AppResult<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = fs::read(source)?;
    crate::config::atomic_write(dest, &bytes)
}

fn prune_mirror_provider(
    backup_dir: &Path,
    provider: SessionProvider,
    active_days: u32,
    live_relatives: &HashMap<PathBuf, PathBuf>,
) -> AppResult<()> {
    let dest_root = simplified_path(&mirror_provider_dir(backup_dir, provider));
    if !dest_root.is_dir() {
        return Ok(());
    }
    let mut mirrored = Vec::new();
    collect_files(&dest_root, &mut mirrored, 0)?;
    for dest in mirrored {
        let Ok(relative) = relative_to_root(&dest, &dest_root) else {
            continue;
        };
        let source = live_relatives.get(&relative);
        let dest_mtime = file_mtime_secs(&dest).unwrap_or(0);
        let should_remove = match source {
            Some(source_path) => {
                let mtime = file_mtime_secs(source_path).unwrap_or(dest_mtime);
                !is_within_active_window(mtime, active_days)
            }
            None => !is_within_active_window(dest_mtime, active_days),
        };
        if should_remove {
            let _ = fs::remove_file(&dest);
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u32) -> AppResult<()> {
    if depth > 8 || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            collect_files(&path, files, depth + 1)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

pub fn restore_session_mirror(
    provider: SessionProvider,
    backup_dir: &Path,
    overwrite: bool,
) -> AppResult<SessionBatchRestoreResult> {
    if matches!(provider, SessionProvider::OpenCode | SessionProvider::Cline) {
        return Err(AppError::Config("此 Agent 不支持从文件镜像恢复".to_string()));
    }
    let dest_root = simplified_path(&mirror_provider_dir(backup_dir, provider));
    if !dest_root.is_dir() {
        return Err(AppError::Config("当前备份目录下没有该 Agent 的会话镜像".to_string()));
    }
    let mut mirrored = Vec::new();
    collect_files(&dest_root, &mut mirrored, 0)?;
    let mut restored_count = 0;
    let mut skipped_count = 0;
    for source in mirrored {
        let Ok(relative) = relative_to_root(&source, &dest_root) else {
            skipped_count += 1;
            continue;
        };
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        let Ok(target) = import_target(provider, &relative_str) else {
            skipped_count += 1;
            continue;
        };
        let Ok(content) = fs::read(&source) else {
            skipped_count += 1;
            continue;
        };
        if target.exists() {
            let existing = fs::read(&target).unwrap_or_default();
            match restore_decision(&target, &source, &existing, &content, overwrite) {
                RestoreDecision::Skip => {
                    skipped_count += 1;
                    continue;
                }
                RestoreDecision::Write => {}
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::config::atomic_write(&target, &content)?;
        restored_count += 1;
    }
    if provider == SessionProvider::Codex && restored_count > 0 {
        let _ = crate::config::codex_provider_sync::sync_to_managed_provider();
    }
    Ok(SessionBatchRestoreResult {
        restored_count,
        skipped_count,
        total_count: restored_count + skipped_count,
        message: format!("已从镜像恢复 {restored_count} 个会话，跳过 {skipped_count} 个会话"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreDecision {
    Write,
    Skip,
}

fn restore_decision(
    live: &Path,
    mirror: &Path,
    live_bytes: &[u8],
    mirror_bytes: &[u8],
    overwrite: bool,
) -> RestoreDecision {
    if Sha256::digest(live_bytes) == Sha256::digest(mirror_bytes) {
        return RestoreDecision::Skip;
    }
    if overwrite {
        return RestoreDecision::Write;
    }
    if live_is_newer(live, mirror) {
        return RestoreDecision::Skip;
    }
    RestoreDecision::Skip
}

fn live_is_newer(live: &Path, mirror: &Path) -> bool {
    match (live.metadata().and_then(|meta| meta.modified()), mirror.metadata().and_then(|meta| meta.modified())) {
        (Ok(live_time), Ok(mirror_time)) => live_time > mirror_time,
        _ => false,
    }
}

pub fn spawn_session_auto_backup_loop(db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        let copied = Arc::new(Mutex::new(HashMap::<PathBuf, FileStamp>::new()));
        let pending = Arc::new(Mutex::new(HashMap::<PathBuf, PendingCopy>::new()));
        let mut last_zip = Instant::now();
        loop {
            let loaded = db.with_conn(|conn| {
                Ok((
                    load_auto_backup_settings(conn)?,
                    get_configured_session_backup_dir(conn)?,
                ))
            });
            match loaded {
                Ok((settings, backup_dir)) => {
                    if settings.mirror_enabled || settings.schedule_enabled {
                        let interval = Duration::from_secs(u64::from(settings.interval_minutes) * 60);
                        let due_zip = settings.schedule_enabled && last_zip.elapsed() >= interval;
                        let copied_state = Arc::clone(&copied);
                        let pending_state = Arc::clone(&pending);
                        let tick_dir = backup_dir.clone();
                        let tick_settings = settings.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let mut copied_guard = copied_state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let mut pending_guard = pending_state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            run_backup_tick(
                                &tick_dir,
                                &tick_settings,
                                due_zip,
                                &mut copied_guard,
                                &mut pending_guard,
                            );
                        })
                        .await;
                        if due_zip {
                            last_zip = Instant::now();
                        }
                    }
                }
                Err(error) => log::warn!("读取会话自动备份设置失败: {error}"),
            }
            tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn prune_auto_zips_keeps_manual_archives() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("claude-code-all-backup-1.zip"), "manual");
        for index in 1..=10 {
            write_file(
                &dir.path().join(format!("claude-code-auto-backup-{index}.zip")),
                "auto",
            );
        }
        let removed =
            prune_auto_session_backups(dir.path(), SessionProvider::ClaudeCode, 8).unwrap();
        assert_eq!(removed, 2);
        assert!(dir.path().join("claude-code-all-backup-1.zip").is_file());
        assert!(dir.path().join("claude-code-auto-backup-10.zip").is_file());
        assert!(!dir.path().join("claude-code-auto-backup-1.zip").is_file());
        assert!(!dir.path().join("claude-code-auto-backup-2.zip").is_file());
    }

    #[test]
    fn inactive_files_are_outside_window() {
        let now = chrono::Utc::now().timestamp();
        assert!(is_within_active_window(now, 30));
        assert!(!is_within_active_window(now - 40 * 86_400, 30));
        assert!(is_within_active_window(now - 40 * 86_400, 0));
    }

    #[test]
    fn backup_dir_is_not_treated_as_session_source() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("mirror").join("claude-code").join("x.jsonl");
        write_file(&nested, "session");
        assert!(path_is_under(&nested, dir.path()));
        let outside = tempdir().unwrap();
        let live = outside.path().join("live.jsonl");
        write_file(&live, "live");
        assert!(!path_is_under(&live, dir.path()));
    }

    #[test]
    fn path_is_under_ignores_windows_verbatim_prefix() {
        let parent = Path::new(r"J:\Temp\aiswitcher");
        let child = Path::new(r"\\?\J:\Temp\aiswitcher\mirror\claude-code\a.jsonl");
        assert!(path_is_under(child, parent));
        let outside = Path::new(r"\\?\C:\Users\admin\.claude\projects\a.jsonl");
        assert!(!path_is_under(outside, parent));
    }

    #[test]
    fn restore_skips_newer_live_file() {
        let backup = tempdir().unwrap();
        let live_root = tempdir().unwrap();
        let relative = "proj/chat.jsonl";
        let mirror = mirror_provider_dir(backup.path(), SessionProvider::ClaudeCode).join(relative);
        write_file(&mirror, "old-mirror");
        std::thread::sleep(Duration::from_millis(20));
        let live = live_root.path().join(relative);
        write_file(&live, "newer-live");

        let live_bytes = fs::read(&live).unwrap();
        let mirror_bytes = fs::read(&mirror).unwrap();
        assert_eq!(
            restore_decision(&live, &mirror, &live_bytes, &mirror_bytes, false),
            RestoreDecision::Skip
        );
        assert_eq!(
            restore_decision(&live, &mirror, &live_bytes, &mirror_bytes, true),
            RestoreDecision::Write
        );
        assert_eq!(
            restore_decision(&live, &mirror, b"same", b"same", true),
            RestoreDecision::Skip
        );
    }

    #[test]
    fn schedule_and_mirror_flags_are_independent() {
        let only_zip = SessionAutoBackupSettings {
            schedule_enabled: true,
            mirror_enabled: false,
            ..SessionAutoBackupSettings::default()
        };
        let only_mirror = SessionAutoBackupSettings {
            schedule_enabled: false,
            mirror_enabled: true,
            ..SessionAutoBackupSettings::default()
        };
        assert_ne!(only_zip.schedule_enabled, only_mirror.schedule_enabled);
        assert_ne!(only_zip.mirror_enabled, only_mirror.mirror_enabled);
    }
}
