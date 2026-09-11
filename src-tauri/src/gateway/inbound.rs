//! Optional inbound limiter for the smart gateway. All zeros = off.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

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

struct LimiterState {
    settings: InboundLimitSettings,
    semaphore: Option<std::sync::Arc<Semaphore>>,
    last_request: Option<Instant>,
    tokens: f64,
    last_refill: Instant,
}

fn state() -> &'static Mutex<LimiterState> {
    static STATE: OnceLock<Mutex<LimiterState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(LimiterState {
            settings: InboundLimitSettings::default(),
            semaphore: None,
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
    slot.settings = settings.clone();
    slot.semaphore = if settings.max_concurrency > 0 {
        Some(std::sync::Arc::new(Semaphore::new(settings.max_concurrency as usize)))
    } else {
        None
    };
    slot.tokens = settings.burst as f64;
    slot.last_refill = Instant::now();
}

pub struct InboundPermit {
    _permit: Option<OwnedSemaphorePermit>,
}

pub async fn acquire() -> Result<InboundPermit, InboundLimitError> {
    let (settings, semaphore, wait_interval, retry_after) = {
        let mut slot = lock_state();
        let settings = slot.settings.clone();
        if !settings.is_active() {
            return Ok(InboundPermit { _permit: None });
        }
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
            settings.clone(),
            slot.semaphore.clone(),
            Duration::from_secs(settings.acquire_timeout_secs.max(1)),
            settings.acquire_timeout_secs.max(1),
        )
    };
    let permit = if let Some(semaphore) = semaphore {
        match tokio::time::timeout(wait_interval, semaphore.clone().acquire_owned()).await {
            Ok(Ok(permit)) => Some(permit),
            _ => {
                return Err(InboundLimitError {
                    message: "local rate limit: concurrency".into(),
                    retry_after_secs: retry_after.max(1),
                });
            }
        }
    } else {
        None
    };
    let _ = settings;
    Ok(InboundPermit { _permit: permit })
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
