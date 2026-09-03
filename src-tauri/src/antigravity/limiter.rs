//! Per-account concurrency gates and adaptive RPM throttling for Antigravity.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::{AppError, AppResult};

/// Shared in-flight cap for main sessions and subagents on one account.
pub const DEFAULT_ACCOUNT_CONCURRENCY: usize = 4;
/// Extra cap for catalog subagent traffic on the same account.
pub const DEFAULT_SUBAGENT_CONCURRENCY: usize = 2;
/// Wait this long for a slot, then return busy (never bypass the gate).
pub const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 8;
pub const DEFAULT_MIN_REQUEST_INTERVAL_MS: u64 = 300;
pub const DEFAULT_RATE_PER_MIN: u32 = 30;
pub const DEFAULT_TOKEN_BURST: u32 = 8;

const MIN_RATE_PER_MIN: f64 = 6.0;
const RATE_RECOVERY_SUCCESS_STREAK: u32 = 5;
const RATE_RECOVERY_STEP: f64 = 3.0;
const BACKOFF_AFTER_429: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimiterSettings {
    pub account_concurrency: usize,
    pub subagent_concurrency: usize,
    pub min_request_interval_ms: u64,
    /// 0 disables the token bucket (concurrency gates still apply).
    pub rate_per_min: u32,
    pub token_burst: u32,
    pub acquire_timeout_secs: u64,
}

impl Default for LimiterSettings {
    fn default() -> Self {
        Self {
            account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
            subagent_concurrency: DEFAULT_SUBAGENT_CONCURRENCY,
            min_request_interval_ms: DEFAULT_MIN_REQUEST_INTERVAL_MS,
            rate_per_min: DEFAULT_RATE_PER_MIN,
            token_burst: DEFAULT_TOKEN_BURST,
            acquire_timeout_secs: DEFAULT_ACQUIRE_TIMEOUT_SECS,
        }
    }
}

impl LimiterSettings {
    pub fn validate(&self) -> AppResult<()> {
        if !(1..=16).contains(&self.account_concurrency) {
            return Err(AppError::Config("账号并发须在 1–16".into()));
        }
        if !(1..=8).contains(&self.subagent_concurrency) {
            return Err(AppError::Config("子代理并发须在 1–8".into()));
        }
        if self.subagent_concurrency > self.account_concurrency {
            return Err(AppError::Config("子代理并发不能大于账号并发".into()));
        }
        if self.min_request_interval_ms > 5000 {
            return Err(AppError::Config("最小请求间隔不能超过 5000ms".into()));
        }
        if self.rate_per_min > 120 {
            return Err(AppError::Config("每分钟请求上限不能超过 120".into()));
        }
        if !(1..=32).contains(&self.token_burst) {
            return Err(AppError::Config("突发令牌须在 1–32".into()));
        }
        if !(1..=120).contains(&self.acquire_timeout_secs) {
            return Err(AppError::Config("并发等待超时须在 1–120 秒".into()));
        }
        Ok(())
    }

    fn to_config(&self) -> LimiterConfig {
        LimiterConfig {
            account_concurrency: self.account_concurrency,
            subagent_concurrency: self.subagent_concurrency,
            min_request_interval: Duration::from_millis(self.min_request_interval_ms),
            rate_per_min: self.rate_per_min as f64,
            token_capacity: self.token_burst as f64,
            acquire_timeout: Duration::from_secs(self.acquire_timeout_secs),
        }
    }

    pub fn from_config(config: &LimiterConfig) -> Self {
        Self {
            account_concurrency: config.account_concurrency,
            subagent_concurrency: config.subagent_concurrency,
            min_request_interval_ms: config.min_request_interval.as_millis() as u64,
            rate_per_min: config.rate_per_min.round().max(0.0) as u32,
            token_burst: config.token_capacity.round().max(1.0) as u32,
            acquire_timeout_secs: config.acquire_timeout.as_secs().max(1),
        }
    }
}

#[derive(Debug, Clone)]
struct LimiterConfig {
    account_concurrency: usize,
    subagent_concurrency: usize,
    min_request_interval: Duration,
    rate_per_min: f64,
    token_capacity: f64,
    acquire_timeout: Duration,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        LimiterSettings::default().to_config()
    }
}

