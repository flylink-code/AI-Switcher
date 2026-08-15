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
    let quota_result = fetch_quota(&access_token, account.token.project_id.as_deref()).await;
    let (quota, project_id) = match quota_result {
        Ok(result) => result,
        Err(error) if error.to_string().contains("401") || error.to_string().contains("Unauthorized") => {
            log::info!(
                "Antigravity quota access token rejected for {}; forcing token renewal and retrying",
                account.email
            );
            let (access_token, refreshed_account) = match account_store().force_refresh_access_token(account_id) {
                Ok(result) => result,
                Err(refresh_error) => {
                    let detail = refresh_error.to_string();
                    if detail.contains("invalid_grant") || detail.contains("revoked") {
                        account_store().mark_reauthorization_required(
                            account_id,
                            "Google 授权已失效，请重新用浏览器登录此账号",
                        )?;
                        return Err(AppError::Config(format!(
                            "账号 {} 的 Google 授权已失效，请重新用浏览器登录后刷新额度",
                            account.email
                        )));
                    }
                    return Err(refresh_error);
                }
            };
            fetch_quota(&access_token, refreshed_account.token.project_id.as_deref()).await?
        }
        Err(error) => return Err(error),
    };
    account_store().update_quota(account_id, quota, project_id)
}

/// Refresh every configured account; partial failures still return the latest snapshots.
/// Runs account probes concurrently so one slow account does not starve the rest.
pub async fn refresh_all_account_quotas() -> AppResult<Vec<AntigravityAccountPublic>> {
    let accounts = account_store().list_accounts()?;
    if accounts.is_empty() {
        return Ok(Vec::new());
    }

    let mut handles = Vec::with_capacity(accounts.len());
    for account in accounts {
        let id = account.id.clone();
        let email = account.email.clone();
        let fallback = AntigravityAccountPublic::from(&account);
        handles.push(async move {
            match refresh_one_account_quota(&id).await {
                Ok(public) => Ok(public),
                Err(error) => {
                    log::warn!("Antigravity quota refresh failed for {email}: {error}");
                    Err((format!("{email}: {error}"), fallback))
                }
            }
        });
    }

    let settled = futures_util::future::join_all(handles).await;
    let mut results = Vec::with_capacity(settled.len());
    let mut errors = Vec::new();
    for item in settled {
        match item {
            Ok(public) => results.push(public),
            Err((message, fallback)) => {
                errors.push(message);
                results.push(fallback);
            }
        }
    }

    // Failed accounts retain their cached rows for partial success, but a manual
    // refresh must report failure when every live request failed.
    if !errors.is_empty() && errors.len() == results.len() {
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

    let mut handles = Vec::with_capacity(accounts.len());
    for account in accounts {
        let id = account.id.clone();
        let email = account.email.clone();
        handles.push(async move {
            match refresh_one_account_quota(&id).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    log::warn!("Antigravity quota auto-refresh failed for {email}: {error}");
                    Err(())
                }
            }
        });
    }

    for item in futures_util::future::join_all(handles).await {
        match item {
            Ok(()) => summary.succeeded += 1,
            Err(()) => summary.failed += 1,
        }
    }
    Ok(summary)
}
