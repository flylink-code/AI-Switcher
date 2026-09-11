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
    db.with_conn(|conn| get_setting(conn, HEALTH_PROBE_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HEALTH_PROBE_SECS)
}

pub async fn probe_loop(db: std::sync::Arc<Database>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .no_proxy()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    loop {
        let interval = probe_interval_secs(&db);
        if interval == 0 {
            sleep(Duration::from_secs(30)).await;
            continue;
        }
        let _ = probe_once(&db, &client).await;
        sleep(Duration::from_secs(interval.max(30))).await;
    }
}

async fn probe_once(db: &Database, client: &reqwest::Client) -> AppResult<()> {
    let providers = db.with_conn(|conn| list_upstream_providers(conn, false))?;
    for provider in providers {
        if provider.base_url.trim().is_empty() || provider.is_smart_gateway() {
            continue;
        }
        let url = match api_endpoint_url(&provider.base_url, "/v1/models") {
            Ok(url) => url,
            Err(_) => continue,
        };
        let started = Instant::now();
        let result = client.get(&url).send().await;
        let latency = started.elapsed().as_millis() as i64;
        match result {
            Ok(response) if response.status().is_success() => {
                record_success(&provider.id, Some(latency));
            }
            Ok(_) | Err(_) => {
                record_failure(&provider.id);
            }
        }
    }
    Ok(())
}
