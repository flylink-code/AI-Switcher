//! Usage dashboard commands backed by proxy request logs.

use chrono::{Duration, Utc};
use serde::Serialize;
use std::sync::Arc;

use crate::database::dao::proxy_logs::{
    delete_model_pricing as delete_pricing, get_usage_by_model, get_usage_by_provider,
    get_usage_summary, get_usage_trend, list_model_pricing as list_pricing,
    list_proxy_request_logs, save_model_pricing as save_pricing, ModelPricing, PaginatedProxyLogs,
    ProxyLogFilters, UsageBreakdown, UsageSummary, UsageTrendPoint, LogMaintenancePreview,
    LogMaintenanceResult, maintain_proxy_logs as maintain_logs,
    preview_proxy_log_maintenance as preview_logs,
};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::store::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboard {
    pub summary: UsageSummary,
    pub by_provider: Vec<UsageBreakdown>,
    pub by_model: Vec<UsageBreakdown>,
    pub trend: Vec<UsageTrendPoint>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInput {
    pub model: String,
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
    pub currency: String,
}

const LOG_RETENTION_DAYS_KEY: &str = "proxy_log_retention_days";
const LOG_MAX_ROWS_KEY: &str = "proxy_log_max_rows";
const LOG_AUTO_MAINTAIN_KEY: &str = "proxy_log_auto_maintain";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMaintenancePolicy {
    pub retention_days: u32,
    pub max_rows: u32,
    pub auto_maintain: bool,
}

impl Default for LogMaintenancePolicy {
    fn default() -> Self {
        Self { retention_days: 90, max_rows: 100_000, auto_maintain: false }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyLogListInput {
    pub days: Option<u32>,
    pub target_app: Option<String>,
    pub status_code: Option<i64>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[tauri::command]
pub fn list_proxy_request_logs_cmd(
    input: ProxyLogListInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<PaginatedProxyLogs> {
    let days = input.days.unwrap_or(30).clamp(1, 365);
    let since = (Utc::now() - Duration::days(i64::from(days))).timestamp_millis();
    let page = input.page.unwrap_or(0);
    let page_size = input.page_size.unwrap_or(20);
    let filters = ProxyLogFilters {
        since: Some(since),
        target_app: input.target_app,
        status_code: input.status_code,
    };
    state
        .db
        .with_conn(|conn| list_proxy_request_logs(conn, &filters, page, page_size))
}

#[tauri::command]
pub fn get_usage_dashboard(days: Option<u32>, state: tauri::State<'_, AppState>) -> AppResult<UsageDashboard> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let since = (Utc::now() - Duration::days(i64::from(days))).timestamp_millis();
    state.db.with_conn(|conn| {
        Ok(UsageDashboard {
            summary: get_usage_summary(conn, since)?,
            by_provider: get_usage_by_provider(conn, since)?,
            by_model: get_usage_by_model(conn, since)?,
            trend: get_usage_trend(conn, since)?,
        })
    })
}

#[tauri::command]
pub fn list_model_pricing(state: tauri::State<'_, AppState>) -> AppResult<Vec<ModelPricing>> {
    state.db.with_conn(list_pricing)
}

#[tauri::command]
pub fn save_model_pricing(input: ModelPricingInput, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let model = input.model.trim();
    if model.is_empty() {
        return Err(AppError::Config("模型名不能为空".to_string()));
    }
    if input.input_price_per_million < 0.0 || input.output_price_per_million < 0.0 {
        return Err(AppError::Config("模型价格不能为负数".to_string()));
    }
    state.db.with_conn(|conn| {
        save_pricing(conn, &ModelPricing {
            model: model.to_string(),
            input_price_per_million: input.input_price_per_million,
            output_price_per_million: input.output_price_per_million,
            currency: if input.currency.trim().is_empty() { "USD".to_string() } else { input.currency },
        })
    })
}

#[tauri::command]
pub fn delete_model_pricing(model: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| delete_pricing(conn, &model))
}

#[tauri::command]
pub fn get_log_maintenance_policy(state: tauri::State<'_, AppState>) -> AppResult<LogMaintenancePolicy> {
    state.db.with_conn(load_log_maintenance_policy)
}

#[tauri::command]
pub fn save_log_maintenance_policy(policy: LogMaintenancePolicy, state: tauri::State<'_, AppState>) -> AppResult<LogMaintenancePolicy> {
    let policy = normalize_log_maintenance_policy(policy);
    state.db.with_conn(|conn| {
        set_setting(conn, LOG_RETENTION_DAYS_KEY, &policy.retention_days.to_string())?;
        set_setting(conn, LOG_MAX_ROWS_KEY, &policy.max_rows.to_string())?;
        set_setting(conn, LOG_AUTO_MAINTAIN_KEY, if policy.auto_maintain { "true" } else { "false" })?;
        Ok(policy)
    })
}

#[tauri::command]
pub fn preview_proxy_log_maintenance(policy: Option<LogMaintenancePolicy>, state: tauri::State<'_, AppState>) -> AppResult<LogMaintenancePreview> {
    state.db.with_conn(|conn| {
        let policy = policy.map(normalize_log_maintenance_policy)
            .unwrap_or(load_log_maintenance_policy(conn)?);
        preview_logs(conn, policy.retention_days, policy.max_rows)
    })
}

/// Apply the persisted proxy-log retention policy and optionally reclaim SQLite space.
#[tauri::command]
pub async fn maintain_proxy_logs(vacuum: Option<bool>, state: tauri::State<'_, AppState>) -> AppResult<LogMaintenanceResult> {
    let db = Arc::clone(&state.db);
    tokio::task::spawn_blocking(move || db.with_conn(|conn| {
        let policy = load_log_maintenance_policy(conn)?;
        maintain_logs(conn, policy.retention_days, policy.max_rows, vacuum.unwrap_or(false))
    }))
    .await
    .map_err(|error| AppError::Other(format!("日志维护任务异常结束: {error}")))?
}

fn load_log_maintenance_policy(conn: &rusqlite::Connection) -> AppResult<LogMaintenancePolicy> {
    let defaults = LogMaintenancePolicy::default();
    let policy = LogMaintenancePolicy {
        retention_days: get_setting(conn, LOG_RETENTION_DAYS_KEY)?.and_then(|value| value.parse().ok()).unwrap_or(defaults.retention_days),
        max_rows: get_setting(conn, LOG_MAX_ROWS_KEY)?.and_then(|value| value.parse().ok()).unwrap_or(defaults.max_rows),
        auto_maintain: get_setting(conn, LOG_AUTO_MAINTAIN_KEY)?.as_deref() == Some("true"),
    };
    Ok(normalize_log_maintenance_policy(policy))
}

fn normalize_log_maintenance_policy(mut policy: LogMaintenancePolicy) -> LogMaintenancePolicy {
    policy.retention_days = policy.retention_days.clamp(1, 3650);
    policy.max_rows = policy.max_rows.clamp(100, 5_000_000);
    policy
}
