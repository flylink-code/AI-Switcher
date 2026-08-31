//! Official subscription quota queries for Claude Code and ChatGPT / Codex.

use crate::config::paths;
use crate::quota::types::*;
use crate::quota::{millis_to_iso8601, now_millis, quota_http_client};
use reqwest::header;
use serde::Deserialize;
use serde_json::Value;
use std::fs;

// ─────────────────────────────────────────────────────────────
// Claude Code Official Subscription Quota
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ClaudeOAuthEntry {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<Value>,
}

/// Read Claude Code OAuth access token from ~/.claude/.credentials.json (or macOS Keychain)
pub fn read_claude_credentials() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(token) = read_claude_credentials_from_keychain() {
            return Some(token);
        }
    }

    read_claude_credentials_from_file()
}

#[cfg(target_os = "macos")]
fn read_claude_credentials_from_keychain() -> Option<String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8(output.stdout).ok()?;
    parse_claude_access_token(json_str.trim())
}

fn read_claude_credentials_from_file() -> Option<String> {
    let cred_path = paths::get_claude_config_dir().join(".credentials.json");
    if !cred_path.exists() {
        return None;
    }
    let content = fs::read_to_string(cred_path).ok()?;
    parse_claude_access_token(&content)
}

fn parse_claude_access_token(content: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(content).ok()?;
    let entry_value = parsed
        .get("claudeAiOauth")
        .or_else(|| parsed.get("claude.ai_oauth"))?;

    let entry: ClaudeOAuthEntry = serde_json::from_value(entry_value.clone()).ok()?;
    let token = entry.access_token?;
    if token.trim().is_empty() {
        return None;
    }
    Some(token)
}

