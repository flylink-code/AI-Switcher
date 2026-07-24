//! Request-log inserts written by the local proxy.

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::AppResult;

/// Persist a proxy request summary.
pub fn insert_proxy_log(
    conn: &Connection,
    provider_id: Option<&str>,
    provider_name: Option<&str>,
    model: Option<&str>,
    status_code: Option<i64>,
    duration_ms: i64,
) -> AppResult<()> {
    let id = format!("log_{}", Uuid::new_v4().simple());
    conn.execute(
        "INSERT INTO proxy_request_logs
            (id, created_at, provider_id, provider_name, model, status_code, input_tokens, output_tokens, duration_ms)
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?);",
        params![
            id,
            Utc::now().timestamp_millis(),
            provider_id,
            provider_name,
            model,
            status_code,
            duration_ms,
        ],
    )?;
    Ok(())
}