pub struct AccountLimiter {
    config: Mutex<LimiterConfig>,
    accounts: Mutex<HashMap<String, Arc<AccountSlots>>>,
}

struct AccountSlots {
    total: Arc<Semaphore>,
    subagent: Arc<Semaphore>,
    rate: Mutex<RateState>,
}

#[derive(Debug)]
struct RateState {
    last_request: Option<Instant>,
    tokens: f64,
    last_refill: Instant,
    rate_per_min: f64,
    token_capacity: f64,
    backoff_until: Option<Instant>,
    success_streak: u32,
}

impl RateState {
    fn new(config: &LimiterConfig) -> Self {
        Self {
            last_request: None,
            tokens: config.token_capacity,
            last_refill: Instant::now(),
            rate_per_min: if config.rate_per_min > 0.0 {
                config.rate_per_min
            } else {
                DEFAULT_RATE_PER_MIN as f64
            },
            token_capacity: config.token_capacity,
            backoff_until: None,
            success_streak: 0,
        }
    }

    fn effective_rate_ceiling(&self, config: &LimiterConfig) -> f64 {
        if config.rate_per_min <= 0.0 {
            f64::MAX
        } else {
            config.rate_per_min
        }
    }

    fn refill(&mut self, now: Instant, config: &LimiterConfig) {
        if config.rate_per_min <= 0.0 {
            self.tokens = config.token_capacity;
            self.last_refill = now;
            return;
        }
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }
        let add = (self.rate_per_min / 60.0) * elapsed;
        self.tokens = (self.tokens + add).min(config.token_capacity);
        self.last_refill = now;
    }

    fn min_interval_ok(&self, now: Instant, config: &LimiterConfig) -> bool {
        if config.min_request_interval.is_zero() {
            return true;
        }
        self.last_request
            .is_none_or(|last| now.duration_since(last) >= config.min_request_interval)
    }

    fn wait_hint(&self, now: Instant, config: &LimiterConfig) -> Option<Duration> {
        if let Some(until) = self.backoff_until {
            if until > now {
                return Some(until.saturating_duration_since(now));
            }
        }
        if !self.min_interval_ok(now, config) {
            if let Some(last) = self.last_request {
                let elapsed = now.saturating_duration_since(last);
                if elapsed < config.min_request_interval {
                    return Some(config.min_request_interval - elapsed);
                }
            }
        }
        if config.rate_per_min > 0.0 && self.tokens < 1.0 {
            let need = (1.0 - self.tokens).max(0.0);
            let secs = (need * 60.0 / config.rate_per_min.max(0.1)).clamp(0.05, 8.0);
            return Some(Duration::from_secs_f64(secs));
        }
        None
    }

    fn is_under_pressure(&mut self, now: Instant, config: &LimiterConfig) -> bool {
        self.refill(now, config);
        if self.backoff_until.is_some_and(|until| until > now) {
            return true;
        }
        if config.rate_per_min > 0.0 && self.tokens < 1.0 {
            return true;
        }
        !self.min_interval_ok(now, config)
    }

    fn try_consume(&mut self, now: Instant, config: &LimiterConfig) -> bool {
        self.refill(now, config);
        if self.backoff_until.is_some_and(|until| until > now) {
            return false;
        }
        if !self.min_interval_ok(now, config) {
            return false;
        }
        if config.rate_per_min > 0.0 && self.tokens < 1.0 {
            return false;
        }
        if config.rate_per_min > 0.0 {
            self.tokens -= 1.0;
        }
        self.last_request = Some(now);
        true
    }

    fn on_success(&mut self, config: &LimiterConfig) {
        self.backoff_until = None;
        self.success_streak += 1;
        let ceiling = self.effective_rate_ceiling(config);
        if self.success_streak >= RATE_RECOVERY_SUCCESS_STREAK {
            self.rate_per_min = (self.rate_per_min + RATE_RECOVERY_STEP).min(ceiling);
            self.success_streak = 0;
        }
    }

    fn on_rate_limited(&mut self, now: Instant, config: &LimiterConfig) {
        if config.rate_per_min > 0.0 {
            self.rate_per_min = (self.rate_per_min / 2.0).max(MIN_RATE_PER_MIN);
        }
        self.backoff_until = Some(now + BACKOFF_AFTER_429);
        self.success_streak = 0;
        self.tokens = self.tokens.min(1.0);
    }
}

