//! Usage dashboard commands backed by proxy request logs.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use calamine::{open_workbook_auto, Reader};
use rust_xlsxwriter::Workbook;

use crate::database::dao::proxy_logs::{
    delete_model_pricing as delete_pricing, get_usage_by_model_for_target,
    get_usage_by_provider_for_target, get_usage_summary_for_target,
    get_usage_trend_for_target, list_model_pricing as list_pricing,
    list_proxy_request_logs, save_model_pricing as save_pricing, ModelPricing, PaginatedProxyLogs,
    ProxyLogFilters, UsageBreakdown, UsageSummary, UsageTrendPoint, LogMaintenancePreview,
    LogMaintenanceResult, maintain_proxy_logs as maintain_logs,
    preview_proxy_log_maintenance as preview_logs,
};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const CODEX_LOCAL_PROVIDER_KEY: &str = "Codex local events";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageSource {
    All,
    ClaudeCode,
    ClaudeDesktop,
    Codex,
}

impl UsageSource {
    fn parse(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("all") {
            "all" => Ok(Self::All),
            "claude_code" => Ok(Self::ClaudeCode),
            "claude_desktop" => Ok(Self::ClaudeDesktop),
            "codex" => Ok(Self::Codex),
            _ => Err(AppError::Config("未知的用量来源筛选".to_string())),
        }
    }

    fn proxy_target(self) -> Option<Option<&'static str>> {
        match self {
            Self::All => Some(None),
            Self::ClaudeCode => Some(Some("claude_code")),
            Self::ClaudeDesktop => Some(Some("claude_desktop")),
            // Codex rows live in proxy_request_logs (proxy HTTP + session sync).
            Self::Codex => Some(Some("codex")),
        }
    }

    fn includes_local_codex(self) -> bool {
        matches!(self, Self::All | Self::Codex)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboard {
    pub summary: UsageSummary,
    pub by_provider: Vec<UsageBreakdown>,
    pub by_model: Vec<UsageBreakdown>,
    pub trend: Vec<UsageTrendPoint>,
    pub local_codex: LocalCodexUsage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCodexUsage {
    pub available: bool,
    pub session_count: i64,
    pub event_count: i64,
    pub message: String,
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
    pub hours: Option<u32>,
    pub today: Option<bool>,
    pub target_app: Option<String>,
    pub status_code: Option<i64>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

fn resolve_usage_since(days: Option<u32>, hours: Option<u32>, today: Option<bool>) -> i64 {
    if today.unwrap_or(false) {
        return local_midnight_millis();
    }
    if let Some(hours) = hours {
        let hours = hours.clamp(1, 24 * 365);
        return (Utc::now() - Duration::hours(i64::from(hours))).timestamp_millis();
    }
    let days = days.unwrap_or(365).clamp(1, 365);
    (Utc::now() - Duration::days(i64::from(days))).timestamp_millis()
}

fn local_midnight_millis() -> i64 {
    use chrono::Local;
    let today = Local::now().date_naive();
    today
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|| (Utc::now() - Duration::days(1)).timestamp_millis())
}

#[tauri::command]
pub async fn list_proxy_request_logs_cmd(
    input: ProxyLogListInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<PaginatedProxyLogs> {
    let since = resolve_usage_since(input.days, input.hours, input.today);
    let page = input.page.unwrap_or(0);
    let page_size = input.page_size.unwrap_or(20);
    let filters = ProxyLogFilters {
        since: Some(since),
        target_app: input.target_app,
        status_code: input.status_code,
    };
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        db.with_conn(|conn| list_proxy_request_logs(conn, &filters, page, page_size))
    })
    .await
    .map_err(|e| AppError::Database(format!("list proxy logs task failed: {e}")))?
}

#[tauri::command]
pub async fn get_usage_dashboard(
    days: Option<u32>,
    hours: Option<u32>,
    today: Option<bool>,
    source: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<UsageDashboard> {
    let since = resolve_usage_since(days, hours, today);
    let source = UsageSource::parse(source.as_deref())?;
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        db.with_conn(|conn| {
            let local_codex = if source.includes_local_codex() {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM proxy_request_logs
                     WHERE target_app = 'codex' AND created_at >= ?
                       AND COALESCE(data_source, 'proxy') IN ('proxy', 'codex_session');",
                    rusqlite::params![since],
                    |row| row.get(0),
                )?;
                let session_files: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM session_log_sync;",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                LocalCodexUsage {
                    available: count > 0,
                    session_count: session_files,
                    event_count: count,
                    message: if count > 0 {
                        "Codex usage includes local-proxy HTTP logs and synced session token events".to_string()
                    } else {
                        "No Codex usage rows yet; session sync runs in the background or via Sync".to_string()
                    },
                }
            } else {
                local_codex_not_selected()
            };
            let target = source.proxy_target();
            Ok(if let Some(target) = target {
                UsageDashboard {
                    summary: get_usage_summary_for_target(conn, since, target)?,
                    by_provider: get_usage_by_provider_for_target(conn, since, target)?,
                    by_model: get_usage_by_model_for_target(conn, since, target)?,
                    trend: get_usage_trend_for_target(conn, since, target)?,
                    local_codex,
                }
            } else {
                UsageDashboard {
                    summary: empty_summary(),
                    by_provider: Vec::new(),
                    by_model: Vec::new(),
                    trend: Vec::new(),
                    local_codex,
                }
            })
        })
    })
    .await
    .map_err(|e| AppError::Database(format!("usage dashboard task failed: {e}")))?
}

