//! Multi-account selection with sticky sessions and cooldown rotation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::account::{store, AntigravityAccount};
use super::limiter::AccountLimiter;
use super::quota::{quota_family_from_model, QuotaFamily};
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
/// A missing response is transient and must not pin the preferred account for
/// the normal 20-second server-error cooldown.
const UPSTREAM_TIMEOUT_COOLDOWN_SECS: i64 = 5;

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
        requested_model: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let pool = Arc::clone(self);
        let preferred = preferred_account_id.map(str::to_owned);
        let session = session_key.map(str::to_owned);
        let model = requested_model.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            pool.select(
                preferred.as_deref(),
                session.as_deref(),
                model.as_deref(),
            )
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
        requested_model: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let pool = Arc::clone(self);
        let failed = failed_account_id.to_owned();
        let session = session_key.map(str::to_owned);
        let exclude = exclude.to_vec();
        let model = requested_model.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            pool.rotate_after_failure(
                &failed,
                status,
                session.as_deref(),
                &exclude,
                model.as_deref(),
            )
        })
        .await
        .map_err(|error| AppError::Other(format!("Antigravity 账号轮换任务失败: {error}")))?
    }

    pub fn select(
        &self,
        preferred_account_id: Option<&str>,
        session_key: Option<&str>,
        requested_model: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let family = requested_family(requested_model);
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let mut candidates = collect_schedulable_with_family_fallback(&accounts, now, family);
        if candidates.is_empty() {
            // Desktop health probes + short upstream blips can cool every account
            // at once. Prefer a soft retry over hard-failing with "no accounts".
            if let Some(soft) = soft_select_cooled_account(&accounts, now, family) {
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
            family,
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

    /// Remove one session's preferred account after a transient timeout so the
    /// next retry can select another healthy account.
    pub fn clear_session(&self, session_key: Option<&str>) {
        let Some(session) = session_key.filter(|value| !value.is_empty()) else {
            return;
        };
        if let Ok(mut guard) = self.sticky.lock() {
            guard.remove(session);
        }
    }

    pub fn rotate_after_failure(
        &self,
        failed_account_id: &str,
        status: u16,
        session_key: Option<&str>,
        exclude: &[String],
        requested_model: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let family = requested_family(requested_model);
        if status == 403 {
            let _ = store().mark_forbidden_403(failed_account_id, "上游返回 403 权限受限/账号异常");
        } else {
            let cooldown = if status == 401 {
                AUTH_COOLDOWN_SECS
            } else if status == 429 {
                RATE_LIMIT_COOLDOWN_SECS
            } else if status == 504 {
                timeout_cooldown_secs()
            } else {
                DEFAULT_COOLDOWN_SECS
            };
            let _ = store().mark_cooldown(
                failed_account_id,
                cooldown,
                &format!("upstream status {status}"),
            );
        }
        self.clear_session(session_key);
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let remaining: Vec<_> = accounts
            .iter()
            .filter(|account| account.id != failed_account_id)
            .filter(|account| !exclude.contains(&account.id))
            .cloned()
            .collect();
        let mut candidates = collect_schedulable_with_family_fallback(&remaining, now, family);
        if candidates.is_empty() {
            if remaining.is_empty() {
                return Err(AppError::Other(
                    "Antigravity 已尝试所有可用账号，上游均失败".into(),
                ));
            }
            if let Some(soft) = soft_select_cooled_account(&remaining, now, family) {
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
        sort_candidates_best_first(&mut candidates, family);
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
            .filter(|account| account_is_schedulable(account, now, None))
            .collect();
        if candidates.is_empty() {
            return Ok(None);
        }
        sort_candidates_best_first(&mut candidates, None);
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
            .filter(|a| account_is_schedulable(a, now, None))
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

fn requested_family(requested_model: Option<&str>) -> Option<QuotaFamily> {
    requested_model
        .filter(|model| !model.is_empty())
        .map(quota_family_from_model)
}

fn quota_usable(account: &AntigravityAccount, family: Option<QuotaFamily>) -> bool {
    let Some(quota) = account.quota.as_ref() else {
        return true;
    };
    match family {
        Some(family) => quota.has_usable_quota_for_family(family),
        None => quota.has_usable_quota(),
    }
}

fn account_has_family_remaining(
    account: &AntigravityAccount,
    family: Option<QuotaFamily>,
) -> bool {
    match family {
        None => true,
        Some(family) => account
            .quota
            .as_ref()
            .map(|quota| quota.has_usable_quota_for_family(family))
            .unwrap_or(true),
    }
}

fn collect_schedulable(
    accounts: &[AntigravityAccount],
    now: i64,
    family: Option<QuotaFamily>,
) -> Vec<AntigravityAccount> {
    accounts
        .iter()
        .filter(|account| account_is_schedulable(account, now, family))
        .cloned()
        .collect()
}

fn collect_schedulable_with_family_fallback(
    accounts: &[AntigravityAccount],
    now: i64,
    family: Option<QuotaFamily>,
) -> Vec<AntigravityAccount> {
    let mut candidates = collect_schedulable(accounts, now, family);
    if candidates.is_empty() && family.is_some() {
        log::warn!(
            "Antigravity pool: no accounts with {:?} remaining; soft-fallback to any schedulable account",
            family
        );
        candidates = collect_schedulable(accounts, now, None);
    }
    candidates
}

fn account_is_schedulable(
    account: &AntigravityAccount,
    now: i64,
    family: Option<QuotaFamily>,
) -> bool {
    if account.disabled {
        return false;
    }
    if account
        .cooldown_until
        .is_some_and(|until| until > now)
    {
        return false;
    }
    quota_usable(account, family)
}

/// When every otherwise-healthy account is only blocked by cooldown, pick the
/// one that cools down soonest so Desktop probes are not bricked for a full window.
fn soft_select_cooled_account(
    accounts: &[AntigravityAccount],
    now: i64,
    family: Option<QuotaFamily>,
) -> Option<AntigravityAccount> {
    let non_disabled: Vec<&AntigravityAccount> =
        accounts.iter().filter(|account| !account.disabled).collect();
    if non_disabled.is_empty() {
        return None;
    }
    let all_cooling = non_disabled.iter().all(|account| {
        account.cooldown_until.is_some_and(|until| until > now)
            && quota_usable(account, family)
    });
    if !all_cooling {
        return None;
    }
    let mut cooled: Vec<&AntigravityAccount> = non_disabled
        .into_iter()
        .filter(|account| quota_usable(account, family))
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
    family: Option<QuotaFamily>,
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
        if account_has_family_remaining(active, family) {
            let active_hot = account_under_pressure(limiter, &active.id);
            if !active_hot {
                return Some(active);
            }
            let mut ranked: Vec<&AntigravityAccount> = candidates.iter().collect();
            ranked.sort_by(|left, right| compare_candidates(left, right, family));
            if let Some(alternate) = ranked.iter().find(|account| {
                account.id != active.id
                    && !account_under_pressure(limiter, &account.id)
                    && account_has_family_remaining(account, family)
            }) {
                log::info!(
                    "Antigravity pool: active {} is hot; soft-selecting {}",
                    active.email, alternate.email
                );
                return Some(*alternate);
            }
            return Some(active);
        }
        log::info!(
            "Antigravity pool: active {} has no remaining {:?} quota; skipping",
            active.email, family
        );
    }
    if let Some(sticky) = sticky_account_id.filter(|value| !value.is_empty()) {
        if let Some(account) = candidates.iter().find(|item| item.id == sticky) {
            if account_has_family_remaining(account, family) {
                return Some(account);
            }
        }
    }
    let mut ranked: Vec<&AntigravityAccount> = candidates.iter().collect();
    ranked.sort_by(|left, right| compare_candidates(left, right, family));
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
    candidate_scheduling_score_for(account, None)
}

fn candidate_scheduling_score_for(
    account: &AntigravityAccount,
    family: Option<QuotaFamily>,
) -> f64 {
    let quota_fraction = if let Some(family) = family {
        account
            .quota
            .as_ref()
            .and_then(|q| q.remaining_fraction_for_family(family))
            .or_else(|| account.remaining_quota.map(|q| (q as f64) / 100.0))
            .unwrap_or(1.0)
    } else {
        account
            .quota
            .as_ref()
            .and_then(|q| q.best_remaining_fraction())
            .or_else(|| account.remaining_quota.map(|q| (q as f64) / 100.0))
            .unwrap_or(1.0)
    }
    .clamp(0.0, 1.0);
    let health = (account.health_score as f64).clamp(0.0, 1.0);
    // Dynamic weighted score: 60% remaining quota + 40% health score
    quota_fraction * 0.6 + health * 0.4
}

fn compare_candidates(
    left: &AntigravityAccount,
    right: &AntigravityAccount,
    family: Option<QuotaFamily>,
) -> std::cmp::Ordering {
    let score_right = candidate_scheduling_score_for(right, family);
    let score_left = candidate_scheduling_score_for(left, family);
    score_right
        .partial_cmp(&score_left)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            // Least recently used tie breaker for load balancing
            left.last_used.cmp(&right.last_used)
        })
}

fn sort_candidates_best_first(
    candidates: &mut [AntigravityAccount],
    family: Option<QuotaFamily>,
) {
    candidates.sort_by(|left, right| compare_candidates(left, right, family));
}

/// 429 with remaining quota for the requested family is a Cloud Code SKU/RPM
/// limit — do not walk the rest of the pool. Rotate freely only when this
/// family's 5h/7d bars are empty (the other family must not keep the account
/// "schedulable").
pub(crate) fn should_rotate_pool_on_429(
    account: &AntigravityAccount,
    family: Option<QuotaFamily>,
) -> bool {
    account
        .quota
        .as_ref()
        .is_some_and(|quota| match family {
            Some(family) => !quota.has_usable_quota_for_family(family),
            None => !quota.has_usable_quota(),
        })
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

pub(crate) fn timeout_cooldown_secs() -> i64 {
    UPSTREAM_TIMEOUT_COOLDOWN_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::antigravity::account::AntigravityToken;
    use crate::antigravity::quota::{QuotaBucket, QuotaGroup, QuotaSnapshot};

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
        let soft = soft_select_cooled_account(&accounts, now, None).expect("soft");
        assert_eq!(soft.id, "a2");
    }

    #[test]
    fn clearing_one_session_keeps_other_sticky_bindings() {
        let pool = AccountPool::new();
        pool.bind_session(Some("session-a"), "a1");
        pool.bind_session(Some("session-b"), "a2");

        pool.clear_session(Some("session-a"));

        let sticky = pool.sticky.lock().unwrap();
        assert!(!sticky.contains_key("session-a"));
        assert_eq!(sticky.get("session-b").map(String::as_str), Some("a2"));
    }

    #[test]
    fn timeout_cooldown_is_shorter_than_generic_failure_cooldown() {
        assert_eq!(timeout_cooldown_secs(), UPSTREAM_TIMEOUT_COOLDOWN_SECS);
        assert!(timeout_cooldown_secs() < DEFAULT_COOLDOWN_SECS);
    }

    #[test]
    fn no_soft_select_when_one_is_ready() {
        let now = Utc::now().timestamp();
        let accounts = vec![sample("a1", Some(now + 30)), sample("a2", None)];
        assert!(soft_select_cooled_account(&accounts, now, None).is_none());
    }

    #[test]
    fn active_account_beats_sticky_session() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        let candidates = vec![a1, a2];
        let chosen = choose_candidate(&candidates, None, Some("a2"), None, None).expect("chosen");
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
            choose_candidate(&candidates, None, None, Some(&limiter), None).expect("chosen");
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
        let chosen = choose_candidate(&candidates, None, Some("a2"), None, None).expect("chosen");
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
            choose_candidate(&candidates, Some("a2"), Some("a1"), None, None).expect("chosen");
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
        assert!(!should_rotate_pool_on_429(&ready, None));

        let mut exhausted = sample("a2", None);
        exhausted.quota = Some(QuotaSnapshot::empty_forbidden("5h empty"));
        exhausted.remaining_quota = Some(0);
        assert!(should_rotate_pool_on_429(&exhausted, None));
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

    fn family_quota(
        gemini_5h: f64,
        gemini_week: f64,
        claude_5h: f64,
        claude_week: f64,
    ) -> QuotaSnapshot {
        QuotaSnapshot {
            last_updated: Utc::now().timestamp(),
            groups: vec![
                QuotaGroup {
                    display_name: "Gemini Models".into(),
                    buckets: vec![
                        QuotaBucket {
                            bucket_id: "gemini-5h".into(),
                            window: "5h".into(),
                            remaining_fraction: gemini_5h,
                            reset_time: "t".into(),
                            display_name: None,
                        },
                        QuotaBucket {
                            bucket_id: "gemini-weekly".into(),
                            window: "weekly".into(),
                            remaining_fraction: gemini_week,
                            reset_time: "t".into(),
                            display_name: None,
                        },
                    ],
                },
                QuotaGroup {
                    display_name: "Claude + GPT".into(),
                    buckets: vec![
                        QuotaBucket {
                            bucket_id: "3p-claude-5h".into(),
                            window: "5h".into(),
                            remaining_fraction: claude_5h,
                            reset_time: "t".into(),
                            display_name: None,
                        },
                        QuotaBucket {
                            bucket_id: "3p-claude-weekly".into(),
                            window: "weekly".into(),
                            remaining_fraction: claude_week,
                            reset_time: "t".into(),
                            display_name: None,
                        },
                    ],
                },
            ],
            ..QuotaSnapshot::default()
        }
    }

    fn pick_for_model(accounts: &[AntigravityAccount], model: &str) -> String {
        let now = 0;
        let family = Some(quota_family_from_model(model));
        let candidates: Vec<_> = accounts
            .iter()
            .filter(|account| account_is_schedulable(account, now, family))
            .cloned()
            .collect();
        let chosen = choose_candidate(&candidates, None, None, None, family).expect("chosen");
        chosen.id.clone()
    }

    #[test]
    fn gemini_request_skips_account_with_only_claude_remaining() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        a1.quota = Some(family_quota(0.0, 0.0, 0.8, 0.8));
        a2.quota = Some(family_quota(0.6, 0.6, 0.0, 0.0));
        assert_eq!(
            pick_for_model(&[a1, a2], "gemini-3.7-flash-high"),
            "a2"
        );
    }

    #[test]
    fn claude_request_skips_account_with_only_gemini_remaining() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        a1.quota = Some(family_quota(0.8, 0.8, 0.0, 0.0));
        a2.quota = Some(family_quota(0.0, 0.0, 0.7, 0.7));
        assert_eq!(pick_for_model(&[a1, a2], "claude-opus-4-6"), "a2");
    }

    #[test]
    fn active_account_skipped_when_requested_family_is_empty() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = true;
        a2.is_active = false;
        a1.quota = Some(family_quota(0.0, 0.0, 0.9, 0.9));
        a2.quota = Some(family_quota(0.4, 0.4, 0.1, 0.1));
        let family = Some(QuotaFamily::Gemini);
        let candidates = vec![a1, a2];
        let chosen =
            choose_candidate(&candidates, None, Some("a1"), None, family).expect("chosen");
        assert_eq!(chosen.id, "a2");
        assert_eq!(selection_reason(chosen, None, Some("a1")), "best");
    }

    #[test]
    fn rotate_on_429_when_requested_family_empty() {
        let mut gemini_empty = sample("a1", None);
        gemini_empty.quota = Some(family_quota(0.0, 0.0, 0.8, 0.8));
        assert!(should_rotate_pool_on_429(
            &gemini_empty,
            Some(QuotaFamily::Gemini)
        ));
        assert!(!should_rotate_pool_on_429(
            &gemini_empty,
            Some(QuotaFamily::ClaudeGpt)
        ));
    }
}
