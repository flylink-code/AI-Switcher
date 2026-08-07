//! Multi-account selection with sticky sessions and cooldown rotation.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use uuid::Uuid;

use super::account::{store, AntigravityAccount};
use crate::error::{AppError, AppResult};

const DEFAULT_COOLDOWN_SECS: i64 = 60;
const AUTH_COOLDOWN_SECS: i64 = 300;

pub struct AccountPool {
    sticky: Mutex<HashMap<String, String>>,
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
        }
    }

    pub fn select(
        &self,
        preferred_account_id: Option<&str>,
        session_key: Option<&str>,
    ) -> AppResult<(String, AntigravityAccount)> {
        let accounts = store().list_accounts()?;
        let now = Utc::now().timestamp();
        let mut candidates: Vec<_> = accounts
            .into_iter()
            .filter(|account| account_is_schedulable(account, now))
            .collect();
        if candidates.is_empty() {
            return Err(AppError::Config(
                "没有可用的 Antigravity 账号（请导入账号、等待冷却结束，或刷新额度）".into(),
            ));
        }

        if let Some(session) = session_key.filter(|value| !value.is_empty()) {
            if let Ok(guard) = self.sticky.lock() {
                if let Some(account_id) = guard.get(session) {
                    if let Some(account) = candidates.iter().find(|item| &item.id == account_id) {
                        return store().ensure_access_token(&account.id);
                    }
                }
            }
        }

        if let Some(preferred) = preferred_account_id.filter(|value| !value.is_empty()) {
            if let Some(account) = candidates.iter().find(|item| item.id == preferred) {
                let selected = store().ensure_access_token(&account.id)?;
                self.bind_session(session_key, &selected.1.id);
                return Ok(selected);
            }
        }

        if let Some(active) = candidates.iter().find(|item| item.is_active) {
            let selected = store().ensure_access_token(&active.id)?;
            self.bind_session(session_key, &selected.1.id);
            return Ok(selected);
        }

        sort_candidates_best_first(&mut candidates);
        let best = &candidates[0];
        let selected = store().ensure_access_token(&best.id)?;
        self.bind_session(session_key, &selected.1.id);
        Ok(selected)
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
            .into_iter()
            .filter(|account| account.id != failed_account_id)
            .filter(|account| !exclude.contains(&account.id))
            .filter(|account| account_is_schedulable(account, now))
            .collect();
        if candidates.is_empty() {
            return Err(AppError::Config(
                "账号池已耗尽，没有可轮换的 Antigravity 账号".into(),
            ));
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

fn sort_candidates_best_first(candidates: &mut [AntigravityAccount]) {
    candidates.sort_by(|left, right| {
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
    });
}
