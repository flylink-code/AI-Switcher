//! Opportunistic quota probe for self-hosted sub2api gateways (`GET /v1/usage`).
//!
//! Do not use NewAPI `/api/user/self` — sub2api does not implement that route.

use crate::provider::{strip_anthropic_compat_path, strip_trailing_v1_path};
use crate::quota::types::*;
use crate::quota::now_millis;
use reqwest::header;
use serde_json::Value;
use std::time::Duration;

const PROVIDER_TYPE: &str = "sub2api";
const PROBE_TIMEOUT_SECS: u64 = 5;

fn parse_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|n| n as f64))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

fn json_string(value: &Value) -> Option<String> {
    value.as_str().map(String::from)
}

fn utilization(used: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        ((used / max) * 100.0).clamp(0.0, 100.0)
    }
}

fn add_days_rfc3339(ts: &str, days: i64) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    Some((dt + chrono::Duration::days(days)).to_rfc3339())
}

fn is_blocked_host(base_url: &str) -> bool {
    let parsed = match url::Url::parse(base_url.trim()) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "api.openai.com",
        "api.anthropic.com",
        "chatgpt.com",
        "www.chatgpt.com",
        "cloudcode-pa.googleapis.com",
        "generativelanguage.googleapis.com",
        "oauth2.googleapis.com",
    ];
    EXACT.iter().any(|blocked| host == *blocked)
        || host.ends_with(".googleapis.com")
        || host.ends_with(".openai.azure.com")
}

/// Gateway origin used to build `GET {origin}/v1/usage`.
pub fn gateway_origin(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_blocked_host(trimmed) {
        return None;
    }
    let without_anthropic = strip_anthropic_compat_path(trimmed)
        .unwrap_or_else(|| trimmed.trim_end_matches('/').to_string());
    let origin = strip_trailing_v1_path(&without_anthropic);
    if origin.contains("://") {
        Some(origin)
    } else {
        None
    }
}

/// `GET /v1/usage` URL, or `None` when the host is blocked / not a URL.
pub fn usage_url(base_url: &str) -> Option<String> {
    gateway_origin(base_url).map(|origin| format!("{origin}/v1/usage"))
}

fn looks_like_sub2api_auth_error(body: &Value) -> bool {
    let ty = body
        .get("error")
        .and_then(|err| err.get("type"))
        .or_else(|| body.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    ty.eq_ignore_ascii_case("authentication_error")
}

fn probe_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .build()
        .unwrap_or_default()
}

fn unsupported(reason: &str) -> ProviderQuotaResult {
    ProviderQuotaResult::Unsupported {
        reason: Some(reason.to_string()),
    }
}

/// Probe a custom provider base URL for sub2api `GET /v1/usage`.
pub async fn query_sub2api_usage(base_url: &str, api_key: &str) -> ProviderQuotaResult {
    let Some(url) = usage_url(base_url) else {
        return unsupported("当前供应商未内置额度/余额查询接口");
    };

    let client = probe_http_client();
    let resp = match client
        .get(&url)
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => {
            // Opportunistic probe: dead hosts / timeouts stay silent.
            return unsupported("无法连接供应商额度接口");
        }
    };

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        if looks_like_sub2api_auth_error(&body) {
            return ProviderQuotaResult::Error {
                code: "AUTH_FAILED".to_string(),
                message: format!("sub2api API Key 鉴权失败 (HTTP {status})"),
                queried_at: now_millis(),
            };
        }
        return unsupported("供应商额度接口鉴权失败");
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        return unsupported("供应商未提供 /v1/usage");
    }

    if !status.is_success() {
        return unsupported("供应商额度接口不可用");
    }

    match parse_sub2api_usage(&body) {
        Some(result) => result,
        None => unsupported("响应不是 sub2api /v1/usage"),
    }
}

/// Parse a `/v1/usage` JSON body. Returns `None` when the sub2api signature is missing.
pub fn parse_sub2api_usage(body: &Value) -> Option<ProviderQuotaResult> {
    let mode = body.get("mode").and_then(Value::as_str)?;
    if mode != "quota_limited" && mode != "unrestricted" {
        return None;
    }
    let queried_at = now_millis();
    Some(match mode {
        "quota_limited" => parse_quota_limited(body, queried_at),
        _ => parse_unrestricted(body, queried_at),
    })
}

