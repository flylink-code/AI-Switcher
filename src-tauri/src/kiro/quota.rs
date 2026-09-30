//! Per-account Kiro credit usage. Stored on the account file, not the main database.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

use super::account::{now_ms, store, streaming_profile_arn, KiroAccount, KiroAccountPublic};
use super::outbound::build_async_client;
use super::token::{ensure_access_token, should_force_refresh};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroQuotaSnapshot {
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub used: f64,
    #[serde(default)]
    pub limit: f64,
    #[serde(default)]
    pub reset_at: String,
    #[serde(default)]
    pub overage: String,
    #[serde(default)]
    pub trial_used: Option<f64>,
    #[serde(default)]
    pub trial_limit: Option<f64>,
    #[serde(default)]
    pub queried_at: i64,
    #[serde(default)]
    pub error: String,
}

pub async fn refresh_one(account_id: &str) -> AppResult<KiroAccountPublic> {
    let Some(account) = store().get(account_id) else {
        return Err(AppError::Config("Kiro 账号不存在".into()));
    };
    let mut account = match ensure_access_token(&account, false).await {
        Ok(account) => account,
        Err(error) => {
            if matches!(error, AppError::Network(_)) {
                return store().save_quota(account_id, error_snapshot(error.to_string()));
            }
            return Err(error);
        }
    };
    let mut refreshed = false;
    loop {
        match fetch_limits(&account).await {
            Ok(snapshot) => return store().save_quota(&account.id, snapshot),
            Err(error) if should_force_refresh(error.status, &error.body, refreshed) => {
                refreshed = true;
                match ensure_access_token(&account, true).await {
                    Ok(updated) => account = updated,
                    Err(refresh_error) => {
                        if matches!(refresh_error, AppError::Network(_)) {
                            return store().save_quota(&account.id, error_snapshot(refresh_error.to_string()));
                        }
                        return Err(refresh_error);
                    }
                }
            }
            Err(error) if error.network => {
                return store().save_quota(&account.id, error_snapshot(error.body));
            }
            Err(error) => {
                let message = if error.status == 401 {
                    "Kiro 鉴权失败".to_string()
                } else if error.body.trim().is_empty() {
                    format!("查询额度失败 HTTP {}", error.status)
                } else {
                    format!("查询额度失败 HTTP {}: {}", error.status, truncate(&error.body, 240))
                };
                return store().save_quota(&account.id, error_snapshot(message));
            }
        }
    }
}

pub async fn refresh_all() -> AppResult<Vec<KiroAccountPublic>> {
    let ids: Vec<String> = store()
        .list_public()?
        .into_iter()
        .map(|account| account.id)
        .collect();
    for id in ids {
        let _ = refresh_one(&id).await;
    }
    store().list_public()
}

struct FetchError {
    status: u16,
    body: String,
    network: bool,
}

async fn fetch_limits(account: &KiroAccount) -> Result<KiroQuotaSnapshot, FetchError> {
    let region = if account.region.trim().is_empty() {
        "us-east-1"
    } else {
        account.region.trim()
    };
    let client = build_async_client(20);
    let response = client
        .get(format!("https://q.{region}.amazonaws.com/getUsageLimits"))
        .query(&[
            ("origin", "AI_EDITOR"),
            ("profileArn", streaming_profile_arn(account).as_str()),
            ("resourceType", "CREDIT"),
            ("isEmailRequired", "true"),
        ])
        .header(
            "authorization",
            format!("Bearer {}", account.access_token.trim()),
        )
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| FetchError {
            status: 0,
            body: error.to_string(),
            network: true,
        })?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if status == 401 || should_force_refresh(status, &body, false) && status != 200 {
        return Err(FetchError {
            status,
            body,
            network: false,
        });
    }
    if !(200..300).contains(&status) {
        return Err(FetchError {
            status,
            body,
            network: false,
        });
    }
    let value: Value = serde_json::from_str(&body).map_err(|error| FetchError {
        status,
        body: error.to_string(),
        network: false,
    })?;
    Ok(parse_usage_limits(&value))
}