#[tauri::command]
pub async fn sync_codex_session_usage_cmd(
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::usage::session_usage_codex::CodexSessionSyncResult> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        crate::usage::session_usage_codex::try_sync_codex_session_usage_db(&db)
    })
    .await
    .map_err(|e| AppError::Database(format!("codex session sync task failed: {e}")))?
}

#[tauri::command]
pub async fn rebuild_codex_session_usage_cmd(
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::usage::session_usage_codex::CodexSessionSyncResult> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        crate::usage::session_usage_codex::rebuild_codex_session_usage_db(&db)
    })
    .await
    .map_err(|e| AppError::Database(format!("codex session rebuild task failed: {e}")))?
}

#[derive(Debug)]
struct LocalCodexAggregation {
    status: LocalCodexUsage,
    summary: UsageSummary,
    by_model: BTreeMap<String, UsageBreakdown>,
    trend: BTreeMap<String, UsageTrendPoint>,
}

#[derive(Default, Clone, Copy)]
struct TokenTotals { input: i64, cached: i64, output: i64 }

fn collect_codex_local_usage(since: i64) -> LocalCodexAggregation {
    let config_dir = crate::config::get_codex_config_dir();
    collect_codex_local_usage_from_roots(
        &[
            config_dir.join("sessions"),
            config_dir.join("archived_sessions"),
        ],
        since,
    )
}

fn collect_codex_local_usage_from_root(root: &Path, since: i64) -> LocalCodexAggregation {
    collect_codex_local_usage_from_roots(&[root.to_path_buf()], since)
}

fn collect_codex_local_usage_from_roots(roots: &[PathBuf], since: i64) -> LocalCodexAggregation {
    let mut aggregation = LocalCodexAggregation {
        status: LocalCodexUsage {
            available: false,
            session_count: 0,
            event_count: 0,
            message: "No parseable local Codex token events".to_string(),
        },
        summary: empty_summary(),
        by_model: BTreeMap::new(),
        trend: BTreeMap::new(),
    };
    let mut files = Vec::new();
    let mut any_root = false;
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        any_root = true;
        collect_codex_jsonl_files(root, &mut files);
    }
    if !any_root {
        aggregation.status.message = "Codex local session directory was not found".to_string();
        return aggregation;
    }
    aggregation.status.session_count = files.len() as i64;
    for path in files {
        collect_codex_file_usage(&path, since, &mut aggregation);
    }
    aggregation.status.available = aggregation.status.event_count > 0;
    if aggregation.status.available {
        aggregation.status.message = "Token totals are reconstructed from local Codex session events, not HTTP or billing records".to_string();
    }
    aggregation
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