fn parse_quota_limited(body: &Value, queried_at: i64) -> ProviderQuotaResult {
    let mut tiers = Vec::new();

    if let Some(quota) = body.get("quota") {
        let limit = quota.get("limit").and_then(parse_f64).unwrap_or(0.0);
        let used = quota.get("used").and_then(parse_f64).unwrap_or(0.0);
        if limit > 0.0 {
            tiers.push(QuotaTier {
                name: TIER_CREDITS.to_string(),
                utilization: utilization(used, limit),
                resets_at: None,
                used_value: Some(used),
                max_value: Some(limit),
            });
        }
    }

    if let Some(limits) = body.get("rate_limits").and_then(Value::as_array) {
        for item in limits {
            let window = item
                .get("window")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let name = match window.as_str() {
                "5h" => TIER_FIVE_HOUR,
                "1d" | "daily" => TIER_DAILY,
                "7d" => TIER_SEVEN_DAY,
                other if other.is_empty() => continue,
                other => other,
            };
            let limit = item.get("limit").and_then(parse_f64).unwrap_or(0.0);
            let used = item.get("used").and_then(parse_f64).unwrap_or(0.0);
            if limit <= 0.0 {
                continue;
            }
            let resets_at = item
                .get("reset_at")
                .and_then(json_string)
                .or_else(|| item.get("reset_at").and_then(|v| parse_f64(v).and_then(|n| {
                    crate::quota::millis_to_iso8601(if n < 1_000_000_000_000.0 {
                        (n * 1000.0) as i64
                    } else {
                        n as i64
                    })
                })));
            tiers.push(QuotaTier {
                name: name.to_string(),
                utilization: utilization(used, limit),
                resets_at,
                used_value: Some(used),
                max_value: Some(limit),
            });
        }
    }

    if tiers.is_empty() {
        return parse_unrestricted(body, queried_at);
    }

    ProviderQuotaResult::Subscription {
        provider_type: PROVIDER_TYPE.to_string(),
        plan_name: body.get("planName").and_then(json_string),
        tiers,
        extra_usage: None,
        queried_at,
    }
}

fn parse_unrestricted(body: &Value, queried_at: i64) -> ProviderQuotaResult {
    let unit = body
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("USD")
        .to_string();
    let remaining = body.get("remaining").and_then(parse_f64);
    let plan_name = body.get("planName").and_then(json_string);

    if let Some(sub) = body.get("subscription") {
        let mut tiers = Vec::new();
        push_sub_window(
            &mut tiers,
            TIER_DAILY,
            sub.get("daily_usage_usd").and_then(parse_f64),
            sub.get("daily_limit_usd").and_then(parse_f64),
            None,
        );
        let weekly_reset = sub
            .get("weekly_window_start")
            .and_then(json_string)
            .and_then(|start| add_days_rfc3339(&start, 7));
        push_sub_window(
            &mut tiers,
            TIER_WEEKLY_LIMIT,
            sub.get("weekly_usage_usd").and_then(parse_f64),
            sub.get("weekly_limit_usd").and_then(parse_f64),
            weekly_reset,
        );
        let monthly_reset = sub.get("expires_at").and_then(json_string);
        push_sub_window(
            &mut tiers,
            TIER_MONTHLY,
            sub.get("monthly_usage_usd").and_then(parse_f64),
            sub.get("monthly_limit_usd").and_then(parse_f64),
            monthly_reset,
        );
        if !tiers.is_empty() {
            return ProviderQuotaResult::Subscription {
                provider_type: PROVIDER_TYPE.to_string(),
                plan_name,
                tiers,
                extra_usage: None,
                queried_at,
            };
        }
    }

    let total = remaining
        .or_else(|| body.get("balance").and_then(parse_f64))
        .unwrap_or(0.0);
    ProviderQuotaResult::Balance {
        provider_type: PROVIDER_TYPE.to_string(),
        currency: unit,
        total_balance: total,
        granted_balance: None,
        topped_up_balance: None,
        is_available: total > 0.0 || total < 0.0,
        queried_at,
    }
}

