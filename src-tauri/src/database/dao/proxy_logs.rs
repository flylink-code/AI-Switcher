//! Request-log persistence and usage-statistic queries for the local proxy.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use rusqlite::{named_params, params, Connection};
use serde::Serialize;
#[cfg(test)]
use ts_rs::TS;
use uuid::Uuid;

use crate::error::AppResult;

pub const DATA_SOURCE_PROXY: &str = "proxy";
pub const DATA_SOURCE_CODEX_SESSION: &str = "codex_session";
pub const DATA_SOURCE_CLAUDE_CODE_SESSION: &str = "claude_code_session";
pub const DATA_SOURCE_OPENCODE_SESSION: &str = "opencode_session";
pub const DATA_SOURCE_PI_SESSION: &str = "pi_session";
pub const DATA_SOURCE_DSH_SESSION: &str = "dsh_session";
pub const CODEX_SESSION_PROVIDER_ID: &str = "_codex_session";
pub const CLAUDE_CODE_SESSION_PROVIDER_ID: &str = "_claude_code_session";
pub const OPENCODE_SESSION_PROVIDER_ID: &str = "_opencode_session";
pub const PI_SESSION_PROVIDER_ID: &str = "_pi_session";
pub const DSH_SESSION_PROVIDER_ID: &str = "_dsh_session";
/// Hide session rows when a matching proxy row exists within ±10 minutes.
const SESSION_PROXY_DEDUP_WINDOW_MS: i64 = 10 * 60 * 1000;

/// SQL fragment: drop session-sync rows that duplicate a nearby proxy row.
/// Uses a created_at range (not ABS) so SQLite can use indexes.
/// Session-vs-proxy row dedup. Log lists keep outer hops so the chain is visible.
const SESSION_DEDUP_FILTER: &str = "
  AND (
    COALESCE(l.data_source, 'proxy') NOT IN ('codex_session', 'claude_code_session', 'pi_session')
    OR NOT EXISTS (
      SELECT 1 FROM proxy_request_logs p
      WHERE COALESCE(p.data_source, 'proxy') = 'proxy'
        AND (
          CASE COALESCE(l.data_source, 'proxy')
            WHEN 'claude_code_session' THEN p.target_app = 'claude_code'
            WHEN 'codex_session' THEN p.target_app = 'codex'
            WHEN 'pi_session' THEN p.target_app IN ('pi', 'antigravity')
            ELSE 0
          END
        )
        AND p.status_code BETWEEN 200 AND 299
        AND p.created_at BETWEEN l.created_at - 600000 AND l.created_at + 600000
        AND p.input_tokens = l.input_tokens
        AND p.output_tokens = l.output_tokens
        AND p.cache_read_input_tokens = l.cache_read_input_tokens
        AND (
          lower(COALESCE(p.model, '')) = lower(COALESCE(l.model, ''))
          OR lower(COALESCE(l.model, '')) IN ('', 'unknown')
          OR lower(COALESCE(p.model, '')) IN ('', 'unknown')
          OR lower(COALESCE(p.model, '')) = lower(COALESCE(l.model, '')) || '-fast'
          OR lower(COALESCE(l.model, '')) = lower(COALESCE(p.model, '')) || '-fast'
        )
    )
  )
";

const USAGE_COUNTED_SQL: &str = "
    CASE
      WHEN l.correlation_id IS NULL OR trim(l.correlation_id) = ''
        OR l.hop IS NULL OR trim(l.hop) = ''
        OR l.hop = (
          SELECT CASE
            WHEN SUM(CASE WHEN hop = 'antigravity' THEN 1 ELSE 0 END) > 0 THEN 'antigravity'
            WHEN SUM(CASE WHEN hop = 'smart_gateway' THEN 1 ELSE 0 END) > 0 THEN 'smart_gateway'
            ELSE MAX(hop)
          END
          FROM proxy_request_logs c
          WHERE c.correlation_id = l.correlation_id
        )
      THEN 1 ELSE 0
    END
";

/// SQL fragment: drop session-sync rows that duplicate a nearby proxy row,
/// and keep only the innermost hop for a correlated multi-hop request.
pub(crate) const EFFECTIVE_USAGE_FILTER: &str = "
  AND (
    COALESCE(l.data_source, 'proxy') NOT IN ('codex_session', 'claude_code_session', 'pi_session')
    OR NOT EXISTS (
      SELECT 1 FROM proxy_request_logs p
      WHERE COALESCE(p.data_source, 'proxy') = 'proxy'
        AND (
          CASE COALESCE(l.data_source, 'proxy')
            WHEN 'claude_code_session' THEN p.target_app = 'claude_code'
            WHEN 'codex_session' THEN p.target_app = 'codex'
            WHEN 'pi_session' THEN p.target_app IN ('pi', 'antigravity')
            ELSE 0
          END
        )
        AND p.status_code BETWEEN 200 AND 299
        AND p.created_at BETWEEN l.created_at - 600000 AND l.created_at + 600000
        AND p.input_tokens = l.input_tokens
        AND p.output_tokens = l.output_tokens
        AND p.cache_read_input_tokens = l.cache_read_input_tokens
        AND (
          lower(COALESCE(p.model, '')) = lower(COALESCE(l.model, ''))
          OR lower(COALESCE(l.model, '')) IN ('', 'unknown')
          OR lower(COALESCE(p.model, '')) IN ('', 'unknown')
          OR lower(COALESCE(p.model, '')) = lower(COALESCE(l.model, '')) || '-fast'
          OR lower(COALESCE(l.model, '')) = lower(COALESCE(p.model, '')) || '-fast'
        )
    )
  )
  AND (
    l.correlation_id IS NULL OR trim(l.correlation_id) = ''
    OR l.hop IS NULL OR trim(l.hop) = ''
    OR l.hop = (
      SELECT CASE
        WHEN SUM(CASE WHEN hop = 'antigravity' THEN 1 ELSE 0 END) > 0 THEN 'antigravity'
        WHEN SUM(CASE WHEN hop = 'smart_gateway' THEN 1 ELSE 0 END) > 0 THEN 'smart_gateway'
        ELSE MAX(hop)
      END
      FROM proxy_request_logs c
      WHERE c.correlation_id = l.correlation_id
    )
  )
";

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct CurrencyAmount {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub request_count: i64,
    pub successful_request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
    /// Currency for `estimated_cost` (dominant / sole matched pricing currency).
    pub estimated_cost_currency: String,
    /// All matched pricing currencies; amounts are never mixed across currencies.
    pub estimated_costs_by_currency: Vec<CurrencyAmount>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
    /// Pricing currency for this row. Mixed-currency groups are converted to USD.
    pub currency: String,
}