fn collect_codex_file_usage(path: &Path, since: i64, aggregation: &mut LocalCodexAggregation) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut model = "unknown".to_string();
    let mut prev_total: Option<TokenTotals> = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(next) = extract_model_name(value.pointer("/payload")) {
                model = next;
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("event_msg")
            || value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
        {
            continue;
        }
        let timestamp = value
            .get("timestamp")
            .and_then(parse_event_timestamp)
            .unwrap_or(0);
        let info = value.pointer("/payload/info");
        if let Some(next) = extract_model_name(info) {
            model = next;
        }
        let Some(delta) = compute_codex_token_delta(info, &mut prev_total) else {
            continue;
        };
        if timestamp < since || (delta.input == 0 && delta.cached == 0 && delta.output == 0) {
            continue;
        }
        let cached = delta.cached.min(delta.input);
        let fresh_input = delta.input.saturating_sub(cached);
        aggregation.status.event_count += 1;
        aggregation.summary.request_count += 1;
        aggregation.summary.successful_request_count += 1;
        aggregation.summary.input_tokens += fresh_input;
        aggregation.summary.cache_read_input_tokens += cached;
        aggregation.summary.output_tokens += delta.output;
        let model_key = normalize_model_name(&model);
        let model_item = aggregation
            .by_model
            .entry(model_key.clone())
            .or_insert_with(|| empty_breakdown(model_key));
        add_breakdown(model_item, fresh_input, cached, delta.output);
        let date = DateTime::from_timestamp_millis(timestamp)
            .unwrap_or_else(Utc::now)
            .format("%Y-%m-%d")
            .to_string();
        let trend = aggregation
            .trend
            .entry(date.clone())
            .or_insert_with(|| empty_trend(date));
        add_trend(trend, fresh_input, cached, delta.output);
    }
}

/// Prefer cumulative `total_token_usage` (diff vs previous total). Otherwise treat
/// `last_token_usage` as an already-incremental turn delta and do not touch `prev_total`.
fn compute_codex_token_delta(
    info: Option<&Value>,
    prev_total: &mut Option<TokenTotals>,
) -> Option<TokenTotals> {
    let info = info?;
    if let Some(total) = info.get("total_token_usage").and_then(read_token_totals) {
        let previous = prev_total.unwrap_or_default();
        let delta = TokenTotals {
            input: total.input.saturating_sub(previous.input),
            cached: total.cached.saturating_sub(previous.cached),
            output: total.output.saturating_sub(previous.output),
        };
        *prev_total = Some(total);
        return Some(delta);
    }
    info.get("last_token_usage").and_then(read_token_totals)
}

