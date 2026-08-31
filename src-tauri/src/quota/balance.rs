//! Pay-as-you-go provider balance queries (DeepSeek, SiliconFlow, StepFun, OpenRouter, Novita AI).

use crate::quota::types::*;
use crate::quota::{now_millis, quota_http_client};
use reqwest::header;
use serde_json::Value;

fn parse_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

// ─────────────────────────────────────────────────────────────
// DeepSeek
// ─────────────────────────────────────────────────────────────

pub async fn query_deepseek_balance(api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get("https://api.deepseek.com/user/balance")
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
            message: format!("DeepSeek API Key 鉴权失败 (HTTP {status})"),
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

    let is_available = body
        .get("is_available")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if let Some(infos) = body.get("balance_infos").and_then(Value::as_array) {
        if let Some(info) = infos.first() {
            let currency = info
                .get("currency")
                .and_then(Value::as_str)
                .unwrap_or("CNY")
                .to_string();
            let total_balance = info
                .get("total_balance")
                .and_then(parse_f64)
                .unwrap_or(0.0);
            let granted_balance = info.get("granted_balance").and_then(parse_f64);
            let topped_up_balance = info.get("topped_up_balance").and_then(parse_f64);

            return ProviderQuotaResult::Balance {
                provider_type: "deepseek".to_string(),
                currency,
                total_balance,
                granted_balance,
                topped_up_balance,
                is_available,
                queried_at: now_millis(),
            };
        }
    }

    ProviderQuotaResult::Error {
        code: "PARSE_ERROR".to_string(),
        message: "未从响应中解析出余额信息".to_string(),
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// SiliconFlow / 硅基流动
// ─────────────────────────────────────────────────────────────

pub async fn query_siliconflow_balance(base_url: &str, api_key: &str) -> ProviderQuotaResult {
    let domain = if base_url.to_lowercase().contains("siliconflow.com") {
        "https://api.siliconflow.com"
    } else {
        "https://api.siliconflow.cn"
    };
    let url = format!("{domain}/v1/user/info");

    let client = quota_http_client();
    let resp = match client
        .get(&url)
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
            message: format!("SiliconFlow API Key 鉴权失败 (HTTP {status})"),
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

    let total_balance = data.get("totalBalance").and_then(parse_f64).unwrap_or(0.0);
    let charge_balance = data.get("chargeBalance").and_then(parse_f64);
    let balance = data.get("balance").and_then(parse_f64);

    let currency = if domain.contains(".com") { "USD" } else { "CNY" };

    ProviderQuotaResult::Balance {
        provider_type: "siliconflow".to_string(),
        currency: currency.to_string(),
        total_balance,
        granted_balance: balance,
        topped_up_balance: charge_balance,
        is_available: total_balance > 0.0,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// StepFun / 阶跃星辰
// ─────────────────────────────────────────────────────────────

pub async fn query_stepfun_balance(api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get("https://api.stepfun.com/v1/user/balance")
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
            message: format!("StepFun API Key 鉴权失败 (HTTP {status})"),
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

    let total_balance = body.get("balance").and_then(parse_f64).unwrap_or(0.0);
    let currency = body
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("CNY")
        .to_string();

    ProviderQuotaResult::Balance {
        provider_type: "stepfun".to_string(),
        currency,
        total_balance,
        granted_balance: None,
        topped_up_balance: None,
        is_available: total_balance > 0.0,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// OpenRouter
// ─────────────────────────────────────────────────────────────

pub async fn query_openrouter_balance(api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get("https://openrouter.ai/api/v1/auth/key")
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
            message: format!("OpenRouter API Key 鉴权失败 (HTTP {status})"),
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

    let limit = data.get("limit").and_then(parse_f64);
    let usage = data.get("usage").and_then(parse_f64).unwrap_or(0.0);
    let limit_remaining = data.get("limit_remaining").and_then(parse_f64);

    let remaining_balance = limit_remaining.or_else(|| limit.map(|l| (l - usage).max(0.0))).unwrap_or(0.0);

    ProviderQuotaResult::Balance {
        provider_type: "openrouter".to_string(),
        currency: "USD".to_string(),
        total_balance: remaining_balance,
        granted_balance: None,
        topped_up_balance: limit,
        is_available: remaining_balance > 0.0,
        queried_at: now_millis(),
    }
}

// ─────────────────────────────────────────────────────────────
// Novita AI
// ─────────────────────────────────────────────────────────────

pub async fn query_novita_balance(api_key: &str) -> ProviderQuotaResult {
    let client = quota_http_client();
    let resp = match client
        .get("https://api.novita.ai/v3/user/credit")
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
            message: format!("Novita AI API Key 鉴权失败 (HTTP {status})"),
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

    let credit = body.get("credit").and_then(parse_f64).unwrap_or(0.0);

    ProviderQuotaResult::Balance {
        provider_type: "novita".to_string(),
        currency: "CREDIT".to_string(),
        total_balance: credit,
        granted_balance: None,
        topped_up_balance: None,
        is_available: credit > 0.0,
        queried_at: now_millis(),
    }
}