/// Holds semaphore permits until dropped (end of request / stream).
pub struct LimiterPermit {
    _total: Option<OwnedSemaphorePermit>,
    _subagent: Option<OwnedSemaphorePermit>,
}

pub enum AcquireOutcome {
    Granted(LimiterPermit),
    /// Token bucket / min-interval / 429 backoff still blocked after waiting.
    RateLimited,
    /// Concurrency slots were full for the whole wait; request must not bypass.
    Busy,
}

impl std::fmt::Debug for AcquireOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Granted(_) => write!(f, "Granted(..)"),
            Self::RateLimited => write!(f, "RateLimited"),
            Self::Busy => write!(f, "Busy"),
        }
    }
}

impl AccountLimiter {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(LimiterConfig::default()),
            accounts: Mutex::new(HashMap::new()),
        }
    }

    pub fn current_settings(&self) -> LimiterSettings {
        LimiterSettings::from_config(
            &self
                .config
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    pub fn apply_settings(&self, settings: &LimiterSettings) -> AppResult<()> {
        settings.validate()?;
        {
            let mut config = self
                .config
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *config = settings.to_config();
        }
        self.accounts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        log::info!(
            "Antigravity limiter settings applied: account={} subagent={} rpm={} burst={}",
            settings.account_concurrency,
            settings.subagent_concurrency,
            settings.rate_per_min,
            settings.token_burst
        );
        Ok(())
    }

    fn config(&self) -> LimiterConfig {
        self.config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn with_rate_state<R>(
        &self,
        account_id: &str,
        f: impl FnOnce(&mut RateState, &LimiterConfig) -> R,
    ) -> R {
        let config = self.config();
        let slots = self.slots_for(&config, account_id);
        let mut state = slots
            .rate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut state, &config)
    }

    /// Sync check used by the account pool (runs on `spawn_blocking`).
    pub fn is_under_pressure(&self, account_id: &str) -> bool {
        self.with_rate_state(account_id, |state, config| {
            state.is_under_pressure(Instant::now(), config)
        })
    }

    pub fn note_upstream_success(&self, account_id: &str) {
        self.with_rate_state(account_id, |state, config| state.on_success(config));
    }

    pub fn note_upstream_rate_limited(&self, account_id: &str) {
        self.with_rate_state(account_id, |state, config| {
            state.on_rate_limited(Instant::now(), config)
        });
    }

    pub async fn acquire(&self, account_id: &str, is_subagent: bool) -> AcquireOutcome {
        let timeout = self.config().acquire_timeout;
        self.acquire_until(account_id, is_subagent, Instant::now() + timeout)
            .await
    }

    pub async fn acquire_until(
        &self,
        account_id: &str,
        is_subagent: bool,
        deadline: Instant,
    ) -> AcquireOutcome {
        let config = self.config();
        let slots = self.slots_for(&config, account_id);

        let timeout = remaining_until(deadline).min(config.acquire_timeout);
        if timeout.is_zero() {
            return AcquireOutcome::Busy;
        }

        let total = match tokio::time::timeout(timeout, slots.total.clone().acquire_owned()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => {
                log::debug!("Antigravity account limiter timeout for {account_id}; not bypassing");
                return AcquireOutcome::Busy;
            }
        };

        let subagent = if is_subagent {
            let sub_timeout = remaining_until(deadline).min(config.acquire_timeout);
            if sub_timeout.is_zero() {
                return AcquireOutcome::Busy;
            }
            match tokio::time::timeout(sub_timeout, slots.subagent.clone().acquire_owned()).await {
                Ok(Ok(permit)) => Some(permit),
                Ok(Err(_)) | Err(_) => {
                    log::debug!(
                        "Antigravity subagent limiter timeout for {account_id}; not bypassing"
                    );
                    drop(total);
                    return AcquireOutcome::Busy;
                }
            }
        } else {
            None
        };

        if !wait_for_rate(&slots, &config, deadline).await {
            drop(subagent);
            drop(total);
            return AcquireOutcome::RateLimited;
        }

        AcquireOutcome::Granted(LimiterPermit {
            _total: Some(total),
            _subagent: subagent,
        })
    }

    fn slots_for(&self, config: &LimiterConfig, account_id: &str) -> Arc<AccountSlots> {
        let mut guard = self
            .accounts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .entry(account_id.to_string())
            .or_insert_with(|| {
                Arc::new(AccountSlots {
                    total: Arc::new(Semaphore::new(config.account_concurrency)),
                    subagent: Arc::new(Semaphore::new(config.subagent_concurrency)),
                    rate: Mutex::new(RateState::new(config)),
                })
            })
            .clone()
    }
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

async fn wait_for_rate(slots: &AccountSlots, config: &LimiterConfig, deadline: Instant) -> bool {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let consumed = slots
            .rate
            .lock()
            .map(|mut state| state.try_consume(now, config))
            .unwrap_or(true);
        if consumed {
            return true;
        }
        let hint = slots
            .rate
            .lock()
            .map(|mut state| {
                state.refill(now, config);
                state.wait_hint(now, config)
            })
            .unwrap_or(None)
            .unwrap_or(Duration::from_millis(20));
        let remaining = remaining_until(deadline);
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(hint.min(remaining)).await;
    }
}

impl Default for AccountLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn granted(outcome: AcquireOutcome) -> LimiterPermit {
        match outcome {
            AcquireOutcome::Granted(permit) => permit,
            other => panic!("expected Granted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subagent_requests_share_a_smaller_cap() {
        let limiter = AccountLimiter::new();
        let _main = granted(limiter.acquire("acct-a", false).await);
        let _sub1 = granted(limiter.acquire("acct-a", true).await);
        let _sub2 = granted(limiter.acquire("acct-a", true).await);
        let late =
            tokio::time::timeout(Duration::from_millis(200), limiter.acquire("acct-a", true)).await;
        assert!(
            late.is_err(),
            "third subagent should wait past short timeout"
        );
    }

    #[tokio::test]
    async fn acquire_timeout_does_not_bypass_gate() {
        let limiter = AccountLimiter::new();
        let mut updated = LimiterSettings::default();
        updated.account_concurrency = 1;
        updated.subagent_concurrency = 1;
        limiter.apply_settings(&updated).unwrap();
        let _held = granted(limiter.acquire("acct-busy", false).await);
        let denied = limiter
            .acquire_until(
                "acct-busy",
                false,
                Instant::now() + Duration::from_millis(80),
            )
            .await;
        assert!(
            matches!(denied, AcquireOutcome::Busy),
            "full account must 429 rather than proceed without a slot: {denied:?}"
        );
    }

    #[tokio::test]
    async fn rate_limiter_denies_during_backoff_window() {
        let limiter = AccountLimiter::new();
        limiter.note_upstream_rate_limited("acct-b");
        let denied = limiter
            .acquire_until("acct-b", false, Instant::now() + Duration::from_millis(50))
            .await;
        assert!(
            matches!(denied, AcquireOutcome::RateLimited),
            "429 backoff should deny a short acquire: {denied:?}"
        );
        assert!(limiter.is_under_pressure("acct-b"));
    }

    #[tokio::test]
    async fn rate_limiter_recovers_after_success_streak() {
        let limiter = AccountLimiter::new();
        limiter.note_upstream_rate_limited("acct-c");
        assert!(limiter.is_under_pressure("acct-c"));
        for _ in 0..RATE_RECOVERY_SUCCESS_STREAK {
            limiter.note_upstream_success("acct-c");
        }
        assert!(
            !limiter.is_under_pressure("acct-c"),
            "success streak should clear backoff pressure"
        );
    }

    #[test]
    fn limiter_settings_validate_subagent_cap() {
        let mut settings = LimiterSettings::default();
        settings.subagent_concurrency = 5;
        settings.account_concurrency = 3;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn apply_settings_clears_existing_slots() {
        let limiter = AccountLimiter::new();
        let mut updated = LimiterSettings::default();
        updated.account_concurrency = 2;
        limiter.apply_settings(&updated).unwrap();
        assert_eq!(limiter.current_settings().account_concurrency, 2);
    }
}