/// Token totals grouped by provider name and model, before pricing.
#[derive(Debug, Clone)]
pub struct UsageProviderModelGroup {
    pub provider: String,
    pub model: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub date: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
    pub currency: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub model: String,
    pub provider: String,
    pub input_price_per_million: f64,
    pub cache_read_price_per_million: f64,
    pub cache_write_price_per_million: f64,
    pub output_price_per_million: f64,
    pub batch_input_price_per_million: f64,
    pub batch_output_price_per_million: f64,
    pub currency: String,
    pub source_url: String,
    pub effective_date: String,
    pub is_default: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMaintenanceResult {
    pub deleted: i64,
    pub deleted_by_age: i64,
    pub deleted_by_limit: i64,
    pub integrity_ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMaintenancePreview {
    pub total_rows: i64,
    pub delete_by_age: i64,
    pub delete_by_limit: i64,
}

pub fn preview_proxy_log_maintenance(conn: &Connection, retention_days: u32, max_rows: u32) -> AppResult<LogMaintenancePreview> {
    let cutoff = (Utc::now() - chrono::Duration::days(i64::from(retention_days.clamp(1, 3650)))).timestamp_millis();
    let total_rows: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| row.get(0))?;
    let delete_by_age: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE created_at < ?",
        params![cutoff],
        |row| row.get(0),
    )?;
    let remaining = total_rows - delete_by_age;
    let delete_by_limit = (remaining - i64::from(max_rows.max(100))).max(0);
    Ok(LogMaintenancePreview { total_rows, delete_by_age, delete_by_limit })
}

pub fn maintain_proxy_logs(conn: &Connection, retention_days: u32, max_rows: u32, vacuum: bool) -> AppResult<LogMaintenanceResult> {
    let cutoff = (Utc::now() - chrono::Duration::days(i64::from(retention_days.clamp(1, 3650)))).timestamp_millis();
    let tx = conn.unchecked_transaction()?;
    let by_age = tx.execute("DELETE FROM proxy_request_logs WHERE created_at < ?", params![cutoff])? as i64;
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| row.get(0))?;
    let by_limit = if count > i64::from(max_rows.max(100)) {
        tx.execute(
            "DELETE FROM proxy_request_logs WHERE id IN (
                SELECT id FROM proxy_request_logs ORDER BY created_at ASC LIMIT ?
             )",
            params![count - i64::from(max_rows.max(100))],
        )? as i64
    } else { 0 };
    tx.commit()?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if vacuum {
        conn.execute_batch("VACUUM")?;
    }
    Ok(LogMaintenanceResult {
        deleted: by_age + by_limit,
        deleted_by_age: by_age,
        deleted_by_limit: by_limit,
        integrity_ok: integrity == "ok",
    })
}

/// Create a proxy request log and return its id so token usage can be completed
/// once the upstream response body has been streamed.
pub fn insert_proxy_log(
    conn: &Connection,
    provider_id: Option<&str>,
    provider_name: Option<&str>,
    model: Option<&str>,
    status_code: Option<i64>,
    duration_ms: i64,
    target_app: Option<&str>,
    protocol: Option<&str>,
    route: Option<&str>,
    is_stream: bool,
    error_category: Option<&str>,
    diagnostic: Option<&str>,
) -> AppResult<String> {
    insert_proxy_log_with_source(
        conn,
        None,
        Utc::now().timestamp_millis(),
        provider_id,
        provider_name,
        model,
        status_code,
        0,
        0,
        0,
        0,
        false,
        duration_ms,
        target_app,
        protocol,
        route,
        is_stream,
        error_category,
        diagnostic,
        DATA_SOURCE_PROXY,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn insert_proxy_log_with_source(
    conn: &Connection,
    id: Option<&str>,
    created_at: i64,
    provider_id: Option<&str>,
    provider_name: Option<&str>,
    model: Option<&str>,
    status_code: Option<i64>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
    usage_available: bool,
    duration_ms: i64,
    target_app: Option<&str>,
    protocol: Option<&str>,
    route: Option<&str>,
    is_stream: bool,
    error_category: Option<&str>,
    diagnostic: Option<&str>,
    data_source: &str,
    session_id: Option<&str>,
) -> AppResult<String> {
    let id = id
        .map(str::to_string)
        .unwrap_or_else(|| format!("log_{}", Uuid::new_v4().simple()));
    conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs
            (id, created_at, provider_id, provider_name, model, status_code,
             input_tokens, cache_read_input_tokens, cache_creation_input_tokens, output_tokens,
             usage_available, duration_ms, target_app, protocol, route, is_stream,
             error_category, diagnostic, data_source, session_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        params![
            id,
            created_at,
            provider_id,
            provider_name,
            model,
            status_code,
            input_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
            output_tokens,
            usage_available,
            duration_ms,
            target_app,
            protocol,
            route,
            is_stream,
            error_category,
            diagnostic,
            data_source,
            session_id,
        ],
    )?;
    Ok(id)
}

pub fn update_proxy_log_hop(
    conn: &Connection,
    id: &str,
    correlation_id: Option<&str>,
    hop: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE proxy_request_logs SET correlation_id = COALESCE(?, correlation_id), hop = COALESCE(?, hop) WHERE id = ?;",
        params![correlation_id, hop, id],
    )?;
    Ok(())
}

pub fn should_skip_codex_session_insert(
    conn: &Connection,
    created_at: i64,
    model: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<bool> {
    should_skip_session_insert_for_target(
        conn,
        "codex",
        created_at,
        model,
        input_tokens,
        cache_read_input_tokens,
        output_tokens,
    )
}

pub fn should_skip_claude_code_session_insert(
    conn: &Connection,
    created_at: i64,
    model: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<bool> {
    should_skip_session_insert_for_target(
        conn,
        "claude_code",
        created_at,
        model,
        input_tokens,
        cache_read_input_tokens,
        output_tokens,
    )
}

pub fn should_skip_opencode_session_insert(
    conn: &Connection,
    created_at: i64,
    model: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<bool> {
    should_skip_session_insert_for_target(
        conn,
        "opencode",
        created_at,
        model,
        input_tokens,
        cache_read_input_tokens,
        output_tokens,
    )
}

pub fn should_skip_pi_session_insert(
    conn: &Connection,
    created_at: i64,
    model: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<bool> {
    if should_skip_session_insert_for_target(
        conn,
        "pi",
        created_at,
        model,
        input_tokens,
        cache_read_input_tokens,
        output_tokens,
    )? {
        return Ok(true);
    }
    // Pi Antigravity models already hit the AG gateway (`target_app=antigravity`).
    should_skip_session_insert_for_target(
        conn,
        "antigravity",
        created_at,
        model,
        input_tokens,
        cache_read_input_tokens,
        output_tokens,
    )
}

fn should_skip_session_insert_for_target(
    conn: &Connection,
    target_app: &str,
    created_at: i64,
    model: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<bool> {
    let model = model.unwrap_or("unknown");
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs
         WHERE COALESCE(data_source, 'proxy') = 'proxy'
           AND target_app = ?
           AND status_code BETWEEN 200 AND 299
           AND created_at BETWEEN ? AND ?
           AND input_tokens = ?
           AND output_tokens = ?
           AND cache_read_input_tokens = ?
           AND (
             lower(COALESCE(model, '')) = lower(?)
             OR lower(COALESCE(model, '')) IN ('', 'unknown')
             OR lower(?) IN ('', 'unknown')
             OR lower(COALESCE(model, '')) = lower(?) || '-fast'
             OR lower(?) = lower(COALESCE(model, '')) || '-fast'
           );",
        params![
            target_app,
            created_at - SESSION_PROXY_DEDUP_WINDOW_MS,
            created_at + SESSION_PROXY_DEDUP_WINDOW_MS,
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            model,
            model,
            model,
            model,
        ],
        |row| row.get(0),
    )?;
    Ok(exists > 0)
}

pub fn normalize_sync_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn get_session_sync_state(
    conn: &Connection,
    file_path: &str,
) -> AppResult<Option<(i64, i64, i64)>> {
    let normalized = file_path.replace('\\', "/");
    let candidates = [file_path, normalized.as_str()];
    for candidate in candidates {
        let mut stmt = conn.prepare(
            "SELECT last_modified, last_line_offset, COALESCE(last_file_size, 0)
             FROM session_log_sync WHERE file_path = ?;",
        )?;
        let mut rows = stmt.query(params![candidate])?;
        if let Some(row) = rows.next()? {
            return Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?)));
        }
    }
    // Windows may have stored the opposite slash style.
    let mut stmt = conn.prepare(
        "SELECT last_modified, last_line_offset, COALESCE(last_file_size, 0)
         FROM session_log_sync
         WHERE replace(file_path, '\\', '/') = ?;",
    )?;
    let mut rows = stmt.query(params![normalized])?;
    if let Some(row) = rows.next()? {
        Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?)))
    } else {
        Ok(None)
    }
}

