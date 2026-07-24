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
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub date: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub model: String,
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
    pub currency: String,
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
) -> AppResult<String> {
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
    Ok(id)
}

/// Fill in token counts when they become available in a completed response.
pub fn update_proxy_log_usage(
    conn: &Connection,
    id: &str,
    input_tokens: i64,
    output_tokens: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE proxy_request_logs SET input_tokens = ?, output_tokens = ? WHERE id = ?;",
        params![input_tokens, output_tokens, id],
    )?;
    Ok(())
}

pub fn get_usage_summary(conn: &Connection, since: i64) -> AppResult<UsageSummary> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                    + output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE created_at >= ?;",
        params![since],
        |row| Ok(UsageSummary {
            request_count: row.get(0)?,
            successful_request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            output_tokens: row.get(3)?,
            estimated_cost: row.get(4)?,
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
        "SELECT {grouping}, COUNT(*), COALESCE(SUM(l.input_tokens), 0), COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM(l.input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                  + l.output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= ? GROUP BY {grouping} ORDER BY 2 DESC, 1 ASC;"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![since], |row| {
        Ok(UsageBreakdown {
            key: row.get(0)?,
            request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            output_tokens: row.get(3)?,
            estimated_cost: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_usage_trend(conn: &Connection, since: i64) -> AppResult<Vec<UsageTrendPoint>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch', 'localtime'), COUNT(*),
                COALESCE(SUM(l.input_tokens), 0), COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM(l.input_tokens * COALESCE(p.input_price_per_million, 0) / 1000000.0
                  + l.output_tokens * COALESCE(p.output_price_per_million, 0) / 1000000.0), 0)
         FROM proxy_request_logs l LEFT JOIN model_pricing p ON p.model = l.model
         WHERE l.created_at >= ? GROUP BY 1 ORDER BY 1 ASC;",
    )?;
    let rows = stmt.query_map(params![since], |row| {
        Ok(UsageTrendPoint {
            date: row.get(0)?,
            request_count: row.get(1)?,
            input_tokens: row.get(2)?,
            output_tokens: row.get(3)?,
            estimated_cost: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn list_model_pricing(conn: &Connection) -> AppResult<Vec<ModelPricing>> {
    let mut stmt = conn.prepare(
        "SELECT model, input_price_per_million, output_price_per_million, currency
         FROM model_pricing ORDER BY model COLLATE NOCASE;",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ModelPricing {
            model: row.get(0)?,
            input_price_per_million: row.get(1)?,
            output_price_per_million: row.get(2)?,
            currency: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn save_model_pricing(conn: &Connection, pricing: &ModelPricing) -> AppResult<()> {
    conn.execute(
        "INSERT INTO model_pricing (model, input_price_per_million, output_price_per_million, currency)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(model) DO UPDATE SET input_price_per_million = excluded.input_price_per_million,
             output_price_per_million = excluded.output_price_per_million, currency = excluded.currency;",
        params![pricing.model, pricing.input_price_per_million, pricing.output_price_per_million, pricing.currency],
    )?;
    Ok(())
}

pub fn delete_model_pricing(conn: &Connection, model: &str) -> AppResult<()> {
    conn.execute("DELETE FROM model_pricing WHERE model = ?;", params![model])?;
    Ok(())
}
