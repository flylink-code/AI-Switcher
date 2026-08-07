//! Cloud Code quota fetch (independent implementation).
//!
//! Uses `fetchAvailableModels` + `retrieveUserQuotaSummary` with sandbox → daily → prod
//! fallbacks. Does not vendor third-party Antigravity-Manager source.

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::system_proxy;

const USER_AGENT: &str = "antigravity";

const MODELS_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
];

const SUMMARY_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
];

const PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBucket {
    pub bucket_id: String,
    /// API window: `5h` or `weekly` (shown as 7d in UI).
    pub window: String,
    /// Remaining fraction 0.0–1.0.
    pub remaining_fraction: f64,
    pub reset_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaGroup {
    pub display_name: String,
    #[serde(default)]
    pub buckets: Vec<QuotaBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelQuota {
    pub name: String,
    /// Remaining percent 0–100.
    pub percentage: i32,
    pub reset_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    #[serde(default)]
    pub models: Vec<ModelQuota>,
    #[serde(default)]
    pub groups: Vec<QuotaGroup>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_forbidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forbidden_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
}

impl QuotaSnapshot {
    pub fn empty_forbidden(reason: impl Into<String>) -> Self {
        Self {
            last_updated: Utc::now().timestamp(),
            is_forbidden: true,
            forbidden_reason: Some(reason.into()),
            ..Self::default()
        }
    }

    /// Best single remaining-% hint for pool sorting (prefer 5h, else any bucket/model).
    pub fn remaining_hint_percent(&self) -> Option<i32> {
        if self.is_forbidden {
            return Some(0);
        }
        let from_5h = self
            .groups
            .iter()
            .flat_map(|group| group.buckets.iter())
            .filter(|bucket| bucket.window.eq_ignore_ascii_case("5h"))
            .map(|bucket| (bucket.remaining_fraction * 100.0).round() as i32)
            .max();
        if from_5h.is_some() {
            return from_5h;
        }
        let from_any_bucket = self
            .groups
            .iter()
            .flat_map(|group| group.buckets.iter())
            .map(|bucket| (bucket.remaining_fraction * 100.0).round() as i32)
            .max();
        if from_any_bucket.is_some() {
            return from_any_bucket;
        }
        self.models.iter().map(|model| model.percentage).max()
    }

    pub fn window_percent(&self, window: &str) -> Option<i32> {
        self.groups
            .iter()
            .flat_map(|group| group.buckets.iter())
            .filter(|bucket| bucket.window.eq_ignore_ascii_case(window))
            .map(|bucket| (bucket.remaining_fraction * 100.0).round() as i32)
            .max()
    }

    pub fn has_usable_quota(&self) -> bool {
        if self.is_forbidden {
            return false;
        }
        match self.remaining_hint_percent() {
            Some(pct) => pct > 0,
            None => true, // unknown → allow until 429
        }
    }
}

struct ProjectMeta {
    project_id: Option<String>,
    subscription_tier: Option<String>,
}

/// Fetch quota for one account access token.
pub async fn fetch_quota(
    access_token: &str,
    cached_project_id: Option<&str>,
) -> AppResult<(QuotaSnapshot, Option<String>)> {
    let client = system_proxy::apply_to_builder(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT),
    )
    .build()
    .map_err(|error| AppError::Other(format!("创建配额 HTTP 客户端失败: {error}")))?;

    let meta = if let Some(pid) = cached_project_id.filter(|value| !value.trim().is_empty()) {
        ProjectMeta {
            project_id: Some(pid.to_string()),
            subscription_tier: None,
        }
    } else {
        fetch_project_meta(&client, access_token).await
    };

    let mut snapshot = fetch_models_quota(
        &client,
        access_token,
        meta.project_id.as_deref(),
        meta.subscription_tier.clone(),
    )
    .await?;

    if let Some(groups) =
        fetch_quota_summary(&client, access_token, meta.project_id.as_deref()).await
    {
        snapshot.groups = groups;
    }
    if snapshot.subscription_tier.is_none() {
        snapshot.subscription_tier = meta.subscription_tier;
    }
    snapshot.last_updated = Utc::now().timestamp();
    Ok((snapshot, meta.project_id))
}

async fn fetch_project_meta(client: &reqwest::Client, access_token: &str) -> ProjectMeta {
    let body = json!({ "metadata": { "ideType": "ANTIGRAVITY" } });
    for url in PROJECT_ENDPOINTS {
        let Ok(response) = client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .header("x-client-name", "antigravity")
            .json(&body)
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            continue;
        };
        let project_id = value
            .get("cloudaicompanionProject")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let subscription_tier = extract_tier(&value);
        return ProjectMeta {
            project_id,
            subscription_tier,
        };
    }
    ProjectMeta {
        project_id: None,
        subscription_tier: None,
    }
}

