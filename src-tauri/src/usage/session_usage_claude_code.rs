//! Sync Claude Code project JSONL assistant usage into `proxy_request_logs`.
//!
//! Works for any Anthropic-compatible upstream (official, Kimi, DeepSeek, etc.)
//! because Claude Code writes standard `message.usage` fields into session files.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::database::dao::proxy_logs::{
    get_session_sync_state, insert_proxy_log_with_source, normalize_sync_path,
    reset_claude_code_session_usage, should_skip_claude_code_session_insert,
    update_session_sync_state, CLAUDE_CODE_SESSION_PROVIDER_ID, DATA_SOURCE_CLAUDE_CODE_SESSION,
};
use crate::database::Database;
use crate::error::AppResult;

const REQUEST_ID_PREFIX: &str = "claude_code_session";

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static SYNC_STARTED_MS: AtomicU64 = AtomicU64::new(0);
const SYNC_STALE_MS: u64 = 120_000;

struct SyncLockGuard;

impl Drop for SyncLockGuard {
    fn drop(&mut self) {
        SYNC_STARTED_MS.store(0, Ordering::SeqCst);
        SYNC_RUNNING.store(false, Ordering::SeqCst);
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn try_acquire_sync_lock() -> Option<SyncLockGuard> {
    for _ in 0..2 {
        if SYNC_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            SYNC_STARTED_MS.store(now_unix_ms(), Ordering::SeqCst);
            return Some(SyncLockGuard);
        }
        let started = SYNC_STARTED_MS.load(Ordering::SeqCst);
        let now = now_unix_ms();
        if started > 0 && now.saturating_sub(started) > SYNC_STALE_MS {
            SYNC_RUNNING.store(false, Ordering::SeqCst);
            SYNC_STARTED_MS.store(0, Ordering::SeqCst);
            continue;
        }
        break;
    }
    None
}

fn wait_acquire_sync_lock(attempts: u32) -> Option<SyncLockGuard> {
    for _ in 0..attempts {
        if let Some(guard) = try_acquire_sync_lock() {
            return Some(guard);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeSessionSyncResult {
    pub scanned_files: i64,
    pub inserted_rows: i64,
    pub skipped_rows: i64,
    pub message: String,
}

/// Collect session files, then sync each file under a short DB lock.
pub fn sync_claude_code_session_usage_db(db: &Database) -> AppResult<ClaudeCodeSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    for path in &files {
        let (file_inserted, file_skipped) = db.with_conn(|conn| sync_one_file(conn, path))?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(ClaudeCodeSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} Claude Code session files; inserted {}; skipped {}",
            files.len(),
            inserted,
            skipped
        ),
    })
}

pub fn try_sync_claude_code_session_usage_db(db: &Database) -> AppResult<ClaudeCodeSessionSyncResult> {
    let Some(_guard) = try_acquire_sync_lock() else {
        return Ok(ClaudeCodeSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "Claude Code session sync already in progress".to_string(),
        });
    };
    sync_claude_code_session_usage_db(db)
}

pub fn sync_claude_code_session_usage_db_blocking(
    db: &Database,
) -> AppResult<ClaudeCodeSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Claude Code 会话用量同步仍在进行，请稍后重试".into(),
        ));
    };
    sync_claude_code_session_usage_db(db)
}

pub fn rebuild_claude_code_session_usage_db(db: &Database) -> AppResult<ClaudeCodeSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Claude Code 会话用量同步繁忙，请稍后重试重建".into(),
        ));
    };
    let deleted = db.with_conn(reset_claude_code_session_usage)?;
    let mut result = sync_claude_code_session_usage_db(db)?;
    result.message = format!(
        "Rebuilt Claude Code session usage (removed {deleted} old rows). {}",
        result.message
    );
    Ok(result)
}

pub fn sync_claude_code_session_usage(conn: &Connection) -> AppResult<ClaudeCodeSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    for path in &files {
        let (file_inserted, file_skipped) = sync_one_file(conn, path)?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(ClaudeCodeSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} Claude Code session files; inserted {}; skipped {}",
            files.len(),
            inserted,
            skipped
        ),
    })
}

fn collect_session_files() -> Vec<PathBuf> {
    let root = crate::config::get_claude_config_dir().join("projects");
    let mut files = Vec::new();
    if root.is_dir() {
        collect_jsonl_files(&root, &mut files);
    }
    files
}

fn collect_jsonl_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_jsonl_files(&path, files);
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.ends_with(".jsonl") || name.starts_with("agent-") {
            continue;
        }
        files.push(path);
    }
}

