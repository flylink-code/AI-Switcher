//! 同步 OpenCode `opencode.db` 中 assistant 消息的 token 用量到 `proxy_request_logs`。
//!
//! 参考 cc-switch `services/session_usage_opencode.rs`，适配本项目同步原语：
//! - 增量水位：`session_log_sync`（文件级 + 会话级两级游标）
//! - 入库：`insert_proxy_log_with_source`（`data_source = "opencode_session"`，
//!   `target_app = "opencode"`），成本在查询时按定价表计算，这里只存 token 数。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::database::dao::proxy_logs::{
    get_session_sync_state, insert_proxy_log_with_source, normalize_sync_path,
    reset_opencode_session_usage, should_skip_opencode_session_insert, update_session_sync_state,
    DATA_SOURCE_OPENCODE_SESSION, OPENCODE_SESSION_PROVIDER_ID,
};
use crate::database::Database;
use crate::error::{AppError, AppResult};

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
pub struct OpenCodeSessionSyncResult {
    pub scanned_sessions: i64,
    pub inserted_rows: i64,
    pub skipped_rows: i64,
    pub message: String,
}

/// 从 opencode message.data JSON 提取的 token 数据。
struct OpenCodeMessageData {
    input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    model_id: String,
    timestamp_ms: i64,
}

fn file_modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 查询所有会话的 (id, 同步水位) —— 水位取会话与消息更新时间的大者。
fn query_sessions(conn: &Connection) -> AppResult<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT s.id,
                MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
         FROM session s
         LEFT JOIN message m ON m.session_id = s.id
         GROUP BY s.id
         ORDER BY sync_watermark",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row?);
    }
    Ok(sessions)
}

