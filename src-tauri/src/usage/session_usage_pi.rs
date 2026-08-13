//! Sync Pi `~/.pi/agent/sessions` JSONL assistant usage into `proxy_request_logs`.
//!
//! Pi talks to upstreams directly (like OpenCode), so local-proxy logs stay empty.
//! Session files already record per-turn `message.usage` for Anthropic and OpenAI.

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
    reset_pi_session_usage, should_skip_pi_session_insert, update_session_sync_state,
    DATA_SOURCE_PI_SESSION, PI_SESSION_PROVIDER_ID,
};
use crate::database::Database;
use crate::error::AppResult;

const REQUEST_ID_PREFIX: &str = "pi_session";

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
pub struct PiSessionSyncResult {
    pub scanned_files: i64,
    pub inserted_rows: i64,
    pub skipped_rows: i64,
    pub message: String,
}

pub fn sync_pi_session_usage_db(db: &Database) -> AppResult<PiSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    for path in &files {
        let (file_inserted, file_skipped) = db.with_conn(|conn| sync_one_file(conn, path))?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(PiSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} Pi session files; inserted {}; skipped {}",
            files.len(),
            inserted,
            skipped
        ),
    })
}

pub fn try_sync_pi_session_usage_db(db: &Database) -> AppResult<PiSessionSyncResult> {
    let Some(_guard) = try_acquire_sync_lock() else {
        return Ok(PiSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "Pi session sync already in progress".to_string(),
        });
    };
    sync_pi_session_usage_db(db)
}

pub fn sync_pi_session_usage_db_blocking(db: &Database) -> AppResult<PiSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Pi 会话用量同步仍在进行，请稍后重试".into(),
        ));
    };
    sync_pi_session_usage_db(db)
}

pub fn rebuild_pi_session_usage_db(db: &Database) -> AppResult<PiSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Pi 会话用量同步繁忙，请稍后重试重建".into(),
        ));
    };
    let deleted = db.with_conn(reset_pi_session_usage)?;
    let mut result = sync_pi_session_usage_db(db)?;
    result.message = format!(
        "Rebuilt Pi session usage (removed {deleted} old rows). {}",
        result.message
    );
    Ok(result)
}

fn collect_session_files() -> Vec<PathBuf> {
    let root = crate::coding::pi::session::get_pi_sessions_dir();
    let mut files = Vec::new();
    if root.is_dir() {
        collect_session_files_in(&root, &mut files);
    }
    files
}

fn collect_session_files_in(directory: &Path, files: &mut Vec<PathBuf>) {
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
            collect_session_files_in(&path, files);
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.ends_with(".jsonl") || name.ends_with(".json") {
            files.push(path);
        }
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

    let events = match read_session_events(path) {
        Some(events) => events,
        None => {
            update_session_sync_state(conn, &path_key, modified, 0)?;
            return Ok((0, 0));
        }
    };

    let fallback_session_id = session_id_from_filename(path);
    let mut session_id = fallback_session_id.clone();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    let mut seen_message_ids = std::collections::HashSet::<String>::new();
    let mut line_offset = 0_i64;

    for value in events {
        line_offset += 1;
        if let Some(id) = value
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| *kind == "session")
            .and_then(|_| value.get("id").and_then(Value::as_str))
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            session_id = id.to_string();
        }

        let Some(event) = parse_assistant_usage(&value) else {
            continue;
        };
        if !seen_message_ids.insert(event.message_id.clone()) {
            skipped += 1;
            continue;
        }
        if should_skip_pi_session_insert(
            conn,
            event.created_at,
            Some(&event.model),
            event.input_tokens,
            event.cache_read_tokens,
            event.output_tokens,
        )? {
            skipped += 1;
            continue;
        }

        let request_id = format!("{REQUEST_ID_PREFIX}:{session_id}:{}", event.message_id);
        insert_proxy_log_with_source(
            conn,
            Some(&request_id),
            event.created_at,
            Some(PI_SESSION_PROVIDER_ID),
            Some("Pi local sessions"),
            Some(&event.model),
            Some(200),
            event.input_tokens,
            event.cache_read_tokens,
            event.cache_write_tokens,
            event.output_tokens,
            true,
            0,
            Some("pi"),
            Some(&event.protocol),
            Some("assistant"),
            true,
            None,
            None,
            DATA_SOURCE_PI_SESSION,
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

struct PiUsageEvent {
    message_id: String,
    model: String,
    protocol: String,
    created_at: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

fn read_session_events(path: &Path) -> Option<Vec<Value>> {
    let ext = path.extension().and_then(|value| value.to_str())?;
    if ext.eq_ignore_ascii_case("jsonl") {
        let file = File::open(path).ok()?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                events.push(value);
            }
        }
        return Some(events);
    }
    if ext.eq_ignore_ascii_case("json") {
        let content = std::fs::read_to_string(path).ok()?;
        let value: Value = serde_json::from_str(&content).ok()?;
        return Some(flatten_json_events(value));
    }
    None
}

