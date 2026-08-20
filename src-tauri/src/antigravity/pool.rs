//! Multi-account selection with sticky sessions and cooldown rotation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::account::{store, AntigravityAccount};
use super::limiter::AccountLimiter;
use crate::error::{AppError, AppResult};

const DEFAULT_COOLDOWN_SECS: i64 = 20;
const RATE_LIMIT_COOLDOWN_SECS: i64 = 45;
const AUTH_COOLDOWN_SECS: i64 = 180;
/// Cloud Code sometimes sends a huge Retry-After (hourly/daily reset). Capping
/// keeps a global RESOURCE_EXHAUSTED from parking every account for hours.
const MAX_RATE_LIMIT_COOLDOWN_SECS: i64 = 120;
/// SKU/RPM 429 while 5h/7d bars still have remaining. Short so another account
/// can pick up the same request without parking the first number for 45s+.
const SKU_RATE_LIMIT_COOLDOWN_SECS: i64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PoolQuotaWarning {
    pub has_warning: bool,
    pub warning_level: String, // "none" | "low_quota" | "exhausted"
    pub message: String,
    pub min_remaining_fraction: f64,
    pub total_usable_accounts: usize,
}

pub struct AccountPool {
    sticky: Mutex<HashMap<String, String>>,
    limiter: Option<Arc<AccountLimiter>>,
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
            limiter: None,
        }
    }

    pub fn with_limiter(limiter: Arc<AccountLimiter>) -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
            limiter: Some(limiter),
        }
    }

    /// Async entry for gateway handlers. Token refresh uses reqwest::blocking and
    /// must not run on the Tokio worker (it panics / drops the connection).
    pub async fn select_async(
        self: &Arc<Self>,
        preferred_account_id: Option<&str>,
        session_key: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let pool = Arc::clone(self);
        let preferred = preferred_account_id.map(str::to_owned);
        let session = session_key.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            pool.select(preferred.as_deref(), session.as_deref())
        })
        .await
        .map_err(|error| AppError::Other(format!("Antigravity 账号选择任务失败: {error}")))?
    }

    /// See [`Self::select_async`].
    pub async fn rotate_after_failure_async(
        self: &Arc<Self>,
        failed_account_id: &str,
        status: u16,
        session_key: Option<&str>,
        exclude: &[String],
    ) -> AppResult<(String, AntigravityAccount)> {
        let pool = Arc::clone(self);
        let failed = failed_account_id.to_owned();
        let session = session_key.map(str::to_owned);
        let exclude = exclude.to_vec();
        tokio::task::spawn_blocking(move || {
            pool.rotate_after_failure(&failed, status, session.as_deref(), &exclude)
        })
        .await
        .map_err(|error| AppError::Other(format!("Antigravity 账号轮换任务失败: {error}")))?
    }

    pub fn select(
        &self,
        preferred_account_id: Option<&str>,
        session_key: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let mut candidates: Vec<_> = accounts
            .iter()
            .filter(|account| account_is_schedulable(account, now))
            .cloned()
            .collect();
        if candidates.is_empty() {
            // Desktop health probes + short upstream blips can cool every account
            // at once. Prefer a soft retry over hard-failing with "no accounts".
            if let Some(soft) = soft_select_cooled_account(&accounts, now) {
                log::warn!(
                    "Antigravity pool: all accounts cooling; soft-selecting {}",
                    soft.email
                );
                let _ = store().clear_cooldown(&soft.id);
                candidates.push(soft);
            } else {
                return Err(AppError::Other(explain_unavailable(&accounts, now)));
            }
        }

        let sticky_id = session_key
            .filter(|value| !value.is_empty())
            .and_then(|session| {
                self.sticky
                    .lock()
                    .ok()
                    .and_then(|guard| guard.get(session).cloned())
            });
        let Some(chosen) = choose_candidate(
            &candidates,
            preferred_account_id,
            sticky_id.as_deref(),
            self.limiter.as_deref(),
        ) else {
            return Err(AppError::Other(explain_unavailable(&accounts, now)));
        };
        log::info!(
            "Antigravity pool select {} via {}",
            chosen.email,
            selection_reason(chosen, preferred_account_id, sticky_id.as_deref())
        );
        let selected = store().ensure_access_token(&chosen.id)?;
        self.bind_session(session_key, &selected.1.id);
        Ok(selected)
    }

    pub fn clear_sticky(&self) {
        if let Ok(mut guard) = self.sticky.lock() {
            guard.clear();
        }
    }

    pub fn rotate_after_failure(
        &self,
        failed_account_id: &str,
        status: u16,
        session_key: Option<&str>,
        exclude: &[String],
    ) -> AppResult<(String, AntigravityAccount)> {
        if status == 403 {
            let _ = store().mark_forbidden_403(failed_account_id, "上游返回 403 权限受限/账号异常");
        } else {
            let cooldown = if status == 401 {
                AUTH_COOLDOWN_SECS
            } else if status == 429 {
                RATE_LIMIT_COOLDOWN_SECS
            } else {
                DEFAULT_COOLDOWN_SECS
            };
            let _ = store().mark_cooldown(
                failed_account_id,
                cooldown,
                &format!("upstream status {status}"),
            );
        }
        if let Some(session) = session_key {
            if let Ok(mut guard) = self.sticky.lock() {
                guard.remove(session);
            }
        }
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let mut candidates: Vec<_> = accounts
            .iter()
            .filter(|account| account.id != failed_account_id)
            .filter(|account| !exclude.contains(&account.id))
            .filter(|account| account_is_schedulable(account, now))
            .cloned()
            .collect();
        if candidates.is_empty() {
            let remaining: Vec<_> = accounts
                .iter()
                .filter(|account| account.id != failed_account_id)
                .filter(|account| !exclude.contains(&account.id))
                .cloned()
                .collect();
            if remaining.is_empty() {
                return Err(AppError::Other(
                    "Antigravity 已尝试所有可用账号，上游均失败".into(),
                ));
            }
            if let Some(soft) = soft_select_cooled_account(&remaining, now) {
                log::warn!(
                    "Antigravity pool rotate: soft-selecting cooled {}",
                    soft.email
                );
                let _ = store().clear_cooldown(&soft.id);
                candidates.push(soft);
            } else {
                return Err(AppError::Other(explain_unavailable(&remaining, now)));
            }
        }
        sort_candidates_best_first(&mut candidates);
        let account = &candidates[0];
        let selected = store().ensure_access_token(&account.id)?;
        self.bind_session(session_key, &selected.1.id);
        Ok(selected)
    }

    pub fn note_success(&self, account_id: &str) {
        let _ = store().mark_success(account_id);
    }

    pub fn new_session_key() -> String {
        Uuid::new_v4().to_string()
    }

    /// Recommends the highest scored account currently schedulable in the pool.
    pub fn recommend_best_account(&self) -> AppResult<Option<AntigravityAccount>> {
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let mut candidates: Vec<_> = accounts
            .into_iter()
            .filter(|account| account_is_schedulable(account, now))
            .collect();
        if candidates.is_empty() {
            return Ok(None);
        }
        sort_candidates_best_first(&mut candidates);
        Ok(candidates.into_iter().next())
    }

    /// Check if the account pool is nearing quota exhaustion and needs warning.
    pub fn check_quota_warning(&self) -> AppResult<PoolQuotaWarning> {
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let non_disabled: Vec<_> = accounts.into_iter().filter(|a| !a.disabled).collect();
        if non_disabled.is_empty() {
            return Ok(PoolQuotaWarning {
                has_warning: true,
                warning_level: "exhausted".into(),
                message: "没有已启用的 Antigravity 账号".into(),
                min_remaining_fraction: 0.0,
                total_usable_accounts: 0,
            });
        }
        let usable: Vec<_> = non_disabled
            .iter()
            .filter(|a| account_is_schedulable(a, now))
            .cloned()
            .collect();
        let total_usable = usable.len();
        if total_usable == 0 {
            return Ok(PoolQuotaWarning {
                has_warning: true,
                warning_level: "exhausted".into(),
                message: "所有 Antigravity 账号均处于冷却或配额耗尽状态".into(),
                min_remaining_fraction: 0.0,
                total_usable_accounts: 0,
            });
        }
        let min_fraction = usable
            .iter()
            .map(|a| {
                a.quota
                    .as_ref()
                    .and_then(|q| q.best_remaining_fraction())
                    .or_else(|| a.remaining_quota.map(|q| (q as f64) / 100.0))
                    .unwrap_or(1.0)
            })
            .fold(1.0f64, f64::min);

        if min_fraction < 0.15 {
            Ok(PoolQuotaWarning {
                has_warning: true,
                warning_level: "low_quota".into(),
                message: format!(
                    "Antigravity 账号配额较低（剩余约 {:.0}%），建议关注或补充账号",
                    min_fraction * 100.0
                ),
                min_remaining_fraction: min_fraction,
                total_usable_accounts: total_usable,
            })
        } else {
            Ok(PoolQuotaWarning {
                has_warning: false,
                warning_level: "none".into(),
                message: "配额充足".into(),
                min_remaining_fraction: min_fraction,
                total_usable_accounts: total_usable,
            })
        }
    }

    fn bind_session(&self, session_key: Option<&str>, account_id: &str) {
        let Some(session) = session_key.filter(|value| !value.is_empty()) else {
            return;
        };
        if let Ok(mut guard) = self.sticky.lock() {
            guard.insert(session.to_string(), account_id.to_string());
        }
    }
}

