//! Token Plan / Coding Plan quota queries (Kimi, Zhipu GLM, MiniMax, ZenMux, Volcengine, OpenCode Go).

use crate::quota::types::*;
use crate::quota::{millis_to_iso8601, now_millis, quota_http_client};
use reqwest::header;
use serde_json::Value;

// ─────────────────────────────────────────────────────────────
// Helper parsing functions
// ─────────────────────────────────────────────────────────────

fn extract_reset_time(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = value.as_i64() {
        if n <= 0 {
            return None;
        }
        let ms = if n < 1_000_000_000_000 { n * 1000 } else { n };
        return millis_to_iso8601(ms);
    }
    None
}

fn parse_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

// ─────────────────────────────────────────────────────────────
// Kimi For Coding
// ─────────────────────────────────────────────────────────────

pub async fn query_kimi_quota(api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get("https://api.kimi.com/coding/v1/usages")
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
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
            message: format!("Kimi API Key 鉴权失败 (HTTP {status})"),
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

    // 5-hour limit
    if let Some(limits) = body.get("limits").and_then(Value::as_array) {
        for limit_item in limits {
            if let Some(detail) = limit_item.get("detail") {
                let limit = detail.get("limit").and_then(parse_f64).unwrap_or(1.0);
                let remaining = detail.get("remaining").and_then(parse_f64).unwrap_or(0.0);
                let resets_at = detail.get("resetTime").and_then(extract_reset_time);

                let used = (limit - remaining).max(0.0);
                let utilization = if limit > 0.0 {
                    (used / limit) * 100.0
                } else {
                    0.0
                };

                tiers.push(QuotaTier {
                    name: TIER_FIVE_HOUR.to_string(),
                    utilization,
                    resets_at,
                    used_value: Some(used),
                    max_value: Some(limit),
                });
            }
        }
    }

    // Weekly limit
    if let Some(usage) = body.get("usage") {
        let limit = usage.get("limit").and_then(parse_f64).unwrap_or(1.0);
        let remaining = usage.get("remaining").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = usage.get("resetTime").and_then(extract_reset_time);

        let used = (limit - remaining).max(0.0);
        let utilization = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };

        tiers.push(QuotaTier {
            name: TIER_WEEKLY_LIMIT.to_string(),
            utilization,
            resets_at,
            used_value: Some(used),
            max_value: Some(limit),
        });
    }

    ProviderQuotaResult::Subscription {
        provider_type: "kimi".to_string(),
        plan_name: Some("Kimi Coding Plan".to_string()),
        tiers,
        extra_usage: None,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// 智谱 GLM
// ─────────────────────────────────────────────────────────────

pub async fn query_zhipu_quota(base_url: &str, api_key: &str) -> ProviderQuotaResult {
    let domain = if base_url.to_lowercase().contains("bigmodel.cn") {
        "https://open.bigmodel.cn"
    } else {
        "https://api.z.ai"
    };
    let url = format!("{domain}/api/monitor/usage/quota/limit");

    let client = quota_http_client();
    let resp = match client
        .get(&url)
        .header(header::AUTHORIZATION, api_key) // Note: Zhipu does not use Bearer prefix
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT_LANGUAGE, "zh-CN,zh,en")
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
            message: format!("智谱 GLM API Key 鉴权失败 (HTTP {status})"),
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

    let data = match body.get("data") {
        Some(d) => d,
        None => {
            return ProviderQuotaResult::Error {
                code: "PARSE_ERROR".to_string(),
                message: "响应缺少 data 字段".to_string(),
                queried_at: now_millis(),
            };
        }
    };

    let plan_name = data.get("level").and_then(Value::as_str).map(|s| format!("GLM {s}"));
    let mut tiers = Vec::new();

    if let Some(limits) = data.get("limits").and_then(Value::as_array) {
        for item in limits {
            let limit_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            if !limit_type.eq_ignore_ascii_case("TOKENS_LIMIT")
                && !limit_type.eq_ignore_ascii_case("CREDIT_LIMIT")
            {
                continue;
            }

            let percentage = item.get("percentage").and_then(Value::as_f64).unwrap_or(0.0);
            let reset_ms = item.get("nextResetTime").and_then(Value::as_i64);
            let resets_at = reset_ms.and_then(millis_to_iso8601);

            let unit = item.get("unit").and_then(Value::as_i64).unwrap_or(3);
            let tier_name = if unit == 6 {
                TIER_WEEKLY_LIMIT
            } else {
                TIER_FIVE_HOUR
            };

            tiers.push(QuotaTier {
                name: tier_name.to_string(),
                utilization: percentage,
                resets_at,
                used_value: None,
                max_value: None,
            });
        }
    }

    ProviderQuotaResult::Subscription {
        provider_type: "zhipu".to_string(),
        plan_name,
        tiers,
        extra_usage: None,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// MiniMax 编程套餐
// ─────────────────────────────────────────────────────────────

pub async fn query_minimax_quota(base_url: &str, api_key: &str) -> ProviderQuotaResult {
    let domain = if base_url.to_lowercase().contains("minimax.io") {
        "api.minimax.io"
    } else {
        "api.minimaxi.com"
    };
    let url = format!("https://{domain}/v1/api/openplatform/coding_plan/remains");

    let client = quota_http_client();
    let resp = match client
        .get(&url)
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::CONTENT_TYPE, "application/json")
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
            message: format!("MiniMax API Key 鉴权失败 (HTTP {status})"),
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
    if let Some(model_remains) = body.get("model_remains").and_then(Value::as_array) {
        if let Some(item) = model_remains.iter().find(|i| {
            i.get("model_name")
                .and_then(Value::as_str)
                .map(|s| s == "general")
                .unwrap_or(false)
        }) {
            if let Some(remain_pct) = item
                .get("current_interval_remaining_percent")
                .and_then(Value::as_f64)
            {
                let resets_at = item
                    .get("end_time")
                    .and_then(Value::as_i64)
                    .and_then(millis_to_iso8601);
                tiers.push(QuotaTier {
                    name: TIER_FIVE_HOUR.to_string(),
                    utilization: (100.0 - remain_pct).max(0.0),
                    resets_at,
                    used_value: None,
                    max_value: None,
                });
            }

            if item.get("current_weekly_status").and_then(Value::as_i64) == Some(1) {
                if let Some(remain_pct) = item
                    .get("current_weekly_remaining_percent")
                    .and_then(Value::as_f64)
                {
                    let resets_at = item
                        .get("weekly_end_time")
                        .and_then(Value::as_i64)
                        .and_then(millis_to_iso8601);
                    tiers.push(QuotaTier {
                        name: TIER_WEEKLY_LIMIT.to_string(),
                        utilization: (100.0 - remain_pct).max(0.0),
                        resets_at,
                        used_value: None,
                        max_value: None,
                    });
                }
            }
        }
    }

    ProviderQuotaResult::Subscription {
        provider_type: "minimax".to_string(),
        plan_name: Some("MiniMax Coding Plan".to_string()),
        tiers,
        extra_usage: None,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// ZenMux
// ─────────────────────────────────────────────────────────────

pub async fn query_zenmux_quota(base_url: &str, api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get(base_url)
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
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
            message: format!("ZenMux 鉴权失败 (HTTP {status})"),
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

    let data = match body.get("data") {
        Some(d) => d,
        None => {
            return ProviderQuotaResult::Error {
                code: "PARSE_ERROR".to_string(),
                message: "响应缺少 data 字段".to_string(),
                queried_at: now_millis(),
            };
        }
    };

    let mut tiers = Vec::new();

    if let Some(q5h) = data.get("quota_5_hour") {
        let usage_pct = q5h.get("usage_percentage").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = q5h.get("resets_at").and_then(Value::as_str).map(String::from);
        let used_usd = q5h.get("used_value_usd").and_then(parse_f64);
        let max_usd = q5h.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: TIER_FIVE_HOUR.to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value: used_usd,
            max_value: max_usd,
        });
    }

    if let Some(q7d) = data.get("quota_7_day") {
        let usage_pct = q7d.get("usage_percentage").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = q7d.get("resets_at").and_then(Value::as_str).map(String::from);
        let used_usd = q7d.get("used_value_usd").and_then(parse_f64);
        let max_usd = q7d.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: TIER_WEEKLY_LIMIT.to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value: used_usd,
            max_value: max_usd,
        });
    }

    let plan_tier = data.get("plan").and_then(|p| p.get("tier")).and_then(Value::as_str).unwrap_or("ZenMux");

    ProviderQuotaResult::Subscription {
        provider_type: "zenmux".to_string(),
        plan_name: Some(plan_tier.to_string()),
        tiers,
        extra_usage: None,
        queried_at: now_millis(),
    }
}