pub fn update_session_sync_state(
    conn: &Connection,
    file_path: &str,
    last_modified: i64,
    last_line_offset: i64,
    last_file_size: i64,
) -> AppResult<()> {
    let normalized = file_path.replace('\\', "/");
    // Drop legacy slash-variant rows so one canonical key remains.
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(file_path, '\\', '/') = ?
           AND file_path <> ?;",
        params![normalized, normalized],
    )?;
    conn.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_file_size, last_synced_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(file_path) DO UPDATE SET
           last_modified = excluded.last_modified,
           last_line_offset = excluded.last_line_offset,
           last_file_size = excluded.last_file_size,
           last_synced_at = excluded.last_synced_at;",
        params![
            normalized,
            last_modified,
            last_line_offset,
            last_file_size,
            Utc::now().timestamp_millis()
        ],
    )?;
    Ok(())
}

pub fn reset_codex_session_usage(conn: &Connection) -> AppResult<i64> {
    let deleted = conn.execute(
        "DELETE FROM proxy_request_logs WHERE data_source = ?;",
        params![DATA_SOURCE_CODEX_SESSION],
    )? as i64;
    // Clear sync cursors for Codex session trees only (keep Claude Code cursors).
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(lower(file_path), '\\', '/') LIKE '%/sessions/%'
            OR replace(lower(file_path), '\\', '/') LIKE '%/archived_sessions/%';",
        [],
    )?;
    Ok(deleted)
}

pub fn reset_claude_code_session_usage(conn: &Connection) -> AppResult<i64> {
    let deleted = conn.execute(
        "DELETE FROM proxy_request_logs WHERE data_source = ?;",
        params![DATA_SOURCE_CLAUDE_CODE_SESSION],
    )? as i64;
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(lower(file_path), '\\', '/') LIKE '%/.claude/projects/%'
            OR replace(lower(file_path), '\\', '/') LIKE '%/claude/projects/%';",
        [],
    )?;
    Ok(deleted)
}

pub fn reset_opencode_session_usage(conn: &Connection) -> AppResult<i64> {
    let deleted = conn.execute(
        "DELETE FROM proxy_request_logs WHERE data_source = ?;",
        params![DATA_SOURCE_OPENCODE_SESSION],
    )? as i64;
    // 同步游标键为 `<db路径>` 或 `<db路径>:<session_id>`（见 usage/session_usage_opencode.rs）。
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(lower(file_path), '\\', '/') LIKE '%/opencode.db%';",
        [],
    )?;
    Ok(deleted)
}

pub fn reset_pi_session_usage(conn: &Connection) -> AppResult<i64> {
    let deleted = conn.execute(
        "DELETE FROM proxy_request_logs WHERE data_source = ?;",
        params![DATA_SOURCE_PI_SESSION],
    )? as i64;
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(lower(file_path), '\\', '/') LIKE '%/.pi/agent/sessions/%'
            OR replace(lower(file_path), '\\', '/') LIKE '%/pi/agent/sessions/%';",
        [],
    )?;
    Ok(deleted)
}

pub fn reset_dsh_session_usage(conn: &Connection) -> AppResult<i64> {
    let deleted = conn.execute(
        "DELETE FROM proxy_request_logs WHERE data_source = ?;",
        params![DATA_SOURCE_DSH_SESSION],
    )? as i64;
    conn.execute(
        "DELETE FROM session_log_sync
         WHERE replace(lower(file_path), '\\', '/') LIKE '%/.dsh/sessions/%'
            OR replace(lower(file_path), '\\', '/') LIKE '%/dsh/sessions/%';",
        [],
    )?;
    Ok(deleted)
}

/// Persist usage and optionally rematerialize the log row under a stable
/// response-scoped id so retries/replays of the same upstream response do not
/// stack duplicate rows.
#[allow(clippy::too_many_arguments)]
pub fn update_proxy_log_usage_idempotent(
    conn: &Connection,
    id: &str,
    target_app: Option<&str>,
    provider_id: Option<&str>,
    envelope_id: Option<&str>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<()> {
    let stable_id = envelope_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|envelope| {
            stable_proxy_usage_id(
                target_app.unwrap_or("unknown"),
                provider_id.unwrap_or("unknown"),
                envelope,
                input_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
                output_tokens,
            )
        });

    if let Some(stable) = stable_id.as_deref() {
        if stable != id {
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE id = ?;",
                params![stable],
                |row| row.get(0),
            )?;
            if exists > 0 {
                conn.execute("DELETE FROM proxy_request_logs WHERE id = ?;", params![id])?;
                return Ok(());
            }
            conn.execute(
                "UPDATE proxy_request_logs SET id = ? WHERE id = ?;",
                params![stable, id],
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET input_tokens = ?, cache_read_input_tokens = ?,
                     cache_creation_input_tokens = ?, output_tokens = ?, usage_available = 1
                 WHERE id = ?;",
                params![
                    input_tokens,
                    cache_read_input_tokens,
                    cache_creation_input_tokens,
                    output_tokens,
                    stable
                ],
            )?;
            return Ok(());
        }
    }

    conn.execute(
        "UPDATE proxy_request_logs
         SET input_tokens = ?, cache_read_input_tokens = ?,
             cache_creation_input_tokens = ?, output_tokens = ?, usage_available = 1
         WHERE id = ?;",
        params![
            input_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
            output_tokens,
            id
        ],
    )?;
    Ok(())
}

