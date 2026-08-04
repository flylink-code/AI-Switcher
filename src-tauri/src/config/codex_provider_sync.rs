//! Rewrite Codex historical session `model_provider` values after a switch.
//!
//! Codex filters the sidebar by matching `config.toml` `model_provider` against
//! rollout `session_meta` and SQLite `threads.model_provider`. When AI-Switcher
//! changes the active provider id, those historical records must be rewritten
//! or sessions appear to vanish. This module is a focused port of CodexPlusPlus
//! provider sync: rollout rewrite + SQLite update + backup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::codex::managed_provider_id;
use crate::config::{atomic_write, get_backup_dir, get_codex_config_dir, get_codex_config_path};
use crate::error::{AppError, AppResult};

const SESSION_DIRS: [&str; 2] = ["sessions", "archived_sessions"];
const BACKUP_KEEP_COUNT: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSyncResult {
    pub status: String,
    pub message: String,
    pub target_provider: String,
    pub backup_dir: Option<String>,
    pub changed_session_files: usize,
    pub sqlite_rows_updated: usize,
    pub skipped_locked_files: Vec<String>,
}

#[derive(Debug, Clone)]
struct SessionChange {
    path: PathBuf,
    original_text: String,
    next_text: String,
    rewrite_needed: bool,
}

#[derive(Debug, Default)]
struct RolloutRewrite {
    next_text: String,
    rewrite_needed: bool,
    session_meta_count: usize,
}

/// Sync historical sessions to the stable AI-Switcher provider id.
pub fn sync_to_managed_provider() -> AppResult<CodexProviderSyncResult> {
    sync_sessions_to_provider(None, Some(managed_provider_id()))
}

/// Sync historical sessions to an explicit provider, or to the current
/// `config.toml` `model_provider` when omitted.
pub fn sync_sessions_to_provider(
    home: Option<&Path>,
    target_provider: Option<&str>,
) -> AppResult<CodexProviderSyncResult> {
    let home = home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_config_dir);
    if !home.exists() {
        return Ok(CodexProviderSyncResult {
            status: "skipped".into(),
            message: format!("Codex home 不存在：{}", home.display()),
            target_provider: managed_provider_id().into(),
            backup_dir: None,
            changed_session_files: 0,
            sqlite_rows_updated: 0,
            skipped_locked_files: Vec::new(),
        });
    }

    let target = resolve_target_provider(&home, target_provider)?;
    let collected = collect_session_changes(&home, &target)?;
    let rewrite_changes: Vec<_> = collected
        .changes
        .iter()
        .filter(|change| change.rewrite_needed)
        .cloned()
        .collect();
    let sqlite_paths = provider_sync_db_paths(&home);
    let pending_sqlite = count_sqlite_provider_updates(&sqlite_paths, &target)?;

    if rewrite_changes.is_empty() && pending_sqlite == 0 {
        let status = if collected.skipped_locked_files.is_empty()
            && collected.skipped_locked_databases.is_empty()
        {
            "synced"
        } else {
            "warning"
        };
        let message = if status == "warning" {
            format!(
                "历史会话看起来已对齐，但有 {} 个文件 / {} 个数据库因占用未能核对；请先退出 Codex 再点「修复历史会话」",
                collected.skipped_locked_files.len(),
                collected.skipped_locked_databases.len()
            )
        } else {
            "历史会话已与当前供应商一致，无需修改".into()
        };
        let mut skipped = collected.skipped_locked_files;
        skipped.extend(collected.skipped_locked_databases);
        return Ok(CodexProviderSyncResult {
            status: status.into(),
            message,
            target_provider: target,
            backup_dir: None,
            changed_session_files: 0,
            sqlite_rows_updated: 0,
            skipped_locked_files: skipped,
        });
    }

    let backup_dir = create_backup(&home, &target, &rewrite_changes, &sqlite_paths)?;
    let applied = apply_session_changes(&rewrite_changes)?;
    let mut skipped = applied.skipped_locked_files;
    skipped.extend(collected.skipped_locked_files);
    let sqlite_rows = match apply_sqlite_provider_updates(&sqlite_paths, &target) {
        Ok(rows) => rows,
        Err(error) => {
            let refs: Vec<&SessionChange> = applied.written.iter().collect();
            let _ = restore_session_changes(&refs);
            return Err(error);
        }
    };
    prune_backups()?;

    Ok(CodexProviderSyncResult {
        status: "synced".into(),
        message: format!(
            "已将历史会话同步到 {target}：改写 {} 个会话文件，更新 {sqlite_rows} 条 SQLite 记录",
            applied.changed
        ),
        target_provider: target,
        backup_dir: Some(backup_dir.display().to_string()),
        changed_session_files: applied.changed,
        sqlite_rows_updated: sqlite_rows,
        skipped_locked_files: skipped,
    })
}

