//! Usage dashboard commands backed by proxy request logs.

use chrono::{Duration, Utc};
use serde::Serialize;

use crate::database::dao::proxy_logs::{
    delete_model_pricing as delete_pricing, get_usage_by_model, get_usage_by_provider,
    get_usage_summary, get_usage_trend, list_model_pricing as list_pricing,
    save_model_pricing as save_pricing, ModelPricing, UsageBreakdown, UsageSummary,
    UsageTrendPoint, LogMaintenanceResult, maintain_proxy_logs as maintain_logs,
};
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

/// Apply the proxy-log retention policy and optionally reclaim SQLite space.
#[tauri::command]
pub fn maintain_proxy_logs(retention_days: Option<u32>, max_rows: Option<u32>, vacuum: Option<bool>, state: tauri::State<'_, AppState>) -> AppResult<LogMaintenanceResult> {
    let retention_days = retention_days.unwrap_or(90).clamp(1, 3650);
    let max_rows = max_rows.unwrap_or(100_000).clamp(100, 5_000_000);
    state.db.with_conn(|conn| maintain_logs(conn, retention_days, max_rows, vacuum.unwrap_or(false)))
}
