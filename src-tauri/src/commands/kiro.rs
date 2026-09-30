//! Tauri commands for the Kiro reverse-proxy gateway.

use tauri::AppHandle;

use crate::database::dao::providers as dao;
use crate::error::{AppError, AppResult};
use crate::kiro::{
    gateway_status, import_accounts_json, list_accounts, login_builder_id, login_social,
    preferred_default_model, remove_account, set_gateway_api_key, set_gateway_port,
    set_outbound_proxy, start_gateway, stop_gateway, KiroAccountPublic, KiroGatewayStatus,
};
use crate::kiro::models::{preferred_haiku, preferred_opus};
use crate::provider::{
    ClaudeModelMapping, ProtocolType, Provider, ProviderInput, ProviderKind, ProviderTarget,
};
use crate::store::AppState;

#[tauri::command]
pub fn list_kiro_accounts() -> AppResult<Vec<KiroAccountPublic>> {
    list_accounts()
}

#[tauri::command]
pub fn import_kiro_accounts(raw: String) -> AppResult<usize> {
    import_accounts_json(&raw)
}

#[tauri::command]
pub fn remove_kiro_account(id: String) -> AppResult<()> {
    remove_account(&id)
}

#[tauri::command]
pub async fn start_kiro_builder_id_login(app: AppHandle) -> AppResult<KiroAccountPublic> {
    login_builder_id(&app).await
}

#[tauri::command]
pub async fn start_kiro_social_login(app: AppHandle) -> AppResult<KiroAccountPublic> {
    login_social(&app).await
}

#[tauri::command]
pub fn get_kiro_gateway_status() -> AppResult<KiroGatewayStatus> {
    gateway_status()
}

#[tauri::command]
pub fn set_kiro_gateway_port(port: u16) -> AppResult<()> {
    set_gateway_port(port)
}

#[tauri::command]
pub fn set_kiro_gateway_api_key(api_key: String) -> AppResult<()> {
    set_gateway_api_key(api_key)
}

#[tauri::command]
pub fn set_kiro_outbound_proxy(mode: String, proxy_url: String) -> AppResult<KiroGatewayStatus> {
    set_outbound_proxy(&mode, &proxy_url)
}

#[tauri::command]
pub async fn start_kiro_gateway(port: Option<u16>) -> AppResult<KiroGatewayStatus> {
    start_gateway(port).await
}

#[tauri::command]
pub async fn stop_kiro_gateway() -> AppResult<KiroGatewayStatus> {
    stop_gateway().await
}

#[tauri::command]
pub async fn refresh_kiro_account_quota(id: String) -> AppResult<KiroAccountPublic> {
    crate::kiro::quota::refresh_one(&id).await
}

#[tauri::command]
pub async fn refresh_kiro_quotas() -> AppResult<Vec<KiroAccountPublic>> {
    crate::kiro::quota::refresh_all().await
}

#[tauri::command]
pub async fn test_kiro_account(
    id: String,
    model: Option<String>,
    prompt: Option<String>,
) -> AppResult<crate::kiro::account_test::KiroAccountTestResult> {
    crate::kiro::account_test::test_account(&id, model, prompt).await
}

#[tauri::command]
pub fn set_kiro_exit_proxy(
    entries: Vec<crate::kiro::outbound::ExitProxyEntry>,
) -> AppResult<KiroGatewayStatus> {
    crate::kiro::gateway::set_exit_proxies(entries)
}

#[tauri::command]
pub async fn probe_kiro_exit_proxy(
    id: String,
    proxy_url: String,
) -> AppResult<crate::kiro::outbound::ExitProxyProbeResult> {
    crate::kiro::outbound::probe_exit_proxy(&id, &proxy_url).await
}

#[tauri::command]
pub async fn probe_kiro_exit_latency(
    id: String,
    proxy_url: String,
) -> AppResult<crate::kiro::outbound::ExitProxyLatencyResult> {
    crate::kiro::outbound::probe_exit_latency(&id, &proxy_url).await
}

pub async fn ensure_gateway_running_for_provider(provider: &Provider) -> AppResult<()> {
    if provider.provider_kind != ProviderKind::Kiro {
        return Ok(());
    }
    let status = gateway_status()?;
    if !status.running {
        start_gateway(None).await?;
    } else {
        let _ = crate::kiro::account::store().clear_cooldowns();
    }
    Ok(())
}

#[tauri::command]
pub async fn ensure_kiro_provider(
    target: ProviderTarget,
    model: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if list_accounts()?.is_empty() {
        return Err(AppError::Config(
            "还没有 Kiro 账号。请先导入凭据，或使用 Builder ID / Social 登录。".into(),
        ));
    }
    let status = match start_gateway(None).await {
        Ok(status) => status,
        Err(error) => {
            let current = gateway_status()?;
            if current.running {
                current
            } else {
                return Err(AppError::Config(format!("启动 Kiro 网关失败: {error}")));
            }
        }
    };
    let existing = state.db.with_conn(|conn| {
        Ok(dao::list_providers(conn, target)?
            .into_iter()
            .find(|provider| provider.provider_kind == ProviderKind::Kiro))
    })?;
    let default_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(preferred_default_model);
    let opus = preferred_opus();
    let haiku = preferred_haiku();
    let (protocol_type, base_url, model_mapping) = match target {
        ProviderTarget::Codex | ProviderTarget::Cline => (
            ProtocolType::OpenAiResponses,
            format!("{}/v1", status.base_url.trim_end_matches('/')),
            ClaudeModelMapping::default(),
        ),
        ProviderTarget::OpenCode => (
            ProtocolType::Anthropic,
            format!("{}/v1", status.base_url.trim_end_matches('/')),
            ClaudeModelMapping::default(),
        ),
        ProviderTarget::Pi | ProviderTarget::Dsh => (
            ProtocolType::Anthropic,
            status.base_url.trim_end_matches('/').to_string(),
            ClaudeModelMapping::default(),
        ),
        ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop => (
            ProtocolType::Anthropic,
            status.base_url.clone(),
            ClaudeModelMapping {
                sonnet: default_model.clone(),
                opus: opus.clone(),
                haiku: haiku.clone(),
                fable: default_model.clone(),
                subagent: if target == ProviderTarget::ClaudeCode {
                    haiku.clone()
                } else {
                    String::new()
                },
            },
        ),
    };
    let input = ProviderInput {
        id: existing.as_ref().map(|provider| provider.id.clone()),
        name: "Kiro (Built-in)".to_string(),
        base_url,
        api_key: status.api_key,
        clear_api_key: false,
        model: default_model,
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping,
        protocol_type,
        provider_kind: ProviderKind::Kiro,
        auth_binding: String::new(),
        target_app: target,
        notes: "内建 Kiro 反代 :15831".to_string(),
        failover_group: 0,
        failover_models: vec![opus, haiku],
        hidden_models: existing
            .as_ref()
            .map(|provider| provider.hidden_models.clone())
            .unwrap_or_default(),
        thinking_config: None,
        custom_headers: None,
    };
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}
