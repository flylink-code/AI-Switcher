//! Multi-account selection with sticky sessions and cooldown rotation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use uuid::Uuid;

use super::account::{store, AntigravityAccount};
use crate::error::{AppError, AppResult};

const DEFAULT_COOLDOWN_SECS: i64 = 20;
const AUTH_COOLDOWN_SECS: i64 = 120;

pub struct AccountPool {
    sticky: Mutex<HashMap<String, String>>,
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
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
        let Some(chosen) =
            choose_candidate(&candidates, preferred_account_id, sticky_id.as_deref())
        else {
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
        let cooldown = if status == 401 || status == 403 {
            AUTH_COOLDOWN_SECS
        } else {
            DEFAULT_COOLDOWN_SECS
        };
        let _ = store().mark_cooldown(
            failed_account_id,
            cooldown,
            &format!("upstream status {status}"),
        );
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

/// Prefer an explicit account, then the user-marked active account, then a
/// sticky session binding. Sticky must not override a newly set active account.
fn choose_candidate<'a>(
    candidates: &'a [AntigravityAccount],
    preferred_account_id: Option<&str>,
    sticky_account_id: Option<&str>,
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

fn compare_candidates(left: &AntigravityAccount, right: &AntigravityAccount) -> std::cmp::Ordering {
    right
        .health_score
        .partial_cmp(&left.health_score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            right
                .remaining_quota
                .unwrap_or(0)
                .cmp(&left.remaining_quota.unwrap_or(0))
        })
}

fn sort_candidates_best_first(candidates: &mut [AntigravityAccount]) {
    candidates.sort_by(|left, right| compare_candidates(left, right));
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
        let chosen = choose_candidate(&candidates, None, Some("a2")).expect("chosen");
        assert_eq!(chosen.id, "a1");
        assert_eq!(
            selection_reason(chosen, None, Some("a2")),
            "active"
        );
    }

    #[test]
    fn sticky_used_when_active_is_not_schedulable() {
        let mut a1 = sample("a1", None);
        let mut a2 = sample("a2", None);
        a1.is_active = false;
        a2.is_active = false;
        let candidates = vec![a1, a2];
        let chosen = choose_candidate(&candidates, None, Some("a2")).expect("chosen");
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
        let chosen = choose_candidate(&candidates, Some("a2"), Some("a1")).expect("chosen");
        assert_eq!(chosen.id, "a2");
    }
}
