//! Daily spend cap for the smart gateway.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::database::dao::proxy_logs::{EFFECTIVE_USAGE_FILTER, ROW_COST_SQL};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::usage::fx::to_usd;

pub const BUDGET_USD_SETTING: &str = "smart_gateway_daily_budget_usd";
pub const BUDGET_ACTION_SETTING: &str = "smart_gateway_budget_action";
pub const BUDGET_FALLBACK_SETTING: &str = "smart_gateway_budget_fallback_model";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSettings {
    pub daily_budget_usd: f64,
    /// warn | reject | fallback
    pub action: String,
    pub fallback_model: String,
}

impl Default for BudgetSettings {
    fn default() -> Self {
        Self {
            daily_budget_usd: 0.0,
            action: "warn".into(),
            fallback_model: String::new(),
        }
    }
}

impl BudgetSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.daily_budget_usd < 0.0 {
            return Err(AppError::Config("日预算不能为负".into()));
        }
        if !matches!(self.action.as_str(), "warn" | "reject" | "fallback") {
            return Err(AppError::Config("超限动作须为 warn / reject / fallback".into()));
        }
        if self.action == "fallback" && self.fallback_model.trim().is_empty() {
            return Err(AppError::Config("降级动作需要指定便宜模型".into()));
        }
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.daily_budget_usd > 0.0
    }
}

struct SpendCache {
    fetched_at: Instant,
    usd: f64,
}

fn cache() -> &'static Mutex<Option<SpendCache>> {
    static CACHE: OnceLock<Mutex<Option<SpendCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub fn load_from_db(db: &Database) -> BudgetSettings {
    db.with_conn(|conn| {
        Ok(BudgetSettings {
            daily_budget_usd: get_setting(conn, BUDGET_USD_SETTING)?
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0),
            action: get_setting(conn, BUDGET_ACTION_SETTING)?
                .unwrap_or_else(|| "warn".into()),
            fallback_model: get_setting(conn, BUDGET_FALLBACK_SETTING)?.unwrap_or_default(),
        })
    })
    .unwrap_or_default()
}

pub fn persist(db: &Database, settings: &BudgetSettings) -> AppResult<BudgetSettings> {
    settings.validate()?;
    db.with_conn(|conn| {
        set_setting(conn, BUDGET_USD_SETTING, &settings.daily_budget_usd.to_string())?;
        set_setting(conn, BUDGET_ACTION_SETTING, &settings.action)?;
        set_setting(conn, BUDGET_FALLBACK_SETTING, settings.fallback_model.trim())?;
        Ok(())
    })?;
    invalidate_cache();
    Ok(settings.clone())
}

pub fn invalidate_cache() {
    if let Ok(mut slot) = cache().lock() {
        *slot = None;
    }
}

pub fn today_spend_usd(db: &Database) -> f64 {
    if let Ok(slot) = cache().lock() {
        if let Some(cached) = slot.as_ref() {
            if cached.fetched_at.elapsed() < Duration::from_secs(5) {
                return cached.usd;
            }
        }
    }
    let usd = db
        .with_read_conn(|conn| query_today_spend_usd(conn))
        .unwrap_or(0.0);
    if let Ok(mut slot) = cache().lock() {
        *slot = Some(SpendCache {
            fetched_at: Instant::now(),
            usd,
        });
    }
    usd
}

fn query_today_spend_usd(conn: &rusqlite::Connection) -> AppResult<f64> {
    let start = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|stamp| stamp.timestamp_millis())
        .unwrap_or(0);
    let sql = format!(
        "SELECT UPPER(COALESCE(NULLIF(TRIM(p.currency), ''), 'USD')),
                COALESCE(SUM({ROW_COST_SQL}), 0)
         FROM proxy_request_logs l
         LEFT JOIN model_pricing p ON lower(p.model) = lower(COALESCE(l.model, ''))
         WHERE l.created_at >= ? {EFFECTIVE_USAGE_FILTER}
         GROUP BY 1;"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![start], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut usd = 0.0;
    for row in rows {
        let (currency, amount) = row?;
        usd += to_usd(amount, &currency);
    }
    Ok(usd)
}

pub fn apply_to_request(db: &Database, requested_model: &mut String) -> Result<(), (String, u64)> {
    match evaluate(db) {
        BudgetDecision::Allow => Ok(()),
        BudgetDecision::Warn { spent, cap } => {
            log::warn!("smart gateway daily budget warning: {spent:.4}/{cap:.2} USD");
            Ok(())
        }
        BudgetDecision::Reject { spent, cap } => Err((
            format!("daily budget exceeded ({spent:.4}/{cap:.2} USD)"),
            60,
        )),
        BudgetDecision::Fallback {
            spent,
            cap,
            model,
        } => {
            log::warn!(
                "smart gateway daily budget {spent:.4}/{cap:.2} USD; falling back to {model}"
            );
            *requested_model = model;
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum BudgetDecision {
    Allow,
    Warn { spent: f64, cap: f64 },
    Reject { spent: f64, cap: f64 },
    Fallback { spent: f64, cap: f64, model: String },
}

pub fn evaluate(db: &Database) -> BudgetDecision {
    let settings = load_from_db(db);
    if !settings.is_active() {
        return BudgetDecision::Allow;
    }
    let spent = today_spend_usd(db);
    if spent < settings.daily_budget_usd {
        return BudgetDecision::Allow;
    }
    match settings.action.as_str() {
        "reject" => BudgetDecision::Reject {
            spent,
            cap: settings.daily_budget_usd,
        },
        "fallback" => BudgetDecision::Fallback {
            spent,
            cap: settings.daily_budget_usd,
            model: settings.fallback_model,
        },
        _ => BudgetDecision::Warn {
            spent,
            cap: settings.daily_budget_usd,
        },
    }
}
