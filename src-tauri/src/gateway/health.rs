//! In-memory upstream health for smart-gateway failover and UI.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::time::sleep;

use crate::database::dao::gateway::list_upstream_providers;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::AppResult;
use crate::provider::api_endpoint_url;
use crate::proxy::CIRCUIT_OPEN_SECONDS;

pub const HEALTH_PROBE_SETTING: &str = "smart_gateway_health_probe_secs";
pub const DEFAULT_HEALTH_PROBE_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamHealth {
    pub upstream_id: String,
    pub status: String,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: i64,
    pub last_latency_ms: Option<i64>,
    pub last_checked_at: i64,
}

#[derive(Debug, Clone)]
struct HealthEntry {
    consecutive_failures: u32,
    open_until: Option<Instant>,
    last_latency_ms: Option<i64>,
    last_checked_at: i64,
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

pub fn record_success(upstream_id: &str, latency_ms: Option<i64>) {
    let mut table = lock_table();
    let previous = table.get(upstream_id).and_then(|entry| entry.last_latency_ms);
    table.insert(
        upstream_id.to_string(),
        HealthEntry {
            consecutive_failures: 0,
            open_until: None,
            last_latency_ms: latency_ms.or(previous),
            last_checked_at: chrono::Utc::now().timestamp_millis(),
        },
    );
}

pub fn record_failure(upstream_id: &str) {
    let mut table = lock_table();
    let now = Instant::now();
    let entry = table.entry(upstream_id.to_string()).or_insert(HealthEntry {
        consecutive_failures: 0,
        open_until: None,
        last_latency_ms: None,
        last_checked_at: 0,
    });
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();
    if entry.consecutive_failures >= 2 {
        entry.open_until = Some(now + Duration::from_secs(CIRCUIT_OPEN_SECONDS));
    }
}

/// Background probe failure: update the UI counter without opening the
/// failover circuit. Unauthenticated `/v1/models` 404s must not cool a
/// working upstream that is still serving real traffic.
pub fn record_probe_failure(upstream_id: &str) {
    let mut table = lock_table();
    let entry = table.entry(upstream_id.to_string()).or_insert(HealthEntry {
        consecutive_failures: 0,
        open_until: None,
        last_latency_ms: None,
        last_checked_at: 0,
    });
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    entry.last_checked_at = chrono::Utc::now().timestamp_millis();
}

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
            table.get(id).map(|entry| to_public(id, entry, now)).unwrap_or_else(|| {
                UpstreamHealth {
                    upstream_id: id.clone(),
                    status: "unknown".into(),
                    consecutive_failures: 0,
                    cooldown_remaining_ms: 0,
                    last_latency_ms: None,
                    last_checked_at: 0,
                }
            })
        })
        .collect()
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

pub fn lookup(upstream_id: &str) -> Option<UpstreamHealth> {
    let now = Instant::now();
    let table = lock_table();
    table.get(upstream_id).map(|entry| to_public(upstream_id, entry, now))
}

fn to_public(id: &str, entry: &HealthEntry, now: Instant) -> UpstreamHealth {
    let cooldown_remaining_ms = entry
        .open_until
        .filter(|until| *until > now)
        .map(|until| until.saturating_duration_since(now).as_millis() as i64)
        .unwrap_or(0);
    let status = if cooldown_remaining_ms > 0 {
        "cooling"
    } else if entry.consecutive_failures > 0 {
        "failing"
    } else {
        "ok"
    };
    UpstreamHealth {
        upstream_id: id.to_string(),
        status: status.into(),
        consecutive_failures: entry.consecutive_failures,
        cooldown_remaining_ms,
        last_latency_ms: entry.last_latency_ms,
        last_checked_at: entry.last_checked_at,
    }
}

pub fn rank_for_failover(upstream_id: &str) -> (u8, i64) {
    match lookup(upstream_id) {
        Some(row) if row.status == "cooling" => (2, row.last_latency_ms.unwrap_or(i64::MAX)),
        Some(row) if row.status == "failing" => (1, row.last_latency_ms.unwrap_or(i64::MAX / 2)),
        Some(row) => (0, row.last_latency_ms.unwrap_or(0)),
        None => (0, i64::MAX / 4),
    }
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
    lower.contains("://127.0.0.1")
        || lower.contains("://localhost")
        || lower.contains("://[::1]")
}

fn probe_http_client(url: &str) -> reqwest::Client {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(8));
    if url_targets_loopback(url) {
        // Clash/system proxy often 502s loopback, matching model discovery.
        builder = builder.no_proxy();
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

fn classify_probe_status(status: reqwest::StatusCode) -> bool {
    // Reachability check: 2xx/4xx means the origin answered. 5xx/timeouts are
    // the failures that should show in the upstream health column.
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
        Ok(response) if classify_probe_status(response.status()) => {
            record_success(&provider.id, Some(latency));
        }
        Ok(_) | Err(_) => {
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
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn unique_id(suffix: &str) -> String {
        format!(
            "up_health_{suffix}_{}",
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn probe_failure_does_not_open_circuit() {
        let id = unique_id("probe");
        record_probe_failure(&id);
        record_probe_failure(&id);
        let row = lookup(&id).expect("row");
        assert_eq!(row.status, "failing");
        assert_eq!(row.consecutive_failures, 2);
        assert_eq!(row.cooldown_remaining_ms, 0);
        assert_eq!(rank_for_failover(&id).0, 1);
    }

    #[test]
    fn live_failure_opens_circuit_after_two_hits() {
        let id = unique_id("live");
        record_failure(&id);
        record_failure(&id);
        let row = lookup(&id).expect("row");
        assert_eq!(row.status, "cooling");
        assert!(row.cooldown_remaining_ms > 0);
        assert_eq!(rank_for_failover(&id).0, 2);
    }

    #[test]
    fn success_clears_probe_failures() {
        let id = unique_id("ok");
        record_probe_failure(&id);
        record_success(&id, Some(12));
        let row = lookup(&id).expect("row");
        assert_eq!(row.status, "ok");
        assert_eq!(row.consecutive_failures, 0);
        assert_eq!(row.last_latency_ms, Some(12));
    }

    #[test]
    fn unknown_upstream_is_not_cooling() {
        let id = unique_id("missing");
        assert!(lookup(&id).is_none());
        assert_eq!(rank_for_failover(&id).0, 0);
    }

    #[test]
    fn classify_probe_status_treats_client_errors_as_reachable() {
        assert!(classify_probe_status(reqwest::StatusCode::OK));
        assert!(classify_probe_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(classify_probe_status(reqwest::StatusCode::NOT_FOUND));
        assert!(classify_probe_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(!classify_probe_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!classify_probe_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn materialize_empty_key_is_none() {
        assert_eq!(crate::database::dao::materialize_api_key("").unwrap(), None);
        assert_eq!(crate::database::dao::materialize_api_key("   ").unwrap(), None);
    }
}
