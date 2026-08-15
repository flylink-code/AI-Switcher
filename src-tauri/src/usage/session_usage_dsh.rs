//! Sync DeepSeek Harness `~/.dsh/sessions/**/*.jsonl.zstd` usage into `proxy_request_logs`.
//!
//! Dsh writes an append-only chain of independently compressed Zstandard frames.
//! We count only durable `assistant/message` events because their `data.usage` is
//! one completed model request; `assistant/chunk` has duplicate streaming usage.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::config::get_dsh_config_dir;
use crate::database::dao::proxy_logs::{
    get_session_sync_state, insert_proxy_log_with_source, normalize_sync_path,
    reset_dsh_session_usage, update_session_sync_state, DATA_SOURCE_DSH_SESSION,
    DSH_SESSION_PROVIDER_ID,
};
use crate::database::Database;
use crate::error::{AppError, AppResult};

const REQUEST_ID_PREFIX: &str = "dsh_session";
const ZSTD_MAGIC: u32 = 0xFD2F_B528;
const SYNC_STALE_MS: u64 = 120_000;

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static SYNC_STARTED_MS: AtomicU64 = AtomicU64::new(0);

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
        if started > 0 && now_unix_ms().saturating_sub(started) > SYNC_STALE_MS {
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
pub struct DshSessionSyncResult {
    pub scanned_files: i64,
    pub inserted_rows: i64,
    pub skipped_rows: i64,
    pub message: String,
}

pub fn sync_dsh_session_usage_db(db: &Database) -> AppResult<DshSessionSyncResult> {
    let files = collect_session_files();
    let mut inserted = 0;
    let mut skipped = 0;
    for path in &files {
        let (file_inserted, file_skipped) = db.with_conn(|conn| sync_one_file(conn, path))?;
        inserted += file_inserted;
        skipped += file_skipped;
    }
    Ok(DshSessionSyncResult {
        scanned_files: files.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} DeepSeek Harness session files; inserted {}; skipped {}",
            files.len(), inserted, skipped
        ),
    })
}

pub fn try_sync_dsh_session_usage_db(db: &Database) -> AppResult<DshSessionSyncResult> {
    let Some(_guard) = try_acquire_sync_lock() else {
        return Ok(DshSessionSyncResult {
            scanned_files: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "DeepSeek Harness session sync already in progress".to_string(),
        });
    };
    sync_dsh_session_usage_db(db)
}

pub fn sync_dsh_session_usage_db_blocking(db: &Database) -> AppResult<DshSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(AppError::Config("DeepSeek Harness 会话用量同步仍在进行，请稍后重试".into()));
    };
    sync_dsh_session_usage_db(db)
}

pub fn rebuild_dsh_session_usage_db(db: &Database) -> AppResult<DshSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(AppError::Config("DeepSeek Harness 会话用量同步繁忙，请稍后重试重建".into()));
    };
    let deleted = db.with_conn(reset_dsh_session_usage)?;
    let mut result = sync_dsh_session_usage_db(db)?;
    result.message = format!("Rebuilt DeepSeek Harness session usage (removed {deleted} old rows). {}", result.message);
    Ok(result)
}

fn collect_session_files() -> Vec<PathBuf> {
    let root = get_dsh_config_dir().join("sessions");
    let mut files = Vec::new();
    if root.is_dir() {
        collect_session_files_in(&root, &mut files);
    }
    files
}

fn collect_session_files_in(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() { continue }
        if kind.is_dir() {
            collect_session_files_in(&path, files);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl.zstd") {
            files.push(path);
        }
    }
}

fn sync_one_file(conn: &Connection, path: &Path) -> AppResult<(i64, i64)> {
    let modified = std::fs::metadata(path)?.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let path_key = normalize_sync_path(path);
    if get_session_sync_state(conn, &path_key)?.is_some_and(|(last_modified, _)| last_modified == modified) {
        return Ok((0, 0));
    }
    let events = match read_dsh_events(path) {
        Ok(events) => events,
        Err(error) => {
            log::warn!("DeepSeek Harness session log ignored ({}): {error}", path.display());
            update_session_sync_state(conn, &path_key, modified, 0)?;
            return Ok((0, 0));
        }
    };

    let mut session_id = path.parent().and_then(|p| p.file_name()).and_then(|v| v.to_str()).unwrap_or_default().to_string();
    let mut inserted = 0;
    let mut skipped = 0;
    let mut seen = HashSet::new();
    for event in &events {
        if event.get("type").and_then(Value::as_str) == Some("session") {
            if let Some(id) = event.get("id").and_then(Value::as_str).filter(|id| !id.trim().is_empty()) {
                session_id = id.to_string();
            }
        }
        let Some(usage) = parse_assistant_message_usage(event) else { continue };
        let key = format!("{}:{}", session_id, usage.seq);
        if !seen.insert(key.clone()) {
            skipped += 1;
            continue;
        }
        insert_proxy_log_with_source(
            conn,
            Some(&format!("{REQUEST_ID_PREFIX}:{key}")),
            usage.created_at,
            Some(DSH_SESSION_PROVIDER_ID),
            Some(&usage.provider),
            Some(&usage.model),
            Some(200),
            usage.input_tokens,
            usage.cache_read_tokens,
            usage.cache_write_tokens,
            usage.output_tokens,
            true,
            0,
            Some("dsh"),
            Some(&usage.protocol),
            Some("assistant"),
            true,
            None,
            None,
            DATA_SOURCE_DSH_SESSION,
            Some(&session_id),
        )?;
        inserted += 1;
    }
    update_session_sync_state(conn, &path_key, modified, events.len() as i64)?;
    Ok((inserted, skipped))
}