fn extract_tier(value: &Value) -> Option<String> {
    value
        .get("currentTier")
        .and_then(|tier| {
            tier.get("id")
                .or_else(|| tier.get("name"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/currentTier/0/id")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

async fn fetch_models_quota(
    client: &reqwest::Client,
    access_token: &str,
    project_id: Option<&str>,
    subscription_tier: Option<String>,
) -> AppResult<QuotaSnapshot> {
    let mut payload = if let Some(pid) = project_id {
        json!({ "project": pid })
    } else {
        json!({})
    };
    let mut last_error = String::from("fetchAvailableModels failed");
    let mut tried_without_project = false;

    for url in MODELS_ENDPOINTS {
        loop {
            match client
                .post(url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("x-client-name", "antigravity")
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    if status.as_u16() == 403 {
                        if payload.get("project").is_some() && !tried_without_project {
                            payload = json!({});
                            tried_without_project = true;
                            continue;
                        }
                        let mut forbidden = QuotaSnapshot::empty_forbidden("403 Forbidden");
                        forbidden.subscription_tier = subscription_tier;
                        return Ok(forbidden);
                    }
                    if !status.is_success() {
                        last_error = format!("{url} → {status}: {body}");
                        if status.as_u16() == 429 || status.is_server_error() {
                            break;
                        }
                        return Err(AppError::Other(format!("拉取模型额度失败: {last_error}")));
                    }
                    let value: Value = serde_json::from_str(&body).map_err(|error| {
                        AppError::Other(format!("解析模型额度失败: {error}"))
                    })?;
                    let mut snapshot = parse_models_response(&value);
                    snapshot.subscription_tier = subscription_tier;
                    return Ok(snapshot);
                }
                Err(error) => {
                    last_error = error.to_string();
                    break;
                }
            }
        }
    }
    Err(AppError::Other(format!("拉取模型额度失败: {last_error}")))
}

fn parse_models_response(value: &Value) -> QuotaSnapshot {
    let mut models = Vec::new();
    if let Some(map) = value.get("models").and_then(Value::as_object) {
        for (name, info) in map {
            let quota_info = info.get("quotaInfo");
            let fraction = quota_info
                .and_then(|q| q.get("remainingFraction"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let reset_time = quota_info
                .and_then(|q| q.get("resetTime"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let display_name = info
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string);
            models.push(ModelQuota {
                name: name.clone(),
                percentage: (fraction * 100.0).round() as i32,
                reset_time,
                display_name,
            });
        }
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    QuotaSnapshot {
        models,
        last_updated: Utc::now().timestamp(),
        ..QuotaSnapshot::default()
    }
}

async fn fetch_quota_summary(
    client: &reqwest::Client,
    access_token: &str,
    project_id: Option<&str>,
) -> Option<Vec<QuotaGroup>> {
    let payload = if let Some(pid) = project_id {
        json!({ "project": pid })
    } else {
        json!({})
    };
    for url in SUMMARY_ENDPOINTS {
        let Ok(response) = client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .header("x-client-name", "antigravity")
            .json(&payload)
            .send()
            .await
        else {
            continue;
        };
        let status = response.status();
        if !status.is_success() {
            if status.is_client_error() && status.as_u16() != 429 {
                return None;
            }
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            return None;
        };
        return Some(parse_summary_groups(&value));
    }
    None
}

fn parse_summary_groups(value: &Value) -> Vec<QuotaGroup> {
    let Some(groups) = value.get("groups").and_then(Value::as_array) else {
        return Vec::new();
    };
    groups
        .iter()
        .map(|group| {
            let display_name = group
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let buckets = group
                .get("buckets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|bucket| QuotaBucket {
                    bucket_id: bucket
                        .get("bucketId")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    window: bucket
                        .get("window")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    remaining_fraction: bucket
                        .get("remainingFraction")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    reset_time: bucket
                        .get("resetTime")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    display_name: bucket
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect();
            QuotaGroup {
                display_name,
                buckets,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models_and_summary() {
        let models = json!({
            "models": {
                "claude-sonnet-4-6": {
                    "displayName": "Sonnet",
                    "quotaInfo": { "remainingFraction": 0.42, "resetTime": "2026-08-07T12:00:00Z" }
                }
            }
        });
        let snapshot = parse_models_response(&models);
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(snapshot.models[0].percentage, 42);

        let summary = json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [
                    { "bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.8, "resetTime": "t1" },
                    { "bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.55, "resetTime": "t2" }
                ]
            }]
        });
        let groups = parse_summary_groups(&summary);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].buckets.len(), 2);

        let mut full = snapshot;
        full.groups = groups;
        assert_eq!(full.window_percent("5h"), Some(80));
        assert_eq!(full.window_percent("weekly"), Some(55));
        assert_eq!(full.remaining_hint_percent(), Some(80));
        assert!(full.has_usable_quota());
    }

    #[test]
    fn forbidden_has_zero_hint() {
        let snap = QuotaSnapshot::empty_forbidden("denied");
        assert_eq!(snap.remaining_hint_percent(), Some(0));
        assert!(!snap.has_usable_quota());
    }
}