fn resolve_target_provider(home: &Path, explicit: Option<&str>) -> AppResult<String> {
    if let Some(target) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(target.to_string());
    }
    let config_path = if home.join("config.toml").exists() {
        home.join("config.toml")
    } else {
        get_codex_config_path()
    };
    if config_path.exists() {
        let text = fs::read_to_string(&config_path)?;
        if let Some(provider) = root_toml_string_value(&text, "model_provider") {
            if !provider.is_empty() {
                return Ok(provider);
            }
        }
    }
    Ok(managed_provider_id().to_string())
}

fn root_toml_string_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        let Some((left, right)) = trimmed.split_once('=') else {
            continue;
        };
        if left.trim() != key {
            continue;
        }
        let value = right.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

#[derive(Debug, Default)]
struct CollectedSessionChanges {
    changes: Vec<SessionChange>,
    skipped_locked_files: Vec<String>,
    skipped_locked_databases: Vec<String>,
}

fn collect_session_changes(home: &Path, target_provider: &str) -> AppResult<CollectedSessionChanges> {
    let mut collected = CollectedSessionChanges::default();
    for path in rollout_files(home)? {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if is_locked_io_error(&error) => {
                collected
                    .skipped_locked_files
                    .push(path.display().to_string());
                continue;
            }
            Err(error) => {
                return Err(AppError::Io(format!(
                    "读取 Codex 会话失败 {}: {error}",
                    path.display()
                )));
            }
        };
        let rewrite = rewrite_rollout_session_meta_providers(&text, target_provider)?;
        if rewrite.session_meta_count == 0 {
            continue;
        }
        collected.changes.push(SessionChange {
            path,
            original_text: text,
            next_text: rewrite.next_text,
            rewrite_needed: rewrite.rewrite_needed,
        });
    }
    for path in provider_sync_db_paths(home) {
        if open_codex_sqlite(&path).is_err() {
            collected
                .skipped_locked_databases
                .push(path.display().to_string());
        }
    }
    Ok(collected)
}

fn rewrite_rollout_session_meta_providers(
    text: &str,
    target_provider: &str,
) -> AppResult<RolloutRewrite> {
    let mut rewrite = RolloutRewrite::default();
    for segment in text.split_inclusive('\n') {
        let (line, line_ending) = split_line_ending(segment);
        let mut next_line = line.to_string();
        if !line.trim().is_empty() {
            if let Ok(mut record) = serde_json::from_str::<Value>(line) {
                if record.get("type").and_then(Value::as_str) == Some("session_meta") {
                    let Some(payload) = record.get_mut("payload").and_then(Value::as_object_mut)
                    else {
                        rewrite.next_text.push_str(&next_line);
                        rewrite.next_text.push_str(line_ending);
                        continue;
                    };
                    rewrite.session_meta_count += 1;
                    if payload.get("model_provider").and_then(Value::as_str) != Some(target_provider)
                    {
                        payload.insert(
                            "model_provider".to_string(),
                            json!(target_provider),
                        );
                        next_line = serde_json::to_string(&record).map_err(|error| {
                            AppError::Config(format!("序列化 Codex session_meta 失败: {error}"))
                        })?;
                        rewrite.rewrite_needed = true;
                    }
                }
            }
        }
        rewrite.next_text.push_str(&next_line);
        rewrite.next_text.push_str(line_ending);
    }
    Ok(rewrite)
}

fn split_line_ending(segment: &str) -> (&str, &str) {
    if let Some(stripped) = segment.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = segment.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (segment, "")
    }
}

