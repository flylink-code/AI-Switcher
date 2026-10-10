//! Upstream admission limiter and FIFO queueing kernel.
//!
//! Provides per-upstream rate limiting, concurrency limiting, and FIFO queueing
//! for outbound requests dispatched by the smart gateway.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};

/// Settings key prefix for per-upstream rate-limiting policies.
pub const UPSTREAM_POLICY_KEY_PREFIX: &str = "smart_gateway_upstream_policy:";

/// Hard upper limits for policy validation.
pub const MAX_CONCURRENCY_LIMIT: u32 = 64;
pub const MAX_RPM_LIMIT: u32 = 6000;
pub const MAX_QUEUE_CAPACITY_LIMIT: u32 = 256;
pub const MAX_QUEUE_TIMEOUT_MS_LIMIT: u64 = 120_000;
pub const MAX_FIRST_OUTPUT_TIMEOUT_MS_LIMIT: u64 = 300_000;

/// Default configuration values.
pub const DEFAULT_MAX_CONCURRENCY: u32 = 0;
pub const DEFAULT_RPM: u32 = 0;
pub const DEFAULT_QUEUE_CAPACITY: u32 = 16;
pub const DEFAULT_QUEUE_TIMEOUT_MS: u64 = 8000;
pub const DEFAULT_FIRST_OUTPUT_TIMEOUT_MS: u64 = 0;

/// Sliding window duration for requests-per-minute limiting (60 seconds).
pub const RPM_WINDOW_DURATION: Duration = Duration::from_secs(60);

/// Default idle TTL for cleaning up inactive limiter state.
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(600);

/// Upper bound for tracked upstream states before triggering automatic idle cleanup.
pub const MAX_TRACKED_UPSTREAMS: usize = 1024;

static NEXT_WAITER_ID: AtomicU64 = AtomicU64::new(1);

/// Per-upstream admission and rate-limiting policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpstreamLimitPolicy {
    /// Maximum concurrent active requests (0 = unlimited).
    pub max_concurrency: u32,
    /// Maximum requests allowed per 60-second sliding window (0 = unlimited).
    pub rpm: u32,
    /// Maximum number of requests allowed to wait in the FIFO queue (0 = no queueing).
    pub queue_capacity: u32,
    /// Maximum duration a request can wait in the FIFO queue in milliseconds (0 = do not wait).
    pub queue_timeout_ms: u64,
    /// Expected maximum time-to-first-token in milliseconds for upstream requests (0 = disabled).
    pub first_output_timeout_ms: u64,
}

/// Convenience alias matching coordinator terminology.
pub type UpstreamPolicy = UpstreamLimitPolicy;

impl From<&UpstreamLimitPolicy> for UpstreamLimitPolicy {
    fn from(policy: &UpstreamLimitPolicy) -> Self {
        policy.clone()
    }
}

impl Default for UpstreamLimitPolicy {
    fn default() -> Self {
        Self {
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            rpm: DEFAULT_RPM,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            queue_timeout_ms: DEFAULT_QUEUE_TIMEOUT_MS,
            first_output_timeout_ms: DEFAULT_FIRST_OUTPUT_TIMEOUT_MS,
        }
    }
}

impl UpstreamLimitPolicy {
    /// Validate policy parameters against system upper limits.
    pub fn validate(&self) -> Result<(), LimitError> {
        if self.max_concurrency > MAX_CONCURRENCY_LIMIT {
            return Err(LimitError::Validation(format!(
                "maxConcurrency 超过上限 {MAX_CONCURRENCY_LIMIT}: {}",
                self.max_concurrency
            )));
        }
        if self.rpm > MAX_RPM_LIMIT {
            return Err(LimitError::Validation(format!(
                "rpm 超过上限 {MAX_RPM_LIMIT}: {}",
                self.rpm
            )));
        }
        if self.queue_capacity > MAX_QUEUE_CAPACITY_LIMIT {
            return Err(LimitError::Validation(format!(
                "queueCapacity 超过上限 {MAX_QUEUE_CAPACITY_LIMIT}: {}",
                self.queue_capacity
            )));
        }
        if self.queue_timeout_ms > MAX_QUEUE_TIMEOUT_MS_LIMIT {
            return Err(LimitError::Validation(format!(
                "queueTimeoutMs 超过上限 {MAX_QUEUE_TIMEOUT_MS_LIMIT}: {}",
                self.queue_timeout_ms
            )));
        }
        if self.first_output_timeout_ms > MAX_FIRST_OUTPUT_TIMEOUT_MS_LIMIT {
            return Err(LimitError::Validation(format!(
                "firstOutputTimeoutMs 超过上限 {MAX_FIRST_OUTPUT_TIMEOUT_MS_LIMIT}: {}",
                self.first_output_timeout_ms
            )));
        }
        Ok(())
    }
}

/// Instantaneous snapshot of an upstream's admission limiter state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamLimitSnapshot {
    pub upstream_id: String,
    pub policy: UpstreamLimitPolicy,
    pub active: u32,
    pub queue_len: u32,
    pub current_rpm: u32,
}

/// Limiter rejection and failure errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum LimitError {
    #[error("上游队列已满 (category: {category}, retry_after: {retry_after}s): {message}")]
    QueueFull {
        category: String,
        retry_after: u64,
        message: String,
    },
    #[error("上游排队超时 (category: {category}, retry_after: {retry_after}s): {message}")]
    QueueTimeout {
        category: String,
        retry_after: u64,
        message: String,
    },
    #[error("参数校验失败: {0}")]
    Validation(String),
}

impl LimitError {
    pub fn queue_full(message: impl Into<String>, retry_after: u64) -> Self {
        Self::QueueFull {
            category: "local_queue_full".to_string(),
            retry_after,
            message: message.into(),
        }
    }

    pub fn queue_timeout(message: impl Into<String>, retry_after: u64) -> Self {
        Self::QueueTimeout {
            category: "local_queue_timeout".to_string(),
            retry_after,
            message: message.into(),
        }
    }

    pub fn category(&self) -> &str {
        match self {
            Self::QueueFull { category, .. } => category,
            Self::QueueTimeout { category, .. } => category,
            Self::Validation(_) => "validation_error",
        }
    }

    pub fn retry_after(&self) -> u64 {
        match self {
            Self::QueueFull { retry_after, .. } => *retry_after,
            Self::QueueTimeout { retry_after, .. } => *retry_after,
            Self::Validation(_) => 0,
        }
    }