impl Default for AccountPool {
    fn default() -> Self {
        Self::new()
    }
}

fn account_is_schedulable(account: &AntigravityAccount, now: i64) -> bool {
    if account.disabled {
        return false;
    }
    if account
        .cooldown_until
        .is_some_and(|until| until > now)
    {
        return false;
    }
    if let Some(quota) = account.quota.as_ref() {
        if !quota.has_usable_quota() {
            return false;
        }
    }
    true
}

/// When every otherwise-healthy account is only blocked by cooldown, pick the
/// one that cools down soonest so Desktop probes are not bricked for a full window.
fn soft_select_cooled_account(
    accounts: &[AntigravityAccount],
    now: i64,
) -> Option<AntigravityAccount> {
    let non_disabled: Vec<&AntigravityAccount> =
        accounts.iter().filter(|account| !account.disabled).collect();
    if non_disabled.is_empty() {
        return None;
    }
    let all_cooling = non_disabled.iter().all(|account| {
        account.cooldown_until.is_some_and(|until| until > now)
            && account
                .quota
                .as_ref()
                .map(|quota| quota.has_usable_quota())
                .unwrap_or(true)
    });
    if !all_cooling {
        return None;
    }
    let mut cooled: Vec<&AntigravityAccount> = non_disabled
        .into_iter()
        .filter(|account| {
            account
                .quota
                .as_ref()
                .map(|quota| quota.has_usable_quota())
                .unwrap_or(true)
        })
        .collect();
    cooled.sort_by_key(|account| account.cooldown_until.unwrap_or(0));
    cooled.first().map(|account| (*account).clone())
}