fn rollout_files(home: &Path) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dirname in SESSION_DIRS {
        let root = home.join(dirname);
        if root.exists() {
            collect_rollout_files(&root, &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn collect_rollout_files(root: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    let entries = fs::read_dir(root).map_err(|error| {
        AppError::Io(format!("扫描 Codex 会话目录失败 {}: {error}", root.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            AppError::Io(format!("读取 Codex 会话目录项失败: {error}"))
        })?;
        let path = entry.path();
        let is_symlink = entry
            .file_type()
            .map(|file_type| file_type.is_symlink())
            .unwrap_or(true);
        if is_symlink {
            continue;
        }
        if path.is_dir() {
            collect_rollout_files(&path, files)?;
            continue;
        }
        if is_rollout_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name.ends_with(".jsonl") && !name.starts_with("agent-")
}

fn provider_sync_db_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let legacy = home.join("state_5.sqlite");
    if legacy.exists() {
        paths.push(legacy);
    }
    let sqlite_dir = home.join("sqlite");
    if let Ok(entries) = fs::read_dir(sqlite_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !(name.ends_with(".sqlite") || name.ends_with(".db")) {
                continue;
            }
            if name.ends_with("-wal") || name.ends_with("-shm") {
                continue;
            }
            if sqlite_has_provider_table(&path) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn open_codex_sqlite(path: &Path) -> AppResult<Connection> {
    let db = Connection::open(path).map_err(|error| {
        AppError::Database(format!("打开 Codex SQLite 失败 {}: {error}", path.display()))
    })?;
    let _ = db.busy_timeout(std::time::Duration::from_millis(2_000));
    Ok(db)
}

fn sqlite_has_provider_table(path: &Path) -> bool {
    let Ok(db) = open_codex_sqlite(path) else {
        return false;
    };
    table_has_column(&db, "threads", "model_provider")
        || table_has_column(&db, "local_thread_catalog", "model_provider")
}

fn table_has_column(db: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = db.prepare(&format!(
        "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1 LIMIT 1"
    )) else {
        return false;
    };
    stmt.exists([column]).unwrap_or(false)
}

fn count_sqlite_provider_updates(paths: &[PathBuf], target_provider: &str) -> AppResult<usize> {
    let mut total = 0usize;
    for path in paths {
        total += count_sqlite_provider_update(path, target_provider)?;
    }
    Ok(total)
}

fn count_sqlite_provider_update(path: &Path, target_provider: &str) -> AppResult<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let db = match open_codex_sqlite(path) {
        Ok(db) => db,
        Err(_) => {
            // Locked while Codex is running — caller surfaces skipped DBs as warning.
            return Ok(0);
        }
    };
    let mut total = 0usize;
    if table_has_column(&db, "threads", "model_provider") {
        total += db
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE COALESCE(model_provider, '') <> ?1",
                [target_provider],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize;
    }
    if table_has_column(&db, "local_thread_catalog", "model_provider") {
        total += db
            .query_row(
                "SELECT COUNT(*) FROM local_thread_catalog WHERE COALESCE(model_provider, '') <> ?1",
                [target_provider],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize;
    }
    Ok(total)
}

fn apply_sqlite_provider_updates(paths: &[PathBuf], target_provider: &str) -> AppResult<usize> {
    let mut total = 0usize;
    for path in paths {
        total += apply_sqlite_provider_update(path, target_provider)?;
    }
    Ok(total)
}

fn apply_sqlite_provider_update(path: &Path, target_provider: &str) -> AppResult<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let mut db = open_codex_sqlite(path)?;
    let tx = db.transaction().map_err(|error| {
        AppError::Database(format!("开始 Codex SQLite 事务失败: {error}"))
    })?;
    let mut counts = 0usize;
    if table_has_column(&tx, "threads", "model_provider") {
        counts += tx
            .execute(
                "UPDATE threads SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
                [target_provider],
            )
            .map_err(|error| AppError::Database(format!("更新 threads.model_provider 失败: {error}")))?;
    }
    if table_has_column(&tx, "local_thread_catalog", "model_provider") {
        counts += tx
            .execute(
                "UPDATE local_thread_catalog SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
                [target_provider],
            )
            .map_err(|error| {
                AppError::Database(format!("更新 local_thread_catalog.model_provider 失败: {error}"))
            })?;
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Codex SQLite 事务失败: {error}")))?;
    Ok(counts)
}

fn create_backup(
    home: &Path,
    target_provider: &str,
    changes: &[SessionChange],
    sqlite_paths: &[PathBuf],
) -> AppResult<PathBuf> {
    let backup_root = get_backup_dir().join("codex-provider-sync");
    fs::create_dir_all(&backup_root)?;
    let mut backup_dir = backup_root.join(timestamp_name());
    let mut suffix = 0;
    while backup_dir.exists() {
        suffix += 1;
        backup_dir = backup_root.join(format!("{}-{suffix}", timestamp_name()));
    }
    fs::create_dir_all(&backup_dir)?;

    let config = home.join("config.toml");
    if config.exists() {
        fs::copy(&config, backup_dir.join("config.toml"))?;
    }

    let sessions_dir = backup_dir.join("sessions");
    fs::create_dir_all(&sessions_dir)?;
    for (index, change) in changes.iter().enumerate() {
        let name = change
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("rollout.jsonl");
        let dest = sessions_dir.join(format!("{index:04}-{name}"));
        atomic_write(&dest, change.original_text.as_bytes())?;
    }

    let db_dir = backup_dir.join("db");
    fs::create_dir_all(&db_dir)?;
    let mut db_files = Vec::new();
    for db_path in sqlite_paths {
        if !db_path.exists() {
            continue;
        }
        let relative = db_path
            .strip_prefix(home)
            .unwrap_or(db_path)
            .to_string_lossy()
            .replace('\\', "/");
        let dest = db_dir.join(&relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(db_path, &dest)?;
        db_files.push(relative.clone());
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", db_path.display()));
            if sidecar.exists() {
                let rel = format!("{relative}{suffix}");
                let side_dest = db_dir.join(&rel);
                if let Some(parent) = side_dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let _ = fs::copy(&sidecar, &side_dest);
                db_files.push(rel);
            }
        }
    }

    let metadata = json!({
        "version": 1,
        "namespace": "codex-provider-sync",
        "codexHome": home.to_string_lossy(),
        "targetProvider": target_provider,
        "createdAt": Utc::now().to_rfc3339(),
        "dbFiles": db_files,
        "changedSessionFiles": changes.len(),
        "managedBy": "AI-Switcher provider sync",
        "managedProviderId": managed_provider_id(),
        "acceptsLegacyManagedIds": true,
    });
    atomic_write(
        &backup_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?.as_bytes(),
    )?;
    Ok(backup_dir)
}

#[derive(Debug, Default)]
struct AppliedSessionChanges {
    changed: usize,
    skipped_locked_files: Vec<String>,
    written: Vec<SessionChange>,
}

fn apply_session_changes(changes: &[SessionChange]) -> AppResult<AppliedSessionChanges> {
    let mut applied = AppliedSessionChanges::default();
    for change in changes {
        match fs::write(&change.path, &change.next_text) {
            Ok(()) => {
                applied.changed += 1;
                applied.written.push(change.clone());
            }
            Err(error) if is_locked_io_error(&error) => {
                applied
                    .skipped_locked_files
                    .push(change.path.display().to_string());
            }
            Err(error) => {
                let refs: Vec<&SessionChange> = applied.written.iter().collect();
                let _ = restore_session_changes(&refs);
                return Err(AppError::Io(format!(
                    "写入 Codex 会话失败 {}: {error}",
                    change.path.display()
                )));
            }
        }
    }
    Ok(applied)
}

fn restore_session_changes(changes: &[&SessionChange]) -> AppResult<()> {
    for change in changes {
        atomic_write(&change.path, change.original_text.as_bytes())?;
    }
    Ok(())
}

fn prune_backups() -> AppResult<()> {
    let backup_root = get_backup_dir().join("codex-provider-sync");
    if !backup_root.exists() {
        return Ok(());
    }
    let mut dirs: Vec<_> = fs::read_dir(&backup_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    while dirs.len() > BACKUP_KEEP_COUNT {
        let oldest = dirs.remove(0);
        let _ = fs::remove_dir_all(oldest);
    }
    Ok(())
}

fn timestamp_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{millis}")
}

fn is_locked_io_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
    ) || {
        #[cfg(windows)]
        {
            error.raw_os_error() == Some(32) // ERROR_SHARING_VIOLATION
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn write_rollout(path: &Path, provider: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let line = json!({
            "type": "session_meta",
            "payload": {
                "id": "thread-1",
                "cwd": "C:/workspace",
                "model_provider": provider
            }
        });
        fs::write(path, format!("{line}\n")).unwrap();
    }

    fn write_rollout_multi(path: &Path, providers: &[&str]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut body = String::new();
        for (index, provider) in providers.iter().enumerate() {
            let line = json!({
                "type": "session_meta",
                "payload": {
                    "id": format!("thread-{index}"),
                    "cwd": "C:/workspace",
                    "model_provider": provider
                }
            });
            body.push_str(&format!("{line}\n"));
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn rewrites_all_session_meta_providers() {
        let text = {
            let mut body = String::new();
            for provider in ["openai", "custom", "ai_switcher_old"] {
                body.push_str(&format!(
                    "{}\n",
                    json!({
                        "type": "session_meta",
                        "payload": { "id": "t", "model_provider": provider }
                    })
                ));
            }
            body.push_str(&format!(
                "{}\n",
                json!({ "type": "event_msg", "payload": { "type": "user_message" } })
            ));
            body
        };
        let rewrite = rewrite_rollout_session_meta_providers(&text, "ai_switcher").unwrap();
        assert!(rewrite.rewrite_needed);
        assert_eq!(rewrite.session_meta_count, 3);
        assert!(!rewrite.next_text.contains("\"model_provider\":\"openai\""));
        assert!(!rewrite.next_text.contains("\"model_provider\":\"custom\""));
        assert_eq!(
            rewrite
                .next_text
                .matches("\"model_provider\":\"ai_switcher\"")
                .count(),
            3
        );
    }

    #[test]
    fn sync_updates_rollout_and_sqlite() {
        let home = tempdir().unwrap();
        let home_path = home.path();
        fs::write(
            home_path.join("config.toml"),
            "model_provider = \"ai_switcher\"\n",
        )
        .unwrap();
        let rollout = home_path.join("sessions/2026/rollout-1.jsonl");
        write_rollout_multi(&rollout, &["openai", "CodexPlusPlus"]);

        let db_path = home_path.join("state_5.sqlite");
        {
            let db = Connection::open(&db_path).unwrap();
            db.execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    model_provider TEXT,
                    archived INTEGER,
                    has_user_event INTEGER,
                    cwd TEXT
                );
                INSERT INTO threads (id, model_provider, archived, has_user_event, cwd)
                VALUES ('thread-1', 'openai', 0, 0, '');",
            )
            .unwrap();
        }

        let result = sync_sessions_to_provider(Some(home_path), Some("ai_switcher")).unwrap();
        assert_eq!(result.status, "synced");
        assert_eq!(result.changed_session_files, 1);
        assert!(result.sqlite_rows_updated >= 1);
        assert!(result.backup_dir.is_some());

        let rewritten = fs::read_to_string(&rollout).unwrap();
        assert!(rewritten.contains("\"model_provider\":\"ai_switcher\""));
        assert!(!rewritten.contains("\"model_provider\":\"openai\""));

        let db = Connection::open(&db_path).unwrap();
        let provider: String = db
            .query_row(
                "SELECT model_provider FROM threads WHERE id = ?1",
                params!["thread-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider, "ai_switcher");
    }

    #[test]
    fn sync_is_noop_when_already_current() {
        let home = tempdir().unwrap();
        let home_path = home.path();
        fs::write(
            home_path.join("config.toml"),
            "model_provider = \"ai_switcher\"\n",
        )
        .unwrap();
        write_rollout(
            &home_path.join("sessions/2026/rollout-ok.jsonl"),
            "ai_switcher",
        );
        let result = sync_sessions_to_provider(Some(home_path), Some("ai_switcher")).unwrap();
        assert_eq!(result.status, "synced");
        assert_eq!(result.changed_session_files, 0);
        assert_eq!(result.sqlite_rows_updated, 0);
        assert!(result.backup_dir.is_none());
    }
}
