//! Tauri commands for the built-in Antigravity gateway.

use crate::antigravity::account::store as account_store;
use crate::antigravity::quota::fetch_quota;
use crate::antigravity::{
    gateway_status, import_accounts_json, list_accounts, login_with_browser, remove_account,
    set_active_account, set_gateway_api_key, set_gateway_port, set_outbound_proxy, start_gateway,
    stop_gateway, AntigravityAccountPublic, AntigravityGatewayStatus, DEFAULT_CLASH_PROXY_URL,
    DEFAULT_GATEWAY_PORT,
};
use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::{
    ClaudeModelMapping, ProtocolType, Provider, ProviderInput, ProviderKind, ProviderTarget,
};
use crate::store::AppState;
use tauri::AppHandle;

#[tauri::command]
pub fn list_antigravity_accounts() -> AppResult<Vec<AntigravityAccountPublic>> {
    list_accounts()
}

#[tauri::command]
pub fn import_antigravity_accounts(json: String) -> AppResult<usize> {
    import_accounts_json(&json)
}

#[tauri::command]
pub async fn start_antigravity_oauth_login(
    app: AppHandle,
) -> AppResult<AntigravityAccountPublic> {
    login_with_browser(&app).await
}

#[tauri::command]
pub fn remove_antigravity_account(account_id: String) -> AppResult<()> {
    remove_account(&account_id)
}

#[tauri::command]
pub fn set_antigravity_active_account(account_id: String) -> AppResult<()> {
    set_active_account(&account_id)
}

#[tauri::command]
pub fn get_antigravity_gateway_status() -> AppResult<AntigravityGatewayStatus> {
    gateway_status()
}

#[tauri::command]
pub fn set_antigravity_gateway_port(port: u16) -> AppResult<()> {
    set_gateway_port(port)
}

#[tauri::command]
pub fn set_antigravity_gateway_api_key(api_key: String) -> AppResult<()> {
    set_gateway_api_key(api_key)
}

#[tauri::command]
pub fn set_antigravity_outbound_proxy(
    mode: String,
    proxy_url: Option<String>,
) -> AppResult<AntigravityGatewayStatus> {
    set_outbound_proxy(
        &mode,
        proxy_url
            .as_deref()
            .unwrap_or(DEFAULT_CLASH_PROXY_URL),
    )
}

#[tauri::command]
pub async fn start_antigravity_gateway(port: Option<u16>) -> AppResult<AntigravityGatewayStatus> {
    start_gateway(port).await
}

#[tauri::command]
pub async fn stop_antigravity_gateway() -> AppResult<AntigravityGatewayStatus> {
    stop_gateway().await
}

#[tauri::command]
pub async fn refresh_antigravity_account_quota(
    account_id: String,
) -> AppResult<AntigravityAccountPublic> {
    refresh_one_account_quota(&account_id).await
}