fn sync_one_file(conn: &Connection, path: &Path) -> AppResult<(i64, i64)> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|duration| duration.as_millis() as i64)
        })
        .unwrap_or(0);
    let path_key = normalize_sync_path(path);
    if let Some((last_modified, _)) = get_session_sync_state(conn, &path_key)? {
        if last_modified == modified {
            return Ok((0, 0));
        }
    }

    let Ok(file) = File::open(path) else {
        return Ok((0, 0));
    };
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    let mut line_offset = 0_i64;
    let mut seen_message_ids = std::collections::HashSet::<String>::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        line_offset += 1;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let Some(usage) = message.get("usage") else {
            continue;
        };
        let input = token_number(usage, &["input_tokens", "inputTokens"]).unwrap_or(0);
        let cache_read =
            token_number(usage, &["cache_read_input_tokens", "cacheReadInputTokens"]).unwrap_or(0);
        let cache_creation = token_number(
            usage,
            &["cache_creation_input_tokens", "cacheCreationInputTokens"],
        )
        .unwrap_or(0);
        let output = token_number(usage, &["output_tokens", "outputTokens"]).unwrap_or(0);
        if input == 0 && cache_read == 0 && cache_creation == 0 && output == 0 {
            continue;
        }

        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("uuid")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            });
        let Some(message_id) = message_id else {
            continue;
        };
        // Streaming may rewrite the same assistant message multiple times.
        if !seen_message_ids.insert(message_id.clone()) {
            skipped += 1;
            continue;
        }

        let model = message
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let created_at = value
            .get("timestamp")
            .and_then(parse_event_timestamp)
            .unwrap_or_else(|| Utc::now().timestamp_millis());

        if should_skip_claude_code_session_insert(
            conn,
            created_at,
            Some(&model),
            input,
            cache_read,
            output,
        )? {
            skipped += 1;
            continue;
        }

        let request_id = format!("{REQUEST_ID_PREFIX}:{message_id}");
        insert_proxy_log_with_source(
            conn,
            Some(&request_id),
            created_at,
            Some(CLAUDE_CODE_SESSION_PROVIDER_ID),
            Some("Claude Code local sessions"),
            Some(&model),
            Some(200),
            input,
            cache_read,
            cache_creation,
            output,
            true,
            0,
            Some("claude_code"),
            Some("anthropic"),
            Some("assistant"),
            true,
            None,
            None,
            DATA_SOURCE_CLAUDE_CODE_SESSION,
            if session_id.is_empty() {
                None
            } else {
                Some(session_id.as_str())
            },
        )?;
        inserted += 1;
    }

    update_session_sync_state(conn, &path_key, modified, line_offset)?;
    Ok((inserted, skipped))
}

fn token_number(value: &Value, names: &[&str]) -> Option<i64> {
    for name in names {
        if let Some(number) = value.get(*name).and_then(Value::as_i64) {
            return Some(number.max(0));
        }
        if let Some(number) = value.get(*name).and_then(Value::as_f64) {
            return Some(number.max(0.0) as i64);
        }
    }
    None
}

fn parse_event_timestamp(value: &Value) -> Option<i64> {
    if let Some(millis) = value.as_i64() {
        return Some(if millis < 1_000_000_000_000 {
            millis * 1000
        } else {
            millis
        });
    }
    let text = value.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::io::Write;

    #[test]
    fn syncs_anthropic_usage_from_assistant_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects").join("demo");
        std::fs::create_dir_all(&projects).unwrap();
        let session = projects.join("sess.jsonl");
        let mut file = File::create(&session).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","sessionId":"s1","timestamp":"2026-08-03T12:00:00.000Z","uuid":"u1","message":{{"id":"msg_abc","model":"k3-256k","usage":{{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":1,"output_tokens":5}}}}}}"#
        )
        .unwrap();
        // Duplicate stream rewrite of same message id — should skip.
        writeln!(
            file,
            r#"{{"type":"assistant","sessionId":"s1","timestamp":"2026-08-03T12:00:01.000Z","uuid":"u2","message":{{"id":"msg_abc","model":"k3-256k","usage":{{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":1,"output_tokens":5}}}}}}"#
        )
        .unwrap();
        drop(file);

        let db = Database::memory().unwrap();
        let inserted = db
            .with_conn(|conn| sync_one_file(conn, &session))
            .unwrap();
        assert_eq!(inserted.0, 1);
        assert_eq!(inserted.1, 1);
        let count: i64 = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = ?1",
                    rusqlite::params![DATA_SOURCE_CLAUDE_CODE_SESSION],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(count, 1);
        let model: String = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT model FROM proxy_request_logs WHERE data_source = ?1",
                    rusqlite::params![DATA_SOURCE_CLAUDE_CODE_SESSION],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(model, "k3-256k");
    }
}
