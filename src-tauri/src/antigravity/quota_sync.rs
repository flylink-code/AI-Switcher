//! Background + manual Antigravity quota refresh against Cloud Code.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::antigravity::account::store as account_store;
use crate::antigravity::quota::fetch_quota;
use crate::antigravity::AntigravityAccountPublic;
use crate::error::{AppError, AppResult};

pub const QUOTA_REFRESH_INTERVAL_SECS: u64 = 5 * 60;
pub const QUOTA_REFRESH_EVENT: &str = "antigravity-quota-refreshed";

static REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Default)]
pub struct QuotaRefreshSummary {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
}

struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESH_RUNNING.store(false, Ordering::SeqCst);
    }
}

fn try_acquire_refresh_lock() -> Option<RefreshGuard> {
    if REFRESH_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return None;
    }
    Some(RefreshGuard)
}

pub async fn refresh_one_account_quota(account_id: &str) -> AppResult<AntigravityAccountPublic> {
    let (access_token, account) = account_store().ensure_access_token(account_id)?;
    let (quota, project_id) =
        fetch_quota(&access_token, account.token.project_id.as_deref()).await?;
    account_store().update_quota(account_id, quota, project_id)
}

/// Refresh every configured account; partial failures still return the latest snapshots.
pub async fn refresh_all_account_quotas() -> AppResult<Vec<AntigravityAccountPublic>> {
    let accounts = account_store().list_accounts()?;
    let mut results = Vec::with_capacity(accounts.len());
    let mut errors = Vec::new();
    for account in accounts {
        match refresh_one_account_quota(&account.id).await {
            Ok(public) => results.push(public),
            Err(error) => {
                log::warn!(
                    "Antigravity quota refresh failed for {}: {error}",
                    account.email
                );
                errors.push(format!("{}: {error}", account.email));
                results.push(AntigravityAccountPublic::from(&account));
            }
        }
    }
    if results.is_empty() && !errors.is_empty() {
        return Err(AppError::Other(format!(
            "刷新额度失败: {}",
            errors.join("; ")
        )));
    }
    Ok(results)
}

/// Best-effort refresh used by the background loop; skips when a run is already active.
pub async fn try_refresh_all_quotas() -> AppResult<QuotaRefreshSummary> {
    let Some(_guard) = try_acquire_refresh_lock() else {
        return Ok(QuotaRefreshSummary::default());
    };

    let accounts = account_store().list_accounts()?;
    if accounts.is_empty() {
        return Ok(QuotaRefreshSummary::default());
    }

    let mut summary = QuotaRefreshSummary {
        attempted: accounts.len(),
        ..QuotaRefreshSummary::default()
    };
    for account in accounts {
        match refresh_one_account_quota(&account.id).await {
            Ok(_) => summary.succeeded += 1,
            Err(error) => {
                summary.failed += 1;
                log::warn!(
                    "Antigravity quota auto-refresh failed for {}: {error}",
                    account.email
                );
            }
        }
    }
    Ok(summary)
}
