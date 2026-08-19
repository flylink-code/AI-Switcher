//! Per-account concurrency gates for Antigravity upstream requests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

/// Shared in-flight cap for main sessions and subagents on one account.
pub const ACCOUNT_CONCURRENCY_LIMIT: usize = 4;
/// Extra cap for catalog subagent traffic on the same account.
pub const SUBAGENT_CONCURRENCY_LIMIT: usize = 2;
/// Wait this long for a slot before proceeding without a permit.
pub const ACQUIRE_TIMEOUT_SECS: u64 = 20;

pub struct AccountLimiter {
    accounts: Mutex<HashMap<String, Arc<AccountSlots>>>,
}

struct AccountSlots {
    total: Arc<Semaphore>,
    subagent: Arc<Semaphore>,
}

/// Holds semaphore permits until dropped (end of request / stream).
pub struct LimiterPermit {
    _total: Option<OwnedSemaphorePermit>,
    _subagent: Option<OwnedSemaphorePermit>,
}

impl AccountLimiter {
    pub fn new() -> Self {
        Self {
            accounts: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, account_id: &str, is_subagent: bool) -> LimiterPermit {
        let slots = self.slots_for(account_id).await;
        let timeout = Duration::from_secs(ACQUIRE_TIMEOUT_SECS);

        let total = match tokio::time::timeout(timeout, slots.total.clone().acquire_owned()).await
        {
            Ok(Ok(permit)) => Some(permit),
            Ok(Err(_)) | Err(_) => None,
        };

        let subagent = if is_subagent {
            match tokio::time::timeout(timeout, slots.subagent.clone().acquire_owned()).await {
                Ok(Ok(permit)) => Some(permit),
                Ok(Err(_)) | Err(_) => None,
            }
        } else {
            None
        };

        if is_subagent && subagent.is_none() {
            log::debug!(
                "Antigravity subagent limiter timeout for {account_id}; proceeding without subagent slot"
            );
        } else if total.is_none() {
            log::debug!(
                "Antigravity account limiter timeout for {account_id}; proceeding without slot"
            );
        }

        LimiterPermit {
            _total: total,
            _subagent: subagent,
        }
    }

    async fn slots_for(&self, account_id: &str) -> Arc<AccountSlots> {
        let mut guard = self.accounts.lock().await;
        guard
            .entry(account_id.to_string())
            .or_insert_with(|| {
                Arc::new(AccountSlots {
                    total: Arc::new(Semaphore::new(ACCOUNT_CONCURRENCY_LIMIT)),
                    subagent: Arc::new(Semaphore::new(SUBAGENT_CONCURRENCY_LIMIT)),
                })
            })
            .clone()
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

    #[tokio::test]
    async fn subagent_requests_share_a_smaller_cap() {
        let limiter = AccountLimiter::new();
        let _main = limiter.acquire("acct-a", false).await;
        let _sub1 = limiter.acquire("acct-a", true).await;
        let _sub2 = limiter.acquire("acct-a", true).await;
        let late = tokio::time::timeout(
            Duration::from_millis(200),
            limiter.acquire("acct-a", true),
        )
        .await;
        assert!(late.is_err(), "third subagent should wait past short timeout");
    }
}