pub fn parse_usage_limits(root: &Value) -> KiroQuotaSnapshot {
    let breakdown = credit_breakdown(root);
    let used = breakdown
        .and_then(|item| number_of(item, &["currentUsageWithPrecision", "currentUsage"]))
        .unwrap_or(0.0);
    let limit = breakdown
        .and_then(|item| number_of(item, &["usageLimitWithPrecision", "usageLimit"]))
        .unwrap_or(0.0);
    let trial = breakdown.and_then(|item| item.get("freeTrialUsage"));
    KiroQuotaSnapshot {
        plan: root
            .get("subscriptionInfo")
            .and_then(|item| item.get("subscriptionTitle"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        used,
        limit,
        reset_at: reset_text(root, breakdown),
        overage: root
            .get("overageConfiguration")
            .and_then(|item| item.get("overageStatus"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        trial_used: trial.and_then(|item| number_of(item, &["currentUsageWithPrecision", "currentUsage"])),
        trial_limit: trial.and_then(|item| number_of(item, &["usageLimitWithPrecision", "usageLimit"])),
        queried_at: now_ms(),
        error: String::new(),
    }
}

fn credit_breakdown(root: &Value) -> Option<&Value> {
    let list = root
        .get("usageBreakdownList")
        .and_then(|item| item.as_array())
        .or_else(|| root.get("usageBreakdowns").and_then(|item| item.as_array()))?;
    list.iter()
        .find(|item| {
            item.get("resourceType")
                .or_else(|| item.get("type"))
                .and_then(|kind| kind.as_str())
                .is_some_and(|kind| kind.eq_ignore_ascii_case("credit"))
        })
        .or(list.first())
}

fn reset_text(root: &Value, breakdown: Option<&Value>) -> String {
    for source in [breakdown, Some(root)].into_iter().flatten() {
        if let Some(text) = source.get("nextDateReset").and_then(|item| item.as_str()) {
            if !text.trim().is_empty() {
                return text.trim().to_string();
            }
        }
    }
    if let Some(days) = root.get("daysUntilReset").and_then(|item| item.as_i64()) {
        return format!("{days}d");
    }
    String::new()
}

fn number_of(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        let Some(item) = value.get(*key) else {
            continue;
        };
        if let Some(number) = item.as_f64() {
            return Some(number);
        }
        if let Some(number) = item.as_i64() {
            return Some(number as f64);
        }
        if let Some(text) = item.as_str() {
            if let Ok(number) = text.trim().parse::<f64>() {
                return Some(number);
            }
        }
    }
    None
}

fn error_snapshot(message: String) -> KiroQuotaSnapshot {
    KiroQuotaSnapshot {
        queried_at: now_ms(),
        error: message,
        ..KiroQuotaSnapshot::default()
    }
}

fn truncate(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_credit_breakdown_and_trial() {
        let snapshot = parse_usage_limits(&json!({
            "subscriptionInfo": { "subscriptionTitle": "KIRO PRO" },
            "nextDateReset": "2026-10-01T00:00:00.000Z",
            "overageConfiguration": { "overageStatus": "DISABLED" },
            "usageBreakdownList": [{
                "resourceType": "CREDIT",
                "currentUsage": 10,
                "currentUsageWithPrecision": 10.5,
                "usageLimit": 50,
                "usageLimitWithPrecision": 500,
                "freeTrialUsage": { "currentUsage": 2, "usageLimit": 100 }
            }]
        }));
        assert_eq!(snapshot.plan, "KIRO PRO");
        assert!((snapshot.used - 10.5).abs() < 0.01);
        assert!((snapshot.limit - 500.0).abs() < 0.01);
        assert_eq!(snapshot.reset_at, "2026-10-01T00:00:00.000Z");
        assert_eq!(snapshot.overage, "DISABLED");
        assert_eq!(snapshot.trial_used, Some(2.0));
        assert_eq!(snapshot.trial_limit, Some(100.0));
        assert!(snapshot.error.is_empty());
    }
}