struct DshUsageEvent {
    seq: i64,
    created_at: i64,
    provider: String,
    model: String,
    protocol: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

fn parse_assistant_message_usage(event: &Value) -> Option<DshUsageEvent> {
    if event.get("type").and_then(Value::as_str) != Some("assistant/message") { return None }
    let data = event.get("data")?;
    let usage = data.get("usage")?;
    let input_tokens = token_number(usage, "inputTokens");
    let output_tokens = token_number(usage, "outputTokens");
    let cache_read_tokens = token_number(usage, "cacheReadTokens");
    let cache_write_tokens = token_number(usage, "cacheWriteTokens");
    if input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 && cache_write_tokens == 0 { return None }
    let source = data.pointer("/message/source")?;
    let provider = source.get("provider").and_then(Value::as_str)?.trim();
    let model = source.get("model").and_then(Value::as_str)?.trim();
    if provider.is_empty() || model.is_empty() { return None }
    Some(DshUsageEvent {
        seq: event.get("seq").and_then(Value::as_i64)?,
        created_at: event.get("time").and_then(Value::as_i64).unwrap_or_else(|| Utc::now().timestamp_millis()),
        provider: provider.to_string(),
        model: model.to_string(),
        protocol: source.pointer("/replayState/api").and_then(Value::as_str).unwrap_or("dsh_session").to_string(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

fn token_number(usage: &Value, key: &str) -> i64 {
    usage.get(key).and_then(Value::as_i64).unwrap_or(0).max(0)
}

pub(crate) fn read_dsh_events(path: &Path) -> Result<Vec<Value>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let frames = scan_zstd_frames(&bytes)?;
    let mut events = Vec::new();
    for (start, end) in frames {
        let content = zstd::stream::decode_all(&bytes[start..end]).map_err(|e| e.to_string())?;
        for line in String::from_utf8_lossy(&content).lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                events.push(value);
            }
        }
    }
    Ok(events)
}

/// Dsh appends independently decodable Zstandard frames. Locate boundaries first;
/// zstd's regular stream reader otherwise stops after the first frame.
fn scan_zstd_frames(bytes: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let mut frames = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let start = offset;
        if bytes.len().saturating_sub(offset) < 5 { break }
        let magic = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if magic != ZSTD_MAGIC { return Err(format!("invalid Zstandard frame magic at byte {offset}")) }
        offset += 4;
        let descriptor = bytes[offset];
        offset += 1;
        if descriptor & 0x18 != 0 { return Err(format!("reserved Zstandard header bit at byte {}", offset - 1)) }
        let content_size_flag = descriptor >> 6;
        let single_segment = descriptor & 0x20 != 0;
        let has_checksum = descriptor & 0x04 != 0;
        let dictionary_flag = descriptor & 0x03;
        let dictionary_bytes = if dictionary_flag == 3 { 4 } else { dictionary_flag as usize };
        let content_size_bytes = if content_size_flag == 0 { if single_segment { 1 } else { 0 } } else { 1usize << content_size_flag };
        let header_remaining = if single_segment { 0 } else { 1 } + dictionary_bytes + content_size_bytes;
        if bytes.len().saturating_sub(offset) < header_remaining { break }
        offset += header_remaining;
        loop {
            if bytes.len().saturating_sub(offset) < 3 { return Ok(frames) }
            let header = (bytes[offset] as usize) | ((bytes[offset + 1] as usize) << 8) | ((bytes[offset + 2] as usize) << 16);
            offset += 3;
            let last_block = header & 1 != 0;
            let block_type = (header >> 1) & 3;
            if block_type == 3 { return Err(format!("reserved Zstandard block at byte {}", offset - 3)) }
            let payload_size = if block_type == 1 { 1 } else { header >> 3 };
            if bytes.len().saturating_sub(offset) < payload_size { return Ok(frames) }
            offset += payload_size;
            if last_block { break }
        }
        if has_checksum {
            if bytes.len().saturating_sub(offset) < 4 { return Ok(frames) }
            offset += 4;
        }
        frames.push((start, offset));
    }
    Ok(frames)
}
