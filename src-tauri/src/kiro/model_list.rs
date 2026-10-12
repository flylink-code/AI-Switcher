//! Live Kiro model list (`ListAvailableModels`). Kiro lists models per account and
//! changes the set often, so the catalog is refreshed from a real account and cached.

use crate::error::{AppError, AppResult};

use super::account::{now_ms, store, streaming_profile_arn, KiroAccount};
use super::models::{parse_available_models, save_live_models, KiroModelList};
use super::outbound::build_async_client;
use super::token::{ensure_access_token, should_force_refresh};

/// Asks Kiro for the model list with the first enabled account that answers, caches it
/// and refreshes the catalog views.
pub async fn refresh_models() -> AppResult<KiroModelList> {
    let accounts = store().list_public()?;
    let mut last_error: Option<AppError> = None;
    for public in accounts.into_iter().filter(|account| !account.disabled) {
        let Some(account) = store().get(&public.id) else {
            continue;
        };
        match fetch_for_account(account).await {
            Ok(list) => {
                crate::catalog::invalidate_view_cache();
                return Ok(list);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::Config("还没有可用的 Kiro 账号，无法刷新模型列表".into())))
}

async fn fetch_for_account(account: KiroAccount) -> AppResult<KiroModelList> {
    let mut account = ensure_access_token(&account, false).await?;
    let mut refreshed = false;
    loop {
        let (status, body) = request_models(&account).await?;
        if (200..300).contains(&status) {
            let models = parse_available_models(&body).map_err(AppError::Other)?;
            return Ok(save_live_models(models, now_ms()));
        }
        if should_force_refresh(status, &body, refreshed) {
            refreshed = true;
            account = ensure_access_token(&account, true).await?;
            continue;
        }
        let detail: String = body.chars().take(200).collect();
        return Err(AppError::Other(format!("获取 Kiro 模型列表失败 HTTP {status}: {detail}")));
    }
}

async fn request_models(account: &KiroAccount) -> AppResult<(u16, String)> {
    let region = account.region.trim();
    let region = if region.is_empty() { "us-east-1" } else { region };
    let response = build_async_client(20)
        .get(format!("https://q.{region}.amazonaws.com/ListAvailableModels"))
        .query(&[
            ("origin", "AI_EDITOR"),
            ("profileArn", streaming_profile_arn(account).as_str()),
        ])
        .header("authorization", format!("Bearer {}", account.access_token.trim()))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    Ok((status, body))
}
