//! Sync Codex session JSONL token events into `proxy_request_logs`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::database::dao::proxy_logs::{
    get_session_sync_state, insert_proxy_log_with_source, reset_codex_session_usage,
    should_skip_codex_session_insert, update_session_sync_state, CODEX_SESSION_PROVIDER_ID,
    DATA_SOURCE_CODEX_SESSION,
};
use crate::database::Database;
use crate::error::AppResult;

const REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionSyncResult {
    pub scanned_files: i64,
    pub inserted_rows: i64,
    pub skipped_rows: i64,
    pub message: String,
}

#[derive(Default, Clone, Copy)]
struct TokenTotals {
    input: i64,
    cached: i64,
    output: i64,
}

/// Collect session files, then sync each file under a short DB lock so UI queries
/// are not blocked for the whole scan.
pub fn sync_codex_session_usage_db(db: &Database) -> AppResult<CodexSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    for path in &files {
        let (file_inserted, file_skipped) = db.with_conn(|conn| sync_one_file(conn, path))?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(CodexSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} Codex session files; inserted {}; skipped {}",
            files.len(),
            inserted,
            skipped
        ),
    })
}

/// Non-overlapping wrapper used by background + manual sync.
pub fn try_sync_codex_session_usage_db(db: &Database) -> AppResult<CodexSessionSyncResult> {
    if SYNC_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(CodexSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "Codex session sync already in progress".to_string(),
        });
    }
    let result = sync_codex_session_usage_db(db);
    SYNC_RUNNING.store(false, Ordering::SeqCst);
    result
}

pub fn rebuild_codex_session_usage_db(db: &Database) -> AppResult<CodexSessionSyncResult> {
    // Wait briefly if a sync is running; otherwise take the lock.
    let mut acquired = false;
    for _ in 0..50 {
        if SYNC_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            acquired = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !acquired {
        return Ok(CodexSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "Codex session sync busy; try rebuild again shortly".to_string(),
        });
    }
    let result = (|| {
        let deleted = db.with_conn(reset_codex_session_usage)?;
        let mut result = sync_codex_session_usage_db(db)?;
        result.message = format!(
            "Rebuilt Codex session usage (removed {deleted} old rows). {}",
            result.message
        );
        Ok(result)
    })();
    SYNC_RUNNING.store(false, Ordering::SeqCst);
    result
}

/// Kept for tests that already hold a connection.
pub fn sync_codex_session_usage(conn: &Connection) -> AppResult<CodexSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    for path in &files {
        let (file_inserted, file_skipped) = sync_one_file(conn, path)?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(CodexSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} Codex session files; inserted {}; skipped {}",
            files.len(),
            inserted,
            skipped
        ),
    })
}

pub fn rebuild_codex_session_usage(conn: &Connection) -> AppResult<CodexSessionSyncResult> {
    let deleted = reset_codex_session_usage(conn)?;
    let mut result = sync_codex_session_usage(conn)?;
    result.message = format!(
        "Rebuilt Codex session usage (removed {deleted} old rows). {}",
        result.message
    );
    Ok(result)
}

fn collect_session_files() -> Vec<PathBuf> {
    let config_dir = crate::config::get_codex_config_dir();
    let roots = [
        config_dir.join("sessions"),
        config_dir.join("archived_sessions"),
    ];
    let mut files = Vec::new();
    for root in &roots {
        if root.is_dir() {
            collect_codex_jsonl_files(root, &mut files);
        }
    }
    files
}