fn push_sub_window(
    tiers: &mut Vec<QuotaTier>,
    name: &str,
    used: Option<f64>,
    max: Option<f64>,
    resets_at: Option<String>,
) {
    let Some(max) = max else {
        return;
    };
    if max <= 0.0 {
        return;
    }
    let used = used.unwrap_or(0.0);
    tiers.push(QuotaTier {
        name: name.to_string(),
        utilization: utilization(used, max),
        resets_at,
        used_value: Some(used),
        max_value: Some(max),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_url_strips_v1_and_trailing_slash() {
        assert_eq!(
            usage_url("https://gw.example.com/v1/").as_deref(),
            Some("https://gw.example.com/v1/usage")
        );
        assert_eq!(
            usage_url("https://gw.example.com").as_deref(),
            Some("https://gw.example.com/v1/usage")
        );
    }

    #[test]
    fn usage_url_strips_anthropic_compat_path() {
        assert_eq!(
            usage_url("https://gw.example.com/anthropic").as_deref(),
            Some("https://gw.example.com/v1/usage")
        );
        assert_eq!(
            usage_url("https://gw.example.com/v1/anthropic/").as_deref(),
            Some("https://gw.example.com/v1/usage")
        );
    }

    #[test]
    fn usage_url_blocks_official_hosts() {
        assert!(usage_url("https://api.openai.com/v1").is_none());
        assert!(usage_url("https://api.anthropic.com").is_none());
        assert!(usage_url("https://chatgpt.com").is_none());
        assert!(usage_url("https://cloudcode-pa.googleapis.com").is_none());
        assert!(usage_url("https://generativelanguage.googleapis.com/v1").is_none());
    }

    #[test]
    fn parse_rejects_missing_mode() {
        assert!(parse_sub2api_usage(&json!({"remaining": 12.0})).is_none());
        assert!(parse_sub2api_usage(&json!({"mode": "other", "remaining": 1.0})).is_none());
        assert!(parse_sub2api_usage(&json!({})).is_none());
    }

    #[test]
    fn parse_wallet_unrestricted() {
        let result = parse_sub2api_usage(&json!({
            "mode": "unrestricted",
            "isValid": true,
            "planName": "钱包余额",
            "remaining": 12.34,
            "unit": "USD",
            "balance": 12.34
        }))
        .expect("wallet body");
        match result {
            ProviderQuotaResult::Balance {
                provider_type,
                currency,
                total_balance,
                is_available,
                ..
            } => {
                assert_eq!(provider_type, "sub2api");
                assert_eq!(currency, "USD");
                assert!((total_balance - 12.34).abs() < 1e-9);
                assert!(is_available);
            }
            other => panic!("expected balance, got {other:?}"),
        }
    }

    #[test]
    fn parse_unlimited_remaining() {
        let result = parse_sub2api_usage(&json!({
            "mode": "unrestricted",
            "isValid": true,
            "remaining": -1,
            "unit": "USD"
        }))
        .expect("unlimited body");
        match result {
            ProviderQuotaResult::Balance {
                total_balance,
                is_available,
                ..
            } => {
                assert!(total_balance < 0.0);
                assert!(is_available);
            }
            other => panic!("expected unlimited balance, got {other:?}"),
        }
    }

    #[test]
    fn parse_subscription_windows() {
        let result = parse_sub2api_usage(&json!({
            "mode": "unrestricted",
            "isValid": true,
            "planName": "Pro Claude",
            "unit": "USD",
            "remaining": 4.2,
            "subscription": {
                "daily_usage_usd": 0.8,
                "weekly_usage_usd": 5.1,
                "monthly_usage_usd": 12.0,
                "daily_limit_usd": 5.0,
                "weekly_limit_usd": 30.0,
                "monthly_limit_usd": 100.0,
                "weekly_window_start": "2026-07-13T00:30:00+08:00",
                "expires_at": "2026-09-30T00:00:00Z"
            }
        }))
        .expect("subscription body");
        match result {
            ProviderQuotaResult::Subscription {
                provider_type,
                plan_name,
                tiers,
                ..
            } => {
                assert_eq!(provider_type, "sub2api");
                assert_eq!(plan_name.as_deref(), Some("Pro Claude"));
                assert_eq!(tiers.len(), 3);
                assert_eq!(tiers[0].name, TIER_DAILY);
                assert!((tiers[0].utilization - 16.0).abs() < 0.1);
                assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
                assert!(tiers[1].resets_at.is_some());
                assert_eq!(tiers[2].name, TIER_MONTHLY);
                assert_eq!(tiers[2].resets_at.as_deref(), Some("2026-09-30T00:00:00Z"));
            }
            other => panic!("expected subscription, got {other:?}"),
        }
    }

    #[test]
    fn parse_quota_limited_with_windows() {
        let result = parse_sub2api_usage(&json!({
            "mode": "quota_limited",
            "isValid": true,
            "status": "active",
            "quota": {
                "limit": 100.0,
                "used": 23.45,
                "remaining": 76.55,
                "unit": "USD"
            },
            "remaining": 76.55,
            "unit": "USD",
            "rate_limits": [
                {
                    "window": "5h",
                    "limit": 10.0,
                    "used": 2.1,
                    "remaining": 7.9,
                    "reset_at": "2026-08-31T08:00:00Z"
                },
                {
                    "window": "1d",
                    "limit": 40.0,
                    "used": 8.0,
                    "remaining": 32.0,
                    "reset_at": "2026-09-01T00:00:00Z"
                },
                {
                    "window": "7d",
                    "limit": 80.0,
                    "used": 20.0,
                    "remaining": 60.0,
                    "reset_at": "2026-09-07T00:00:00Z"
                }
            ]
        }))
        .expect("quota_limited body");
        match result {
            ProviderQuotaResult::Subscription { tiers, .. } => {
                assert_eq!(tiers.len(), 4);
                assert_eq!(tiers[0].name, TIER_CREDITS);
                assert!((tiers[0].utilization - 23.45).abs() < 0.01);
                assert_eq!(tiers[1].name, TIER_FIVE_HOUR);
                assert_eq!(tiers[1].resets_at.as_deref(), Some("2026-08-31T08:00:00Z"));
                assert_eq!(tiers[2].name, TIER_DAILY);
                assert_eq!(tiers[3].name, TIER_SEVEN_DAY);
            }
            other => panic!("expected subscription, got {other:?}"),
        }
    }

    #[test]
    fn auth_error_signature_matches_sub2api() {
        let body = json!({
            "error": { "type": "authentication_error", "message": "Invalid API key" }
        });
        assert!(looks_like_sub2api_auth_error(&body));
        assert!(!looks_like_sub2api_auth_error(&json!({
            "error": { "type": "invalid_request_error" }
        })));
    }
}
