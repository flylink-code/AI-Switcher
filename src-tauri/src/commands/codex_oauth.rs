use crate::codex_oauth::{
    manager, CodexOauthAccount, CodexOauthDeviceStart, CodexOauthPollResult, CODEX_OAUTH_BASE_URL,
};
use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::{
    ClaudeModelMapping, ProtocolType, Provider, ProviderInput, ProviderKind, ProviderTarget,
};
use crate::store::AppState;

#[tauri::command]
pub async fn start_codex_oauth_login() -> AppResult<CodexOauthDeviceStart> {
    tauri::async_runtime::spawn_blocking(|| manager().start_device_flow())
        .await
        .map_err(|error| AppError::Tauri(format!("OAuth 登录任务失败: {error}")))?
}

#[tauri::command]
pub async fn poll_codex_oauth_login(device_code: String) -> AppResult<CodexOauthPollResult> {
    tauri::async_runtime::spawn_blocking(move || manager().poll_device_flow(&device_code))
        .await
        .map_err(|error| AppError::Tauri(format!("OAuth 轮询任务失败: {error}")))?
}

#[tauri::command]
pub fn list_codex_oauth_accounts() -> Vec<CodexOauthAccount> {
    manager().list_accounts()
}

#[tauri::command]
pub async fn ensure_codex_oauth_provider(
    target: ProviderTarget,
    account_id: String,
    model: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if !manager()
        .list_accounts()
        .iter()
        .any(|account| account.account_id == account_id)
    {
        return Err(AppError::Config("ChatGPT 账户不存在".to_string()));
    }
    manager().set_default_account(&account_id)?;
    let existing = state.db.with_conn(|conn| {
        Ok(dao::gateway::list_upstream_providers(conn, true)?
            .into_iter()
            .find(|provider| {
                provider.provider_kind == ProviderKind::CodexOauth
                    && provider.auth_binding == account_id
            }))
    })?;
    let input = ProviderInput {
        id: existing.map(|provider| provider.id),
        name: "ChatGPT 订阅".to_string(),
        base_url: CODEX_OAUTH_BASE_URL.to_string(),
        api_key: String::new(),
        clear_api_key: false,
        model: model
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "gpt-6-astra".to_string()),
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: ClaudeModelMapping::default(),
        protocol_type: ProtocolType::OpenAiResponses,
        target_app: target,
        provider_kind: ProviderKind::CodexOauth,
        auth_binding: account_id,
        notes: String::new(),
        failover_group: 0,
        failover_models: Vec::new(),
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    };
    let mut provider = state
        .db
        .with_conn(|conn| dao::gateway::upsert_upstream(conn, &input))?;
    crate::catalog::invalidate_view_cache();
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    provider.target_app = target;
    if !provider.api_key.starts_with("kr://") {
        provider.api_key = String::new();
    }
    Ok(provider)
}

/// 当前默认的 ChatGPT 账号 id；未登录任何账号时为 `None`。
#[tauri::command]
pub fn get_codex_oauth_default_account() -> Option<String> {
    manager().default_account_id()
}

/// 把某个 ChatGPT 账号设为默认账号（网关未指定账号时使用）。
#[tauri::command]
pub fn set_codex_oauth_default_account(account_id: String) -> AppResult<()> {
    manager().set_default_account(&account_id)
}

/// 移除 ChatGPT 账号，并删除绑定到该账号的网关上游。
///
/// 先确认所有绑定上游都可删（未被 Agent 直连占用），再动账号；否则账号已删而
/// 上游残留，网关会指向一个没有凭据的账号。
#[tauri::command]
pub async fn remove_codex_oauth_account(
    account_id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let _guard = crate::commands::providers::agent_connection_lock()
        .lock()
        .await;
    let bound_ids: Vec<String> = state.db.with_conn(|conn| {
        let ids: Vec<String> = dao::gateway::list_upstream_providers(conn, true)?
            .into_iter()
            .filter(|provider| {
                provider.provider_kind == ProviderKind::CodexOauth
                    && provider.auth_binding == account_id
            })
            .map(|provider| provider.id)
            .collect();
        for id in &ids {
            dao::gateway::assert_upstream_deletable(conn, id)?;
        }
        Ok(ids)
    })?;
    manager().remove_account(&account_id)?;
    state.db.with_conn(|conn| {
        for id in &bound_ids {
            dao::gateway::delete_upstream(conn, id)?;
        }
        Ok(())
    })?;
    for id in &bound_ids {
        state.db.gateway_upstream_limiter.remove(id);
    }
    crate::catalog::invalidate_view_cache();
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(())
}