    pub fn retry_after_secs(&self) -> u64 {
        self.retry_after()
    }

    pub fn message(&self) -> &str {
        match self {
            Self::QueueFull { message, .. } => message,
            Self::QueueTimeout { message, .. } => message,
            Self::Validation(msg) => msg,
        }
    }
}

impl From<LimitError> for AppError {
    fn from(err: LimitError) -> Self {
        match err {
            LimitError::Validation(msg) => AppError::Config(msg),
            other => AppError::Other(other.to_string()),
        }
    }
}

/// Monotonic clock abstraction for testing and deterministic timing.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> tokio::time::Instant;
}

/// Standard production clock backed by Tokio's virtual timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

/// Mock clock for deterministic tests.
#[cfg(test)]
#[derive(Debug)]
pub struct MockClock {
    base: tokio::time::Instant,
    offset: std::sync::RwLock<Duration>,
}

#[cfg(test)]
impl MockClock {
    pub fn new() -> Self {
        Self {
            base: tokio::time::Instant::now(),
            offset: std::sync::RwLock::new(Duration::ZERO),
        }
    }

    pub fn advance(&self, duration: Duration) {
        let mut guard = self.offset.write().unwrap();
        *guard += duration;
    }

    pub fn set_offset(&self, duration: Duration) {
        let mut guard = self.offset.write().unwrap();
        *guard = duration;
    }
}

#[cfg(test)]
impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl Clock for MockClock {
    fn now(&self) -> tokio::time::Instant {
        let offset = *self.offset.read().unwrap();
        self.base + offset
    }
}

fn elapsed_since(now: tokio::time::Instant, past: tokio::time::Instant) -> Duration {
    if now >= past {
        now - past
    } else {
        Duration::ZERO
    }
}

/// Internal queue waiter representation.
struct Waiter {
    id: u64,
    notify: Arc<Notify>,
}

/// Internal per-upstream admission and window state.
struct UpstreamState {
    policy: UpstreamLimitPolicy,
    active: u32,
    reserved_rpm: u32,
    rpm_timestamps: VecDeque<tokio::time::Instant>,
    waiters: VecDeque<Waiter>,
    last_activity: tokio::time::Instant,
    removed: bool,
}

impl UpstreamState {
    fn new(policy: UpstreamLimitPolicy, now: tokio::time::Instant) -> Self {
        Self {
            policy,
            active: 0,
            reserved_rpm: 0,
            rpm_timestamps: VecDeque::new(),
            waiters: VecDeque::new(),
            last_activity: now,
            removed: false,
        }
    }

    fn prune_rpm(&mut self, now: tokio::time::Instant) {
        while let Some(&front) = self.rpm_timestamps.front() {
            if elapsed_since(now, front) >= RPM_WINDOW_DURATION {
                self.rpm_timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn has_rpm_capacity(&self) -> bool {
        self.policy.rpm == 0
            || self.rpm_timestamps.len() + (self.reserved_rpm as usize) < self.policy.rpm as usize
    }

    fn record_rpm(&mut self, now: tokio::time::Instant) {
        self.rpm_timestamps.push_back(now);
        if self.rpm_timestamps.len() > MAX_RPM_LIMIT as usize {
            self.rpm_timestamps.pop_front();
        }
    }

    /// Calculate retry-after in seconds, rounding up to whole seconds.
    fn retry_after(&self, now: tokio::time::Instant) -> u64 {
        if self.policy.rpm > 0 && self.rpm_timestamps.len() >= self.policy.rpm as usize {
            if let Some(&oldest) = self.rpm_timestamps.front() {
                let expiry = oldest + RPM_WINDOW_DURATION;
                if expiry > now {
                    let millis = (expiry - now).as_millis();
                    return millis.div_ceil(1000).max(1) as u64;
                }
            }
        }
        1
    }
}

/// RAII guard ensuring that a waiting request is removed from the FIFO queue on drop/cancel.
struct WaiterGuard {
    waiter_id: u64,
    state: Arc<Mutex<UpstreamState>>,
    active: bool,
}

impl WaiterGuard {
    fn new(waiter_id: u64, state: Arc<Mutex<UpstreamState>>) -> Self {
        Self {
            waiter_id,
            state,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };
        let was_head = state.waiters.front().map(|w| w.id) == Some(self.waiter_id);
        state.waiters.retain(|w| w.id != self.waiter_id);
        if was_head {
            if let Some(new_head) = state.waiters.front() {
                new_head.notify.notify_one();
            }
        }
    }
}

/// In-flight admission permit held by caller during request lifecycle.
pub struct Permit {
    inner: Option<PermitInner>,
}

impl fmt::Debug for Permit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Permit")
            .field("upstream_id", &self.upstream_id())
            .field("policy", self.policy())
            .finish()
    }
}

struct PermitInner {
    upstream_id: String,
    policy: UpstreamLimitPolicy,
    state: Arc<Mutex<UpstreamState>>,
    clock: Arc<dyn Clock>,
    rpm_timestamp: Option<tokio::time::Instant>,
}

impl Permit {
    pub fn upstream_id(&self) -> &str {
        self.inner
            .as_ref()
            .map(|i| i.upstream_id.as_str())
            .unwrap_or("")
    }

    pub fn policy(&self) -> &UpstreamLimitPolicy {
        static DEFAULT_POLICY: UpstreamLimitPolicy = UpstreamLimitPolicy {
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            rpm: DEFAULT_RPM,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            queue_timeout_ms: DEFAULT_QUEUE_TIMEOUT_MS,
            first_output_timeout_ms: DEFAULT_FIRST_OUTPUT_TIMEOUT_MS,
        };
        self.inner
            .as_ref()
            .map(|i| &i.policy)
            .unwrap_or(&DEFAULT_POLICY)
    }

    /// 健康门禁通过后将预留转为实际出站计数，重复提交不会重复计数。
    pub fn commit_outbound(&mut self) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        if inner.rpm_timestamp.is_some() {
            return;
        }
        let now = inner.clock.now();
        let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.prune_rpm(now);
        state.reserved_rpm = state.reserved_rpm.saturating_sub(1);
        state.record_rpm(now);
        inner.rpm_timestamp = Some(now);
        if let Some(head) = state.waiters.front() {
            head.notify.notify_one();
        }
    }

    /// Return the permit's configured first-output deadline.
    pub fn first_output_timeout_ms(&self) -> u64 {
        self.policy().first_output_timeout_ms
    }
}

