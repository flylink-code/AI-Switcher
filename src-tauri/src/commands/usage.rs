//! Usage dashboard commands backed by proxy request logs.

use chrono::{Duration, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use calamine::{open_workbook_auto, Reader};
use rust_xlsxwriter::Workbook;

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
    #[serde(default)]
    pub provider: String,
    pub input_price_per_million: f64,
    #[serde(default)]
    pub cache_read_price_per_million: f64,
    #[serde(default)]
    pub cache_write_price_per_million: f64,
    pub output_price_per_million: f64,
    #[serde(default)]
    pub batch_input_price_per_million: f64,
    #[serde(default)]
    pub batch_output_price_per_million: f64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingImportPreview {
    pub new_models: Vec<String>,
    pub updated_models: Vec<String>,
    pub errors: Vec<String>,
    pub valid_rows: usize,
}

const PRICING_COLUMNS: [&str; 9] = [
    "model", "provider", "inputPricePerMillion", "cacheReadPricePerMillion",
    "cacheWritePricePerMillion", "outputPricePerMillion", "batchInputPricePerMillion",
    "batchOutputPricePerMillion", "currency",
];

/// Stable, release-bundled catalog metadata. The individual entries are
/// returned by `list_model_pricing`; this keeps source and date information
/// available without querying a third-party pricing endpoint at runtime.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingCatalog {
    pub version: String,
    pub entries: Vec<ModelPricing>,
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
    let days = days.unwrap_or(365).clamp(1, 365);
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
pub fn get_pricing_catalog(state: tauri::State<'_, AppState>) -> AppResult<PricingCatalog> {
    state.db.with_conn(|conn| Ok(PricingCatalog {
        version: "2026-07-28".to_string(),
        entries: list_pricing(conn)?,
    }))
}

#[tauri::command]
pub fn save_model_pricing(input: ModelPricingInput, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let pricing = normalize_pricing_input(input)?;
    state.db.with_conn(|conn| save_pricing(conn, &pricing))
}

#[tauri::command]
pub fn export_model_pricing_xlsx(destination_path: String, state: tauri::State<'_, AppState>) -> AppResult<String> {
    let destination = Path::new(&destination_path);
    if destination.extension().and_then(|value| value.to_str()).is_none_or(|value| !value.eq_ignore_ascii_case("xlsx")) {
        return Err(AppError::Config("导出文件必须使用 .xlsx 扩展名".to_string()));
    }
    let pricing = state.db.with_conn(list_pricing)?;
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Model Pricing").map_err(|error| AppError::Other(format!("设置 Excel 工作表名称失败: {error}")))?;
    for (column, header) in PRICING_COLUMNS.iter().enumerate() {
        worksheet.write_string(0, column as u16, *header).map_err(excel_write_error)?;
    }
    for (index, entry) in pricing.iter().enumerate() {
        let row = (index + 1) as u32;
        worksheet.write_string(row, 0, &entry.model).map_err(excel_write_error)?;
        worksheet.write_string(row, 1, &entry.provider).map_err(excel_write_error)?;
        worksheet.write_number(row, 2, entry.input_price_per_million).map_err(excel_write_error)?;
        worksheet.write_number(row, 3, entry.cache_read_price_per_million).map_err(excel_write_error)?;
        worksheet.write_number(row, 4, entry.cache_write_price_per_million).map_err(excel_write_error)?;
        worksheet.write_number(row, 5, entry.output_price_per_million).map_err(excel_write_error)?;
        worksheet.write_number(row, 6, entry.batch_input_price_per_million).map_err(excel_write_error)?;
        worksheet.write_number(row, 7, entry.batch_output_price_per_million).map_err(excel_write_error)?;
        worksheet.write_string(row, 8, &entry.currency).map_err(excel_write_error)?;
    }
    workbook.save(destination).map_err(|error| AppError::Other(format!("导出 Excel 失败: {error}")))?;
    Ok(destination_path)
}

fn excel_write_error(error: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Other(format!("写入 Excel 失败: {error}"))
}

#[tauri::command]
pub fn preview_model_pricing_xlsx(source_path: String, state: tauri::State<'_, AppState>) -> AppResult<PricingImportPreview> {
    let parsed = parse_pricing_xlsx(Path::new(&source_path))?;
    preview_pricing_rows(&state.db, &parsed.rows, parsed.errors)
}

#[tauri::command]
pub fn import_model_pricing_xlsx(source_path: String, state: tauri::State<'_, AppState>) -> AppResult<PricingImportPreview> {
    let parsed = parse_pricing_xlsx(Path::new(&source_path))?;
    if !parsed.errors.is_empty() {
        return Err(AppError::Config(format!("Excel 包含 {} 行无效数据，请修正后重新导入", parsed.errors.len())));
    }
    let preview = preview_pricing_rows(&state.db, &parsed.rows, Vec::new())?;
    state.db.with_conn_mut(|conn| {
        let transaction = conn.transaction()?;
        for entry in &parsed.rows {
            save_pricing(&transaction, entry)?;
        }
        transaction.commit()?;
        Ok(())
    })?;
    Ok(preview)
}

struct ParsedPricingImport { rows: Vec<ModelPricing>, errors: Vec<String> }

fn preview_pricing_rows(db: &crate::database::Database, rows: &[ModelPricing], errors: Vec<String>) -> AppResult<PricingImportPreview> {
    let existing = db.with_conn(list_pricing)?.into_iter().map(|entry| entry.model).collect::<BTreeSet<_>>();
    Ok(PricingImportPreview {
        new_models: rows.iter().filter(|entry| !existing.contains(&entry.model)).map(|entry| entry.model.clone()).collect(),
        updated_models: rows.iter().filter(|entry| existing.contains(&entry.model)).map(|entry| entry.model.clone()).collect(),
        errors,
        valid_rows: rows.len(),
    })
}

fn parse_pricing_xlsx(path: &Path) -> AppResult<ParsedPricingImport> {
    if !path.is_file() || path.extension().and_then(|value| value.to_str()).is_none_or(|value| !value.eq_ignore_ascii_case("xlsx")) {
        return Err(AppError::Config("请选择有效的 .xlsx 定价文件".to_string()));
    }
    let mut workbook = open_workbook_auto(path).map_err(|error| AppError::Config(format!("无法读取 Excel 文件: {error}")))?;
    let range = workbook.worksheet_range_at(0).ok_or_else(|| AppError::Config("Excel 文件不包含工作表".to_string()))?
        .map_err(|error| AppError::Config(format!("无法读取 Excel 工作表: {error}")))?;
    let mut spreadsheet_rows = range.rows();
    let headers = spreadsheet_rows.next().ok_or_else(|| AppError::Config("Excel 文件缺少表头".to_string()))?;
    let header_values = headers.iter().map(|cell| cell.to_string().trim().to_string()).collect::<Vec<_>>();
    if header_values != PRICING_COLUMNS { return Err(AppError::Config(format!("Excel 表头必须依次为: {}", PRICING_COLUMNS.join(", ")))); }
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut models = BTreeSet::new();
    for (offset, row) in spreadsheet_rows.enumerate() {
        let values = (0..PRICING_COLUMNS.len()).map(|index| row.get(index).map(ToString::to_string).unwrap_or_default().trim().to_string()).collect::<Vec<_>>();
        if values.iter().all(String::is_empty) { continue; }
        match spreadsheet_row_to_pricing(&values) {
            Ok(entry) if models.insert(entry.model.clone()) => rows.push(entry),
            Ok(entry) => errors.push(format!("第 {} 行: 模型 {} 重复", offset + 2, entry.model)),
            Err(error) => errors.push(format!("第 {} 行: {error}", offset + 2)),
        }
    }
    Ok(ParsedPricingImport { rows, errors })
}

fn spreadsheet_row_to_pricing(values: &[String]) -> AppResult<ModelPricing> {
    let price = |index: usize, name: &str, required: bool| -> AppResult<f64> {
        let value = values.get(index).map(String::as_str).unwrap_or("").trim();
        if value.is_empty() && !required { return Ok(0.0); }
        let parsed = value.parse::<f64>().map_err(|_| AppError::Config(format!("{name} 必须是数字")))?;
        if !parsed.is_finite() || parsed < 0.0 { return Err(AppError::Config(format!("{name} 不能为负数"))); }
        Ok(parsed)
    };
    normalize_pricing_input(ModelPricingInput {
        model: values.first().cloned().unwrap_or_default(), provider: values.get(1).cloned().unwrap_or_default(),
        input_price_per_million: price(2, "inputPricePerMillion", true)?, cache_read_price_per_million: price(3, "cacheReadPricePerMillion", false)?,
        cache_write_price_per_million: price(4, "cacheWritePricePerMillion", false)?, output_price_per_million: price(5, "outputPricePerMillion", true)?,
        batch_input_price_per_million: price(6, "batchInputPricePerMillion", false)?, batch_output_price_per_million: price(7, "batchOutputPricePerMillion", false)?,
        currency: values.get(8).cloned().unwrap_or_default(),
    })
}

fn normalize_pricing_input(input: ModelPricingInput) -> AppResult<ModelPricing> {
    let model = input.model.trim();
    if model.is_empty() { return Err(AppError::Config("模型名不能为空".to_string())); }
    if [input.input_price_per_million, input.cache_read_price_per_million, input.cache_write_price_per_million, input.output_price_per_million, input.batch_input_price_per_million, input.batch_output_price_per_million].iter().any(|price| !price.is_finite() || *price < 0.0) {
        return Err(AppError::Config("模型价格不能为负数".to_string()));
    }
    Ok(ModelPricing { model: model.to_string(), provider: input.provider.trim().to_string(), input_price_per_million: input.input_price_per_million, cache_read_price_per_million: input.cache_read_price_per_million, cache_write_price_per_million: input.cache_write_price_per_million, output_price_per_million: input.output_price_per_million, batch_input_price_per_million: input.batch_input_price_per_million, batch_output_price_per_million: input.batch_output_price_per_million, currency: if input.currency.trim().is_empty() { "USD".to_string() } else { input.currency.trim().to_string() }, source_url: String::new(), effective_date: String::new(), is_default: false })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn pricing_import_defaults_optional_prices_and_currency() {
        let pricing = spreadsheet_row_to_pricing(&row(&[
            "example-model", "Example", "2", "", "", "8", "", "", "",
        ])).unwrap();
        assert_eq!(pricing.model, "example-model");
        assert_eq!(pricing.cache_read_price_per_million, 0.0);
        assert_eq!(pricing.batch_output_price_per_million, 0.0);
        assert_eq!(pricing.currency, "USD");
    }

    #[test]
    fn pricing_import_rejects_invalid_numeric_values() {
        let error = spreadsheet_row_to_pricing(&row(&[
            "example-model", "Example", "-1", "", "", "8", "", "", "USD",
        ])).unwrap_err();
        assert!(error.to_string().contains("不能为负数"));
    }
}
