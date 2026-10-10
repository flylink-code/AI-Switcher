//! In-memory upstream health for smart-gateway failover, circuit breaking and UI.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::database::dao::gateway::list_upstream_providers;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::AppResult;
use crate::provider::api_endpoint_url;
use crate::proxy::CIRCUIT_OPEN_SECONDS;

pub const HEALTH_PROBE_SETTING: &str = "smart_gateway_health_probe_secs";
pub const DEFAULT_HEALTH_PROBE_SECS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Auth,
    Quota,
    RateLimit,
    ModelUnsupported,
    Transient,
    ClientErrorIgnored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureClassification {
    pub kind: FailureKind,
    pub cooldown: Option<Duration>,
    pub safe_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamHealth {
    pub upstream_id: String,
    pub status: String,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: i64,
    pub last_latency_ms: Option<i64>,
    pub last_checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelHealthEntry {
    consecutive_failures: u32,
    open_until: Option<Instant>,
    in_flight_probe: bool,
    last_error: Option<String>,
    last_failure_kind: Option<FailureKind>,
}

impl ModelHealthEntry {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            open_until: None,
            in_flight_probe: false,
            last_error: None,
            last_failure_kind: None,
        }
    }
}

#[derive(Debug, Clone)]
struct HealthEntry {
    consecutive_failures: u32,
    consecutive_rate_limits: u32,
    open_until: Option<Instant>,
    in_flight_probe: bool,
    auth_failed: bool,
    is_live_auth: bool,
    is_live_cooling: bool,
    reachable_unknown: bool,
    has_inferred_success: bool,
    last_error: Option<String>,
    primary_failure_kind: Option<FailureKind>,
    last_latency_ms: Option<i64>,
    last_checked_at: i64,
    model_entries: HashMap<String, ModelHealthEntry>,
}

impl HealthEntry {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            consecutive_rate_limits: 0,
            open_until: None,
            in_flight_probe: false,
            auth_failed: false,
            is_live_auth: false,
            is_live_cooling: false,
            reachable_unknown: false,
            has_inferred_success: false,
            last_error: None,
            primary_failure_kind: None,
            last_latency_ms: None,
            last_checked_at: 0,
            model_entries: HashMap::new(),
        }
    }
}

fn table() -> &'static Mutex<HashMap<String, HealthEntry>> {
    static TABLE: OnceLock<Mutex<HashMap<String, HealthEntry>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_table() -> std::sync::MutexGuard<'static, HashMap<String, HealthEntry>> {
    match table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// Include HTTP failure classifier and retry-after parsing
include!("health/classifier.rs");

// ---------------------------------------------------------------------------
// Health Recording & Circuit Breaker Logic
// ---------------------------------------------------------------------------

fn apply_cooldown(entry: &mut HealthEntry, now: Instant, cooldown: Duration) {
    let target = now.checked_add(cooldown)
        .unwrap_or_else(|| now + Duration::from_secs(365 * 24 * 60 * 60));
    entry.open_until = match entry.open_until {
        Some(existing) if existing > target => Some(existing),
        _ => Some(target),
    };
}

fn apply_model_cooldown(m_entry: &mut ModelHealthEntry, now: Instant, cooldown: Duration) {
    let target = now.checked_add(cooldown)
        .unwrap_or_else(|| now + Duration::from_secs(365 * 24 * 60 * 60));
    m_entry.open_until = match m_entry.open_until {
        Some(existing) if existing > target => Some(existing),
        _ => Some(target),
    };
}

pub fn record_http_failure(
    upstream_id: &str,
    model: Option<&str>,
    status: u16,
    headers: Option<&http::HeaderMap>,
    body: Option<&str>,
) -> FailureKind {
    let classification = classify_failure(status, headers, body);
    record_failure_classification(upstream_id, model, classification.clone());
    classification.kind
}