impl Drop for PermitInner {
    fn drop(&mut self) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.active = state.active.saturating_sub(1);
        if self.rpm_timestamp.is_none() {
            state.reserved_rpm = state.reserved_rpm.saturating_sub(1);
        }
        let now = self.clock.now();
        state.last_activity = now;
        state.prune_rpm(now);

        let conc_ok =
            state.policy.max_concurrency == 0 || state.active < state.policy.max_concurrency;
        let rpm_ok =
            state.has_rpm_capacity();
        if conc_ok && rpm_ok {
            if let Some(head) = state.waiters.front() {
                head.notify.notify_one();
            }
        }
    }
}

/// Independent instance-level upstream admission limiter.
#[derive(Clone)]
pub struct UpstreamLimiter {
    inner: Arc<LimiterInner>,
}

pub type Limiter = UpstreamLimiter;

struct LimiterInner {
    states: Mutex<HashMap<String, Arc<Mutex<UpstreamState>>>>,
    clock: Arc<dyn Clock>,
}

impl Default for UpstreamLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl UpstreamLimiter {
    /// Create a new limiter instance with default Tokio clock.
    pub fn new() -> Self {
        Self::with_clock(Arc::new(TokioClock))
    }

    /// Create a new limiter instance with an explicit clock.
    pub fn with_clock<C>(clock: Arc<C>) -> Self
    where
        C: Clock,
    {
        let clock: Arc<dyn Clock> = clock;
        Self {
            inner: Arc::new(LimiterInner {
                states: Mutex::new(HashMap::new()),
                clock,
            }),
        }
    }

    /// Apply or update policy for an upstream in-place without resetting active requests or RPM history.
    pub fn apply(&self, upstream_id: &str, policy: impl Into<UpstreamLimitPolicy>) {
        let policy = policy.into();
        let now = self.inner.clock.now();
        let state_arc = {
            let mut states = self.inner.states.lock().unwrap();
            Arc::clone(states.entry(upstream_id.to_string()).or_insert_with(|| {
                Arc::new(Mutex::new(UpstreamState::new(
                    policy.clone(),
                    now,
                )))
            }))
        };

        let mut state = state_arc.lock().unwrap();
        state.policy = policy;
        state.removed = false;
        state.last_activity = now;
        state.prune_rpm(now);

        // Always notify the head waiter to re-evaluate conditions or updated wait duration.
        if let Some(head) = state.waiters.front() {
            head.notify.notify_one();
        }
    }

    /// Retrieve the current configured policy for an upstream, or default if unset.
    pub fn policy(&self, upstream_id: &str) -> UpstreamLimitPolicy {
        let states = self.inner.states.lock().unwrap();
        states
            .get(upstream_id)
            .map(|s| s.lock().unwrap().policy.clone())
            .unwrap_or_default()
    }

    /// Acquire an admission permit for the given upstream ID following strict FIFO ordering.
    pub async fn acquire(&self, upstream_id: &str) -> Result<Permit, LimitError> {
        self.acquire_internal(upstream_id).await
    }