fn extract_model_name(value: Option<&Value>) -> Option<String> {
    let value = value?;
    ["model", "model_name", "modelName"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(normalize_model_name)
}

fn normalize_model_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    trimmed.to_string()
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

fn empty_summary() -> UsageSummary {
    UsageSummary {
        request_count: 0,
        successful_request_count: 0,
        input_tokens: 0,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        estimated_cost: 0.0,
    }
}
fn local_codex_not_selected() -> LocalCodexUsage {
    LocalCodexUsage {
        available: false,
        session_count: 0,
        event_count: 0,
        message: "Codex local events are excluded by the current source filter".to_string(),
    }
}
fn empty_breakdown(key: String) -> UsageBreakdown {
    UsageBreakdown {
        key,
        request_count: 0,
        input_tokens: 0,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        estimated_cost: 0.0,
    }
}
fn empty_trend(date: String) -> UsageTrendPoint {
    UsageTrendPoint {
        date,
        request_count: 0,
        input_tokens: 0,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        estimated_cost: 0.0,
    }
}
fn add_breakdown(item: &mut UsageBreakdown, input: i64, cached: i64, output: i64) {
    item.request_count += 1;
    item.input_tokens += input;
    item.cache_read_input_tokens += cached;
    item.output_tokens += output;
}
fn add_trend(item: &mut UsageTrendPoint, input: i64, cached: i64, output: i64) {
    item.request_count += 1;
    item.input_tokens += input;
    item.cache_read_input_tokens += cached;
    item.output_tokens += output;
}

fn estimate_token_cost(
    pricing: &ModelPricing,
    input: i64,
    cache_read: i64,
    cache_write: i64,
    output: i64,
) -> f64 {
    if !pricing.currency.eq_ignore_ascii_case("USD") {
        return 0.0;
    }
    (input as f64) * pricing.input_price_per_million / 1_000_000.0
        + (cache_read as f64) * pricing.cache_read_price_per_million / 1_000_000.0
        + (cache_write as f64) * pricing.cache_write_price_per_million / 1_000_000.0
        + (output as f64) * pricing.output_price_per_million / 1_000_000.0
}

fn find_pricing_for_model<'a>(
    pricing: &'a [ModelPricing],
    model: &str,
) -> Option<&'a ModelPricing> {
    pricing
        .iter()
        .find(|entry| entry.model == model)
        .or_else(|| {
            pricing.iter().find(|entry| {
                model.starts_with(&entry.model) || entry.model.starts_with(model)
            })
        })
}

fn apply_codex_estimated_cost(local: &mut LocalCodexAggregation, pricing: &[ModelPricing]) {
    let mut total_cost = 0.0;
    for item in local.by_model.values_mut() {
        let cost = find_pricing_for_model(pricing, &item.key)
            .map(|entry| {
                estimate_token_cost(
                    entry,
                    item.input_tokens,
                    item.cache_read_input_tokens,
                    item.cache_creation_input_tokens,
                    item.output_tokens,
                )
            })
            .unwrap_or(0.0);
        item.estimated_cost = cost;
        total_cost += cost;
    }
    local.summary.estimated_cost = total_cost;
    for item in local.trend.values_mut() {
        // Trend rows are not model-split; leave cost at 0 unless a single model covers all.
        if local.by_model.len() == 1 {
            if let Some(model) = local.by_model.values().next() {
                let share = if model.request_count > 0 {
                    item.request_count as f64 / model.request_count as f64
                } else {
                    0.0
                };
                item.estimated_cost = model.estimated_cost * share;
            }
        }
    }
}