fn explain_unavailable(accounts: &[AntigravityAccount], now: i64) -> String {
    if accounts.is_empty() {
        return "没有可用的 Antigravity 账号（请先在网关页登录或导入）".into();
    }
    let disabled = accounts.iter().filter(|account| account.disabled).count();
    let cooling = accounts
        .iter()
        .filter(|account| {
            !account.disabled && account.cooldown_until.is_some_and(|until| until > now)
        })
        .count();
    let quota_out = accounts
        .iter()
        .filter(|account| {
            !account.disabled
                && account
                    .quota
                    .as_ref()
                    .is_some_and(|quota| !quota.has_usable_quota())
        })
        .count();
    let max_cool_rem = accounts
        .iter()
        .filter_map(|account| account.cooldown_until)
        .filter(|until| *until > now)
        .map(|until| until - now)
        .max()
        .unwrap_or(0);
    if cooling > 0 && disabled + quota_out + cooling >= accounts.len() {
        return format!(
            "Antigravity 账号冷却中（约 {max_cool_rem}s 后可重试；也可在网关页刷新额度）"
        );
    }
    if quota_out > 0 && disabled + quota_out >= accounts.len() {
        return "Antigravity 账号额度已耗尽（请等待重置或切换账号）".into();
    }
    if disabled == accounts.len() {
        return "Antigravity 账号均已禁用（请重新登录）".into();
    }
    "没有可用的 Antigravity 账号（请导入账号、等待冷却结束，或刷新额度）".into()
}