fn record_failure_classification(
    upstream_id: &str,
    model: Option<&str>,
    classification: FailureClassification,
) {
    let mut table = lock_table();
    let now = Instant::now();
    let entry = table
        .entry(upstream_id.to_string())
        .or_insert_with(HealthEntry::new);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();

    match classification.kind {
        FailureKind::Auth => {
            entry.auth_failed = true;
            entry.is_live_auth = true;
            entry.is_live_cooling = true;
            entry.primary_failure_kind = Some(FailureKind::Auth);
            entry.last_error = classification.safe_description;
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            apply_cooldown(entry, now, Duration::from_secs(COOLDOWN_AUTH_SECS));
        }
        FailureKind::Quota => {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            entry.is_live_cooling = true;
            if !entry.auth_failed {
                entry.primary_failure_kind = Some(FailureKind::Quota);
                entry.last_error = classification.safe_description;
            }
            apply_cooldown(entry, now, Duration::from_secs(COOLDOWN_QUOTA_SECS));
        }
        FailureKind::RateLimit => {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            entry.consecutive_rate_limits = entry.consecutive_rate_limits.saturating_add(1);
            entry.is_live_cooling = true;
            if !entry.auth_failed {
                entry.primary_failure_kind = Some(FailureKind::RateLimit);
                entry.last_error = classification.safe_description;
            }
            let cooldown = classification
                .cooldown
                .unwrap_or_else(|| calculate_rate_limit_backoff(entry.consecutive_rate_limits));
            apply_cooldown(entry, now, cooldown);
        }
        FailureKind::ModelUnsupported => {
            if let Some(m) = model {
                let m_entry = entry
                    .model_entries
                    .entry(m.to_string())
                    .or_insert_with(ModelHealthEntry::new);
                m_entry.consecutive_failures = m_entry.consecutive_failures.saturating_add(1);
                apply_model_cooldown(m_entry, now, Duration::from_secs(COOLDOWN_MODEL_SECS));
                m_entry.last_error = classification.safe_description;
                m_entry.last_failure_kind = Some(FailureKind::ModelUnsupported);
            } else {
                entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
                entry.is_live_cooling = true;
                if !entry.auth_failed {
                    entry.primary_failure_kind = Some(FailureKind::ModelUnsupported);
                    entry.last_error = classification.safe_description;
                }
                apply_cooldown(entry, now, Duration::from_secs(COOLDOWN_MODEL_SECS));
            }
        }
        FailureKind::Transient => {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            if entry.consecutive_failures >= TRANSIENT_FAILURE_THRESHOLD {
                entry.is_live_cooling = true;
                if !entry.auth_failed {
                    entry.primary_failure_kind = Some(FailureKind::Transient);
                    entry.last_error = classification.safe_description;
                }
                apply_cooldown(entry, now, Duration::from_secs(CIRCUIT_OPEN_SECONDS));
            }
        }
        FailureKind::ClientErrorIgnored => {}
    }
}

pub fn record_success(upstream_id: &str, latency_ms: Option<i64>) {
    record_success_scope(upstream_id, None, latency_ms);
}

pub fn record_model_success(upstream_id: &str, model: &str, latency_ms: Option<i64>) {
    record_success_scope(upstream_id, Some(model), latency_ms);
}

pub fn record_success_scope(upstream_id: &str, model: Option<&str>, latency_ms: Option<i64>) {
    let mut table = lock_table();
    let previous_latency = table
        .get(upstream_id)
        .and_then(|entry| entry.last_latency_ms);
    let entry = table
        .entry(upstream_id.to_string())
        .or_insert_with(HealthEntry::new);

    entry.in_flight_probe = false;
    entry.last_latency_ms = latency_ms.or(previous_latency);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();
    entry.has_inferred_success = true;
    entry.reachable_unknown = false;

    if let Some(m) = model {
        if let Some(m_entry) = entry.model_entries.get_mut(m) {
            m_entry.consecutive_failures = 0;
            m_entry.open_until = None;
            m_entry.in_flight_probe = false;
            m_entry.last_error = None;
            m_entry.last_failure_kind = None;
        }
    }

    entry.auth_failed = false;
    entry.is_live_auth = false;
    entry.consecutive_failures = 0;
    entry.consecutive_rate_limits = 0;
    entry.open_until = None;
    entry.last_error = None;
    entry.primary_failure_kind = None;
    entry.is_live_cooling = false;
}