    async fn acquire_internal(&self, upstream_id: &str) -> Result<Permit, LimitError> {
        let clock = Arc::clone(&self.inner.clock);
        let now = clock.now();

        // 1. Locate or create state entry.
        let upstream_state = {
            let mut states = self.inner.states.lock().unwrap();
            if states.len() >= MAX_TRACKED_UPSTREAMS {
                self.sweep_idle_internal(&mut states, DEFAULT_IDLE_TTL, now);
            }
            Arc::clone(states.entry(upstream_id.to_string()).or_insert_with(|| {
                Arc::new(Mutex::new(UpstreamState::new(
                    UpstreamLimitPolicy::default(),
                    now,
                )))
            }))
        };

        let waiter_id = NEXT_WAITER_ID.fetch_add(1, Ordering::Relaxed);
        let notify = Arc::new(Notify::new());

        // 2. ATOMIC check and enqueue in ONE single lock hold:
        //    prevents multithreaded capacity bypass between inspection and enqueue.
        let (immediate_permit, queue_timeout_ms, rejection_err) = {
            let mut state = upstream_state.lock().unwrap();
            state.last_activity = now;

            if state.removed {
                return Err(LimitError::Validation(format!(
                    "上游已移除: {upstream_id}"
                )));
            }

            state.prune_rpm(now);

            let queue_empty = state.waiters.is_empty();
            let conc_ok =
                state.policy.max_concurrency == 0 || state.active < state.policy.max_concurrency;
            let rpm_ok =
                state.has_rpm_capacity();

            if queue_empty && conc_ok && rpm_ok {
                state.active += 1;
                state.reserved_rpm += 1;
                let permit = Permit {
                    inner: Some(PermitInner {
                        upstream_id: upstream_id.to_string(),
                        policy: state.policy.clone(),
                        state: Arc::clone(&upstream_state),
                        clock: Arc::clone(&clock),
                        rpm_timestamp: None,
                    }),
                };
                (Some(permit), 0, None)
            } else {
                let retry_after = state.retry_after(now);
                if state.policy.queue_capacity == 0 {
                    (
                        None,
                        0,
                        Some(LimitError::queue_full(
                            format!("上游 {upstream_id} 限流且队列容量为0"),
                            retry_after,
                        )),
                    )
                } else if state.policy.queue_timeout_ms == 0 {
                    (
                        None,
                        0,
                        Some(LimitError::queue_timeout(
                            format!("上游 {upstream_id} 限流等待超时 (queueTimeoutMs = 0)"),
                            retry_after,
                        )),
                    )
                } else if state.waiters.len() >= state.policy.queue_capacity as usize {
                    (
                        None,
                        0,
                        Some(LimitError::queue_full(
                            format!(
                                "上游 {upstream_id} 排队已满: 当前排队数 {} 达到上限 {}",
                                state.waiters.len(),
                                state.policy.queue_capacity
                            ),
                            retry_after,
                        )),
                    )
                } else {
                    // Atomically join FIFO queue under the same lock
                    state.waiters.push_back(Waiter {
                        id: waiter_id,
                        notify: Arc::clone(&notify),
                    });
                    (None, state.policy.queue_timeout_ms, None)
                }
            }
        };

        if let Some(permit) = immediate_permit {
            return Ok(permit);
        }

        if let Some(err) = rejection_err {
            return Err(err);
        }

        // 3. Register RAII guard now that we are in the queue.
        let mut guard = WaiterGuard::new(waiter_id, Arc::clone(&upstream_state));
        let timeout_duration = Duration::from_millis(queue_timeout_ms);

        // 4. FIFO admission wait loop with RAII cancellation and lost-wakeup protection.
        let wait_fut = async {
            loop {
                // Step A: Register notification future BEFORE inspecting state under lock.
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();

                // Step B: Check admission condition under lock.
                let check_res = {
                    let mut state = upstream_state.lock().unwrap();
                    let current_now = clock.now();
                    state.prune_rpm(current_now);

                    if state.removed {
                        state.waiters.retain(|w| w.id != waiter_id);
                        return Err(LimitError::Validation(format!(
                            "上游已移除: {upstream_id}"
                        )));
                    }

                    let is_head = state.waiters.front().map(|w| w.id) == Some(waiter_id);
                    if is_head {
                        let conc_ok = state.policy.max_concurrency == 0
                            || state.active < state.policy.max_concurrency;
                        let rpm_ok = state.has_rpm_capacity();

                        if conc_ok && rpm_ok {
                            state.waiters.pop_front();
                            state.active += 1;
                state.reserved_rpm += 1;

                            // If capacity still remains, wake next waiter in line.
                            let next_conc_ok = state.policy.max_concurrency == 0
                                || state.active < state.policy.max_concurrency;
                            let next_rpm_ok = state.has_rpm_capacity();
                            if next_conc_ok && next_rpm_ok {
                                if let Some(next_waiter) = state.waiters.front() {
                                    next_waiter.notify.notify_one();
                                }
                            }

                            Some(state.policy.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(policy) = check_res {
                    return Ok(policy);
                }

                // Step C: Determine sleep duration if blocked on RPM window.
                let rpm_wait_dur = {
                    let state = upstream_state.lock().unwrap();
                    let is_head = state.waiters.front().map(|w| w.id) == Some(waiter_id);
                    if is_head
                        && (state.policy.max_concurrency == 0
                            || state.active < state.policy.max_concurrency)
                        && state.policy.rpm > 0
                    {
                        state.rpm_timestamps.front().map(|&oldest| {
                            let current_now = clock.now();
                            let expiry = oldest + RPM_WINDOW_DURATION;
                            if expiry > current_now {
                                expiry - current_now
                            } else {
                                Duration::ZERO
                            }
                        })
                    } else {
                        None
                    }
                };

                // Step D: Await waker notification or RPM window expiration without holding lock.
                match rpm_wait_dur {
                    Some(dur) if dur > Duration::ZERO => {
                        tokio::select! {
                            biased;
                            _ = &mut notified => {}
                            _ = tokio::time::sleep(dur) => {}
                        }
                    }
                    Some(_) => {
                        tokio::task::yield_now().await;
                    }
                    None => {
                        notified.await;
                    }
                }
            }
        };

        match tokio::time::timeout(timeout_duration, wait_fut).await {
            Ok(Ok(policy)) => {
                guard.disarm();
                Ok(Permit {
                    inner: Some(PermitInner {
                        upstream_id: upstream_id.to_string(),
                        policy,
                        state: Arc::clone(&upstream_state),
                        clock: Arc::clone(&clock),
                        rpm_timestamp: None,
                    }),
                })
            }
            Ok(Err(err)) => Err(err),
            Err(_) => {
                let retry_after = {
                    let state = upstream_state.lock().unwrap();
                    state.retry_after(clock.now())
                };
                Err(LimitError::queue_timeout(
                    format!(
                        "上游 {upstream_id} 排队等待超时 ({}ms)",
                        queue_timeout_ms
                    ),
                    retry_after,
                ))
            }
        }
    }

    /// Retrieve snapshot of an upstream's limiter state (pure read-only, does not mutate state).
    pub fn snapshot(&self, upstream_id: &str) -> Option<UpstreamLimitSnapshot> {
        let states = self.inner.states.lock().unwrap();
        let state_arc = states.get(upstream_id)?;
        let state = state_arc.lock().unwrap();
        let now = self.inner.clock.now();
        let active_rpm = state
            .rpm_timestamps
            .iter()
            .filter(|&&ts| elapsed_since(now, ts) < RPM_WINDOW_DURATION)
            .count() as u32;
        Some(UpstreamLimitSnapshot {
            upstream_id: upstream_id.to_string(),
            policy: state.policy.clone(),
            active: state.active,
            queue_len: state.waiters.len() as u32,
            current_rpm: active_rpm,
        })
    }

    /// Retrieve snapshot or default empty state if unconfigured.
    pub fn snapshot_or_default(&self, upstream_id: &str) -> UpstreamLimitSnapshot {
        self.snapshot(upstream_id)
            .unwrap_or_else(|| UpstreamLimitSnapshot {
                upstream_id: upstream_id.to_string(),
                policy: UpstreamLimitPolicy::default(),
                active: 0,
                queue_len: 0,
                current_rpm: 0,
            })
    }

    /// Retrieve snapshots of all currently tracked upstreams (pure read-only).
    pub fn snapshot_all(&self) -> Vec<UpstreamLimitSnapshot> {
        let states = self.inner.states.lock().unwrap();
        let now = self.inner.clock.now();
        let mut results = Vec::with_capacity(states.len());
        for (id, state_arc) in states.iter() {
            let state = state_arc.lock().unwrap();
            let active_rpm = state
                .rpm_timestamps
                .iter()
                .filter(|&&ts| elapsed_since(now, ts) < RPM_WINDOW_DURATION)
                .count() as u32;
            results.push(UpstreamLimitSnapshot {
                upstream_id: id.clone(),
                policy: state.policy.clone(),
                active: state.active,
                queue_len: state.waiters.len() as u32,
                current_rpm: active_rpm,
            });
        }
        results
    }

    /// Remove an upstream: marks as removed tombstone, preserves active count for in-flight requests,
    /// and drains/wakes all queued waiters with an explicit error.
    pub fn remove(&self, upstream_id: &str) -> bool {
        let mut states = self.inner.states.lock().unwrap();
        let state_arc = states.entry(upstream_id.to_string()).or_insert_with(|| {
            Arc::new(Mutex::new(UpstreamState::new(
                UpstreamLimitPolicy::default(), self.inner.clock.now(),
            )))
        });
        let mut state = state_arc.lock().unwrap();
        if state.removed { return false; }
        state.removed = true;
        for waiter in state.waiters.drain(..) {
            waiter.notify.notify_one();
        }
        true
    }

    /// Clear all tracked upstreams, marking them as removed and draining all queues.
    pub fn clear(&self) {
        let states = self.inner.states.lock().unwrap();
        for state_arc in states.values() {
            let mut state = state_arc.lock().unwrap();
            state.removed = true;
            for waiter in state.waiters.drain(..) {
                waiter.notify.notify_one();
            }
        }
    }

    /// Sweep inactive idle upstream states that have exceeded TTL.
    pub fn sweep_idle(&self, max_idle: Duration) -> usize {
        let now = self.inner.clock.now();
        let mut states = self.inner.states.lock().unwrap();
        self.sweep_idle_internal(&mut states, max_idle, now)
    }

    fn sweep_idle_internal(
        &self,
        states: &mut HashMap<String, Arc<Mutex<UpstreamState>>>,
        max_idle: Duration,
        now: tokio::time::Instant,
    ) -> usize {
        let mut to_remove = Vec::new();
        for (id, state_arc) in states.iter() {
            if let Ok(mut state) = state_arc.try_lock() {
                state.prune_rpm(now);
                if state.active == 0
                    && state.waiters.is_empty()
                    && state.rpm_timestamps.is_empty()
                    && (state.removed || state.policy == UpstreamLimitPolicy::default())
                    && elapsed_since(now, state.last_activity) >= max_idle
                {
                    to_remove.push(id.clone());
                }
            }
        }
        let count = to_remove.len();
        for id in to_remove {
            states.remove(&id);
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Settings Table Persistence (Connection DAO)
// ---------------------------------------------------------------------------

/// Construct the settings table key for an upstream's rate limit policy.
pub fn upstream_policy_key(upstream_id: &str) -> String {
    format!("{UPSTREAM_POLICY_KEY_PREFIX}{upstream_id}")
}

/// Extract upstream ID from a settings key if it has the policy prefix.
pub fn upstream_id_from_key(key: &str) -> Option<&str> {
    key.strip_prefix(UPSTREAM_POLICY_KEY_PREFIX)
}

/// Load an upstream's policy from the `settings` table, if present.
pub fn load_policy(
    conn: &Connection,
    upstream_id: &str,
) -> AppResult<Option<UpstreamLimitPolicy>> {
    let key = upstream_policy_key(upstream_id);
    let raw = get_setting(conn, &key)?;
    match raw {
        Some(json_str) => {
            let policy: UpstreamLimitPolicy = serde_json::from_str(&json_str).map_err(|e| {
                AppError::Json(format!("解析上游限流配置失败 ({upstream_id}): {e}"))
            })?;
            policy
                .validate()
                .map_err(|e| AppError::Config(e.to_string()))?;
            Ok(Some(policy))
        }
        None => Ok(None),
    }
}

/// Load all upstream policies stored in the `settings` table as a HashMap.
pub fn load_all_policies(conn: &Connection) -> AppResult<HashMap<String, UpstreamLimitPolicy>> {
    let mut stmt = conn.prepare(
        "SELECT key, value FROM settings WHERE key LIKE 'smart_gateway_upstream_policy:%';",
    )?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let val: String = row.get(1)?;
        Ok((key, val))
    })?;

    let mut map = HashMap::new();
    for item in rows {
        let (key, val) = item?;
        if let Some(id) = upstream_id_from_key(&key) {
            match serde_json::from_str::<UpstreamLimitPolicy>(&val) {
                Ok(policy) => {
                    if let Err(e) = policy.validate() {
                        log::warn!("跳过无效上游限流配置 {id}: {e}");
                        continue;
                    }
                    map.insert(id.to_string(), policy);
                }
                Err(e) => {
                    log::warn!("跳过格式错误的上游限流配置 {id}: {e}");
                }
            }
        }
    }
    Ok(map)
}

/// Load all upstream policies stored in the `settings` table as a sorted list.
pub fn load_all(conn: &Connection) -> AppResult<Vec<(String, UpstreamLimitPolicy)>> {
    let map = load_all_policies(conn)?;
    let mut list: Vec<_> = map.into_iter().collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(list)
}

/// Persist an upstream's policy to the `settings` table, ensuring the upstream exists.
pub fn persist_policy(
    conn: &Connection,
    upstream_id: &str,
    policy: &UpstreamLimitPolicy,
) -> AppResult<()> {
    policy
        .validate()
        .map_err(|e| AppError::Config(e.to_string()))?;

    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='upstreams');",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Err(AppError::Config(format!("上游不存在: {upstream_id}")));
    }

    let upstream_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM upstreams WHERE id = ?);",
        params![upstream_id],
        |row| row.get(0),
    )?;
    if !upstream_exists {
        return Err(AppError::Config(format!("上游不存在: {upstream_id}")));
    }

    let key = upstream_policy_key(upstream_id);
    let value = serde_json::to_string(policy)?;
    set_setting(conn, &key, &value)?;
    Ok(())
}

/// Delete an upstream's policy from the `settings` table.
pub fn delete_policy(conn: &Connection, upstream_id: &str) -> AppResult<bool> {
    let key = upstream_policy_key(upstream_id);
    let count = conn.execute("DELETE FROM settings WHERE key = ?;", params![key])?;
    Ok(count > 0)
}

/// Load all persisted policies and apply them into the limiter instance.
pub fn load_into_limiter(conn: &Connection, limiter: &UpstreamLimiter) -> AppResult<usize> {
    let policies = load_all_policies(conn)?;
    let count = policies.len();
    let ids = {
        let mut statement = conn.prepare("SELECT id FROM upstreams")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    // 恢复资料库时保留旧在途计数；只重新应用当前库里的上游和配置。
    limiter.clear();
    for upstream_id in ids {
        let policy = policies.get(&upstream_id).cloned().unwrap_or_default();
        limiter.apply(&upstream_id, policy);
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE upstreams (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO upstreams (id, name) VALUES ('up_test_1', 'Test Upstream 1');
             INSERT INTO upstreams (id, name) VALUES ('up_test_2', 'Test Upstream 2');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_policy_defaults() {
        let policy = UpstreamLimitPolicy::default();
        assert_eq!(policy.max_concurrency, 0);
        assert_eq!(policy.rpm, 0);
        assert_eq!(policy.queue_capacity, 16);
        assert_eq!(policy.queue_timeout_ms, 8000);
        assert_eq!(policy.first_output_timeout_ms, 0);
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_policy_serde_camel_case() {
        let json = r#"{"maxConcurrency":10,"rpm":60,"queueCapacity":20,"queueTimeoutMs":5000,"firstOutputTimeoutMs":15000}"#;
        let policy: UpstreamLimitPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(policy.max_concurrency, 10);
        assert_eq!(policy.rpm, 60);
        assert_eq!(policy.queue_capacity, 20);
        assert_eq!(policy.queue_timeout_ms, 5000);
        assert_eq!(policy.first_output_timeout_ms, 15000);

        let serialized = serde_json::to_string(&policy).unwrap();
        assert!(serialized.contains(r#""maxConcurrency":10"#));
        assert!(serialized.contains(r#""queueCapacity":20"#));
    }

    #[test]
    fn test_policy_validation_bounds() {
        let mut policy = UpstreamLimitPolicy::default();

        policy.max_concurrency = 65;
        assert!(policy.validate().is_err());
        policy.max_concurrency = 64;
        assert!(policy.validate().is_ok());

        policy.rpm = 6001;
        assert!(policy.validate().is_err());
        policy.rpm = 6000;
        assert!(policy.validate().is_ok());

        policy.queue_capacity = 257;
        assert!(policy.validate().is_err());
        policy.queue_capacity = 256;
        assert!(policy.validate().is_ok());

        policy.queue_timeout_ms = 120_001;
        assert!(policy.validate().is_err());
        policy.queue_timeout_ms = 120_000;
        assert!(policy.validate().is_ok());

        policy.first_output_timeout_ms = 300_001;
        assert!(policy.validate().is_err());
        policy.first_output_timeout_ms = 300_000;
        assert!(policy.validate().is_ok());
    }

    #[tokio::test]
    async fn test_concurrency_limit_and_release() {
        let limiter = Limiter::new();
        let upstream = "up_conc_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 2,
                rpm: 0,
                queue_capacity: 0, // No queueing for immediate test
                queue_timeout_ms: 0,
                first_output_timeout_ms: 0,
            },
        );

        let p1 = limiter.acquire(upstream).await.unwrap();
        assert_eq!(p1.upstream_id(), upstream);
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.active, 1);

        let p2 = limiter.acquire(upstream).await.unwrap();
        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.active, 2);

        // Third acquire should fail immediately because max_concurrency=2 and queue_capacity=0
        let p3_err = limiter.acquire(upstream).await.unwrap_err();
        assert_eq!(p3_err.category(), "local_queue_full");

        // Drop one permit
        drop(p1);
        let snap3 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap3.active, 1);

        // Now third acquire should succeed
        let p3 = limiter.acquire(upstream).await.unwrap();
        let snap4 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap4.active, 2);

        drop(p2);
        drop(p3);
        let snap5 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap5.active, 0);
    }

    #[tokio::test]
    async fn test_fifo_order() {
        let limiter = Limiter::new();
        let upstream = "up_fifo_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 10,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let order = Arc::new(Mutex::new(Vec::new()));

        let mut handles = Vec::new();
        for i in 1..=3 {
            let lim = limiter.clone();
            let up = upstream.to_string();
            let ord = Arc::clone(&order);
            let h = tokio::spawn(async move {
                let permit = lim.acquire(&up).await.unwrap();
                ord.lock().unwrap().push(i);
                tokio::time::sleep(Duration::from_millis(10)).await;
                drop(permit);
            });
            handles.push(h);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let snap_waiting = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap_waiting.queue_len, 3);

        drop(p0);

        for h in handles {
            h.await.unwrap();
        }

        let final_order = order.lock().unwrap().clone();
        assert_eq!(final_order, vec![1, 2, 3]);

        let snap_final = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap_final.active, 0);
        assert_eq!(snap_final.queue_len, 0);
    }

    #[tokio::test]
    async fn test_queue_full_immediate_rejection() {
        let limiter = Limiter::new();
        let upstream = "up_qfull_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 1,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let lim = limiter.clone();
        let up = upstream.to_string();
        let h1 = tokio::spawn(async move {
            lim.acquire(&up).await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;

        let snap = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap.active, 1);
        assert_eq!(snap.queue_len, 1);

        // Queue capacity is 1, so the next acquire should immediately fail with QueueFull
        let err = limiter.acquire(upstream).await.unwrap_err();
        assert_eq!(err.category(), "local_queue_full");
        assert!(err.retry_after() >= 1);

        drop(p0);
        let p1 = h1.await.unwrap().unwrap();
        drop(p1);
    }

    #[tokio::test]
    async fn test_concurrent_queue_capacity_limit() {
        let limiter = Limiter::new();
        let upstream = "up_concurrent_q_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 3,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let mut handles = Vec::new();
        for _ in 0..15 {
            let lim = limiter.clone();
            let up = upstream.to_string();
            handles.push(tokio::spawn(async move {
                lim.acquire(&up).await
            }));
        }

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Exactly 3 should be in queue, the rest 12 should be rejected with local_queue_full
        let snap = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap.queue_len, 3);

        drop(p0);

        let mut success = 0;
        let mut queue_full_count = 0;
        for h in handles {
            match h.await.unwrap() {
                Ok(permit) => {
                    success += 1;
                    drop(permit);
                }
                Err(err) => {
                    if err.category() == "local_queue_full" {
                        queue_full_count += 1;
                    }
                }
            }
        }

        assert_eq!(success, 3);
        assert_eq!(queue_full_count, 12);
    }

    #[tokio::test]
    async fn test_queue_timeout() {
        let limiter = Limiter::new();
        let upstream = "up_qtimeout_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 40,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let start = tokio::time::Instant::now();
        let err = limiter.acquire(upstream).await.unwrap_err();
        let elapsed = start.elapsed();

        assert_eq!(err.category(), "local_queue_timeout");
        assert!(elapsed >= Duration::from_millis(30));

        let snap = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap.queue_len, 0); // Guard dropped and cleaned up waiter

        drop(p0);
    }