pub fn stable_proxy_usage_id(
    target_app: &str,
    provider_id: &str,
    envelope_id: &str,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
) -> String {
    let envelope = envelope_id.trim();
    if !envelope.is_empty() {
        return format!("session:{target_app}:{provider_id}:{envelope}");
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(target_app.as_bytes());
    hasher.update(b"|");
    hasher.update(provider_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input_tokens.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(cache_read_input_tokens.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(cache_creation_input_tokens.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(output_tokens.to_string().as_bytes());
    format!("session:{target_app}:{provider_id}:hash:{}", hex::encode(hasher.finalize()))
}

pub fn extract_usage_envelope_id(value: &serde_json::Value) -> Option<String> {
    value
        .pointer("/response/id")
        .or_else(|| value.get("id"))
        .or_else(|| value.get("responseId"))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.eq_ignore_ascii_case("response.created"))
        .map(str::to_string)
        .or_else(|| {
            value
                .get("id")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|item| item.starts_with("chatcmpl-") || item.starts_with("resp_"))
                .map(str::to_string)
        })
}

pub fn update_proxy_log_diagnostic(
    conn: &Connection,
    id: &str,
    error_category: &str,
    diagnostic: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE proxy_request_logs
         SET error_category = ?, diagnostic = ?
         WHERE id = ?;",
        params![error_category, diagnostic, id],
    )?;
    Ok(())
}

pub fn update_proxy_log_route(
    conn: &Connection,
    id: &str,
    profile_id: Option<&str>,
    route_reason: Option<&str>,
    attempt_index: i64,
    requested_model: Option<&str>,
    upstream_id: Option<&str>,
    route_mode: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE proxy_request_logs
         SET profile_id = ?, route_reason = ?, attempt_index = ?, requested_model = ?, upstream_id = ?, route_mode = ?
         WHERE id = ?;",
        params![
            profile_id,
            route_reason,
            attempt_index,
            requested_model,
            upstream_id,
            route_mode,
            id
        ],
    )?;
    Ok(())
}

/// Per-request cost from matched `model_pricing` (any currency).
pub(crate) const ROW_COST_SQL: &str = "\
    COALESCE(l.input_tokens, 0) * COALESCE(p.input_price_per_million, 0) / 1000000.0 \
    + COALESCE(l.cache_read_input_tokens, 0) * COALESCE(p.cache_read_price_per_million, 0) / 1000000.0 \
    + COALESCE(l.cache_creation_input_tokens, 0) * COALESCE(p.cache_write_price_per_million, 0) / 1000000.0 \
    + COALESCE(l.output_tokens, 0) * COALESCE(p.output_price_per_million, 0) / 1000000.0";

const PRICING_CURRENCY_SQL: &str =
    "UPPER(COALESCE(NULLIF(TRIM(p.currency), ''), 'USD'))";