pub fn record_failure(upstream_id: &str) {
    record_failure_scope(upstream_id, None);
}

pub fn record_failure_scope(upstream_id: &str, model: Option<&str>) {
    record_failure_classification(
        upstream_id,
        model,
        FailureClassification {
            kind: FailureKind::Transient,
            cooldown: Some(Duration::from_secs(CIRCUIT_OPEN_SECONDS)),
            safe_description: Some(SAFE_ERR_TRANSIENT.to_string()),
        },
    );
}

pub fn record_probe_failure(upstream_id: &str) {
    let mut table = lock_table();
    let entry = table
        .entry(upstream_id.to_string())
        .or_insert_with(HealthEntry::new);
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();
}

pub fn record_probe_http_status(upstream_id: &str, status: u16, latency_ms: Option<i64>) {
    let mut table = lock_table();
    let now = Instant::now();
    let entry = table
        .entry(upstream_id.to_string())
        .or_insert_with(HealthEntry::new);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();
    entry.last_latency_ms = latency_ms.or(entry.last_latency_ms);

    match status {
        200..=299 => {
            // Only probe-originated auth failure can be cleared by probe 200
            if entry.auth_failed && !entry.is_live_auth {
                entry.auth_failed = false;
                if entry.primary_failure_kind == Some(FailureKind::Auth) {
                    entry.open_until = None;
                    entry.last_error = None;
                    entry.primary_failure_kind = None;
                }
            }
            // If live cooling is active, probe success does NOT clear live cooling!
            let live_cooling_active =
                entry.is_live_cooling && entry.open_until.map(|u| u > now).unwrap_or(false);
            if !live_cooling_active {
                entry.consecutive_failures = 0;
                entry.consecutive_rate_limits = 0;
                entry.open_until = None;
                entry.last_error = None;
                entry.has_inferred_success = true;
                entry.reachable_unknown = false;
            }
        }
        401 | 403 => {
            entry.auth_failed = true;
            entry.primary_failure_kind = Some(FailureKind::Auth);
            apply_cooldown(entry, now, Duration::from_secs(COOLDOWN_AUTH_SECS));
            entry.last_error = Some(SAFE_ERR_AUTH_FAILED.to_string());
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        }
        404 | 405 => {
            if !entry.has_inferred_success && !entry.auth_failed && entry.open_until.is_none() {
                entry.reachable_unknown = true;
            }
        }
        429 => {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
            // Probe 429 must NOT overwrite live quota or active live open_until!
            let live_cooling_active =
                entry.is_live_cooling && entry.open_until.map(|u| u > now).unwrap_or(false);
            if !live_cooling_active && !entry.auth_failed {
                entry.consecutive_rate_limits = entry.consecutive_rate_limits.saturating_add(1);
                let backoff = calculate_rate_limit_backoff(entry.consecutive_rate_limits);
                apply_cooldown(entry, now, backoff);
                entry.last_error = Some(SAFE_ERR_RATE_LIMITED.to_string());
                entry.primary_failure_kind = Some(FailureKind::RateLimit);
            }
        }
        _ if status >= 500 => {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        }
        _ => {}
    }
}

