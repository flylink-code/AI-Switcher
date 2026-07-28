//! Request-log persistence and usage-statistic queries for the local proxy.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppResult;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub request_count: i64,
    pub successful_request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub date: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
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
    let id = format!("log_{}", Uuid::new_v4().simple());
    conn.execute(
        "INSERT INTO proxy_request_logs
            (id, created_at, provider_id, provider_name, model, status_code, input_tokens, output_tokens, duration_ms,
             target_app, protocol, route, is_stream, error_category, diagnostic)
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?, ?);",
        params![
            id,
            Utc::now().timestamp_millis(),
            provider_id,
            provider_name,
            model,
            status_code,
            duration_ms,
            target_app,
            protocol,
            route,
            is_stream,
            error_category,
            diagnostic,
        ],
    )?;
    Ok(id)
}

/// Fill in token counts when they become available in a completed response.
pub fn update_proxy_log_usage(
    conn: &Connection,
    id: &str,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
) -> AppResult<()> {
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

pub fn get_usage_summary(conn: &Connection, since: i64) -> AppResult<UsageSummary> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(cache_read_input_tokens), 0),
                COALESCE(SUM(cache_creation_input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(CASE WHEN p.currency = 'USD' THEN
                    input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                    + cache_read_input_tokens * COALESCE(p.cache_read_price_per_million, 0) / 1000000.0
                    + cache_creation_input_tokens * COALESCE(p.cache_write_price_per_million, 0) / 1000000.0
                    + output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0
                    ELSE 0 END), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE created_at >= ?;",
        params![since],
        |row| Ok(UsageSummary {
            request_count: row.get(0)?,
            successful_request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            cache_read_input_tokens: row.get(3)?,
            cache_creation_input_tokens: row.get(4)?,
            output_tokens: row.get(5)?,
            estimated_cost: row.get(6)?,
        }),
    )
    .map_err(Into::into)
}

pub fn get_usage_by_provider(conn: &Connection, since: i64) -> AppResult<Vec<UsageBreakdown>> {
    usage_breakdown(conn, since, "COALESCE(l.provider_name, 'Unknown')")
}

pub fn get_usage_by_model(conn: &Connection, since: i64) -> AppResult<Vec<UsageBreakdown>> {
    usage_breakdown(conn, since, "COALESCE(l.model, 'Unknown')")
}

fn usage_breakdown(conn: &Connection, since: i64, grouping: &str) -> AppResult<Vec<UsageBreakdown>> {
    let sql = format!(
        "SELECT {grouping}, COUNT(*), COALESCE(SUM(l.input_tokens), 0),
                COALESCE(SUM(l.cache_read_input_tokens), 0),
                COALESCE(SUM(l.cache_creation_input_tokens), 0),
                COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM(CASE WHEN p.currency = 'USD' THEN
                    l.input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                    + l.cache_read_input_tokens * COALESCE(p.cache_read_price_per_million, 0) / 1000000.0
                    + l.cache_creation_input_tokens * COALESCE(p.cache_write_price_per_million, 0) / 1000000.0
                    + l.output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0
                    ELSE 0 END), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= ? GROUP BY {grouping} ORDER BY 2 DESC, 1 ASC;"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![since], |row| {
        Ok(UsageBreakdown {
            key: row.get(0)?,
            request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            cache_read_input_tokens: row.get(3)?,
            cache_creation_input_tokens: row.get(4)?,
            output_tokens: row.get(5)?,
            estimated_cost: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_usage_trend(conn: &Connection, since: i64) -> AppResult<Vec<UsageTrendPoint>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch', 'localtime'), COUNT(*),
                COALESCE(SUM(l.input_tokens), 0),
                COALESCE(SUM(l.cache_read_input_tokens), 0),
                COALESCE(SUM(l.cache_creation_input_tokens), 0),
                COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM(CASE WHEN p.currency = 'USD' THEN
                    l.input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                    + l.cache_read_input_tokens * COALESCE(p.cache_read_price_per_million, 0) / 1000000.0
                    + l.cache_creation_input_tokens * COALESCE(p.cache_write_price_per_million, 0) / 1000000.0
                    + l.output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0
                    ELSE 0 END), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= ? GROUP BY 1 ORDER BY 1 ASC;",
    )?;
    let rows = stmt.query_map(params![since], |row| {
        Ok(UsageTrendPoint {
            date: row.get(0)?,
            request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            cache_read_input_tokens: row.get(3)?,
            cache_creation_input_tokens: row.get(4)?,
            output_tokens: row.get(5)?,
            estimated_cost: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedProxyLogs {
    pub data: Vec<ProxyRequestLog>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Default)]
pub struct ProxyLogFilters {
    pub since: Option<i64>,
    pub target_app: Option<String>,
    pub status_code: Option<i64>,
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
        conditions.push("created_at >= ?".to_string());
        params.push(Box::new(since));
    }
    if let Some(ref target_app) = filters.target_app {
        conditions.push("target_app = ?".to_string());
        params.push(Box::new(target_app.clone()));
    }
    if let Some(status_code) = filters.status_code {
        conditions.push("status_code = ?".to_string());
        params.push(Box::new(status_code));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM proxy_request_logs {where_clause}");
    let count_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let total: i64 = conn.query_row(&count_sql, count_params.as_slice(), |row| row.get(0))?;

    let data_sql = format!(
        "SELECT id, created_at, provider_id, provider_name, model, status_code,
                input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                output_tokens, usage_available, duration_ms, target_app, protocol, route,
                is_stream, error_category, diagnostic
         FROM proxy_request_logs
         {where_clause}
         ORDER BY created_at DESC
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