fn collect_codex_jsonl_files(directory: &Path, files: &mut Vec<PathBuf>) {
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
            collect_codex_jsonl_files(&path, files);
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
    let path_key = path.to_string_lossy().to_string();
    // Unchanged files that were previously synced: skip without opening.
    if let Some((last_modified, _)) = get_session_sync_state(conn, &path_key)? {
        if last_modified == modified {
            return Ok((0, 0));
        }
    }

    let Ok(file) = File::open(path) else {
        return Ok((0, 0));
    };
    let mut model = "unknown".to_string();
    let mut thread_id = "unknown".to_string();
    let mut prev_total: Option<TokenTotals> = None;
    let mut event_index = 0_i64;
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    let mut line_offset = 0_i64;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        line_offset += 1;

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(id) = value
                .pointer("/payload/id")
                .or_else(|| value.pointer("/payload/thread_id"))
                .and_then(Value::as_str)
            {
                thread_id = id.to_string();
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(next) = extract_model_name(value.pointer("/payload")) {
                model = next;
            }
            continue;
        }
        if !is_token_count_event(&value) {
            continue;
        }
        event_index += 1;
        let info = value.pointer("/payload/info");
        if let Some(next) = extract_model_name(info) {
            model = next;
        }
        let Some(delta) = compute_token_delta(info, &mut prev_total) else {
            continue;
        };
        let cached = delta.cached.min(delta.input);
        let fresh_input = delta.input.saturating_sub(cached);
        if fresh_input == 0 && cached == 0 && delta.output == 0 {
            continue;
        }
        let created_at = value
            .get("timestamp")
            .and_then(parse_event_timestamp)
            .unwrap_or_else(|| Utc::now().timestamp_millis());
        if should_skip_codex_session_insert(
            conn,
            created_at,
            Some(&model),
            fresh_input,
            cached,
            delta.output,
        )? {
            skipped += 1;
            continue;
        }
        let request_id = format!("{REQUEST_ID_PREFIX}:{thread_id}:{event_index}");
        insert_proxy_log_with_source(
            conn,
            Some(&request_id),
            created_at,
            Some(CODEX_SESSION_PROVIDER_ID),
            Some("Codex local events"),
            Some(&model),
            Some(200),
            fresh_input,
            cached,
            0,
            delta.output,
            true,
            0,
            Some("codex"),
            Some("codex_session"),
            Some("token_count"),
            true,
            None,
            None,
            DATA_SOURCE_CODEX_SESSION,
            Some(&thread_id),
        )?;
        inserted += 1;
    }

    update_session_sync_state(conn, &path_key, modified, line_offset)?;
    Ok((inserted, skipped))
}

fn is_token_count_event(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("event_msg")
        && value.pointer("/payload/type").and_then(Value::as_str) == Some("token_count")
}

fn compute_token_delta(
    info: Option<&Value>,
    prev_total: &mut Option<TokenTotals>,
) -> Option<TokenTotals> {
    let info = info?;
    if let Some(total) = info.get("total_token_usage").and_then(read_token_totals) {
        let previous = prev_total.unwrap_or_default();
        let delta = TokenTotals {
            input: total.input.saturating_sub(previous.input),
            cached: total.cached.saturating_sub(previous.cached),
            output: total.output.saturating_sub(previous.output),
        };
        *prev_total = Some(total);
        return Some(delta);
    }
    info.get("last_token_usage").and_then(read_token_totals)
}

fn extract_model_name(value: Option<&Value>) -> Option<String> {
    let value = value?;
    ["model", "model_name", "modelName"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                "unknown".to_string()
            } else {
                trimmed.to_string()
            }
        })
}

fn parse_event_timestamp(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value.as_str().and_then(|text| {
            DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|time| time.timestamp_millis())
        })
    })
}

fn read_token_totals(value: &Value) -> Option<TokenTotals> {
    Some(TokenTotals {
        input: token_number(value, &["input_tokens", "inputTokens"])?,
        output: token_number(value, &["output_tokens", "outputTokens"])?,
        cached: token_number(value, &["cached_input_tokens", "cachedInputTokens"]).unwrap_or(0),
    })
}

fn token_number(value: &Value, names: &[&str]) -> Option<i64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn sync_inserts_last_token_usage_rows() {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions").join("a.jsonl");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(
            &session,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-1\"}}\n",
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":10,\"output_tokens\":20}}}}\n",
            ),
        )
        .unwrap();

        // Point CODEX home via temporary override is not available; call sync_one_file via DB path.
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let (inserted, skipped) = sync_one_file(conn, &session)?;
            assert_eq!(inserted, 1);
            assert_eq!(skipped, 0);
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session';",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sync_skips_unchanged_file_without_reread() {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions").join("b.jsonl");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(
            &session,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-2\"}}\n",
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":50,\"cached_input_tokens\":0,\"output_tokens\":5}}}}\n",
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let (first_inserted, _) = sync_one_file(conn, &session)?;
            assert_eq!(first_inserted, 1);
            // Second pass with same mtime must skip entirely.
            let (second_inserted, second_skipped) = sync_one_file(conn, &session)?;
            assert_eq!(second_inserted, 0);
            assert_eq!(second_skipped, 0);
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session';",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
    }
}