/// Prefer an explicit account, then the user-marked active account (soft when
/// hot), then a sticky session binding. Sticky must not override active.
fn choose_candidate<'a>(
    candidates: &'a [AntigravityAccount],
    preferred_account_id: Option<&str>,
    sticky_account_id: Option<&str>,
    limiter: Option<&AccountLimiter>,
) -> Option<&'a AntigravityAccount> {
    if candidates.is_empty() {
        return None;
    }
    if let Some(preferred) = preferred_account_id.filter(|value| !value.is_empty()) {
        if let Some(account) = candidates.iter().find(|item| item.id == preferred) {
            return Some(account);
        }
    }
    if let Some(active) = candidates.iter().find(|item| item.is_active) {
        let active_hot = account_under_pressure(limiter, &active.id);
        if !active_hot {
            return Some(active);
        }
        let mut ranked: Vec<&AntigravityAccount> = candidates.iter().collect();
        ranked.sort_by(|left, right| compare_candidates(left, right));
        if let Some(alternate) = ranked.iter().find(|account| {
            account.id != active.id && !account_under_pressure(limiter, &account.id)
        }) {
            log::info!(
                "Antigravity pool: active {} is hot; soft-selecting {}",
                active.email, alternate.email
            );
            return Some(*alternate);
        }
        return Some(active);
    }
    if let Some(sticky) = sticky_account_id.filter(|value| !value.is_empty()) {
        if let Some(account) = candidates.iter().find(|item| item.id == sticky) {
            return Some(account);
        }
    }
    let mut ranked: Vec<&AntigravityAccount> = candidates.iter().collect();
    ranked.sort_by(|left, right| compare_candidates(left, right));
    ranked.first().copied()
}

fn account_under_pressure(limiter: Option<&AccountLimiter>, account_id: &str) -> bool {
    limiter
        .map(|limiter| limiter.is_under_pressure(account_id))
        .unwrap_or(false)
}

fn selection_reason(
    chosen: &AntigravityAccount,
    preferred_account_id: Option<&str>,
    sticky_account_id: Option<&str>,
) -> &'static str {
    if preferred_account_id.is_some_and(|id| id == chosen.id) {
        return "preferred";
    }
    if chosen.is_active {
        return "active";
    }
    if sticky_account_id.is_some_and(|id| id == chosen.id) {
        return "sticky";
    }
    "best"
}

pub fn candidate_scheduling_score(account: &AntigravityAccount) -> f64 {
    let quota_fraction = account
        .quota
        .as_ref()
        .and_then(|q| q.best_remaining_fraction())
        .or_else(|| account.remaining_quota.map(|q| (q as f64) / 100.0))
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let health = (account.health_score as f64).clamp(0.0, 1.0);
    // Dynamic weighted score: 60% remaining quota + 40% health score
    quota_fraction * 0.6 + health * 0.4
}

fn compare_candidates(left: &AntigravityAccount, right: &AntigravityAccount) -> std::cmp::Ordering {
    let score_right = candidate_scheduling_score(right);
    let score_left = candidate_scheduling_score(left);
    score_right
        .partial_cmp(&score_left)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            // Least recently used tie breaker for load balancing
            left.last_used.cmp(&right.last_used)
        })
}

fn sort_candidates_best_first(candidates: &mut [AntigravityAccount]) {
    candidates.sort_by(|left, right| compare_candidates(left, right));
}

/// 429 with remaining quota is a Cloud Code SKU/RPM limit — do not walk the
/// rest of the pool. The gateway may still try one extra account (short cooldown).
/// Only treat the snapshot as "rotate freely" when this account is empty.
pub(crate) fn should_rotate_pool_on_429(account: &AntigravityAccount) -> bool {
    account
        .quota
        .as_ref()
        .is_some_and(|quota| !quota.has_usable_quota())
}

pub(crate) fn rate_limit_cooldown_secs(retry_after: Option<u64>) -> i64 {
    match retry_after {
        Some(secs) => (secs as i64).clamp(1, MAX_RATE_LIMIT_COOLDOWN_SECS),
        None => RATE_LIMIT_COOLDOWN_SECS,
    }
}

