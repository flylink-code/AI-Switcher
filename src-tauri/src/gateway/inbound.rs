//! Optional inbound limiter for the smart gateway. All zeros = off.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

pub const SUBAGENT_INHERIT_UPSTREAM_SETTING: &str = "smart_gateway_subagent_inherit_upstream";

pub fn subagent_inherit_upstream(db: &Database) -> bool {
    db.with_read_conn(|conn| Ok(get_setting(conn, SUBAGENT_INHERIT_UPSTREAM_SETTING)?.as_deref() == Some("true")))
        .unwrap_or(false)
}

pub fn persist_subagent_inherit_upstream(db: &Database, enabled: bool) -> AppResult<bool> {
    db.with_conn(|conn| set_setting(conn, SUBAGENT_INHERIT_UPSTREAM_SETTING, if enabled { "true" } else { "false" }))?;
    Ok(enabled)
}

pub const MAX_CONCURRENCY_SETTING: &str = "smart_gateway_max_concurrency";
pub const MIN_INTERVAL_SETTING: &str = "smart_gateway_min_interval_ms";
pub const RPM_SETTING: &str = "smart_gateway_rpm";
pub const BURST_SETTING: &str = "smart_gateway_burst";
pub const ACQUIRE_TIMEOUT_SETTING: &str = "smart_gateway_acquire_timeout_s";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboundLimitSettings {
    pub max_concurrency: u32,
    pub min_interval_ms: u64,
    pub rpm: u32,
    pub burst: u32,
    pub acquire_timeout_secs: u64,
}

impl Default for InboundLimitSettings {
    fn default() -> Self {
        Self {
            max_concurrency: 0,
            min_interval_ms: 0,
            rpm: 0,
            burst: 8,
            acquire_timeout_secs: 8,
        }
    }
}

impl InboundLimitSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.max_concurrency > 64 {
            return Err(AppError::Config("入口并发不能超过 64".into()));
        }
        if self.min_interval_ms > 10_000 {
            return Err(AppError::Config("最小间隔不能超过 10000ms".into()));
        }
        if self.rpm > 600 {
            return Err(AppError::Config("RPM 不能超过 600".into()));
        }
        if self.burst == 0 || self.burst > 64 {
            return Err(AppError::Config("突发令牌须在 1–64".into()));
        }
        if self.acquire_timeout_secs > 120 {
            return Err(AppError::Config("等待超时不能超过 120 秒".into()));
        }
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.max_concurrency > 0 || self.min_interval_ms > 0 || self.rpm > 0
    }
}

struct ConcurrencyState {
    limit: u32,
    active: u32,
}

struct ConcurrencyGate {
    state: Mutex<ConcurrencyState>,
    changed: Notify,
}

impl ConcurrencyGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(ConcurrencyState { limit: 0, active: 0 }),
            changed: Notify::new(),
        }
    }

    fn apply(&self, limit: u32) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).limit = limit;
        self.changed.notify_waiters();
    }

    async fn acquire(self: &Arc<Self>, timeout: Duration) -> Option<InboundPermit> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // 先注册再读状态，避免释放许可发生在检查和等待之间。
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.limit == 0 || state.active < state.limit {
                    state.active = state.active.saturating_add(1);
                    return Some(InboundPermit { gate: self.clone() });
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return None;
            }
        }
    }
}

struct LimiterState {
    settings: InboundLimitSettings,
    gate: Arc<ConcurrencyGate>,
    last_request: Option<Instant>,
    tokens: f64,
    last_refill: Instant,
}

fn state() -> &'static Mutex<LimiterState> {
    static STATE: OnceLock<Mutex<LimiterState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(LimiterState {
            settings: InboundLimitSettings::default(),
            gate: Arc::new(ConcurrencyGate::new()),
            last_request: None,
            tokens: 0.0,
            last_refill: Instant::now(),
        })
    })
}

fn lock_state() -> std::sync::MutexGuard<'static, LimiterState> {
    match state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn current_settings() -> InboundLimitSettings {
    lock_state().settings.clone()
}

pub fn load_from_db(db: &Database) -> InboundLimitSettings {
    let settings = db
        .with_conn(|conn| {
            Ok(InboundLimitSettings {
                max_concurrency: parse_u32(get_setting(conn, MAX_CONCURRENCY_SETTING)?, 0),
                min_interval_ms: parse_u64(get_setting(conn, MIN_INTERVAL_SETTING)?, 0),
                rpm: parse_u32(get_setting(conn, RPM_SETTING)?, 0),
                burst: parse_u32(get_setting(conn, BURST_SETTING)?, 8).max(1),
                acquire_timeout_secs: parse_u64(get_setting(conn, ACQUIRE_TIMEOUT_SETTING)?, 8),
            })
        })
        .unwrap_or_default();
    apply(&settings);
    settings
}