pub fn get_usage_summary_for_target(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
) -> AppResult<UsageSummary> {
    let tokens_sql = format!(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(cache_read_input_tokens), 0),
                COALESCE(SUM(cache_creation_input_tokens), 0),
                COALESCE(SUM(output_tokens), 0)
         FROM proxy_request_logs l
         WHERE l.created_at >= :since
           AND (:target_app IS NULL OR l.target_app = :target_app)
           {EFFECTIVE_USAGE_FILTER};"
    );
    let (request_count, successful_request_count, input_tokens, cache_read_input_tokens, cache_creation_input_tokens, output_tokens) =
        conn.query_row(
            &tokens_sql,
            named_params! { ":since": since, ":target_app": target_app },
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;

    let costs_sql = format!(
        "SELECT {PRICING_CURRENCY_SQL},
                COALESCE(SUM({ROW_COST_SQL}), 0)
         FROM proxy_request_logs l
         INNER JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= :since
           AND (:target_app IS NULL OR l.target_app = :target_app)
           {EFFECTIVE_USAGE_FILTER}
         GROUP BY 1
         ORDER BY 2 DESC, 1 ASC;"
    );
    let mut stmt = conn.prepare(&costs_sql)?;
    let mut estimated_costs_by_currency = stmt
        .query_map(
            named_params! { ":since": since, ":target_app": target_app },
            |row| {
                Ok(CurrencyAmount {
                    currency: row.get(0)?,
                    amount: row.get(1)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    estimated_costs_by_currency.retain(|entry| entry.amount.abs() > f64::EPSILON);
    let (estimated_cost_currency, estimated_cost) =
        pick_primary_currency_amount(&estimated_costs_by_currency);

    Ok(UsageSummary {
        request_count,
        successful_request_count,
        input_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        output_tokens,
        estimated_cost,
        estimated_cost_currency,
        estimated_costs_by_currency,
    })
}

/// Pick the headline currency for a multi-currency cost summary.
/// Single currency stays native; multiple currencies convert to USD and sum.
fn pick_primary_currency_amount(amounts: &[CurrencyAmount]) -> (String, f64) {
    let pairs: Vec<(String, f64)> = amounts
        .iter()
        .map(|entry| (entry.currency.clone(), entry.amount))
        .collect();
    crate::usage::summarize_costs_as_usd(&pairs)
}

pub fn get_usage_by_provider_for_target(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
) -> AppResult<Vec<UsageBreakdown>> {
    usage_breakdown(conn, since, target_app, "COALESCE(l.provider_name, 'Unknown')")
}

pub fn get_usage_by_provider_model_for_target(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
) -> AppResult<Vec<UsageProviderModelGroup>> {
    let sql = format!(
        "SELECT COALESCE(l.provider_name, 'Unknown'),
                COALESCE(l.model, 'Unknown'),
                COUNT(*),
                COALESCE(SUM(l.input_tokens), 0),
                COALESCE(SUM(l.cache_read_input_tokens), 0),
                COALESCE(SUM(l.cache_creation_input_tokens), 0),
                COALESCE(SUM(l.output_tokens), 0)
         FROM proxy_request_logs l
         WHERE l.created_at >= :since
           AND (:target_app IS NULL OR l.target_app = :target_app)
           {EFFECTIVE_USAGE_FILTER}
         GROUP BY COALESCE(l.provider_name, 'Unknown'), COALESCE(l.model, 'Unknown');"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(named_params! { ":since": since, ":target_app": target_app }, |row| {
        Ok(UsageProviderModelGroup {
            provider: row.get(0)?,
            model: row.get(1)?,
            request_count: row.get(2)?,
            input_tokens: row.get(3)?,
            cache_read_input_tokens: row.get(4)?,
            cache_creation_input_tokens: row.get(5)?,
            output_tokens: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_usage_by_model_for_target(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
) -> AppResult<Vec<UsageBreakdown>> {
    usage_breakdown(conn, since, target_app, "COALESCE(l.model, 'Unknown')")
}

fn usage_breakdown(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
    grouping: &str,
) -> AppResult<Vec<UsageBreakdown>> {
    let sql = format!(
        "SELECT {grouping}, COUNT(*), COALESCE(SUM(l.input_tokens), 0),
                COALESCE(SUM(l.cache_read_input_tokens), 0),
                COALESCE(SUM(l.cache_creation_input_tokens), 0),
                COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM({ROW_COST_SQL}), 0),
                {PRICING_CURRENCY_SQL}
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= :since
           AND (:target_app IS NULL OR l.target_app = :target_app)
           {EFFECTIVE_USAGE_FILTER}
         GROUP BY {grouping}, {PRICING_CURRENCY_SQL};"
    );
    struct Acc {
        request_count: i64,
        input_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
        output_tokens: i64,
        costs: Vec<(String, f64)>,
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut grouped: HashMap<String, Acc> = HashMap::new();
    let rows = stmt.query_map(named_params! { ":since": since, ":target_app": target_app }, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, f64>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    for row in rows {
        let (key, request_count, input_tokens, cache_read, cache_write, output_tokens, cost, currency) =
            row?;
        let acc = grouped.entry(key).or_insert_with(|| Acc {
            request_count: 0,
            input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            costs: Vec::new(),
        });
        acc.request_count += request_count;
        acc.input_tokens += input_tokens;
        acc.cache_read_input_tokens += cache_read;
        acc.cache_creation_input_tokens += cache_write;
        acc.output_tokens += output_tokens;
        acc.costs.push((currency, cost));
    }
    let mut out: Vec<UsageBreakdown> = grouped
        .into_iter()
        .map(|(key, acc)| {
            let (currency, estimated_cost) = crate::usage::summarize_costs_as_usd(&acc.costs);
            UsageBreakdown {
                key,
                request_count: acc.request_count,
                input_tokens: acc.input_tokens,
                cache_read_input_tokens: acc.cache_read_input_tokens,
                cache_creation_input_tokens: acc.cache_creation_input_tokens,
                output_tokens: acc.output_tokens,
                estimated_cost,
                currency,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.request_count
            .cmp(&a.request_count)
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendGranularity {
    Day,
    Hour,
}

pub fn get_usage_trend_for_target(
    conn: &Connection,
    since: i64,
    target_app: Option<&str>,
    granularity: TrendGranularity,
) -> AppResult<Vec<UsageTrendPoint>> {
    let bucket = match granularity {
        TrendGranularity::Day => "strftime('%Y-%m-%d', l.created_at / 1000, 'unixepoch', 'localtime')",
        TrendGranularity::Hour => {
            "strftime('%Y-%m-%d %H:00', l.created_at / 1000, 'unixepoch', 'localtime')"
        }
    };
    let sql = format!(
        "SELECT {bucket}, COUNT(*),
                COALESCE(SUM(l.input_tokens), 0),
                COALESCE(SUM(l.cache_read_input_tokens), 0),
                COALESCE(SUM(l.cache_creation_input_tokens), 0),
                COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM({ROW_COST_SQL}), 0),
                {PRICING_CURRENCY_SQL}
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= :since
           AND (:target_app IS NULL OR l.target_app = :target_app)
           {EFFECTIVE_USAGE_FILTER}
         GROUP BY 1, {PRICING_CURRENCY_SQL}
         ORDER BY 1 ASC;"
    );
    struct Acc {
        request_count: i64,
        input_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
        output_tokens: i64,
        costs: Vec<(String, f64)>,
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut grouped: HashMap<String, Acc> = HashMap::new();
    let rows = stmt.query_map(named_params! { ":since": since, ":target_app": target_app }, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, f64>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    for row in rows {
        let (date, request_count, input_tokens, cache_read, cache_write, output_tokens, cost, currency) =
            row?;
        let acc = grouped.entry(date).or_insert_with(|| Acc {
            request_count: 0,
            input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            costs: Vec::new(),
        });
        acc.request_count += request_count;
        acc.input_tokens += input_tokens;
        acc.cache_read_input_tokens += cache_read;
        acc.cache_creation_input_tokens += cache_write;
        acc.output_tokens += output_tokens;
        acc.costs.push((currency, cost));
    }
    let mut out: Vec<UsageTrendPoint> = grouped
        .into_iter()
        .map(|(date, acc)| {
            let (currency, estimated_cost) = crate::usage::summarize_costs_as_usd(&acc.costs);
            UsageTrendPoint {
                date,
                request_count: acc.request_count,
                input_tokens: acc.input_tokens,
                cache_read_input_tokens: acc.cache_read_input_tokens,
                cache_creation_input_tokens: acc.cache_creation_input_tokens,
                output_tokens: acc.output_tokens,
                estimated_cost,
                currency,
            }
        })
        .collect();
    out.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(out)
}

pub fn list_model_pricing(conn: &Connection) -> AppResult<Vec<ModelPricing>> {
    let mut stmt = conn.prepare(
        "SELECT model, provider, input_price_per_million, cache_read_price_per_million,
                cache_write_price_per_million, output_price_per_million,
                batch_input_price_per_million, batch_output_price_per_million,
                currency, source_url, effective_date, is_default
         FROM model_pricing ORDER BY model COLLATE NOCASE;",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ModelPricing {
            model: row.get(0)?,
            provider: row.get(1)?,
            input_price_per_million: row.get(2)?,
            cache_read_price_per_million: row.get(3)?,
            cache_write_price_per_million: row.get(4)?,
            output_price_per_million: row.get(5)?,
            batch_input_price_per_million: row.get(6)?,
            batch_output_price_per_million: row.get(7)?,
            currency: row.get(8)?,
            source_url: row.get(9)?,
            effective_date: row.get(10)?,
            is_default: row.get(11)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn save_model_pricing(conn: &Connection, pricing: &ModelPricing) -> AppResult<()> {
    conn.execute(
        "INSERT INTO model_pricing
            (model, provider, input_price_per_million, cache_read_price_per_million,
             cache_write_price_per_million, output_price_per_million,
             batch_input_price_per_million, batch_output_price_per_million, currency,
             source_url, effective_date, is_default)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', 0)
         ON CONFLICT(model) DO UPDATE SET provider = excluded.provider,
             input_price_per_million = excluded.input_price_per_million,
             cache_read_price_per_million = excluded.cache_read_price_per_million,
             cache_write_price_per_million = excluded.cache_write_price_per_million,
             output_price_per_million = excluded.output_price_per_million,
             batch_input_price_per_million = excluded.batch_input_price_per_million,
             batch_output_price_per_million = excluded.batch_output_price_per_million,
             currency = excluded.currency, source_url = '', effective_date = '', is_default = 0;",
        params![
            pricing.model, pricing.provider, pricing.input_price_per_million,
            pricing.cache_read_price_per_million, pricing.cache_write_price_per_million,
            pricing.output_price_per_million, pricing.batch_input_price_per_million,
            pricing.batch_output_price_per_million, pricing.currency,
        ],
    )?;
    Ok(())
}

pub fn delete_model_pricing(conn: &Connection, model: &str) -> AppResult<()> {
    conn.execute("DELETE FROM model_pricing WHERE model = ?;", params![model])?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestLog {
    pub id: String,
    pub created_at: i64,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<i64>,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub usage_available: bool,
    pub duration_ms: i64,
    pub target_app: Option<String>,
    pub protocol: Option<String>,
    pub route: Option<String>,
    pub is_stream: bool,
    pub error_category: Option<String>,
    pub diagnostic: Option<String>,
    pub stream_outcome: Option<String>,
    pub data_source: String,
    pub session_id: Option<String>,
    pub route_reason: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_id: Option<String>,
    pub profile_id: Option<String>,
    pub correlation_id: Option<String>,
    pub hop: Option<String>,
    pub usage_counted: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct PaginatedProxyLogs {
    pub data: Vec<ProxyRequestLog>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Default)]
#[cfg_attr(test, derive(TS))]
pub struct ProxyLogFilters {
    pub since: Option<i64>,
    pub target_app: Option<String>,
    pub status_code: Option<i64>,
    pub only_failures: Option<bool>,
    pub only_gateway: Option<bool>,
}

pub fn update_proxy_log_stream_outcome(
    conn: &Connection,
    id: &str,
    stream_outcome: &str,
    duration_ms: Option<i64>,
    error_category: Option<&str>,
    diagnostic: Option<&str>,
) -> AppResult<()> {
    if let Some(duration) = duration_ms {
        conn.execute(
            "UPDATE proxy_request_logs
             SET stream_outcome = ?, duration_ms = ?,
                 error_category = COALESCE(?, error_category),
                 diagnostic = COALESCE(?, diagnostic)
             WHERE id = ?;",
            params![stream_outcome, duration, error_category, diagnostic, id],
        )?;
    } else {
        conn.execute(
            "UPDATE proxy_request_logs
             SET stream_outcome = ?,
                 error_category = COALESCE(?, error_category),
                 diagnostic = COALESCE(?, diagnostic)
             WHERE id = ?;",
            params![stream_outcome, error_category, diagnostic, id],
        )?;
    }
    Ok(())
}

pub fn list_proxy_request_logs(
    conn: &Connection,
    filters: &ProxyLogFilters,
    page: u32,
    page_size: u32,
) -> AppResult<PaginatedProxyLogs> {
    let page_size = page_size.clamp(1, 100);
    let page = page;
    let offset = i64::from(page) * i64::from(page_size);

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(since) = filters.since {
        conditions.push("l.created_at >= ?".to_string());
        params.push(Box::new(since));
    }
    if let Some(ref target_app) = filters.target_app {
        conditions.push("l.target_app = ?".to_string());
        params.push(Box::new(target_app.clone()));
    }
    if let Some(status_code) = filters.status_code {
        conditions.push("l.status_code = ?".to_string());
        params.push(Box::new(status_code));
    }
    if filters.only_failures.unwrap_or(false) {
        conditions.push("(l.status_code >= 400 OR l.error_category IS NOT NULL OR l.stream_outcome IN ('midstream_error', 'cancelled'))".to_string());
    }
    if filters.only_gateway.unwrap_or(false) {
        conditions.push("(NULLIF(TRIM(COALESCE(l.route_reason, '')), '') IS NOT NULL OR NULLIF(TRIM(COALESCE(l.profile_id, '')), '') IS NOT NULL)".to_string());
    }

    let where_clause = if conditions.is_empty() {
        format!("WHERE 1=1 {SESSION_DEDUP_FILTER}")
    } else {
        format!("WHERE {} {SESSION_DEDUP_FILTER}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM proxy_request_logs l {where_clause}");
    let count_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let total: i64 = conn.query_row(&count_sql, count_params.as_slice(), |row| row.get(0))?;

    let data_sql = format!(
        "SELECT l.id, l.created_at, l.provider_id, l.provider_name, l.model, l.status_code,
                l.input_tokens, l.cache_read_input_tokens, l.cache_creation_input_tokens,
                l.output_tokens, l.usage_available, l.duration_ms, l.target_app, l.protocol, l.route,
                l.is_stream, l.error_category, l.diagnostic,
                COALESCE(l.data_source, 'proxy'), l.session_id, l.stream_outcome,
                l.route_reason, l.requested_model, l.upstream_id, l.profile_id,
                l.correlation_id, l.hop, {USAGE_COUNTED_SQL}
         FROM proxy_request_logs l
         {where_clause}
         ORDER BY l.created_at DESC
         LIMIT ? OFFSET ?"
    );
    params.push(Box::new(i64::from(page_size)));
    params.push(Box::new(offset));
    let data_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&data_sql)?;
    let rows = stmt.query_map(data_params.as_slice(), |row| {
        Ok(ProxyRequestLog {
            id: row.get(0)?,
            created_at: row.get(1)?,
            provider_id: row.get(2)?,
            provider_name: row.get(3)?,
            model: row.get(4)?,
            status_code: row.get(5)?,
            input_tokens: row.get(6)?,
            cache_read_input_tokens: row.get(7)?,
            cache_creation_input_tokens: row.get(8)?,
            output_tokens: row.get(9)?,
            usage_available: row.get::<_, i64>(10)? != 0,
            duration_ms: row.get(11)?,
            target_app: row.get(12)?,
            protocol: row.get(13)?,
            route: row.get(14)?,
            is_stream: row.get::<_, i64>(15)? != 0,
            error_category: row.get(16)?,
            diagnostic: row.get(17)?,
            data_source: row.get(18)?,
            session_id: row.get(19)?,
            stream_outcome: row.get(20)?,
            route_reason: row.get(21)?,
            requested_model: row.get(22)?,
            upstream_id: row.get(23)?,
            profile_id: row.get(24)?,
            correlation_id: row.get(25)?,
            hop: row.get(26)?,
            usage_counted: row.get::<_, i64>(27)? != 0,
        })
    })?;
    let data = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(PaginatedProxyLogs {
        data,
        total,
        page,
        page_size,
    })
}

/// Map a stored `route_reason` onto one of the nine mode ids.
pub(crate) fn classify_route_reason_mode(reason: &str) -> Option<&'static str> {
    if reason.contains("规划") || reason == "plan" {
        Some("plan")
    } else if reason.contains("改内容") || reason == "edit" {
        Some("edit")
    } else if reason.contains("后台") || reason == "background" || reason == "role_subagent" {
        Some("background")
    } else if reason.contains("思考") || reason == "think" {
        Some("think")
    } else if reason.contains("长上下文") || reason == "long_context" {
        Some("long_context")
    } else if reason.contains("联网") || reason == "web_search" {
        Some("web_search")
    } else if reason.contains("视觉") || reason == "vision" {
        Some("vision")
    } else if reason.contains("图像") || reason == "image_gen" {
        Some("image_gen")
    } else if reason.contains("默认") || reason == "auto" || reason == "profile_default" {
        Some("default")
    } else {
        None
    }
}

fn resolve_route_mode_id(key: &str) -> Option<&'static str> {
    match key {
        "default" | "background" | "plan" | "think" | "edit" | "long_context"
        | "web_search" | "vision" | "image_gen" => Some(match key {
            "default" => "default",
            "background" => "background",
            "plan" => "plan",
            "think" => "think",
            "edit" => "edit",
            "long_context" => "long_context",
            "web_search" => "web_search",
            "vision" => "vision",
            "image_gen" => "image_gen",
            _ => unreachable!(),
        }),
        _ => classify_route_reason_mode(key),
    }
}

#[derive(Debug, Clone)]
pub struct RouteModeUsageRow {
    pub mode_id: String,
    pub request_count: i64,
    pub estimated_cost: f64,
}

/// Count gateway route-mode hits. `route_reason` lives on the smart-gateway hop,
/// so this does **not** use [`EFFECTIVE_USAGE_FILTER`] (that would keep only the
/// innermost hop and drop every AG-backed mode hit). Same `correlation_id`
/// counts once; empty gateway-row tokens fall back to the innermost hop.
pub fn list_route_mode_usage_stats(
    conn: &Connection,
    since: i64,
) -> AppResult<Vec<RouteModeUsageRow>> {
    const GATEWAY_HAS_TOKENS: &str = "\
        (COALESCE(l.input_tokens, 0)
         + COALESCE(l.output_tokens, 0)
         + COALESCE(l.cache_read_input_tokens, 0)
         + COALESCE(l.cache_creation_input_tokens, 0)) > 0";
    const INNERMOST_HOP: &str = "\
        (SELECT CASE
           WHEN SUM(CASE WHEN hop = 'antigravity' THEN 1 ELSE 0 END) > 0 THEN 'antigravity'
           WHEN SUM(CASE WHEN hop = 'smart_gateway' THEN 1 ELSE 0 END) > 0 THEN 'smart_gateway'
           ELSE MAX(hop)
         END
         FROM proxy_request_logs c
         WHERE c.correlation_id = l.correlation_id)";
    let sql = format!(
        "SELECT COALESCE(bill.mode_key, ''), COUNT(*), COALESCE(SUM(
                COALESCE(bill.input_tokens, 0) * COALESCE(p.input_price_per_million, 0) / 1000000.0
              + COALESCE(bill.cache_read_input_tokens, 0) * COALESCE(p.cache_read_price_per_million, 0) / 1000000.0
              + COALESCE(bill.cache_creation_input_tokens, 0) * COALESCE(p.cache_write_price_per_million, 0) / 1000000.0
              + COALESCE(bill.output_tokens, 0) * COALESCE(p.output_price_per_million, 0) / 1000000.0
            ), 0)
         FROM (
           SELECT COALESCE(NULLIF(TRIM(l.route_mode), ''), l.route_reason) AS mode_key,
                  CASE WHEN {GATEWAY_HAS_TOKENS}
                       THEN l.input_tokens ELSE COALESCE(i.input_tokens, l.input_tokens) END AS input_tokens,
                  CASE WHEN {GATEWAY_HAS_TOKENS}
                       THEN l.cache_read_input_tokens ELSE COALESCE(i.cache_read_input_tokens, l.cache_read_input_tokens) END AS cache_read_input_tokens,
                  CASE WHEN {GATEWAY_HAS_TOKENS}
                       THEN l.cache_creation_input_tokens ELSE COALESCE(i.cache_creation_input_tokens, l.cache_creation_input_tokens) END AS cache_creation_input_tokens,
                  CASE WHEN {GATEWAY_HAS_TOKENS}
                       THEN l.output_tokens ELSE COALESCE(i.output_tokens, l.output_tokens) END AS output_tokens,
                  CASE WHEN {GATEWAY_HAS_TOKENS}
                       THEN l.model ELSE COALESCE(i.model, l.model) END AS model
           FROM proxy_request_logs l
           LEFT JOIN proxy_request_logs i
             ON NULLIF(TRIM(COALESCE(l.correlation_id, '')), '') IS NOT NULL
            AND i.id = (
              SELECT MIN(c.id) FROM proxy_request_logs c
              WHERE c.correlation_id = l.correlation_id
                AND c.hop = {INNERMOST_HOP}
            )
           WHERE l.created_at >= ?
             AND COALESCE(l.data_source, 'proxy') = 'proxy'
             AND (
               NULLIF(TRIM(COALESCE(l.route_mode, '')), '') IS NOT NULL
               OR NULLIF(TRIM(COALESCE(l.route_reason, '')), '') IS NOT NULL
             )
             AND (
               l.correlation_id IS NULL OR TRIM(l.correlation_id) = ''
               OR l.id = (
                 SELECT MIN(g.id) FROM proxy_request_logs g
                 WHERE g.correlation_id = l.correlation_id
                   AND COALESCE(g.data_source, 'proxy') = 'proxy'
                   AND (
                     NULLIF(TRIM(COALESCE(g.route_mode, '')), '') IS NOT NULL
                     OR NULLIF(TRIM(COALESCE(g.route_reason, '')), '') IS NOT NULL
                   )
               )
             )
         ) bill
         LEFT JOIN model_pricing p ON lower(p.model) = lower(COALESCE(bill.model, ''))
         GROUP BY bill.mode_key;"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;
    let mut merged: HashMap<String, (i64, f64)> = HashMap::new();
    for row in rows {
        let (key, count, cost) = row?;
        let Some(mode_id) = resolve_route_mode_id(&key) else {
            continue;
        };
        let entry = merged.entry(mode_id.to_string()).or_insert((0, 0.0));
        entry.0 += count;
        entry.1 += cost;
    }
    let mut out: Vec<RouteModeUsageRow> = merged
        .into_iter()
        .map(|(mode_id, (request_count, estimated_cost))| RouteModeUsageRow {
            mode_id,
            request_count,
            estimated_cost,
        })
        .collect();
    out.sort_by(|a, b| a.mode_id.cmp(&b.mode_id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_primary_currency_converts_mixed_to_usd() {
        let amounts = vec![
            CurrencyAmount {
                currency: "USD".to_string(),
                amount: 0.0189,
            },
            CurrencyAmount {
                currency: "CNY".to_string(),
                amount: 72.5,
            },
        ];
        let (currency, amount) = pick_primary_currency_amount(&amounts);
        assert_eq!(currency, "USD");
        assert!((amount - 10.0189).abs() < 1e-9);
    }

    #[test]
    fn pick_primary_currency_keeps_single_currency() {
        let amounts = vec![CurrencyAmount {
            currency: "USD".to_string(),
            amount: 1.25,
        }];
        let (currency, amount) = pick_primary_currency_amount(&amounts);
        assert_eq!(currency, "USD");
        assert!((amount - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn correlation_keeps_innermost_hop_for_usage() {
        let db = crate::database::Database::memory().unwrap();
        db.with_conn(|conn| {
            let outer = insert_proxy_log(
                conn,
                Some("p1"),
                Some("proxy"),
                Some("m"),
                Some(200),
                10,
                Some("claude_code"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
            )?;
            update_proxy_log_hop(conn, &outer, Some("req_abc"), Some("agent_proxy"))?;
            let inner = insert_proxy_log(
                conn,
                Some("ag"),
                Some("Antigravity"),
                Some("m"),
                Some(200),
                20,
                Some("antigravity"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
            )?;
            update_proxy_log_hop(conn, &inner, Some("req_abc"), Some("antigravity"))?;
            let count: i64 = conn.query_row(
                &format!(
                    "SELECT COUNT(*) FROM proxy_request_logs l WHERE 1=1 {EFFECTIVE_USAGE_FILTER}"
                ),
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            let hop: String = conn.query_row(
                &format!(
                    "SELECT hop FROM proxy_request_logs l WHERE 1=1 {EFFECTIVE_USAGE_FILTER}"
                ),
                [],
                |row| row.get(0),
            )?;
            assert_eq!(hop, "antigravity");
            let listed = list_proxy_request_logs(conn, &ProxyLogFilters::default(), 0, 20)?;
            assert_eq!(listed.data.len(), 2);
            let counted: Vec<&str> = listed
                .data
                .iter()
                .filter(|row| row.usage_counted)
                .filter_map(|row| row.hop.as_deref())
                .collect();
            assert_eq!(counted, vec!["antigravity"]);
            let transit = listed
                .data
                .iter()
                .find(|row| !row.usage_counted)
                .and_then(|row| row.hop.as_deref());
            assert_eq!(transit, Some("agent_proxy"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn provider_breakdown_converts_mixed_currency_to_usd() {
        let db = crate::database::Database::memory().unwrap();
        db.with_conn(|conn| {
            save_model_pricing(
                conn,
                &ModelPricing {
                    model: "ais-test-usd-mix".to_string(),
                    provider: "test-mix".to_string(),
                    input_price_per_million: 1.0,
                    cache_read_price_per_million: 0.0,
                    cache_write_price_per_million: 0.0,
                    output_price_per_million: 0.0,
                    batch_input_price_per_million: 0.0,
                    batch_output_price_per_million: 0.0,
                    currency: "USD".to_string(),
                    source_url: String::new(),
                    effective_date: String::new(),
                    is_default: false,
                },
            )?;
            save_model_pricing(
                conn,
                &ModelPricing {
                    model: "ais-test-cny-mix".to_string(),
                    provider: "test-mix".to_string(),
                    input_price_per_million: 7.25,
                    cache_read_price_per_million: 0.0,
                    cache_write_price_per_million: 0.0,
                    output_price_per_million: 0.0,
                    batch_input_price_per_million: 0.0,
                    batch_output_price_per_million: 0.0,
                    currency: "CNY".to_string(),
                    source_url: String::new(),
                    effective_date: String::new(),
                    is_default: false,
                },
            )?;
            let now = Utc::now().timestamp_millis();
            insert_proxy_log_with_source(
                conn,
                None,
                now,
                Some("mix-provider"),
                Some("Mix Provider"),
                Some("ais-test-usd-mix"),
                Some(200),
                1_000_000,
                0,
                0,
                0,
                true,
                10,
                Some("claude_code"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
                DATA_SOURCE_PROXY,
                None,
            )?;
            insert_proxy_log_with_source(
                conn,
                None,
                now,
                Some("mix-provider"),
                Some("Mix Provider"),
                Some("ais-test-cny-mix"),
                Some(200),
                1_000_000,
                0,
                0,
                0,
                true,
                10,
                Some("claude_code"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
                DATA_SOURCE_PROXY,
                None,
            )?;
            let rows = get_usage_by_provider_for_target(conn, now - 1, Some("claude_code"))?;
            let mix = rows
                .iter()
                .find(|row| row.key == "Mix Provider")
                .expect("provider row");
            assert_eq!(mix.request_count, 2);
            assert_eq!(mix.currency, "USD");
            // 1 USD + 7.25 CNY (= 1 USD) = 2 USD
            assert!((mix.estimated_cost - 2.0).abs() < 1e-9);

            let trend = get_usage_trend_for_target(
                conn,
                now - 1,
                Some("claude_code"),
                TrendGranularity::Day,
            )?;
            assert_eq!(trend.len(), 1);
            assert_eq!(trend[0].request_count, 2);
            assert_eq!(trend[0].currency, "USD");
            assert!((trend[0].estimated_cost - 2.0).abs() < 1e-9);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn route_mode_stats_count_gateway_hop_and_bill_inner_tokens() {
        let db = crate::database::Database::memory().unwrap();
        db.with_conn(|conn| {
            save_model_pricing(
                conn,
                &ModelPricing {
                    model: "gemini-3.8-flash-high".to_string(),
                    provider: "ag".to_string(),
                    input_price_per_million: 1.0,
                    cache_read_price_per_million: 0.0,
                    cache_write_price_per_million: 0.0,
                    output_price_per_million: 0.0,
                    batch_input_price_per_million: 0.0,
                    batch_output_price_per_million: 0.0,
                    currency: "USD".to_string(),
                    source_url: String::new(),
                    effective_date: String::new(),
                    is_default: false,
                },
            )?;
            let now = Utc::now().timestamp_millis();
            let gateway = insert_proxy_log_with_source(
                conn,
                Some("log_gw"),
                now,
                Some("sgw"),
                Some("Smart Gateway"),
                Some("gemini-3.8-flash-high"),
                Some(200),
                0,
                0,
                0,
                0,
                false,
                10,
                Some("claude_code"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
                DATA_SOURCE_PROXY,
                None,
            )?;
            update_proxy_log_hop(conn, &gateway, Some("corr_mode"), Some("smart_gateway"))?;
            update_proxy_log_route(
                conn,
                &gateway,
                Some("gprof_shared"),
                Some("命中默认模式"),
                0,
                Some("claude.auto"),
                Some("ag"),
                Some("default"),
            )?;
            let inner = insert_proxy_log_with_source(
                conn,
                Some("log_ag"),
                now,
                Some("ag"),
                Some("Antigravity"),
                Some("gemini-3.8-flash-high"),
                Some(200),
                1_000_000,
                0,
                0,
                0,
                true,
                20,
                Some("antigravity"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
                DATA_SOURCE_PROXY,
                None,
            )?;
            update_proxy_log_hop(conn, &inner, Some("corr_mode"), Some("antigravity"))?;

            let stats = list_route_mode_usage_stats(conn, now - 1)?;
            assert_eq!(stats.len(), 1);
            assert_eq!(stats[0].mode_id, "default");
            assert_eq!(stats[0].request_count, 1);
            assert!((stats[0].estimated_cost - 1.0).abs() < 1e-9);
            Ok(())
        })
        .unwrap();
    }
}
