//! Quota and balance query types for AI providers and official accounts.

use serde::{Deserialize, Serialize};

/// Known tier names
pub const TIER_FIVE_HOUR: &str = "five_hour";
pub const TIER_SEVEN_DAY: &str = "seven_day";
pub const TIER_SEVEN_DAY_OPUS: &str = "seven_day_opus";
pub const TIER_SEVEN_DAY_SONNET: &str = "seven_day_sonnet";
pub const TIER_WEEKLY_LIMIT: &str = "weekly_limit";
pub const TIER_THIRTY_DAY: &str = "30_day";
pub const TIER_DAILY: &str = "daily";
pub const TIER_MONTHLY: &str = "monthly";
pub const TIER_CREDITS: &str = "credits";

/// A single rate limit / quota window (e.g. 5-hour rolling session, 7-day quota).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaTier {
    /// Window identifier: five_hour, seven_day, weekly_limit, 30_day, monthly, credits, etc.
    pub name: String,
    /// Percentage used: 0.0 - 100.0
    pub utilization: f64,
    /// ISO 8601 reset timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// Used value (e.g. USD / Token count)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_value: Option<f64>,
    /// Maximum capacity value (e.g. USD / Token count)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
}

/// Extra / overage usage info (e.g. Claude Code extra usage billing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    pub is_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_credits: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// Unified provider quota / balance query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderQuotaResult {
    /// Rolling window / Token Plan (Claude OAuth, Codex OAuth, Kimi, Zhipu, MiniMax, ZenMux, Volcengine, OpenCode Go)
    Subscription {
        provider_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        plan_name: Option<String>,
        tiers: Vec<QuotaTier>,
        #[serde(skip_serializing_if = "Option::is_none")]
        extra_usage: Option<ExtraUsage>,
        queried_at: i64,
    },
    /// Pay-as-you-go account balance (DeepSeek, SiliconFlow, StepFun, OpenRouter, Novita AI)
    Balance {
        provider_type: String,
        currency: String,
        total_balance: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        granted_balance: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        topped_up_balance: Option<f64>,
        is_available: bool,
        queried_at: i64,
    },
    /// Provider does not support quota / balance queries or not recognized
    Unsupported {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Query failed with an error
    Error {
        code: String, // "AUTH_FAILED", "NETWORK_ERROR", "API_ERROR", "NOT_FOUND"
        message: String,
        queried_at: i64,
    },
}
