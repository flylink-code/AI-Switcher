//! Cloud Code quota fetch (independent implementation).
//!
//! Uses `fetchAvailableModels` + `retrieveUserQuotaSummary` with daily → prod → sandbox
//! fallbacks. Sandbox is last: Clash/SOCKS often hangs there. Timeouts match
//! Antigravity-Manager's 15s info client.

use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

/// Cloud Code quota endpoints validate the native Antigravity client fingerprint.
/// A generic `antigravity` user agent is accepted inconsistently and can return
/// an empty/forbidden quota response despite a valid OAuth token.
const QUOTA_USER_AGENT: &str = "vscode/1.X.X (Antigravity/4.3.0)";

/// Connect / total timeouts for quota probes (Antigravity-Manager uses 15s).
const QUOTA_CONNECT_SECS: u64 = 8;
const QUOTA_TIMEOUT_SECS: u64 = 15;

const MODELS_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels",
];

const SUMMARY_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
];

const PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
];

const ONBOARD_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:onboardUser",
    "https://cloudcode-pa.googleapis.com/v1internal:onboardUser",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:onboardUser",
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

    /// Best single remaining fraction 0.0 - 1.0.
    pub fn best_remaining_fraction(&self) -> Option<f64> {
        if self.is_forbidden {
            return Some(0.0);
        }
        self.remaining_hint_percent()
            .map(|pct| (pct as f64) / 100.0)
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
            .filter(|bucket| bucket_window_matches(bucket, "5h"))
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
            .filter(|bucket| bucket_window_matches(bucket, window))
            .map(|bucket| (bucket.remaining_fraction * 100.0).round() as i32)
            .max()
    }

    /// Remaining-% for Gemini buckets (`gemini-*` / display name contains Gemini).
    pub fn gemini_window_percent(&self, window: &str) -> Option<i32> {
        self.group_window_percent(window, QuotaFamily::Gemini)
    }

    /// Remaining-% for Claude + GPT / third-party buckets (`3p-*` or non-Gemini groups).
    pub fn claude_window_percent(&self, window: &str) -> Option<i32> {
        self.group_window_percent(window, QuotaFamily::ClaudeGpt)
    }

    fn group_window_percent(&self, window: &str, family: QuotaFamily) -> Option<i32> {
        self.groups
            .iter()
            .flat_map(|group| {
                let group_is_gemini = group_looks_gemini(&group.display_name);
                group.buckets.iter().filter(move |bucket| {
                    if !bucket_window_matches(bucket, window) {
                        return false;
                    }
                    match family {
                        QuotaFamily::Gemini => {
                            bucket_looks_gemini(&bucket.bucket_id) || group_is_gemini
                        }
                        QuotaFamily::ClaudeGpt => {
                            bucket_looks_claude_gpt(&bucket.bucket_id)
                                || (!group_is_gemini && !bucket_looks_gemini(&bucket.bucket_id))
                        }
                    }
                })
            })
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

    /// `fetchAvailableModels` has no 5h/weekly groups. If summary is empty or
    /// timed out, keep the previous bars instead of wiping them to `—`.
    pub fn retain_groups_if_empty(&mut self, previous: Option<&QuotaSnapshot>) {
        if !self.groups.is_empty() {
            return;
        }
        if let Some(groups) = previous
            .map(|quota| quota.groups.clone())
            .filter(|groups| !groups.is_empty())
        {
            self.groups = groups;
        }
    }
}

struct ProjectMeta {
    project_id: Option<String>,
    subscription_tier: Option<String>,
}

/// Resolve Cloud Code `cloudaicompanionProject`, onboarding when loadCodeAssist
/// returns a tier but no project yet (new / never-opened Antigravity accounts).
pub async fn resolve_project_id(access_token: &str) -> AppResult<String> {
    let client = crate::antigravity::outbound::build_async_client(
        QUOTA_CONNECT_SECS,
        QUOTA_TIMEOUT_SECS,
    );
    fetch_project_meta(&client, access_token)
        .await
        .project_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Other("账号无法获取 cloudaicompanionProject".into()))
}