/// 查询某会话已完成的 assistant 消息；`has_incomplete` 标记是否还有进行中的消息
/// （进行中只有半截 token，且 INSERT OR IGNORE 无法回填，必须留到下一轮）。
fn query_assistant_messages(
    conn: &Connection,
    session_id: &str,
) -> AppResult<(Vec<(String, OpenCodeMessageData)>, bool)> {
    let mut stmt =
        conn.prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created")?;
    let rows = stmt.query_map([session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut messages = Vec::new();
    let mut has_incomplete = false;
    for row in rows {
        let (message_id, data_json) = row?;
        let value: Value = match serde_json::from_str(&data_json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if value.get("tokens").is_none() {
            continue;
        }
        if value.pointer("/time/completed").is_none() {
            has_incomplete = true;
            continue;
        }
        if let Some(data) = parse_message_data(&value) {
            messages.push((message_id, data));
        }
    }
    Ok((messages, has_incomplete))
}

fn parse_message_data(value: &Value) -> Option<OpenCodeMessageData> {
    let tokens = value.get("tokens")?;
    let token = |key: &str| tokens.get(key).and_then(Value::as_i64).unwrap_or(0);
    let input_tokens = token("input");
    let output_tokens = token("output");
    let reasoning_tokens = token("reasoning");
    let cache_read_tokens = tokens.pointer("/cache/read").and_then(Value::as_i64).unwrap_or(0);
    let cache_write_tokens = tokens.pointer("/cache/write").and_then(Value::as_i64).unwrap_or(0);

    if input_tokens == 0
        && output_tokens == 0
        && reasoning_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
    {
        return None;
    }

    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        model_id: value
            .get("modelID")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        timestamp_ms: value
            .pointer("/time/created")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn sync_opencode_db_inner(conn: &Connection) -> AppResult<OpenCodeSessionSyncResult> {
    let db_path = crate::config::get_opencode_db_path();
    let empty = |message: String| OpenCodeSessionSyncResult {
        scanned_sessions: 0,
        inserted_rows: 0,
        skipped_rows: 0,
        message,
    };
    if !db_path.exists() {
        return Ok(empty("OpenCode database not found".to_string()));
    }

    // opencode.db 运行在 WAL 模式：新提交先落在 -wal 文件，主库 mtime 在
    // checkpoint 前不变，必须取两者大者。
    let file_modified = file_modified_ms(&db_path)
        .max(file_modified_ms(&db_path.with_extension("db-wal")));
    let db_key = normalize_sync_path(&db_path);
    if let Some((last_modified, _)) = get_session_sync_state(conn, &db_key)? {
        if last_modified == file_modified {
            return Ok(empty("OpenCode database unchanged".to_string()));
        }
    }

    let opencode_conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| AppError::Config(format!("无法打开 OpenCode 数据库: {error}")))?;

    let sessions = query_sessions(&opencode_conn)?;
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    let mut had_error = false;

    for (session_id, watermark) in &sessions {
        let session_key = format!("{db_key}:{session_id}");
        if let Some((last_modified, _)) = get_session_sync_state(conn, &session_key)? {
            if *watermark <= last_modified {
                continue;
            }
        }

        let (messages, has_incomplete) = match query_assistant_messages(&opencode_conn, session_id) {
            Ok(result) => result,
            Err(error) => {
                log::warn!("[OPENCODE-SYNC] 会话消息查询失败 {session_id}: {error}");
                had_error = true;
                continue;
            }
        };

        let mut session_had_error = false;
        for (message_id, data) in &messages {
            // output 含 reasoning（按输出计费）。
            let output = data.output_tokens + data.reasoning_tokens;
            let created_at = if data.timestamp_ms > 0 {
                data.timestamp_ms
            } else {
                now_unix_ms() as i64
            };
            if should_skip_opencode_session_insert(
                conn,
                created_at,
                Some(&data.model_id),
                data.input_tokens,
                data.cache_read_tokens,
                output,
            )? {
                skipped += 1;
                continue;
            }
            let request_id = format!("opencode_session:{session_id}:{message_id}");
            insert_proxy_log_with_source(
                conn,
                Some(&request_id),
                created_at,
                Some(OPENCODE_SESSION_PROVIDER_ID),
                Some("OpenCode local events"),
                Some(&data.model_id),
                Some(200),
                data.input_tokens,
                data.cache_read_tokens,
                data.cache_write_tokens,
                output,
                true,
                0,
                Some("opencode"),
                Some("opencode_session"),
                Some("message"),
                true,
                None,
                None,
                DATA_SOURCE_OPENCODE_SESSION,
                Some(session_id),
            )?;
            inserted += 1;
        }

        if session_had_error || has_incomplete {
            had_error |= session_had_error;
            continue;
        }
        update_session_sync_state(conn, &session_key, *watermark, 0)?;
    }

    // 仅本轮无错误时推进文件级游标，保留下次重试入口。
    if !had_error {
        update_session_sync_state(conn, &db_key, file_modified, 0)?;
    }

    if inserted > 0 {
        log::info!(
            "[OPENCODE-SYNC] 同步完成: 导入 {inserted} 条, 跳过 {skipped} 条, 扫描 {} 个会话",
            sessions.len()
        );
    }
    Ok(OpenCodeSessionSyncResult {
        scanned_sessions: sessions.len() as i64,
        inserted_rows: inserted,
        skipped_rows: skipped,
        message: format!(
            "Scanned {} OpenCode sessions; inserted {}; skipped {}",
            sessions.len(),
            inserted,
            skipped
        ),
    })
}

pub fn sync_opencode_session_usage_db(db: &Database) -> AppResult<OpenCodeSessionSyncResult> {
    db.with_conn(sync_opencode_db_inner)
}

/// 后台 + 手动同步共用的非重叠包装。
pub fn try_sync_opencode_session_usage_db(db: &Database) -> AppResult<OpenCodeSessionSyncResult> {
    let Some(_guard) = try_acquire_sync_lock() else {
        return Ok(OpenCodeSessionSyncResult {
            scanned_sessions: 0,
            inserted_rows: 0,
            skipped_rows: 0,
            message: "OpenCode session sync already in progress".to_string(),
        });
    };
    sync_opencode_session_usage_db(db)
}

/// 手动同步：短暂等待锁，仍忙则报错。
pub fn sync_opencode_session_usage_db_blocking(db: &Database) -> AppResult<OpenCodeSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(AppError::Config("OpenCode 会话用量同步仍在进行，请稍后重试".into()));
    };
    sync_opencode_session_usage_db(db)
}

pub fn rebuild_opencode_session_usage_db(db: &Database) -> AppResult<OpenCodeSessionSyncResult> {
    let Some(_guard) = wait_acquire_sync_lock(50) else {
        return Err(AppError::Config("OpenCode 会话用量同步繁忙，请稍后重试重建".into()));
    };
    let deleted = db.with_conn(reset_opencode_session_usage)?;
    let mut result = sync_opencode_session_usage_db(db)?;
    result.message = format!(
        "Rebuilt OpenCode session usage (removed {deleted} old rows). {}",
        result.message
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::parse_message_data;
    use serde_json::json;

    #[test]
    fn parse_message_data_extracts_tokens() {
        let value = json!({
            "role": "assistant",
            "modelID": "claude-sonnet-5",
            "tokens": {
                "input": 120,
                "output": 45,
                "reasoning": 5,
                "cache": { "read": 30, "write": 10 }
            },
            "time": { "created": 1_700_000_000_000_i64, "completed": 1_700_000_001_000_i64 }
        });
        let data = parse_message_data(&value).expect("parsed");
        assert_eq!(data.input_tokens, 120);
        assert_eq!(data.output_tokens, 45);
        assert_eq!(data.reasoning_tokens, 5);
        assert_eq!(data.cache_read_tokens, 30);
        assert_eq!(data.cache_write_tokens, 10);
        assert_eq!(data.model_id, "claude-sonnet-5");
        assert_eq!(data.timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn parse_message_data_skips_zero_tokens() {
        let value = json!({
            "role": "assistant",
            "tokens": { "input": 0, "output": 0 }
        });
        assert!(parse_message_data(&value).is_none());
    }

    #[test]
    fn parse_message_data_requires_tokens_field() {
        let value = json!({ "role": "assistant" });
        assert!(parse_message_data(&value).is_none());
    }
}
