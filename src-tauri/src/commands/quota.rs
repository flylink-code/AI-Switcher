//! Tauri command handlers for provider quota and balance queries.

use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::quota::types::ProviderQuotaResult;
use crate::quota::{query_official_quota, query_provider_quota};
use crate::store::AppState;

#[tauri::command]
pub async fn get_provider_quota(
    provider_id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProviderQuotaResult> {
    let mut provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &provider_id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {provider_id}")))
    })?;
    provider.api_key = state
        .db
        .with_conn(|conn| dao::resolve_api_key(conn, &provider_id))?
        .unwrap_or_default();

    Ok(query_provider_quota(&provider).await)
}

#[tauri::command]
pub async fn get_official_quota(
    target: ProviderTarget,
) -> AppResult<ProviderQuotaResult> {
    Ok(query_official_quota(target).await)
}