/// Query Claude Code subscription quota from Anthropic OAuth usage API.
pub async fn query_claude_official_quota() -> ProviderQuotaResult {
    let access_token = match read_claude_credentials() {
        Some(t) => t,
        None => {
            return ProviderQuotaResult::Unsupported {
                reason: Some("未找到 Claude Code 登录凭据 (~/.claude/.credentials.json)".to_string()),
            };
        }
    };

    let client = quota_http_client();
    let resp = match client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header(header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return ProviderQuotaResult::Error {
                code: "NETWORK_ERROR".to_string(),
                message: format!("网络连接错误: {e}"),
                queried_at: now_millis(),
            };
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return ProviderQuotaResult::Error {
            code: "AUTH_FAILED".to_string(),
            message: format!("Claude 登录凭据已失效 (HTTP {status})，请重新在 CLI 登录"),
            queried_at: now_millis(),
        };
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return ProviderQuotaResult::Error {
            code: "API_ERROR".to_string(),
            message: format!("API 响应错误 (HTTP {status}): {body}"),
            queried_at: now_millis(),
        };
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return ProviderQuotaResult::Error {
                code: "PARSE_ERROR".to_string(),
                message: format!("解析响应 JSON 失败: {e}"),
                queried_at: now_millis(),
            };
        }
    };

    let mut tiers = Vec::new();

    // 5-hour rolling session window
    if let Some(w) = body.get("five_hour") {
        if let Some(util) = w.get("utilization").and_then(Value::as_f64) {
            let resets_at = w.get("resets_at").and_then(Value::as_str).map(String::from);
            tiers.push(QuotaTier {
                name: TIER_FIVE_HOUR.to_string(),
                utilization: util,
                resets_at,
                used_value: None,
                max_value: None,
            });
        }
    }

    // 7-day total window
    if let Some(w) = body.get("seven_day") {
        if let Some(util) = w.get("utilization").and_then(Value::as_f64) {
            let resets_at = w.get("resets_at").and_then(Value::as_str).map(String::from);
            tiers.push(QuotaTier {
                name: TIER_SEVEN_DAY.to_string(),
                utilization: util,
                resets_at,
                used_value: None,
                max_value: None,
            });
        }
    }

    // 7-day Opus window
    if let Some(w) = body.get("seven_day_opus") {
        if let Some(util) = w.get("utilization").and_then(Value::as_f64) {
            let resets_at = w.get("resets_at").and_then(Value::as_str).map(String::from);
            tiers.push(QuotaTier {
                name: TIER_SEVEN_DAY_OPUS.to_string(),
                utilization: util,
                resets_at,
                used_value: None,
                max_value: None,
            });
        }
    }

    // 7-day Sonnet window
    if let Some(w) = body.get("seven_day_sonnet") {
        if let Some(util) = w.get("utilization").and_then(Value::as_f64) {
            let resets_at = w.get("resets_at").and_then(Value::as_str).map(String::from);
            tiers.push(QuotaTier {
                name: TIER_SEVEN_DAY_SONNET.to_string(),
                utilization: util,
                resets_at,
                used_value: None,
                max_value: None,
            });
        }
    }

    // Extra usage
    let extra_usage = body.get("extra_usage").map(|e| ExtraUsage {
        is_enabled: e.get("is_enabled").and_then(Value::as_bool).unwrap_or(false),
        monthly_limit: e.get("monthly_limit").and_then(Value::as_f64),
        used_credits: e.get("used_credits").and_then(Value::as_f64),
        utilization: e.get("utilization").and_then(Value::as_f64),
        currency: e.get("currency").and_then(Value::as_str).map(String::from),
    });

    ProviderQuotaResult::Subscription {
        provider_type: "claude_official".to_string(),
        plan_name: Some("Claude Official Subscription".to_string()),
        tiers,
        extra_usage,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// Codex / ChatGPT Official Subscription Quota
// ─────────────────────────────────────────────────────────────

/// Query ChatGPT / Codex official rate limits
pub async fn query_codex_official_quota(account_id: Option<&str>) -> ProviderQuotaResult {
    let (access_token, active_account_id) = match crate::codex_oauth::manager().get_valid_token(account_id) {
        Ok(res) => res,
        Err(e) => {
            return ProviderQuotaResult::Unsupported {
                reason: Some(format!("未获取到 Codex 登录账户令牌: {e}")),
            };
        }
    };

    let client = quota_http_client();
    let mut req = client
        .get("https://chatgpt.com/backend-api/ratelimits")
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .header(header::ACCEPT, "application/json");

    if !active_account_id.is_empty() {
        req = req.header("chatgpt-account-id", active_account_id);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return ProviderQuotaResult::Error {
                code: "NETWORK_ERROR".to_string(),
                message: format!("网络连接错误: {e}"),
                queried_at: now_millis(),
            };
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return ProviderQuotaResult::Error {
            code: "AUTH_FAILED".to_string(),
            message: format!("ChatGPT 登录凭据已失效 (HTTP {status})，请重新登录"),
            queried_at: now_millis(),
        };
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return ProviderQuotaResult::Error {
            code: "API_ERROR".to_string(),
            message: format!("API 响应错误 (HTTP {status}): {body}"),
            queried_at: now_millis(),
        };
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return ProviderQuotaResult::Error {
                code: "PARSE_ERROR".to_string(),
                message: format!("解析响应 JSON 失败: {e}"),
                queried_at: now_millis(),
            };
        }
    };

    let plan_name = body.get("plan_type").and_then(Value::as_str).map(|s| s.to_string());
    let mut tiers = Vec::new();

    if let Some(rate_limits) = body.get("rate_limits").and_then(Value::as_array) {
        for item in rate_limits {
            let max_requests = item.get("max_requests").and_then(Value::as_f64).unwrap_or(1.0);
            let remaining = item.get("remaining_requests").and_then(Value::as_f64).unwrap_or(0.0);
            let window_seconds = item.get("window_seconds").and_then(Value::as_i64).unwrap_or(18000);

            let used = (max_requests - remaining).max(0.0);
            let utilization = if max_requests > 0.0 {
                (used / max_requests) * 100.0
            } else {
                0.0
            };

            let reset_time_ms = item.get("reset_time")
                .or_else(|| item.get("resets_at"))
                .and_then(Value::as_i64)
                .map(|ts| if ts < 1_000_000_000_000 { ts * 1000 } else { ts });

            let resets_at = reset_time_ms.and_then(millis_to_iso8601);

            let tier_name = if window_seconds <= 18000 {
                TIER_FIVE_HOUR
            } else if window_seconds <= 604800 {
                TIER_SEVEN_DAY
            } else {
                TIER_THIRTY_DAY
            };

            tiers.push(QuotaTier {
                name: tier_name.to_string(),
                utilization,
                resets_at,
                used_value: Some(used),
                max_value: Some(max_requests),
            });
        }
    }

    ProviderQuotaResult::Subscription {
        provider_type: "codex_official".to_string(),
        plan_name,
        tiers,
        extra_usage: None,
        queried_at: now_millis(),
    }
}