/// Fetch quota for one account access token.
pub async fn fetch_quota(
    access_token: &str,
    cached_project_id: Option<&str>,
) -> AppResult<(QuotaSnapshot, Option<String>)> {
    let client = crate::antigravity::outbound::build_async_client(
        QUOTA_CONNECT_SECS,
        QUOTA_TIMEOUT_SECS,
    );

    let cached_pid = cached_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // Bars come from retrieveUserQuotaSummary. Run it alongside models (and
    // loadCodeAssist when project_id is already cached) so a slow host on one
    // API does not starve the 5h/weekly snapshot.
    let (mut meta, mut snapshot) = if let Some(pid) = cached_pid {
        let (meta, models_result, groups) = tokio::join!(
            fetch_project_meta(&client, access_token),
            fetch_models_quota(&client, access_token, Some(pid), None),
            fetch_quota_summary_bounded(&client, access_token, Some(pid)),
        );
        let mut snapshot = models_result?;
        if let Some(groups) = groups {
            snapshot.groups = groups;
        }
        (meta, snapshot)
    } else {
        let meta = fetch_project_meta(&client, access_token).await;
        let (models_result, groups) = tokio::join!(
            fetch_models_quota(
                &client,
                access_token,
                meta.project_id.as_deref(),
                meta.subscription_tier.clone(),
            ),
            fetch_quota_summary_bounded(&client, access_token, meta.project_id.as_deref()),
        );
        let mut snapshot = models_result?;
        if let Some(groups) = groups {
            snapshot.groups = groups;
        }
        (meta, snapshot)
    };

    if meta.project_id.is_none() {
        if let Some(pid) = cached_pid {
            meta.project_id = Some(pid.to_string());
        }
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
        let started = Instant::now();
        let Ok(response) = client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", QUOTA_USER_AGENT)
            .json(&body)
            .send()
            .await
        else {
            log::warn!(
                "Antigravity loadCodeAssist {url} request failed in {}ms",
                started.elapsed().as_millis()
            );
            continue;
        };
        if !response.status().is_success() {
            log::warn!(
                "Antigravity loadCodeAssist {url} → {} in {}ms",
                response.status(),
                started.elapsed().as_millis()
            );
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            continue;
        };
        let subscription_tier = extract_tier(&value);
        if let Some(project_id) = extract_cloudaicompanion_project(&value) {
            return ProjectMeta {
                project_id: Some(project_id),
                subscription_tier,
            };
        }
        // loadCodeAssist often returns paidTier (PRO) with no project until the
        // account has been onboarded. Do not treat this as a finished lookup.
        log::info!(
            "Antigravity loadCodeAssist {url} has no project; trying onboardUser"
        );
        let project_id = onboard_user_project(client, access_token, &value).await;
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

fn extract_cloudaicompanion_project(value: &Value) -> Option<String> {
    for key in ["cloudaicompanionProject", "projectId", "project"] {
        let Some(field) = value.get(key) else {
            continue;
        };
        if let Some(text) = field.as_str().map(str::trim).filter(|text| !text.is_empty()) {
            return Some(normalize_project_id(text));
        }
        if let Some(obj) = field.as_object() {
            for nested in ["id", "projectId", "name"] {
                if let Some(text) = obj
                    .get(nested)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    return Some(normalize_project_id(text));
                }
            }
        }
    }
    None
}

fn extract_project_from_onboard(value: &Value) -> Option<String> {
    value
        .get("response")
        .and_then(extract_cloudaicompanion_project)
        .or_else(|| extract_cloudaicompanion_project(value))
}

fn normalize_project_id(raw: &str) -> String {
    raw.trim()
        .strip_prefix("projects/")
        .unwrap_or(raw.trim())
        .to_string()
}

fn default_onboard_tier_id(load: &Value) -> String {
    if let Some(tiers) = load.get("allowedTiers").and_then(Value::as_array) {
        let default = tiers.iter().find(|tier| {
            tier.get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
        if let Some(id) = default.and_then(tier_id_value) {
            return id;
        }
    }
    extract_tier_id_from_key(load, "currentTier")
        .or_else(|| extract_tier_id_from_key(load, "paidTier"))
        .unwrap_or_else(|| "free-tier".to_string())
}

fn extract_tier_id_from_key(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(tier_id_value)
}

fn tier_id_value(tier: &Value) -> Option<String> {
    if let Some(text) = tier
        .get("id")
        .or_else(|| tier.get("slug"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_string());
    }
    tier.as_array()
        .and_then(|items| items.first())
        .and_then(tier_id_value)
}

async fn onboard_user_project(
    client: &reqwest::Client,
    access_token: &str,
    load: &Value,
) -> Option<String> {
    let tier_id = default_onboard_tier_id(load);
    log::info!("Antigravity onboardUser starting tier={tier_id}");
    let body = json!({
        "tier_id": tier_id,
        "metadata": {
            "ide_type": "ANTIGRAVITY",
            "ide_version": "4.3.0",
            "ide_name": "antigravity",
        }
    });
    for url in ONBOARD_ENDPOINTS {
        for attempt in 1..=5 {
            let started = Instant::now();
            let Ok(response) = client
                .post(url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("User-Agent", QUOTA_USER_AGENT)
                .json(&body)
                .send()
                .await
            else {
                log::warn!(
                    "Antigravity onboardUser {url} request failed in {}ms",
                    started.elapsed().as_millis()
                );
                break;
            };
            let status = response.status();
            let Ok(value) = response.json::<Value>().await else {
                log::warn!(
                    "Antigravity onboardUser {url} → {status} JSON parse failed in {}ms",
                    started.elapsed().as_millis()
                );
                break;
            };
            if !status.is_success() {
                log::warn!(
                    "Antigravity onboardUser {url} → {status} in {}ms",
                    started.elapsed().as_millis()
                );
                break;
            }
            if let Some(project_id) = extract_project_from_onboard(&value) {
                log::info!("Antigravity onboardUser {url} assigned project");
                return Some(project_id);
            }
            let done = value
                .get("done")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if done {
                log::warn!("Antigravity onboardUser {url} completed without project");
                break;
            }
            log::info!("Antigravity onboardUser {url} poll {attempt}/5 not done yet");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    None
}

#[derive(Clone, Copy)]
enum QuotaFamily {
    Gemini,
    ClaudeGpt,
}

fn group_looks_gemini(display_name: &str) -> bool {
    display_name.to_ascii_lowercase().contains("gemini")
}

fn bucket_looks_gemini(bucket_id: &str) -> bool {
    let id = bucket_id.to_ascii_lowercase();
    id.starts_with("gemini-") || id.contains("gemini")
}

fn bucket_looks_claude_gpt(bucket_id: &str) -> bool {
    let id = bucket_id.to_ascii_lowercase();
    id.starts_with("3p-")
        || id.contains("claude")
        || id.contains("gpt")
        || id.contains("openai")
}

/// Cloud Code window labels vary: `weekly` / `7d` / `week` / `168h`, and
/// `5h` / `5hr` / `session`. Empty `window` falls back to `bucketId`.
pub fn normalize_quota_window(window: &str, bucket_id: &str) -> String {
    let haystack = format!("{window} {bucket_id}").to_ascii_lowercase();
    if window_haystack_is_weekly(&haystack) {
        return "weekly".to_string();
    }
    if window_haystack_is_five_hour(&haystack) {
        return "5h".to_string();
    }
    window.trim().to_ascii_lowercase()
}

fn bucket_window_matches(bucket: &QuotaBucket, wanted: &str) -> bool {
    normalize_quota_window(&bucket.window, &bucket.bucket_id)
        == normalize_quota_window(wanted, "")
}

fn window_haystack_is_weekly(haystack: &str) -> bool {
    haystack.contains("weekly")
        || haystack.contains("7d")
        || haystack.contains("7-day")
        || haystack.contains("7day")
        || haystack.contains("168h")
        || haystack.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|part| part == "week")
}

fn window_haystack_is_five_hour(haystack: &str) -> bool {
    haystack.contains("5h")
        || haystack.contains("5hr")
        || haystack.contains("five_hour")
        || haystack.contains("five-hour")
        || haystack.contains("fivehour")
        || haystack.contains("session")
}

fn tier_field_value(tier: &Value) -> Option<&str> {
    // Prefer human-readable name (AG Manager does the same); id is often a slug
    // like `free-tier` even when a paid plan name is present elsewhere.
    tier.get("name")
        .or_else(|| tier.get("id"))
        .or_else(|| tier.get("slug"))
        .or_else(|| tier.get("quotaTier"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_tier_from_key(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|tier| {
        if let Some(text) = tier_field_value(tier) {
            return Some(text.to_string());
        }
        tier.as_array()
            .and_then(|items| items.first())
            .and_then(tier_field_value)
            .map(str::to_string)
    })
}

fn is_ineligible(value: &Value) -> bool {
    value
        .get("ineligibleTiers")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn extract_default_allowed_tier(value: &Value) -> Option<String> {
    let tiers = value.get("allowedTiers")?.as_array()?;
    let default = tiers.iter().find(|tier| {
        tier.get("isDefault")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    default
        .or_else(|| tiers.first())
        .and_then(tier_field_value)
        .map(|text| format!("{text} (Restricted)"))
}

/// Prefer `paidTier` (AI Pro / Ultra) over `currentTier` (often free-tier).
/// Mirrors Antigravity-Manager's multi-level fallback including ineligible/allowed.
fn extract_tier(value: &Value) -> Option<String> {
    let paid = extract_tier_from_key(value, "paidTier");
    if let Some(paid) = paid {
        log::info!(
            "Antigravity tier from paidTier={paid} current={:?}",
            extract_tier_from_key(value, "currentTier")
        );
        return Some(normalize_tier_label(paid));
    }

    if is_ineligible(value) {
        let restricted = extract_default_allowed_tier(value);
        log::info!("Antigravity tier ineligible → {restricted:?}");
        return restricted.map(normalize_tier_label);
    }

    let current = extract_tier_from_key(value, "currentTier");
    log::info!(
        "Antigravity tier from currentTier={current:?} (no paidTier); keys={:?}",
        value
            .as_object()
            .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
    );
    current.map(normalize_tier_label)
}

fn normalize_tier_label(raw: String) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    if upper.contains("ULTRA") {
        return "ULTRA".to_string();
    }
    // Avoid matching the substring in unrelated ids; require PRO as a token-ish hit.
    if upper.contains("PRO") && !upper.contains("PROMPT") {
        return "PRO".to_string();
    }
    if upper.contains("FREE") {
        return "FREE".to_string();
    }
    // Google often returns id/slug `free-tier`.
    if upper == "FREE-TIER" || upper == "FREE_TIER" || upper.ends_with("-FREE") {
        return "FREE".to_string();
    }
    raw.trim().to_string()
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
                .header("User-Agent", QUOTA_USER_AGENT)
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
                        log::warn!("Antigravity fetchAvailableModels {last_error}");
                        // Auth/forbidden handled above. Other 4xx (sandbox 404) and
                        // 5xx/429: try the next host instead of failing the refresh.
                        break;
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
    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut push_model = |name: String, percentage: i32, reset_time: String, display_name: Option<String>| {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() || !seen.insert(trimmed.clone()) {
            return;
        }
        models.push(ModelQuota {
            name: trimmed,
            percentage,
            reset_time,
            display_name,
        });
    };

    if let Some(map) = value.get("models").and_then(Value::as_object) {
        for (name, info) in map {
            let quota_info = info.get("quotaInfo");
            let fraction = quota_info
                .and_then(parse_remaining_fraction)
                .unwrap_or(0.0);
            let reset_time = quota_info
                .and_then(|q| json_string(q.get("resetTime").or_else(|| q.get("reset_time"))))
                .unwrap_or_default();
            let display_name = json_string(info.get("displayName").or_else(|| info.get("display_name")));
            push_model(
                name.clone(),
                (fraction * 100.0).round() as i32,
                reset_time,
                display_name,
            );
        }
    } else if let Some(arr) = value.get("models").and_then(Value::as_array) {
        for info in arr {
            let name = json_string(
                info.get("name")
                    .or_else(|| info.get("id"))
                    .or_else(|| info.get("model")),
            )
            .unwrap_or_default();
            let quota_info = info.get("quotaInfo");
            let fraction = quota_info
                .and_then(parse_remaining_fraction)
                .unwrap_or(0.0);
            let reset_time = quota_info
                .and_then(|q| json_string(q.get("resetTime").or_else(|| q.get("reset_time"))))
                .unwrap_or_default();
            let display_name = json_string(info.get("displayName").or_else(|| info.get("display_name")));
            push_model(
                name,
                (fraction * 100.0).round() as i32,
                reset_time,
                display_name,
            );
        }
    }

    for key in ["agentModelSorts", "clientModelSorts"] {
        if let Some(sorts) = value.get(key) {
            for id in collect_sort_model_ids(sorts) {
                push_model(id, 0, String::new(), None);
            }
        }
    }

    models.sort_by(|left, right| left.name.cmp(&right.name));
    QuotaSnapshot {
        models,
        last_updated: Utc::now().timestamp(),
        ..QuotaSnapshot::default()
    }
}

fn collect_sort_model_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    walk_model_ids(value, &mut ids);
    ids
}

fn walk_model_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(arr) = map.get("modelIds").and_then(Value::as_array) {
                for item in arr {
                    if let Some(id) = item.as_str().map(str::trim).filter(|id| !id.is_empty()) {
                        if !ids.iter().any(|existing| existing == id) {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
            for nested in map.values() {
                walk_model_ids(nested, ids);
            }
        }
        Value::Array(arr) => {
            for nested in arr {
                walk_model_ids(nested, ids);
            }
        }
        _ => {}
    }
}

async fn fetch_quota_summary_bounded(
    client: &reqwest::Client,
    access_token: &str,
    project_id: Option<&str>,
) -> Option<Vec<QuotaGroup>> {
    match tokio::time::timeout(
        Duration::from_secs(25),
        fetch_quota_summary(client, access_token, project_id),
    )
    .await
    {
        Ok(groups) => groups,
        Err(_) => {
            log::warn!("Antigravity quota summary timed out after 25s");
            None
        }
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
    let mut best: Option<Vec<QuotaGroup>> = None;
    let mut best_score = -1i32;
    for url in SUMMARY_ENDPOINTS {
        if url.contains(".sandbox.googleapis.com") && best_score >= 10 {
            log::info!(
                "Antigravity quota summary skipping sandbox (already have score={best_score})"
            );
            continue;
        }
        let started = Instant::now();
        let Ok(response) = client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", QUOTA_USER_AGENT)
            .json(&payload)
            .send()
            .await
        else {
            log::warn!(
                "Antigravity quota summary {url} request failed in {}ms",
                started.elapsed().as_millis()
            );
            continue;
        };
        let status = response.status();
        let elapsed_ms = started.elapsed().as_millis();
        if !status.is_success() {
            log::warn!("Antigravity quota summary {url} → {status} in {elapsed_ms}ms");
            // 401/403: every host will reject the same token.
            if status.as_u16() == 401 || status.as_u16() == 403 {
                break;
            }
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            log::warn!("Antigravity quota summary {url} JSON parse failed in {elapsed_ms}ms");
            continue;
        };
        let groups = parse_summary_groups(&value);
        let score = summary_completeness_score(&groups);
        log::info!(
            "Antigravity quota summary {url} → {} groups score={score} in {elapsed_ms}ms",
            groups.len()
        );
        if score > best_score {
            best_score = score;
            best = Some(groups);
            if score >= 20 {
                break;
            }
        }
    }
    best.filter(|groups| !groups.is_empty())
}

fn summary_completeness_score(groups: &[QuotaGroup]) -> i32 {
    let mut has_5h = false;
    let mut has_weekly = false;
    let mut buckets = 0i32;
    for bucket in groups.iter().flat_map(|group| group.buckets.iter()) {
        buckets += 1;
        let window = normalize_quota_window(&bucket.window, &bucket.bucket_id);
        if window == "5h" {
            has_5h = true;
        }
        if window == "weekly" {
            has_weekly = true;
        }
    }
    i32::from(has_5h) * 10 + i32::from(has_weekly) * 10 + buckets
}

fn parse_summary_groups(value: &Value) -> Vec<QuotaGroup> {
    let Some(groups) = extract_groups_array(value) else {
        return Vec::new();
    };
    groups
        .iter()
        .map(|group| {
            let display_name = json_string(
                group
                    .get("displayName")
                    .or_else(|| group.get("display_name")),
            )
            .unwrap_or_default();
            let buckets = group
                .get("buckets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|bucket| parse_summary_bucket(bucket))
                .collect();
            QuotaGroup {
                display_name,
                buckets,
            }
        })
        .filter(|group| !group.buckets.is_empty() || !group.display_name.is_empty())
        .collect()
}

fn extract_groups_array(value: &Value) -> Option<&Vec<Value>> {
    value
        .get("groups")
        .and_then(Value::as_array)
        .or_else(|| {
            value
                .get("quotaSummary")
                .and_then(|nested| nested.get("groups"))
                .and_then(Value::as_array)
        })
        .or_else(|| {
            value
                .pointer("/userQuotaSummary/groups")
                .and_then(Value::as_array)
        })
}

fn parse_summary_bucket(bucket: &Value) -> Option<QuotaBucket> {
    let remaining_fraction = parse_remaining_fraction(bucket)?;
    let bucket_id = json_string(
        bucket
            .get("bucketId")
            .or_else(|| bucket.get("bucket_id"))
            .or_else(|| bucket.get("id")),
    )
    .unwrap_or_default();
    let raw_window = json_window_str(bucket.get("window").or_else(|| bucket.get("resetWindow")));
    let window = normalize_quota_window(&raw_window, &bucket_id);
    let reset_time = json_string(
        bucket
            .get("resetTime")
            .or_else(|| bucket.get("reset_time")),
    )
    .unwrap_or_default();
    let display_name = json_string(
        bucket
            .get("displayName")
            .or_else(|| bucket.get("display_name")),
    );
    Some(QuotaBucket {
        bucket_id,
        window,
        remaining_fraction,
        reset_time,
        display_name,
    })
}

fn json_window_str(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(map) = value.as_object() {
        for key in ["type", "window", "duration", "id", "name"] {
            if let Some(text) = map.get(key).and_then(Value::as_str) {
                return text.to_string();
            }
        }
    }
    String::new()
}

fn json_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn json_number_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|n| n as f64))
        .or_else(|| value.as_u64().map(|n| n as f64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn parse_remaining_fraction(value: &Value) -> Option<f64> {
    let fraction = value
        .get("remainingFraction")
        .or_else(|| value.get("remaining_fraction"))
        .and_then(json_number_as_f64);
    if let Some(frac) = fraction {
        return normalize_fraction(frac);
    }
    let percent = value
        .get("remainingPercent")
        .or_else(|| value.get("remainingPercentage"))
        .or_else(|| value.get("remaining_percent"))
        .and_then(json_number_as_f64)?;
    if (0.0..=1.0).contains(&percent) {
        return Some(percent);
    }
    if (0.0..=100.0).contains(&percent) {
        return Some(percent / 100.0);
    }
    None
}

fn normalize_fraction(frac: f64) -> Option<f64> {
    if (0.0..=1.0).contains(&frac) {
        return Some(frac);
    }
    if (1.0..=100.0).contains(&frac) {
        return Some(frac / 100.0);
    }
    None
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
            "groups": [
                {
                    "displayName": "Gemini Models",
                    "buckets": [
                        { "bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.8, "resetTime": "t1" },
                        { "bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.55, "resetTime": "t2" }
                    ]
                },
                {
                    "displayName": "Claude + GPT",
                    "buckets": [
                        { "bucketId": "3p-claude-5h", "window": "5h", "remainingFraction": 0.3, "resetTime": "t3" },
                        { "bucketId": "3p-claude-weekly", "window": "weekly", "remainingFraction": 0.2, "resetTime": "t4" }
                    ]
                }
            ]
        });
        let groups = parse_summary_groups(&summary);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].buckets.len(), 2);

        let mut full = snapshot;
        full.groups = groups;
        assert_eq!(full.window_percent("5h"), Some(80));
        assert_eq!(full.window_percent("weekly"), Some(55));
        assert_eq!(full.gemini_window_percent("5h"), Some(80));
        assert_eq!(full.gemini_window_percent("weekly"), Some(55));
        assert_eq!(full.claude_window_percent("5h"), Some(30));
        assert_eq!(full.claude_window_percent("weekly"), Some(20));
        assert_eq!(full.remaining_hint_percent(), Some(80));
        assert!(full.has_usable_quota());
    }

    #[test]
    fn weekly_aliases_and_bucket_ids_map_to_weekly_percent() {
        let summary = json!({
            "quotaSummary": {
                "groups": [
                    {
                        "displayName": "Gemini Models",
                        "buckets": [
                            { "bucketId": "gemini-5h", "window": "5hr", "remainingFraction": 0.9, "resetTime": "t1" },
                            { "bucketId": "gemini-7d", "window": "7d", "remainingFraction": 0.4, "resetTime": "t2" }
                        ]
                    },
                    {
                        "displayName": "Claude and GPT models",
                        "buckets": [
                            { "id": "3p-5h", "window": { "type": "session" }, "remainingPercent": 70, "resetTime": "t3" },
                            { "bucketId": "3p-weekly", "remainingFraction": 0.25, "resetTime": "t4" }
                        ]
                    }
                ]
            }
        });
        let groups = parse_summary_groups(&summary);
        let mut snap = QuotaSnapshot::default();
        snap.groups = groups;
        assert_eq!(snap.gemini_window_percent("5h"), Some(90));
        assert_eq!(snap.gemini_window_percent("weekly"), Some(40));
        assert_eq!(snap.claude_window_percent("5h"), Some(70));
        assert_eq!(snap.claude_window_percent("weekly"), Some(25));
        assert_eq!(normalize_quota_window("7d", ""), "weekly");
        assert_eq!(normalize_quota_window("", "gemini-weekly"), "weekly");
    }

    #[test]
    fn models_response_harvests_agent_model_sorts() {
        let value = json!({
            "models": {
                "claude-sonnet-4-6": {
                    "displayName": "Sonnet",
                    "quotaInfo": { "remainingFraction": 0.5, "resetTime": "t" }
                }
            },
            "agentModelSorts": [
                { "groups": [{ "modelIds": ["gemini-3.7-flash", "claude-sonnet-4-6"] }] }
            ]
        });
        let snapshot = parse_models_response(&value);
        let names: Vec<_> = snapshot.models.iter().map(|model| model.name.as_str()).collect();
        assert!(names.contains(&"claude-sonnet-4-6"));
        assert!(names.contains(&"gemini-3.7-flash"));
    }

    #[test]
    fn extract_tier_prefers_paid_over_current() {
        let paid = json!({
            "paidTier": { "id": "AI_PRO", "name": "Google AI Pro" },
            "currentTier": { "id": "free-tier", "name": "Free" }
        });
        assert_eq!(extract_tier(&paid).as_deref(), Some("PRO"));

        let free_only = json!({ "currentTier": { "name": "free-tier" } });
        assert_eq!(extract_tier(&free_only).as_deref(), Some("FREE"));

        let ultra = json!({ "paidTier": [{ "id": "ULTRA_PLAN" }] });
        assert_eq!(extract_tier(&ultra).as_deref(), Some("ULTRA"));

        // Prefer name over id (id can look free-like while name is Pro).
        let name_wins = json!({
            "paidTier": { "id": "tier-1", "name": "AI Pro" },
            "currentTier": { "id": "free-tier" }
        });
        assert_eq!(extract_tier(&name_wins).as_deref(), Some("PRO"));

        let restricted = json!({
            "currentTier": { "id": "free-tier" },
            "ineligibleTiers": [{ "reasonCode": "REGION" }],
            "allowedTiers": [
                { "id": "standard", "name": "Standard", "isDefault": true }
            ]
        });
        assert_eq!(
            extract_tier(&restricted).as_deref(),
            Some("Standard (Restricted)")
        );
    }

    #[test]
    fn extracts_project_from_string_or_object() {
        let as_string = json!({ "cloudaicompanionProject": "gen-lang-client-abc" });
        assert_eq!(
            extract_cloudaicompanion_project(&as_string).as_deref(),
            Some("gen-lang-client-abc")
        );

        let as_object = json!({
            "paidTier": { "id": "AI_PRO", "name": "Google AI Pro" },
            "cloudaicompanionProject": { "id": "proj-9", "name": "projects/proj-9" }
        });
        assert_eq!(
            extract_cloudaicompanion_project(&as_object).as_deref(),
            Some("proj-9")
        );

        let onboard = json!({
            "done": true,
            "response": {
                "cloudaicompanionProject": { "id": "onboard-1" }
            }
        });
        assert_eq!(
            extract_project_from_onboard(&onboard).as_deref(),
            Some("onboard-1")
        );
    }

    #[test]
    fn onboard_tier_prefers_allowed_default() {
        let load = json!({
            "paidTier": { "id": "AI_PRO" },
            "currentTier": { "id": "free-tier" },
            "allowedTiers": [
                { "id": "legacy-tier" },
                { "id": "standard-tier", "isDefault": true }
            ]
        });
        assert_eq!(default_onboard_tier_id(&load), "standard-tier");

        let paid_only = json!({ "paidTier": { "id": "AI_PRO" } });
        assert_eq!(default_onboard_tier_id(&paid_only), "AI_PRO");
    }

    #[test]
    fn empty_groups_keep_previous_summary_bars() {
        let previous = QuotaSnapshot {
            groups: vec![QuotaGroup {
                display_name: "Gemini Models".into(),
                buckets: vec![QuotaBucket {
                    bucket_id: "gemini-weekly".into(),
                    window: "weekly".into(),
                    remaining_fraction: 0.42,
                    reset_time: "t".into(),
                    display_name: None,
                }],
            }],
            last_updated: 1,
            ..QuotaSnapshot::default()
        };
        let mut next = QuotaSnapshot {
            last_updated: 2,
            ..QuotaSnapshot::default()
        };
        next.retain_groups_if_empty(Some(&previous));
        assert_eq!(next.gemini_window_percent("weekly"), Some(42));
    }

    #[test]
    fn forbidden_has_zero_hint() {
        let snap = QuotaSnapshot::empty_forbidden("denied");
        assert_eq!(snap.remaining_hint_percent(), Some(0));
        assert!(!snap.has_usable_quota());
    }
}
