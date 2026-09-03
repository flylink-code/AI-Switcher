//! Sync Codex session JSONL token events into `proxy_request_logs`.

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
    reset_codex_session_usage, should_skip_codex_session_insert, update_session_sync_state,
    CODEX_SESSION_PROVIDER_ID, DATA_SOURCE_CODEX_SESSION,
};
use crate::database::Database;
use crate::error::AppResult;

const REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";

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
            // Previous sync likely panicked or hung — reclaim.
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
    let Some(_guard) = try_acquire_sync_lock() else {
        return Ok(CodexSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "Codex session sync already in progress".to_string(),
        });
    };
    sync_codex_session_usage_db(db)
}

/// Manual sync: wait briefly for the lock, then error if still busy.
pub fn sync_codex_session_usage_db_blocking(db: &Database) -> AppResult<CodexSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Codex 会话用量同步仍在进行，请稍后重试".into(),
        ));
    };
    sync_codex_session_usage_db(db)
}

pub fn rebuild_codex_session_usage_db(db: &Database) -> AppResult<CodexSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(crate::error::AppError::Config(
            "Codex 会话用量同步繁忙，请稍后重试重建".into(),
        ));
    };
    let deleted = db.with_conn(reset_codex_session_usage)?;
    let mut result = sync_codex_session_usage_db(db)?;
    result.message = format!(
        "Rebuilt Codex session usage (removed {deleted} old rows). {}",
        result.message
    );
    Ok(result)
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
    let path_key = normalize_sync_path(path);
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
    let mut service_tier: Option<String> = None;
    let mut thread_id = "unknown".to_string();
    // A reverted/resumed rollout uses `<thread_id>_<segment_id>.jsonl`. Its
    // meta id stays on the logical thread, while event indexes restart in the
    // physical segment. Use the segment for the unique request id and retain
    // the logical thread as the stored session id.
    let mut segment_id =
        rollout_segment_id_from_filename(path).unwrap_or_else(|| thread_id.clone());
    let mut prev_total: Option<TokenTotals> = None;
    let mut fork_baseline_pending = false;
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
                if segment_id == "unknown" {
                    segment_id = thread_id.clone();
                }
            }
            if session_meta_is_fork(&value) {
                // Fork/subagent rollouts replay parent token history first.
                // Seed the cumulative baseline from the first total without inserting.
                fork_baseline_pending = true;
                prev_total = None;
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            let payload = value.pointer("/payload");
            if let Some(next) = extract_model_name(payload) {
                model = next;
            }
            if let Some(tier) = extract_service_tier(payload) {
                service_tier = Some(tier);
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
        if let Some(tier) =
            extract_service_tier(info).or_else(|| extract_service_tier(value.pointer("/payload")))
        {
            service_tier = Some(tier);
        }
        let billable_model = normalize_usage_model(&model, service_tier.as_deref());
        let Some(delta) = compute_token_delta(info, &mut prev_total, &mut fork_baseline_pending)
        else {
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
            Some(&billable_model),
            fresh_input,
            cached,
            delta.output,
        )? {
            skipped += 1;
            continue;
        }
        let request_id = format!("{REQUEST_ID_PREFIX}:{segment_id}:{event_index}");
        insert_proxy_log_with_source(
            conn,
            Some(&request_id),
            created_at,
            Some(CODEX_SESSION_PROVIDER_ID),
            Some("Codex local events"),
            Some(&billable_model),
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

fn rollout_segment_id_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    uuid::Uuid::parse_str(candidate)
        .ok()
        .map(|value| value.hyphenated().to_string())
}

fn session_meta_is_fork(value: &Value) -> bool {
    value
        .pointer("/payload/forked_from_id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
        || value
            .pointer("/payload/source/subagent/thread_spawn/parent_thread_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
        || value
            .pointer("/payload/source/subagent/parent_thread_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
}

fn is_token_count_event(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("event_msg")
        && value.pointer("/payload/type").and_then(Value::as_str) == Some("token_count")
}

fn compute_token_delta(
    info: Option<&Value>,
    prev_total: &mut Option<TokenTotals>,
    fork_baseline_pending: &mut bool,
) -> Option<TokenTotals> {
    let info = info?;
    if let Some(total) = info.get("total_token_usage").and_then(read_token_totals) {
        if *fork_baseline_pending && prev_total.is_none() {
            // Establish the replayed parent cumulative baseline without billing it.
            *prev_total = Some(total);
            *fork_baseline_pending = false;
            return Some(TokenTotals::default());
        }
        let previous = prev_total.unwrap_or_default();
        let delta = TokenTotals {
            input: total.input.saturating_sub(previous.input),
            cached: total.cached.saturating_sub(previous.cached),
            output: total.output.saturating_sub(previous.output),
        };
        *prev_total = Some(total);
        *fork_baseline_pending = false;
        return Some(delta);
    }
    *fork_baseline_pending = false;
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

fn extract_service_tier(value: Option<&Value>) -> Option<String> {
    let value = value?;
    ["service_tier", "serviceTier", "speed_tier", "speedTier"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|tier| !tier.is_empty())
        .map(str::to_string)
}

/// Map Codex Fast / Priority tier onto seeded `*-fast` pricing rows when possible.
fn normalize_usage_model(model: &str, service_tier: Option<&str>) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    let is_fast = service_tier
        .map(|tier| {
            let lower = tier.to_ascii_lowercase();
            lower == "fast" || lower == "priority"
        })
        .unwrap_or(false);
    if !is_fast || trimmed.to_ascii_lowercase().ends_with("-fast") {
        return trimmed.to_string();
    }
    format!("{trimmed}-fast")
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

    #[test]
    fn fork_session_skips_replayed_parent_totals() {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions").join("fork.jsonl");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(
            &session,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"forked_from_id\":\"parent\"}}\n",
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":100,\"output_tokens\":200}}}}\n",
                "{\"timestamp\":\"2026-07-01T00:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1100,\"cached_input_tokens\":100,\"output_tokens\":250}}}}\n",
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let (inserted, _) = sync_one_file(conn, &session)?;
            assert_eq!(inserted, 1);
            let (input, output): (i64, i64) = conn.query_row(
                "SELECT input_tokens, output_tokens FROM proxy_request_logs WHERE data_source = 'codex_session';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(input, 100);
            assert_eq!(output, 50);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn normalize_usage_model_appends_fast_suffix() {
        assert_eq!(
            normalize_usage_model("claude-opus-5", Some("fast")),
            "claude-opus-5-fast"
        );
        assert_eq!(
            normalize_usage_model("claude-opus-5-fast", Some("fast")),
            "claude-opus-5-fast"
        );
        assert_eq!(normalize_usage_model("gpt-5.6-sol", None), "gpt-5.6-sol");
    }

    #[test]
    fn resumed_rollout_uses_physical_segment_for_request_ids() {
        const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
        const SEGMENT_ID: &str = "22222222-2222-4222-8222-222222222222";
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions").join(format!(
            "rollout-2026-09-03T08-00-00-{THREAD_ID}_{SEGMENT_ID}.jsonl"
        ));
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(
            &session,
            format!(
                concat!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\"}}}}\n",
                    "{{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-5.6-sol\"}}}}\n",
                    "{{\"timestamp\":\"2026-09-03T08:00:00Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":2}}}}}}}}\n"
                ),
                THREAD_ID,
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            assert_eq!(sync_one_file(conn, &session)?.0, 1);
            let (request_id, session_id): (String, String) = conn.query_row(
                "SELECT id, session_id FROM proxy_request_logs WHERE data_source = 'codex_session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(request_id, format!("{REQUEST_ID_PREFIX}:{SEGMENT_ID}:1"));
            assert_eq!(session_id, THREAD_ID);
            Ok(())
        })
        .unwrap();
    }
}