pub fn persist(db: &Database, settings: &InboundLimitSettings) -> AppResult<InboundLimitSettings> {
    settings.validate()?;
    db.with_conn(|conn| {
        set_setting(conn, MAX_CONCURRENCY_SETTING, &settings.max_concurrency.to_string())?;
        set_setting(conn, MIN_INTERVAL_SETTING, &settings.min_interval_ms.to_string())?;
        set_setting(conn, RPM_SETTING, &settings.rpm.to_string())?;
        set_setting(conn, BURST_SETTING, &settings.burst.to_string())?;
        set_setting(conn, ACQUIRE_TIMEOUT_SETTING, &settings.acquire_timeout_secs.to_string())?;
        Ok(())
    })?;
    apply(settings);
    Ok(settings.clone())
}

pub fn apply(settings: &InboundLimitSettings) {
    let mut slot = lock_state();
    let was_active = slot.settings.rpm > 0;
    refill_locked(&mut slot);
    slot.tokens = if was_active {
        slot.tokens.min(settings.burst as f64)
    } else {
        settings.burst as f64
    };
    slot.settings = settings.clone();
    // 原地调整限额，不能让旧请求持有一套 semaphore、新请求另领一套。
    slot.gate.apply(settings.max_concurrency);
    slot.last_refill = Instant::now();
}

pub struct InboundPermit {
    gate: Arc<ConcurrencyGate>,
}

impl Drop for InboundPermit {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().unwrap_or_else(|e| e.into_inner());
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.gate.changed.notify_waiters();
    }
}

pub async fn acquire() -> Result<InboundPermit, InboundLimitError> {
    let (gate, wait_interval, retry_after) = {
        let mut slot = lock_state();
        let settings = slot.settings.clone();
        if settings.min_interval_ms > 0 {
            if let Some(last) = slot.last_request {
                let elapsed = last.elapsed();
                let min = Duration::from_millis(settings.min_interval_ms);
                if elapsed < min {
                    return Err(InboundLimitError {
                        message: "local rate limit: min interval".into(),
                        retry_after_secs: ((min - elapsed).as_millis() as u64).div_ceil(1000).max(1),
                    });
                }
            }
        }
        if settings.rpm > 0 {
            refill_locked(&mut slot);
            if slot.tokens < 1.0 {
                return Err(InboundLimitError {
                    message: "local rate limit: rpm".into(),
                    retry_after_secs: 1,
                });
            }
            slot.tokens -= 1.0;
        }
        slot.last_request = Some(Instant::now());
        (
            slot.gate.clone(),
            Duration::from_secs(settings.acquire_timeout_secs.max(1)),
            settings.acquire_timeout_secs.max(1),
        )
    };
    gate.acquire(wait_interval).await.ok_or_else(|| InboundLimitError {
        message: "local rate limit: concurrency".into(),
        retry_after_secs: retry_after.max(1),
    })
}

fn refill_locked(slot: &mut LimiterState) {
    let rpm = slot.settings.rpm;
    if rpm == 0 {
        return;
    }
    let elapsed = slot.last_refill.elapsed().as_secs_f64();
    let add = elapsed * (rpm as f64 / 60.0);
    slot.tokens = (slot.tokens + add).min(slot.settings.burst as f64);
    slot.last_refill = Instant::now();
}

fn parse_u32(value: Option<String>, default: u32) -> u32 {
    value.and_then(|item| item.parse().ok()).unwrap_or(default)
}

fn parse_u64(value: Option<String>, default: u64) -> u64 {
    value.and_then(|item| item.parse().ok()).unwrap_or(default)
}

#[derive(Debug)]
pub struct InboundLimitError {
    pub message: String,
    pub retry_after_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lowering_limit_keeps_active_requests_counted() {
        let gate = Arc::new(ConcurrencyGate::new());
        gate.apply(2);
        let first = gate.acquire(Duration::ZERO).await.unwrap();
        let second = gate.acquire(Duration::ZERO).await.unwrap();
        gate.apply(1);
        assert!(gate.acquire(Duration::ZERO).await.is_none());
        drop(first);
        assert!(gate.acquire(Duration::ZERO).await.is_none());
        drop(second);
        assert!(gate.acquire(Duration::ZERO).await.is_some());
    }

    #[tokio::test]
    async fn enabling_limit_counts_previously_unlimited_requests() {
        let gate = Arc::new(ConcurrencyGate::new());
        let first = gate.acquire(Duration::ZERO).await.unwrap();
        gate.apply(1);
        assert!(gate.acquire(Duration::ZERO).await.is_none());
        gate.apply(0);
        let second = gate.acquire(Duration::ZERO).await.unwrap();
        gate.apply(1);
        drop(first);
        assert!(gate.acquire(Duration::ZERO).await.is_none());
        drop(second);
        assert!(gate.acquire(Duration::ZERO).await.is_some());
    }

    #[tokio::test]
    async fn increasing_limit_wakes_waiter_without_resetting_active() {
        let gate = Arc::new(ConcurrencyGate::new());
        gate.apply(1);
        let first = gate.acquire(Duration::ZERO).await.unwrap();
        let waiting_gate = gate.clone();
        let waiter = tokio::spawn(async move { waiting_gate.acquire(Duration::from_secs(2)).await });
        tokio::task::yield_now().await;
        gate.apply(2);
        let second = tokio::time::timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap().unwrap();
        assert!(gate.acquire(Duration::ZERO).await.is_none());
        drop((first, second));
        assert_eq!(gate.state.lock().unwrap().active, 0);
    }
}
