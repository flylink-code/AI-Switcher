use crate::codex_oauth::{
    manager, CodexOauthAccount, CodexOauthDeviceStart, CodexOauthPollResult,
    CODEX_OAUTH_BASE_URL,
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
pub fn remove_codex_oauth_account(account_id: String) -> AppResult<()> {
    manager().remove_account(&account_id)
}

#[tauri::command]
pub fn set_default_codex_oauth_account(account_id: String) -> AppResult<()> {
    manager().set_default_account(&account_id)
}

#[tauri::command]
pub fn ensure_codex_oauth_provider(
    target: ProviderTarget,
    account_id: String,
    model: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if !matches!(target, ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop) {
        return Err(AppError::Config(
            "ChatGPT 订阅目前仅支持 Claude Code 和 Claude Desktop".to_string(),
        ));
    }
    if !manager()
        .list_accounts()
        .iter()
        .any(|account| account.account_id == account_id)
    {
        return Err(AppError::Config("ChatGPT 账户不存在".to_string()));
    }
    manager().set_default_account(&account_id)?;
    let existing = state.db.with_conn(|conn| {
        Ok(dao::list_providers(conn, target)?
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
            .unwrap_or_else(|| "gpt-5.4".to_string()),
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: ClaudeModelMapping::default(),
        protocol_type: ProtocolType::OpenAiResponses,
        target_app: target,
        provider_kind: ProviderKind::CodexOauth,
        auth_binding: account_id,
        notes: String::new(),
    };
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}