fn merge_local_codex_usage(dashboard: &mut UsageDashboard, local: LocalCodexAggregation) {
    dashboard.summary.request_count += local.summary.request_count;
    dashboard.summary.successful_request_count += local.summary.successful_request_count;
    dashboard.summary.input_tokens += local.summary.input_tokens;
    dashboard.summary.cache_read_input_tokens += local.summary.cache_read_input_tokens;
    dashboard.summary.output_tokens += local.summary.output_tokens;
    dashboard.summary.estimated_cost += local.summary.estimated_cost;
    if local.status.available {
        let mut provider = empty_breakdown(CODEX_LOCAL_PROVIDER_KEY.to_string());
        provider.request_count = local.summary.request_count;
        provider.input_tokens = local.summary.input_tokens;
        provider.cache_read_input_tokens = local.summary.cache_read_input_tokens;
        provider.output_tokens = local.summary.output_tokens;
        provider.estimated_cost = local.summary.estimated_cost;
        dashboard.by_provider.push(provider);
    }
    for (key, item) in local.by_model {
        if let Some(existing) = dashboard.by_model.iter_mut().find(|existing| existing.key == key)
        {
            existing.request_count += item.request_count;
            existing.input_tokens += item.input_tokens;
            existing.cache_read_input_tokens += item.cache_read_input_tokens;
            existing.output_tokens += item.output_tokens;
            existing.estimated_cost += item.estimated_cost;
        } else {
            dashboard.by_model.push(item);
        }
    }
    for (date, item) in local.trend {
        if let Some(existing) = dashboard.trend.iter_mut().find(|existing| existing.date == date)
        {
            existing.request_count += item.request_count;
            existing.input_tokens += item.input_tokens;
            existing.cache_read_input_tokens += item.cache_read_input_tokens;
            existing.output_tokens += item.output_tokens;
            existing.estimated_cost += item.estimated_cost;
        } else {
            dashboard.trend.push(item);
        }
    }
    dashboard
        .trend
        .sort_by(|left, right| left.date.cmp(&right.date));
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

    fn write_session(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn codex_local_usage_diffs_consecutive_totals() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            "2026/session.jsonl",
            concat!(
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":10,\"output_tokens\":20}}}}\n",
                "{\"timestamp\":\"2026-07-01T01:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":150,\"cached_input_tokens\":40,\"output_tokens\":50}}}}\n",
            ),
        );

        let usage = collect_codex_local_usage_from_root(root.path(), 0);
        assert!(usage.status.available);
        assert_eq!(usage.status.event_count, 2);
        assert_eq!(usage.summary.request_count, 2);
        assert_eq!(usage.summary.successful_request_count, 2);
        // Turn1 fresh=90 cached=10 out=20; turn2 delta fresh=20 cached=30 out=30
        assert_eq!(usage.summary.input_tokens, 110);
        assert_eq!(usage.summary.cache_read_input_tokens, 40);
        assert_eq!(usage.summary.output_tokens, 50);
        assert_eq!(usage.by_model["gpt-5"].input_tokens, 110);
    }

    #[test]
    fn codex_local_usage_adds_last_token_usage_directly() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            "session.jsonl",
            concat!(
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"o4-mini\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"inputTokens\":100,\"cachedInputTokens\":10,\"outputTokens\":20}}}}\n",
                "{\"timestamp\":\"2026-07-01T01:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"inputTokens\":50,\"cachedInputTokens\":5,\"outputTokens\":10}}}}\n",
            ),
        );

        let usage = collect_codex_local_usage_from_root(root.path(), 0);
        assert_eq!(usage.status.event_count, 2);
        assert_eq!(usage.summary.input_tokens, 135); // (100-10)+(50-5)
        assert_eq!(usage.summary.cache_read_input_tokens, 15);
        assert_eq!(usage.summary.output_tokens, 30);
    }

    #[test]
    fn codex_local_usage_total_then_last_do_not_pollute_each_other() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            "2026/session.jsonl",
            concat!(
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n",
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":10,\"output_tokens\":20}}}}\n",
                "{invalid json}\n",
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"o4-mini\"}}\n",
                "{\"timestamp\":\"2026-07-02T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"inputTokens\":150,\"cachedInputTokens\":40,\"outputTokens\":30,\"model\":\"o4-mini\"}}}}\n",
                "{\"timestamp\":\"2026-07-02T02:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":180,\"cached_input_tokens\":50,\"output_tokens\":55}}}}\n",
            ),
        );

        let usage = collect_codex_local_usage_from_root(root.path(), 0);
        assert_eq!(usage.status.event_count, 3);
        // total1: fresh 90/10/20; last: fresh 110/40/30 (direct, prev_total unchanged);
        // total2 delta from prev_total 100/10/20 → 80/40/35 → fresh 40
        assert_eq!(usage.summary.input_tokens, 90 + 110 + 40);
        assert_eq!(usage.summary.cache_read_input_tokens, 10 + 40 + 40);
        assert_eq!(usage.summary.output_tokens, 20 + 30 + 35);
        assert_eq!(usage.by_model["gpt-5"].input_tokens, 90);
        assert_eq!(usage.by_model["o4-mini"].input_tokens, 110 + 40);
        assert_eq!(usage.trend.len(), 2);
    }

    #[test]
    fn codex_local_usage_skips_agent_files_and_reads_archived() {
        let home = tempfile::tempdir().unwrap();
        let sessions = home.path().join("sessions");
        let archived = home.path().join("archived_sessions");
        write_session(
            &sessions,
            "main.jsonl",
            concat!(
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model\":\"gpt-5\",\"last_token_usage\":{\"input_tokens\":20,\"cached_input_tokens\":0,\"output_tokens\":5}}}}\n",
            ),
        );
        write_session(
            &sessions,
            "agent-child.jsonl",
            concat!(
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":999,\"cached_input_tokens\":0,\"output_tokens\":999}}}}\n",
            ),
        );
        write_session(
            &archived,
            "old.jsonl",
            concat!(
                "{\"timestamp\":\"2026-07-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model_name\":\"o4-mini\",\"last_token_usage\":{\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":2}}}}\n",
            ),
        );

        let usage = collect_codex_local_usage_from_roots(&[sessions, archived], 0);
        assert_eq!(usage.status.session_count, 2);
        assert_eq!(usage.status.event_count, 2);
        assert_eq!(usage.summary.input_tokens, 30);
        assert_eq!(usage.summary.output_tokens, 7);
        assert!(usage.by_model.contains_key("gpt-5"));
        assert!(usage.by_model.contains_key("o4-mini"));
    }

    #[test]
    fn codex_local_usage_reports_empty_or_missing_data() {
        let missing = tempfile::tempdir().unwrap();
        let usage = collect_codex_local_usage_from_root(&missing.path().join("missing"), 0);
        assert!(!usage.status.available);
        assert_eq!(usage.status.session_count, 0);

        let empty = tempfile::tempdir().unwrap();
        std::fs::write(empty.path().join("session.jsonl"), "{not json}\n").unwrap();
        let usage = collect_codex_local_usage_from_root(empty.path(), 0);
        assert!(!usage.status.available);
        assert_eq!(usage.status.session_count, 1);
        assert_eq!(usage.status.event_count, 0);
    }

    #[test]
    fn apply_codex_estimated_cost_uses_usd_pricing() {
        let mut local = LocalCodexAggregation {
            status: LocalCodexUsage {
                available: true,
                session_count: 1,
                event_count: 1,
                message: String::new(),
            },
            summary: empty_summary(),
            by_model: BTreeMap::new(),
            trend: BTreeMap::new(),
        };
        local.summary.request_count = 1;
        local.summary.input_tokens = 1_000_000;
        local.summary.output_tokens = 500_000;
        local.by_model.insert(
            "gpt-5".to_string(),
            UsageBreakdown {
                key: "gpt-5".to_string(),
                request_count: 1,
                input_tokens: 1_000_000,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                output_tokens: 500_000,
                estimated_cost: 0.0,
            },
        );
        let pricing = vec![ModelPricing {
            model: "gpt-5".to_string(),
            provider: "OpenAI".to_string(),
            input_price_per_million: 2.0,
            cache_read_price_per_million: 0.0,
            cache_write_price_per_million: 0.0,
            output_price_per_million: 8.0,
            batch_input_price_per_million: 0.0,
            batch_output_price_per_million: 0.0,
            currency: "USD".to_string(),
            source_url: String::new(),
            effective_date: String::new(),
            is_default: false,
        }];
        apply_codex_estimated_cost(&mut local, &pricing);
        assert!((local.summary.estimated_cost - 6.0).abs() < f64::EPSILON);
        assert!((local.by_model["gpt-5"].estimated_cost - 6.0).abs() < f64::EPSILON);
    }

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