fn flatten_json_events(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        Value::Object(map) => {
            if let Some(Value::Array(items)) = map.get("messages").cloned() {
                return items;
            }
            if let Some(Value::Array(items)) = map.get("entries").cloned() {
                return items;
            }
            vec![Value::Object(map)]
        }
        other => vec![other],
    }
}

fn parse_assistant_usage(value: &Value) -> Option<PiUsageEvent> {
    if value.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let message = value.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let usage = message.get("usage")?;
    let input = token_number(usage, &["input", "input_tokens", "inputTokens"]).unwrap_or(0);
    let output = token_number(usage, &["output", "output_tokens", "outputTokens"]).unwrap_or(0);
    let reasoning =
        token_number(usage, &["reasoning", "reasoning_tokens", "reasoningTokens"]).unwrap_or(0);
    let cache_read = token_number(
        usage,
        &[
            "cacheRead",
            "cache_read",
            "cache_read_input_tokens",
            "cacheReadInputTokens",
        ],
    )
    .unwrap_or(0);
    let cache_write = token_number(
        usage,
        &[
            "cacheWrite",
            "cache_write",
            "cache_creation_input_tokens",
            "cacheWriteInputTokens",
        ],
    )
    .unwrap_or(0)
        + token_number(usage, &["cacheWrite1h"]).unwrap_or(0);
    let billed_output = output + reasoning;
    if input == 0 && billed_output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }

    let message_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)?;
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            value
                .get("modelId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("unknown")
        .to_string();
    let protocol = message
        .get("api")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("pi_session")
        .to_string();
    let created_at = value
        .get("timestamp")
        .and_then(parse_event_timestamp)
        .or_else(|| message.get("timestamp").and_then(parse_event_timestamp))
        .unwrap_or_else(|| Utc::now().timestamp_millis());

    Some(PiUsageEvent {
        message_id,
        model,
        protocol,
        created_at,
        input_tokens: input,
        output_tokens: billed_output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
    })
}

fn session_id_from_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    stem.rsplit_once('_')
        .map(|(_, id)| id.to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| stem.to_string())
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
    use std::io::Write;

    #[test]
    fn syncs_pi_assistant_usage_and_skips_errors_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("2026-08-13T01-49-44-924Z_sess-1.jsonl");
        let mut file = File::create(&session).unwrap();
        writeln!(
            file,
            r#"{{"type":"session","version":3,"id":"sess-1","timestamp":"2026-08-13T01:49:44.924Z"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"u1","timestamp":"2026-08-13T01:49:52.667Z","message":{{"role":"user","content":[{{"type":"text","text":"hello"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"a1","timestamp":"2026-08-13T01:49:55.507Z","message":{{"role":"assistant","api":"openai-responses","model":"gpt-5.6-terra","usage":{{"input":7275,"output":14,"cacheRead":0,"cacheWrite":0,"reasoning":8,"totalTokens":7297}}}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"a1","timestamp":"2026-08-13T01:49:55.600Z","message":{{"role":"assistant","api":"openai-responses","model":"gpt-5.6-terra","usage":{{"input":7275,"output":14,"cacheRead":0,"cacheWrite":0,"reasoning":8}}}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"err1","timestamp":"2026-08-13T01:50:00.000Z","message":{{"role":"assistant","api":"openai-responses","model":"gpt-5.6-terra","usage":{{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0}},"stopReason":"error"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"a2","timestamp":"2026-08-13T01:50:33.287Z","message":{{"role":"assistant","api":"anthropic-messages","model":"gemini-3.6-flash-high","usage":{{"input":0,"output":15,"cacheRead":0,"cacheWrite":0,"cacheWrite1h":2}}}}}}"#
        )
        .unwrap();
        drop(file);

        let db = Database::memory().unwrap();
        let inserted = db
            .with_conn(|conn| sync_one_file(conn, &session))
            .unwrap();
        assert_eq!(inserted.0, 2);
        assert_eq!(inserted.1, 1);

        let rows: Vec<(String, i64, i64, i64, String)> = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT model, input_tokens, output_tokens, cache_creation_input_tokens, id
                     FROM proxy_request_logs WHERE data_source = ?1 ORDER BY created_at",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![DATA_SOURCE_PI_SESSION], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "gpt-5.6-terra");
        assert_eq!(rows[0].1, 7275);
        assert_eq!(rows[0].2, 22); // output + reasoning
        assert_eq!(rows[0].4, "pi_session:sess-1:a1");
        assert_eq!(rows[1].0, "gemini-3.6-flash-high");
        assert_eq!(rows[1].1, 0);
        assert_eq!(rows[1].2, 15);
        assert_eq!(rows[1].3, 2);

        let second = db
            .with_conn(|conn| sync_one_file(conn, &session))
            .unwrap();
        assert_eq!(second, (0, 0));
    }
}