pub(crate) fn sku_rate_limit_cooldown_secs(retry_after: Option<u64>) -> i64 {
    match retry_after {
        Some(secs) => (secs as i64).clamp(1, SKU_RATE_LIMIT_COOLDOWN_SECS),
        None => SKU_RATE_LIMIT_COOLDOWN_SECS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::antigravity::account::AntigravityToken;
    use crate::antigravity::quota::QuotaSnapshot;

    fn sample(id: &str, cooldown_until: Option<i64>) -> AntigravityAccount {
        AntigravityAccount {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: None,
            token: AntigravityToken {
                access_token: "a".into(),
                refresh_token: "r".into(),
                expires_in: 3600,
                expiry_timestamp: Utc::now().timestamp() + 3600,
                token_type: "Bearer".into(),
                email: None,
                project_id: Some("p".into()),
                session_id: None,
            },
            is_active: id == "a1",
            created_at: 0,
            last_used: 0,
            health_score: 1.0,
            disabled: false,
            disabled_reason: None,
            cooldown_until,
            remaining_quota: Some(100),
            quota: Some(QuotaSnapshot {
                last_updated: Utc::now().timestamp(),
                ..QuotaSnapshot::default()
            }),
        }
    }

    #[test]
    fn soft_selects_when_all_accounts_cooling() {
        let now = Utc::now().timestamp();
        let accounts = vec![
            sample("a1", Some(now + 30)),
            sample("a2", Some(now + 10)),
        ];
        let soft = soft_select_cooled_account(&accounts, now).expect("soft");
        assert_eq!(soft.id, "a2");
    }

    #[test]
    fn no_soft_select_when_one_is_ready() {
        let now = Utc::now().timestamp();
        let accounts = vec![sample("a1", Some(now + 30)), sample("a2", None)];
        assert!(soft_select_cooled_account(&accounts, now).is_none());
    }

    #[test]
    fn active_account_beats_sticky_session() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        let candidates = vec![a1, a2];
        let chosen = choose_candidate(&candidates, None, Some("a2"), None).expect("chosen");
        assert_eq!(chosen.id, "a1");
        assert_eq!(
            selection_reason(chosen, None, Some("a2")),
            "active"
        );
    }

    #[test]
    fn active_under_pressure_yields_best_alternative() {
        let limiter = AccountLimiter::new();
        limiter.note_upstream_rate_limited("a1");
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        a2.remaining_quota = Some(100);
        a2.health_score = 1.0;
        let candidates = vec![a1, a2];
        let chosen =
            choose_candidate(&candidates, None, None, Some(&limiter)).expect("chosen");
        assert_eq!(chosen.id, "a2");
        assert_eq!(selection_reason(chosen, None, None), "best");
    }

    #[test]
    fn sticky_used_when_active_is_not_schedulable() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = false;
        a2.is_active = false;
        let candidates = vec![a1, a2];
        let chosen = choose_candidate(&candidates, None, Some("a2"), None).expect("chosen");
        assert_eq!(chosen.id, "a2");
        assert_eq!(
            selection_reason(chosen, None, Some("a2")),
            "sticky"
        );
    }

    #[test]
    fn preferred_beats_active() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        let candidates = vec![a1, a2];
        let chosen =
            choose_candidate(&candidates, Some("a2"), Some("a1"), None).expect("chosen");
        assert_eq!(chosen.id, "a2");
    }

    #[test]
    fn candidate_scheduling_score_ranks_health_and_quota() {
        let mut healthy_high_quota = sample("high", None);
        healthy_high_quota.health_score = 1.0;
        healthy_high_quota.remaining_quota = Some(90);

        let mut low_quota = sample("low", None);
        low_quota.health_score = 1.0;
        low_quota.remaining_quota = Some(10);

        assert!(candidate_scheduling_score(&healthy_high_quota) > candidate_scheduling_score(&low_quota));
    }

    #[test]
    fn rate_limit_429_does_not_rotate_when_quota_remains() {
        let ready = sample("a1", None);
        assert!(!should_rotate_pool_on_429(&ready));

        let mut exhausted = sample("a2", None);
        exhausted.quota = Some(QuotaSnapshot::empty_forbidden("5h empty"));
        exhausted.remaining_quota = Some(0);
        assert!(should_rotate_pool_on_429(&exhausted));
    }

    #[test]
    fn rate_limit_cooldown_caps_huge_retry_after() {
        assert_eq!(rate_limit_cooldown_secs(None), 45);
        assert_eq!(rate_limit_cooldown_secs(Some(2)), 2);
        assert_eq!(rate_limit_cooldown_secs(Some(3600)), 120);
        assert_eq!(sku_rate_limit_cooldown_secs(None), 15);
        assert_eq!(sku_rate_limit_cooldown_secs(Some(2)), 2);
        assert_eq!(sku_rate_limit_cooldown_secs(Some(3600)), 15);
    }
}