#[tauri::command]
pub async fn refresh_antigravity_quotas() -> AppResult<Vec<AntigravityAccountPublic>> {
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

async fn refresh_one_account_quota(account_id: &str) -> AppResult<AntigravityAccountPublic> {
    let (access_token, account) = account_store().ensure_access_token(account_id)?;
    let (quota, project_id) =
        fetch_quota(&access_token, account.token.project_id.as_deref()).await?;
    account_store().update_quota(account_id, quota, project_id)
}

#[tauri::command]
pub async fn ensure_antigravity_provider(
    target: ProviderTarget,
    model: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let accounts = list_accounts()?;
    if accounts.is_empty() {
        return Err(AppError::Config(
            "还没有 Antigravity 账号。请先在 Antigravity 网关页点击「用浏览器登录 Google 账号」完成授权（仅在 Antigravity 官网/IDE 登录不够）。"
                .into(),
        ));
    }

    let status = match start_gateway(None).await {
        Ok(status) => status,
        Err(error) => {
            // If already running, reuse current status.
            let current = gateway_status()?;
            if current.running {
                current
            } else {
                return Err(AppError::Config(format!(
                    "启动 Antigravity 网关失败: {error}"
                )));
            }
        }
    };

    let existing = state.db.with_conn(|conn| {
        Ok(dao::list_providers(conn, target)?
            .into_iter()
            .find(|provider| provider.provider_kind == ProviderKind::Antigravity))
    })?;

    // Warm catalog from persisted quotas before binding provider models.
    let _ = list_accounts();
    let default_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(crate::antigravity::model_catalog::preferred_default_model);
    let gemini_flash = crate::antigravity::model_catalog::preferred_gemini_flash()
        .unwrap_or_else(|| "gemini-3-flash".into());
    let gemini_pro = crate::antigravity::model_catalog::preferred_gemini_pro()
        .unwrap_or_else(|| "gemini-3.1-pro-high".into());
    let claude_opus = crate::antigravity::model_catalog::preferred_claude_opus()
        .unwrap_or_else(|| "claude-opus-4-6-thinking".into());
    let suggestions = crate::antigravity::model_catalog::provider_suggestion_ids(16);
    let failover_models: Vec<String> = suggestions
        .into_iter()
        .filter(|id| id != &default_model)
        .collect();

    let (protocol_type, base_url, model_mapping) = match target {
        ProviderTarget::Codex => (
            ProtocolType::OpenAiChat,
            format!("{}/v1", status.base_url),
            ClaudeModelMapping::default(),
        ),
        ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop => (
            ProtocolType::Anthropic,
            status.base_url.clone(),
            ClaudeModelMapping {
                sonnet: default_model.clone(),
                opus: claude_opus.clone(),
                haiku: gemini_flash.clone(),
                fable: default_model.clone(),
                subagent: if target == ProviderTarget::ClaudeCode {
                    gemini_flash.clone()
                } else {
                    String::new()
                },
            },
        ),
    };

    let input = ProviderInput {
        id: existing.map(|provider| provider.id),
        name: "Antigravity (Built-in)".to_string(),
        base_url,
        api_key: status.api_key,
        clear_api_key: false,
        model: default_model,
        model_context_window: if target == ProviderTarget::Codex {
            Some(200_000)
        } else {
            None
        },
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping,
        protocol_type,
        provider_kind: ProviderKind::Antigravity,
        auth_binding: String::new(),
        target_app: target,
        notes: format!(
            "Built-in Antigravity gateway (live catalog; Haiku→{gemini_flash}, Pro hint→{gemini_pro})"
        ),
        failover_group: 0,
        failover_models,
    };

    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}

#[tauri::command]
pub fn list_antigravity_models() -> AppResult<Vec<crate::antigravity::CatalogModel>> {
    let _ = list_accounts();
    Ok(crate::antigravity::list_catalog_models())
}

#[tauri::command]
pub fn get_antigravity_defaults() -> AppResult<serde_json::Value> {
    let _ = list_accounts();
    let status = gateway_status()?;
    Ok(serde_json::json!({
        "defaultPort": DEFAULT_GATEWAY_PORT,
        "externalPort": 8045,
        "port": status.port,
        "baseUrl": status.base_url,
        "apiKey": status.api_key,
        "running": status.running,
        "models": crate::antigravity::list_catalog_models(),
        "defaultModel": crate::antigravity::model_catalog::preferred_default_model(),
        "geminiFlash": crate::antigravity::model_catalog::preferred_gemini_flash(),
        "geminiPro": crate::antigravity::model_catalog::preferred_gemini_pro(),
    }))
}

/// Called from provider switch path when an Antigravity provider becomes current.
pub async fn ensure_gateway_running_for_provider(provider: &Provider) -> AppResult<()> {
    if provider.provider_kind != ProviderKind::Antigravity {
        return Ok(());
    }
    let status = gateway_status()?;
    if !status.running {
        start_gateway(None).await?;
    } else {
        // Binding Desktop/Code after a failed probe cascade should not stay bricked.
        let _ = crate::antigravity::account::store().clear_all_cooldowns();
    }
    Ok(())
}

pub fn antigravity_provider_uses_gateway(provider: &Provider) -> bool {
    provider.provider_kind == ProviderKind::Antigravity
        || provider.base_url.contains("127.0.0.1:15830")
        || provider.base_url.contains("localhost:15830")
}

#[allow(dead_code)]
pub fn validate_has_accounts() -> AppResult<()> {
    let accounts = list_accounts()?;
    if accounts.is_empty() {
        return Err(AppError::Config(
            "请先在 Antigravity 网关页导入至少一个账号".into(),
        ));
    }
    Ok(())
}