#[allow(dead_code)]
pub fn clear_auth_failure(upstream_id: &str) {
    let mut table = lock_table();
    if let Some(entry) = table.get_mut(upstream_id) {
        entry.auth_failed = false;
        entry.is_live_auth = false;
        if entry.primary_failure_kind == Some(FailureKind::Auth) {
            entry.open_until = None;
            entry.last_error = None;
            entry.primary_failure_kind = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Availability Query & RAII Circuit Permit
// ---------------------------------------------------------------------------

pub fn is_available(upstream_id: &str, model: Option<&str>) -> bool {
    let now = Instant::now();
    let table = lock_table();
    let entry = match table.get(upstream_id) {
        Some(e) => e,
        None => return true,
    };

    if entry.auth_failed && entry.open_until.map(|u| u > now).unwrap_or(true) {
        return false;
    }

    if let Some(until) = entry.open_until {
        if until > now {
            return false;
        }
        if entry.in_flight_probe {
            return false;
        }
    }

    if let Some(m) = model {
        if let Some(m_entry) = entry.model_entries.get(m) {
            if let Some(until) = m_entry.open_until {
                if until > now {
                    return false;
                }
                if m_entry.in_flight_probe {
                    return false;
                }
            }
        }
    }

    true
}

#[derive(Debug)]
pub struct CircuitPermit {
    upstream_id: String,
    model: Option<String>,
    is_upstream_half_open: bool,
    is_model_half_open: bool,
    completed: bool,
}

impl CircuitPermit {
    #[allow(dead_code)]
    pub fn upstream_id(&self) -> &str {
        &self.upstream_id
    }

    #[allow(dead_code)]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn is_half_open(&self) -> bool {
        self.is_upstream_half_open || self.is_model_half_open
    }

    #[allow(dead_code)]
    pub fn disarm(mut self) {
        self.release_permit_scope();
        self.completed = true;
    }

    pub fn record_success(mut self, latency_ms: Option<i64>) {
        record_success_scope(&self.upstream_id, self.model.as_deref(), latency_ms);
        self.release_permit_scope();
        self.completed = true;
    }

    pub fn record_http_failure(
        mut self,
        status: u16,
        headers: Option<&http::HeaderMap>,
        body: Option<&str>,
    ) -> FailureKind {
        let kind = record_http_failure(
            &self.upstream_id,
            self.model.as_deref(),
            status,
            headers,
            body,
        );
        self.release_permit_scope();
        self.completed = true;
        kind
    }

    #[allow(dead_code)]
    pub fn record_failure(mut self) {
        record_failure_scope(&self.upstream_id, self.model.as_deref());
        self.release_permit_scope();
        self.completed = true;
    }

    fn release_permit_scope(&mut self) {
        if self.is_upstream_half_open || self.is_model_half_open {
            let mut table = lock_table();
            if let Some(entry) = table.get_mut(&self.upstream_id) {
                if self.is_upstream_half_open {
                    entry.in_flight_probe = false;
                }
                if self.is_model_half_open {
                    if let Some(m) = &self.model {
                        if let Some(m_entry) = entry.model_entries.get_mut(m) {
                            m_entry.in_flight_probe = false;
                        }
                    }
                }
            }
            self.is_upstream_half_open = false;
            self.is_model_half_open = false;
        }
    }
}

impl Drop for CircuitPermit {
    fn drop(&mut self) {
        self.release_permit_scope();
    }
}

pub fn acquire_permit(upstream_id: &str, model: Option<&str>) -> Option<CircuitPermit> {
    let now = Instant::now();
    let mut table = lock_table();
    let entry = table
        .entry(upstream_id.to_string())
        .or_insert_with(HealthEntry::new);

    if entry.auth_failed && entry.open_until.map(|u| u > now).unwrap_or(true) {
        return None;
    }

    let mut is_upstream_half_open = false;
    if let Some(until) = entry.open_until {
        if until > now {
            return None;
        }
        if entry.in_flight_probe {
            return None;
        }
        entry.in_flight_probe = true;
        is_upstream_half_open = true;
    }

    let mut is_model_half_open = false;
    if let Some(m) = model {
        if let Some(m_entry) = entry.model_entries.get_mut(m) {
            if let Some(until) = m_entry.open_until {
                if until > now {
                    if is_upstream_half_open {
                        entry.in_flight_probe = false;
                    }
                    return None;
                }
                if m_entry.in_flight_probe {
                    if is_upstream_half_open {
                        entry.in_flight_probe = false;
                    }
                    return None;
                }
                m_entry.in_flight_probe = true;
                is_model_half_open = true;
            }
        }
    }

    Some(CircuitPermit {
        upstream_id: upstream_id.to_string(),
        model: model.map(|s| s.to_string()),
        is_upstream_half_open,
        is_model_half_open,
        completed: false,
    })
}

// ---------------------------------------------------------------------------
// Cooldown Query Helpers (For Local 429 Retry-After)
// ---------------------------------------------------------------------------

pub fn min_cooldown_remaining(upstream_id: &str, model: Option<&str>) -> Option<Duration> {
    let now = Instant::now();
    let table = lock_table();
    let entry = table.get(upstream_id)?;

    let mut remaining: Option<Duration> = None;
    if let Some(until) = entry.open_until {
        if until > now {
            remaining = Some(until.saturating_duration_since(now));
        }
    }
    if let Some(m) = model {
        if let Some(m_entry) = entry.model_entries.get(m) {
            if let Some(until) = m_entry.open_until {
                if until > now {
                    let m_dur = until.saturating_duration_since(now);
                    remaining = Some(remaining.map(|r| r.max(m_dur)).unwrap_or(m_dur));
                }
            }
        }
    }
    remaining
}

pub fn min_cooldown_remaining_secs(upstream_id: &str, model: Option<&str>) -> Option<u64> {
    min_cooldown_remaining(upstream_id, model).map(|d| d.as_secs().max(1))
}

#[allow(dead_code)]
pub fn min_cooldown_for_candidates(ids: &[String], model: Option<&str>) -> Option<u64> {
    let mut min_secs: Option<u64> = None;
    for id in ids {
        if let Some(secs) = min_cooldown_remaining_secs(id, model) {
            min_secs = Some(min_secs.map(|m| m.min(secs)).unwrap_or(secs));
        }
    }
    min_secs
}

// ---------------------------------------------------------------------------
// Public Snapshot & Query Functions
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub fn snapshot() -> Vec<UpstreamHealth> {
    let now = Instant::now();
    let table = lock_table();
    table
        .iter()
        .map(|(id, entry)| to_public(id, entry, now))
        .collect()
}

pub fn list_for_upstreams(ids: &[String]) -> Vec<UpstreamHealth> {
    let now = Instant::now();
    let table = lock_table();
    ids.iter()
        .map(|id| {
            table
                .get(id)
                .map(|entry| to_public(id, entry, now))
                .unwrap_or_else(|| UpstreamHealth {
                    upstream_id: id.clone(),
                    status: "unknown".into(),
                    consecutive_failures: 0,
                    cooldown_remaining_ms: 0,
                    last_latency_ms: None,
                    last_checked_at: 0,
                    last_error: None,
                })
        })
        .collect()
}

pub fn lookup(upstream_id: &str) -> Option<UpstreamHealth> {
    let now = Instant::now();
    let table = lock_table();
    table
        .get(upstream_id)
        .map(|entry| to_public(upstream_id, entry, now))
}

fn to_public(id: &str, entry: &HealthEntry, now: Instant) -> UpstreamHealth {
    let cooldown_remaining_ms = entry
        .open_until
        .filter(|until| *until > now)
        .map(|until| until.saturating_duration_since(now).as_millis() as i64)
        .unwrap_or(0);

    let status = if entry.auth_failed {
        "auth_failed"
    } else if cooldown_remaining_ms > 0 {
        if entry.primary_failure_kind == Some(FailureKind::RateLimit) {
            "rate_limited"
        } else {
            "cooling"
        }
    } else if entry.consecutive_failures > 0 {
        "failing"
    } else if entry.reachable_unknown {
        "unknown"
    } else if entry.has_inferred_success || entry.last_checked_at > 0 {
        "ok"
    } else {
        "unknown"
    };

    UpstreamHealth {
        upstream_id: id.to_string(),
        status: status.into(),
        consecutive_failures: entry.consecutive_failures,
        cooldown_remaining_ms,
        last_latency_ms: entry.last_latency_ms,
        last_checked_at: entry.last_checked_at,
        last_error: entry.last_error.clone(),
    }
}

pub fn rank_for_failover(upstream_id: &str) -> (u8, i64) {
    rank_for_failover_model(upstream_id, None)
}

pub fn rank_for_failover_model(upstream_id: &str, model: Option<&str>) -> (u8, i64) {
    let now = Instant::now();
    let table = lock_table();
    match table.get(upstream_id) {
        Some(entry) => {
            let row = to_public(upstream_id, entry, now);
            let model_cooling = model
                .and_then(|m| entry.model_entries.get(m))
                .and_then(|m_entry| m_entry.open_until)
                .map(|until| until > now)
                .unwrap_or(false);

            if row.status == "auth_failed" {
                (3, row.last_latency_ms.unwrap_or(i64::MAX))
            } else if row.status == "cooling" || row.status == "rate_limited" || model_cooling {
                (2, row.last_latency_ms.unwrap_or(i64::MAX))
            } else if row.status == "failing" {
                (1, row.last_latency_ms.unwrap_or(i64::MAX / 2))
            } else {
                (0, row.last_latency_ms.unwrap_or(0))
            }
        }
        None => (0, i64::MAX / 4),
    }
}

pub fn persist_probe_interval(db: &Database, secs: u64) -> AppResult<u64> {
    if secs > 0 && secs < 30 {
        return Err(crate::error::AppError::Config(
            "健康探测间隔至少 30 秒（0 = 关闭）".into(),
        ));
    }
    db.with_conn(|conn| set_setting(conn, HEALTH_PROBE_SETTING, &secs.to_string()))?;
    Ok(secs)
}

pub fn probe_interval_secs(db: &Database) -> u64 {
    db.with_read_conn(|conn| get_setting(conn, HEALTH_PROBE_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HEALTH_PROBE_SECS)
}

pub async fn probe_loop(db: std::sync::Arc<Database>) {
    loop {
        let interval = probe_interval_secs(&db);
        if interval == 0 {
            sleep(Duration::from_secs(30)).await;
            continue;
        }
        let _ = probe_once(&db).await;
        sleep(Duration::from_secs(interval.max(30))).await;
    }
}

fn url_targets_loopback(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.contains("://127.0.0.1") || lower.contains("://localhost") || lower.contains("://[::1]")
}

fn probe_http_client(url: &str) -> reqwest::Client {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(8));
    if url_targets_loopback(url) {
        builder = builder.no_proxy();
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

pub fn classify_probe_status(status: reqwest::StatusCode) -> bool {
    !status.is_server_error()
}

async fn probe_provider(provider: crate::provider::Provider) {
    if provider.base_url.trim().is_empty() || provider.is_smart_gateway() {
        return;
    }
    let url = match api_endpoint_url(&provider.base_url, "/v1/models") {
        Ok(url) => url,
        Err(_) => return,
    };
    let mut key = match crate::database::dao::materialize_api_key(&provider.api_key) {
        Ok(value) => value.unwrap_or_default(),
        Err(_) => String::new(),
    };
    if provider.is_antigravity() && key.trim().is_empty() {
        key = crate::antigravity::gateway::builtin_api_key();
    }
    if provider.is_kiro() && key.trim().is_empty() {
        key = crate::kiro::gateway::builtin_api_key();
    }
    let client = probe_http_client(&url);
    let mut request = client.get(&url);
    if !key.is_empty() {
        request = request
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"))
            .header("x-api-key", key.as_str());
    }
    if let Some(headers) = provider.custom_headers.as_ref() {
        for (name, value) in headers {
            if !name.trim().is_empty() {
                request = request.header(name.as_str(), value.as_str());
            }
        }
    }
    let started = Instant::now();
    let result = request.send().await;
    let latency = started.elapsed().as_millis() as i64;
    match result {
        Ok(response) => {
            record_probe_http_status(&provider.id, response.status().as_u16(), Some(latency));
        }
        Err(_) => {
            record_probe_failure(&provider.id);
        }
    }
}

async fn probe_once(db: &Database) -> AppResult<()> {
    let providers = db.with_read_conn(|conn| list_upstream_providers(conn, false))?;
    for provider in providers {
        probe_provider(provider).await;
    }
    Ok(())
}

#[cfg(test)]
include!("health/tests.rs");