    #[tokio::test]
    async fn test_cancellation_raii_cleanup() {
        let limiter = Limiter::new();
        let upstream = "up_raii_test";
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let lim = limiter.clone();
        let up = upstream.to_string();
        let handle = tokio::spawn(async move {
            lim.acquire(&up).await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.queue_len, 1);

        // Abort task to simulate client disconnection
        handle.abort();
        tokio::time::sleep(Duration::from_millis(30)).await;

        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.queue_len, 0); // RAII cleaned up waiter

        drop(p0);
    }

    #[tokio::test]
    async fn test_rpm_sliding_window() {
        let clock = Arc::new(MockClock::new());
        let limiter = Limiter::with_clock(Arc::clone(&clock));
        let upstream = "up_rpm_test";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 0, // unlimited concurrency
                rpm: 2,
                queue_capacity: 0, // reject immediately when RPM exhausted
                queue_timeout_ms: 0,
                first_output_timeout_ms: 0,
            },
        );

        // t = 0s
        let mut p1 = limiter.acquire(upstream).await.unwrap();
        p1.commit_outbound();
        drop(p1);
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.current_rpm, 1);

        // t = 10s
        clock.advance(Duration::from_secs(10));
        let mut p2 = limiter.acquire(upstream).await.unwrap();
        p2.commit_outbound();
        drop(p2);
        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.current_rpm, 2);

        // t = 20s: RPM capacity (2) exhausted
        clock.advance(Duration::from_secs(10));
        let err = limiter.acquire(upstream).await.unwrap_err();
        assert_eq!(err.category(), "local_queue_full");

        // t = 61s: p1 (at t=0s) has rolled out of the 60s sliding window
        clock.set_offset(Duration::from_secs(61));
        let snap3 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap3.current_rpm, 1); // Only p2 remains

        // Now acquire p3 succeeds
        let mut p3 = limiter.acquire(upstream).await.unwrap();
        p3.commit_outbound();
        drop(p3);
        let snap4 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap4.current_rpm, 2);

        // t = 75s: p2 (at t=10s) rolls out
        clock.set_offset(Duration::from_secs(75));
        let snap5 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap5.current_rpm, 1);
    }

    #[tokio::test]
    async fn test_rpm_no_consumption_on_cancel_or_timeout() {
        let limiter = Limiter::new();
        let upstream = "up_rpm_not_consumed";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 1,
                queue_capacity: 2,
                queue_timeout_ms: 30,
                first_output_timeout_ms: 0,
            },
        );

        let mut p0 = limiter.acquire(upstream).await.unwrap();
        p0.commit_outbound();
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.current_rpm, 1);

        // This acquire will time out while queued
        let _ = limiter.acquire(upstream).await.unwrap_err();

        // RPM count must still be 1 (timeout request never consumed RPM)
        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.current_rpm, 1);

        drop(p0);
    }

    #[tokio::test]
    async fn reservation_blocks_parallel_admission_but_only_commit_consumes_rpm() {
        let limiter = Limiter::new();
        let upstream = "rpm_reservation";
        limiter.apply(upstream, UpstreamLimitPolicy {
            rpm: 1,
            queue_capacity: 0,
            ..Default::default()
        });
        let first = limiter.acquire(upstream).await.unwrap();
        assert_eq!(limiter.snapshot(upstream).unwrap().current_rpm, 0);
        assert!(limiter.acquire(upstream).await.is_err());
        // 健康门禁拒绝或提交前取消，归还预留额度。
        drop(first);
        let mut second = limiter.acquire(upstream).await.unwrap();
        second.commit_outbound();
        second.commit_outbound();
        assert_eq!(limiter.snapshot(upstream).unwrap().current_rpm, 1);
        drop(second);
        assert!(limiter.acquire(upstream).await.is_err());
    }

    #[tokio::test]
    async fn releasing_uncommitted_reservation_wakes_fifo_waiter() {
        let limiter = Limiter::new();
        limiter.apply("reserved_queue", UpstreamLimitPolicy {
            rpm: 1,
            queue_timeout_ms: 1000,
            ..Default::default()
        });
        let first = limiter.acquire("reserved_queue").await.unwrap();
        let waiting = limiter.acquire("reserved_queue");
        tokio::pin!(waiting);
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut waiting).await.is_err());
        assert_eq!(limiter.snapshot("reserved_queue").unwrap().queue_len, 1);
        drop(first);
        let mut next = tokio::time::timeout(Duration::from_millis(100), waiting).await.unwrap().unwrap();
        next.commit_outbound();
        assert_eq!(limiter.snapshot("reserved_queue").unwrap().current_rpm, 1);
    }

    #[tokio::test]
    async fn test_dynamic_policy_elevation() {
        let limiter = Limiter::new();
        let upstream = "up_dyn_elevate";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p0 = limiter.acquire(upstream).await.unwrap();

        let lim = limiter.clone();
        let up = upstream.to_string();
        let handle = tokio::spawn(async move {
            lim.acquire(&up).await.unwrap()
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.active, 1);
        assert_eq!(snap1.queue_len, 1);

        // Elevate max_concurrency to 2: queued request should immediately acquire
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 2,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p1 = handle.await.unwrap();
        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.active, 2);
        assert_eq!(snap2.queue_len, 0);

        drop(p0);
        drop(p1);
    }

    #[tokio::test]
    async fn test_dynamic_policy_reduction_no_active_bypass() {
        let limiter = Limiter::new();
        let upstream = "up_dyn_reduce";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 3,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p1 = limiter.acquire(upstream).await.unwrap();
        let p2 = limiter.acquire(upstream).await.unwrap();

        // Reduce max_concurrency to 1: active requests (2) must NOT be bypassed
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 0, // reject immediately
                queue_timeout_ms: 0,
                first_output_timeout_ms: 0,
            },
        );

        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.active, 2);

        // Acquire should fail because active (2) >= max_concurrency (1)
        let err = limiter.acquire(upstream).await.unwrap_err();
        assert_eq!(err.category(), "local_queue_full");

        drop(p1);
        // Still active=1 >= max_concurrency(1), so new acquire should still fail
        let err2 = limiter.acquire(upstream).await.unwrap_err();
        assert_eq!(err2.category(), "local_queue_full");

        drop(p2);
        // Now active=0 < 1, acquire succeeds
        let mut p3 = limiter.acquire(upstream).await.unwrap();
        p3.commit_outbound();
        drop(p3);
    }

    #[tokio::test]
    async fn test_remove_rejection_and_reapply_preserves_active() {
        let limiter = Limiter::new();
        let upstream = "up_remove_test";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 2,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        let p1 = limiter.acquire(upstream).await.unwrap();
        assert_eq!(limiter.snapshot(upstream).unwrap().active, 1);

        // Remove upstream: marks tombstone and drains queue
        let removed = limiter.remove(upstream);
        assert!(removed);

        // New acquire must be rejected immediately (not re-create with default)
        let err = limiter.acquire(upstream).await.unwrap_err();
        match err {
            LimitError::Validation(msg) => assert!(msg.contains("上游已移除")),
            other => panic!("Expected Validation error on removed upstream, got {other:?}"),
        }

        // Active request p1 is still preserved
        assert_eq!(limiter.snapshot(upstream).unwrap().active, 1);

        // Reapply policy: revives upstream and retains active count (1)
        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 2,
                rpm: 0,
                queue_capacity: 5,
                queue_timeout_ms: 5000,
                first_output_timeout_ms: 0,
            },
        );

        assert_eq!(limiter.snapshot(upstream).unwrap().active, 1);
        let p2 = limiter.acquire(upstream).await.unwrap();
        assert_eq!(limiter.snapshot(upstream).unwrap().active, 2);

        drop(p1);
        drop(p2);
        assert_eq!(limiter.snapshot(upstream).unwrap().active, 0);
    }

    #[tokio::test]
    async fn test_snapshot_does_not_mutate_state() {
        let clock = Arc::new(MockClock::new());
        let limiter = Limiter::with_clock(Arc::clone(&clock));
        let upstream = "up_snap_test";

        limiter.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 0,
                rpm: 10,
                queue_capacity: 0,
                queue_timeout_ms: 0,
                first_output_timeout_ms: 0,
            },
        );

        let mut p1 = limiter.acquire(upstream).await.unwrap();
        p1.commit_outbound();
        drop(p1);

        // Advance clock past 60s
        clock.advance(Duration::from_secs(65));

        // Snapshot should show 0 active RPM without pruning timestamps internally
        let snap1 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap1.current_rpm, 0);

        let snap2 = limiter.snapshot(upstream).unwrap();
        assert_eq!(snap2.current_rpm, 0);
    }

    #[tokio::test]
    async fn test_instance_isolation() {
        let lim1 = Limiter::new();
        let lim2 = Limiter::new();
        let upstream = "shared_id";

        lim1.apply(
            upstream,
            UpstreamLimitPolicy {
                max_concurrency: 1,
                rpm: 0,
                queue_capacity: 0,
                queue_timeout_ms: 0,
                first_output_timeout_ms: 0,
            },
        );

        let p1 = lim1.acquire(upstream).await.unwrap();
        assert_eq!(lim1.snapshot(upstream).unwrap().active, 1);

        // lim2 has no policy or state for `shared_id`
        assert!(lim2.snapshot(upstream).is_none());
        let p2 = lim2.acquire(upstream).await.unwrap();
        assert_eq!(lim2.snapshot(upstream).unwrap().active, 1);

        drop(p1);
        drop(p2);
    }

    #[test]
    fn test_settings_persistence_success() {
        let conn = setup_test_db();
        let upstream = "up_test_1";
        let policy = UpstreamLimitPolicy {
            max_concurrency: 8,
            rpm: 120,
            queue_capacity: 32,
            queue_timeout_ms: 10000,
            first_output_timeout_ms: 25000,
        };

        persist_policy(&conn, upstream, &policy).unwrap();

        let loaded = load_policy(&conn, upstream).unwrap().unwrap();
        assert_eq!(loaded, policy);

        let all = load_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, upstream);
        assert_eq!(all[0].1, policy);

        let deleted = delete_policy(&conn, upstream).unwrap();
        assert!(deleted);
        assert!(load_policy(&conn, upstream).unwrap().is_none());
    }

    #[test]
    fn test_settings_persistence_unknown_upstream_rejected() {
        let conn = setup_test_db();
        let policy = UpstreamLimitPolicy::default();
        let err = persist_policy(&conn, "up_nonexistent", &policy).unwrap_err();
        match err {
            AppError::Config(msg) => assert!(msg.contains("上游不存在")),
            other => panic!("Expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn test_settings_load_into_limiter() {
        let conn = setup_test_db();
        let policy = UpstreamLimitPolicy {
            max_concurrency: 4,
            rpm: 200,
            queue_capacity: 10,
            queue_timeout_ms: 5000,
            first_output_timeout_ms: 10000,
        };

        persist_policy(&conn, "up_test_1", &policy).unwrap();
        persist_policy(&conn, "up_test_2", &policy).unwrap();

        let limiter = Limiter::new();
        let count = load_into_limiter(&conn, &limiter).unwrap();
        assert_eq!(count, 2);

        let snap1 = limiter.snapshot("up_test_1").unwrap();
        assert_eq!(snap1.policy.max_concurrency, 4);
        let snap2 = limiter.snapshot("up_test_2").unwrap();
        assert_eq!(snap2.policy.rpm, 200);
    }

    #[tokio::test]
    async fn reload_preserves_active_permits_and_replaces_saved_policy() {
        let conn = setup_test_db();
        let limiter = Limiter::new();
        let permit = limiter.acquire("up_test_1").await.unwrap();
        let policy = UpstreamLimitPolicy { max_concurrency: 1, queue_capacity: 0, ..Default::default() };
        persist_policy(&conn, "up_test_1", &policy).unwrap();
        load_into_limiter(&conn, &limiter).unwrap();
        assert_eq!(limiter.snapshot("up_test_1").unwrap().active, 1);
        assert!(limiter.acquire("up_test_1").await.is_err());
        drop(permit);
        assert!(limiter.acquire("up_test_1").await.is_ok());
        delete_policy(&conn, "up_test_1").unwrap();
        load_into_limiter(&conn, &limiter).unwrap();
        assert_eq!(limiter.policy("up_test_1"), UpstreamLimitPolicy::default());
    }

    #[tokio::test]
    async fn removing_unused_upstream_rejects_stale_dispatch() {
        let limiter = Limiter::new();
        assert!(limiter.remove("deleted"));
        assert!(matches!(limiter.acquire("deleted").await, Err(LimitError::Validation(_))));
    }

    #[test]
    fn test_sweep_idle() {
        let clock = Arc::new(MockClock::new());
        let limiter = Limiter::with_clock(Arc::clone(&clock));

        // Create default states
        limiter.apply("up_idle_1", UpstreamLimitPolicy::default());
        limiter.apply("up_idle_2", UpstreamLimitPolicy::default());
        let _ = limiter.snapshot("up_idle_1");
        let _ = limiter.snapshot("up_idle_2");

        // Advance past idle TTL (600s)
        clock.advance(Duration::from_secs(601));
        let swept = limiter.sweep_idle(DEFAULT_IDLE_TTL);
        assert!(swept >= 2);
    }
}
