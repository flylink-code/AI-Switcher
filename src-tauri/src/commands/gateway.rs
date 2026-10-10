//! Agent connection + gateway profile commands.

use std::sync::Arc;

use serde::Serialize;

use crate::database::dao::gateway::{
    delete_upstream, ensure_profile_for_target, import_providers_as_upstreams,
    list_upstream_models, list_upstream_providers, get_upstream_provider, replace_upstream_models,
    set_upstream_model_visible, upsert_upstream, AgentConnectionView, ConnectionType,
    GatewayUpstreamImportResult, GatewayUpstreamModelRow,
};
use crate::error::{AppError, AppResult};
use crate::provider::{
    ModelDiscoveryResult, Provider, ProviderInput, ProviderKind, ProviderTarget, ProtocolType,
};
use crate::store::AppState;

#[tauri::command]
pub fn get_agent_connection(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<AgentConnectionView> {
    state.db.with_read_conn(|conn| {
        let binding = crate::database::dao::gateway::binding_for_target(conn, target)?;
        match binding {
            Some(b) if b.mode == "direct" => {
                let upstream_id = if b.direct_upstream_id.trim().is_empty() {
                    None
                } else {
                    Some(b.direct_upstream_id)
                };
                Ok(AgentConnectionView {
                    target,
                    connection_type: ConnectionType::External,
                    upstream_id,
                    profile: None,
                })
            }
            Some(_) if crate::database::dao::gateway::is_gateway_connection(conn, target) => {
                let profile = crate::database::dao::gateway::current_profile(conn, target)?;
                Ok(AgentConnectionView {
                    target,
                    connection_type: ConnectionType::Gateway,
                    upstream_id: None,
                    profile,
                })
            }
            _ => {
                Ok(AgentConnectionView {
                    target,
                    connection_type: ConnectionType::External,
                    upstream_id: None,
                    profile: None,
                })
            }
        }
    })
}

#[tauri::command]
pub async fn set_agent_connection(
    target: ProviderTarget,
    connection_type: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<AgentConnectionView> {
    let kind = ConnectionType::from_str_lossy(&connection_type);
    match kind {
        ConnectionType::Gateway => {
            crate::commands::providers::set_agent_gateway_for_target(target, Some(&app), &state).await?;
        }
        ConnectionType::External => {
            crate::commands::providers::switch_to_official_for_target(target, Some(&app), &state).await?;
        }
    }
    crate::commands::proxy::publish_target_status(&app, &state, target).await;
    get_agent_connection(target, state)
}

#[tauri::command]
pub fn list_gateway_upstreams(state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state
        .db
        .with_read_conn(|conn| list_upstream_providers(conn, true))
}

#[tauri::command]
pub async fn upsert_gateway_upstream(
    input: ProviderInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.provider_kind == ProviderKind::SmartGateway {
        return Err(AppError::Config("托管 Auto 卡不能加入上游池".to_string()));
    }
    let _guard = crate::commands::providers::agent_connection_lock().lock().await;

    if let Some(upstream_id) = input.id.as_deref() {
        state.db.with_read_conn(|conn| {
            crate::commands::providers::validate_upstream_update_for_direct_bindings(
                conn,
                upstream_id,
                input.provider_kind,
                input.protocol_type,
            )
        })?;
    }

    let mut provider = state.db.with_conn(|conn| upsert_upstream(conn, &input))?;
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    if let Err(error) = crate::commands::providers::refresh_direct_upstream_locked(&provider.id, Some(&app), &state).await {
        return Err(AppError::Config(format!(
            "上游数据已保存，但刷新直连 Agent 配置失败: {error}"
        )));
    }
    if !provider.api_key.starts_with("kr://") {
        provider.api_key = String::new();
    }
    Ok(provider)
}

#[tauri::command]
pub fn import_gateway_upstreams_from_providers(
    source_target: ProviderTarget,
    provider_ids: Vec<String>,
    add_to_allowlist_target: Option<ProviderTarget>,
    state: tauri::State<'_, AppState>,
) -> AppResult<GatewayUpstreamImportResult> {
    if provider_ids.is_empty() {
        return Err(AppError::Config("请选择要导入的供应商".to_string()));
    }
    state.db.with_conn(|conn| {
        if let Some(target) = add_to_allowlist_target {
            ensure_profile_for_target(conn, target)?;
        }
        import_providers_as_upstreams(conn, source_target, &provider_ids, add_to_allowlist_target)
    })
}

#[tauri::command]
pub fn list_gateway_upstream_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::get_upstream_provider(conn, &id)?
            .ok_or_else(|| AppError::Config(format!("上游不存在: {id}")))?;
        list_upstream_models(conn, &id)
    })
}

#[tauri::command]
pub async fn set_gateway_upstream_model_visible(
    id: String,
    model_id: String,
    visible: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    let _guard = crate::commands::providers::agent_connection_lock().lock().await;
    let rows = state
        .db
        .with_conn(|conn| set_upstream_model_visible(conn, &id, &model_id, visible))?;
    crate::commands::providers::refresh_direct_upstream_locked(&id, Some(&app), &state).await?;
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(rows)
}

#[tauri::command]
pub async fn discover_gateway_upstream_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    discover_one_upstream(&id, &state).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUpstreamDiscoverItem {
    pub id: String,
    pub name: String,
    pub result: ModelDiscoveryResult,
}

#[tauri::command]
pub async fn discover_gateway_upstream_models_batch(
    ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayUpstreamDiscoverItem>> {
    if ids.is_empty() {
        return Err(AppError::Config("请选择要刷新的上游".to_string()));
    }
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        let name = state
            .db
            .with_conn(|conn| {
                Ok(crate::database::dao::gateway::get_upstream_provider(conn, &id)?
                    .map(|provider| provider.name)
                    .unwrap_or_else(|| id.clone()))
            })
            .unwrap_or_else(|_| id.clone());
        let result = discover_one_upstream(&id, &state).await?;
        results.push(GatewayUpstreamDiscoverItem { id, name, result });
    }
    Ok(results)
}

async fn discover_one_upstream(
    id: &str,
    state: &AppState,
) -> AppResult<ModelDiscoveryResult> {
    let provider = state.db.with_conn(|conn| {
        crate::database::dao::gateway::get_upstream_provider(conn, id)?
            .ok_or_else(|| AppError::Config(format!("上游不存在: {id}")))
    })?;
    if provider.is_smart_gateway() {
        return Err(AppError::Config("托管 Auto 卡不能作为上游刷新模型".to_string()));
    }
    let key = state
        .db
        .with_conn(|conn| crate::database::dao::providers::resolve_api_key(conn, &provider.id))?
        .unwrap_or_default();
    let result =
        crate::commands::providers::discover_provider_models_with_key(&provider, key, state, false)
            .await?;
    if result.error.is_none() {
        state.db.with_conn(|conn| {
            replace_upstream_models(conn, &provider.id, &result.models)?;
            Ok(())
        })?;
        let _ = crate::commands::providers::push_bound_gateway_catalogs(state).await;
    }
    Ok(result)
}

#[tauri::command]
pub async fn delete_gateway_upstream(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let _guard = crate::commands::providers::agent_connection_lock().lock().await;
    state.db.with_conn(|conn| delete_upstream(conn, &id))?;
    state.db.gateway_upstream_limiter.remove(&id);
    Ok(())
}

#[tauri::command]
pub fn add_antigravity_gateway_upstream(state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    let (base_url, api_key) = match crate::antigravity::gateway_status() {
        Ok(status) => {
            let port = if status.port == 0 {
                crate::antigravity::gateway::DEFAULT_GATEWAY_PORT
            } else {
                status.port
            };
            let base_url = if status.base_url.trim().is_empty() {
                format!("http://127.0.0.1:{port}")
            } else {
                status.base_url.trim_end_matches('/').to_string()
            };
            let api_key = if status.api_key.trim().is_empty() {
                crate::antigravity::gateway::builtin_api_key()
            } else {
                status.api_key
            };
            (base_url, api_key)
        }
        Err(_) => (
            format!(
                "http://127.0.0.1:{}",
                crate::antigravity::gateway::DEFAULT_GATEWAY_PORT
            ),
            crate::antigravity::gateway::builtin_api_key(),
        ),
    };
    let input = ProviderInput {
        id: Some("up_ag_15830".to_string()),
        name: "Antigravity".to_string(),
        base_url,
        api_key,
        clear_api_key: false,
        model: crate::antigravity::model_catalog::preferred_default_model(),
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: crate::provider::ClaudeModelMapping::default(),
        protocol_type: ProtocolType::Anthropic,
        provider_kind: ProviderKind::Antigravity,
        auth_binding: String::new(),
        target_app: ProviderTarget::ClaudeCode,
        notes: "内建 Antigravity 反代 :15830".to_string(),
        failover_group: 0,
        failover_models: crate::antigravity::model_catalog::provider_suggestion_ids(16),
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    };
    let mut provider = state.db.with_conn(|conn| upsert_upstream(conn, &input))?;
    if !provider.api_key.starts_with("kr://") {
        provider.api_key = String::new();
    }
    Ok(provider)
}

#[tauri::command]
pub fn add_kiro_gateway_upstream(state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    let (base_url, api_key) = match crate::kiro::gateway_status() {
        Ok(status) => {
            let port = if status.port == 0 {
                crate::kiro::gateway::DEFAULT_GATEWAY_PORT
            } else {
                status.port
            };
            let base_url = if status.base_url.trim().is_empty() {
                format!("http://127.0.0.1:{port}")
            } else {
                status.base_url.trim_end_matches('/').to_string()
            };
            let api_key = if status.api_key.trim().is_empty() {
                crate::kiro::gateway::builtin_api_key()
            } else {
                status.api_key
            };
            (base_url, api_key)
        }
        Err(_) => (
            format!(
                "http://127.0.0.1:{}",
                crate::kiro::gateway::DEFAULT_GATEWAY_PORT
            ),
            crate::kiro::gateway::builtin_api_key(),
        ),
    };
    let input = ProviderInput {
        id: Some("up_kiro_15831".to_string()),
        name: "Kiro".to_string(),
        base_url,
        api_key,
        clear_api_key: false,
        model: crate::kiro::models::preferred_default_model(),
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: crate::provider::ClaudeModelMapping::default(),
        protocol_type: ProtocolType::Anthropic,
        provider_kind: ProviderKind::Kiro,
        auth_binding: String::new(),
        target_app: ProviderTarget::ClaudeCode,
        notes: "内建 Kiro 反代 :15831".to_string(),
        failover_group: 0,
        failover_models: vec![
            crate::kiro::models::preferred_opus(),
            crate::kiro::models::preferred_haiku(),
        ],
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    };
    let mut provider = state.db.with_conn(|conn| upsert_upstream(conn, &input))?;
    if !provider.api_key.starts_with("kr://") {
        provider.api_key = String::new();
    }
    Ok(provider)
}

#[tauri::command]
pub fn get_smart_gateway_status() -> AppResult<crate::gateway::service::SmartGatewayStatus> {
    Ok(crate::gateway::service::current_status())
}

#[tauri::command]
pub fn set_smart_gateway_port(port: u16, state: tauri::State<'_, AppState>) -> AppResult<()> {
    crate::gateway::service::persist_port(state.db.as_ref(), port)
}

#[tauri::command]
pub fn set_smart_gateway_api_key(
    api_key: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::service::SmartGatewayStatus> {
    crate::gateway::service::persist_api_key(state.db.as_ref(), &api_key)?;
    Ok(crate::gateway::service::current_status())
}

#[tauri::command]
pub fn rotate_smart_gateway_api_key(
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::service::SmartGatewayStatus> {
    crate::gateway::service::rotate_api_key(state.db.as_ref())?;
    Ok(crate::gateway::service::current_status())
}

#[tauri::command]
pub async fn start_smart_gateway(
    port: Option<u16>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::service::SmartGatewayStatus> {
    let result = crate::gateway::service::start_via_state(&state, port).await;
    crate::gateway::service::emit_status(&app);
    result
}

#[tauri::command]
pub async fn stop_smart_gateway(app: tauri::AppHandle) -> AppResult<crate::gateway::service::SmartGatewayStatus> {
    let status = crate::gateway::service::stop_service().await?;
    crate::gateway::service::emit_status(&app);
    Ok(status)
}

#[tauri::command]
pub fn list_smart_gateway_bindings(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::database::dao::gateway::GatewayBinding>> {
    state.db.with_read_conn(crate::database::dao::gateway::list_bindings)
}

#[tauri::command]
pub async fn bind_smart_gateway(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let mut provider = crate::commands::providers::set_agent_gateway_for_target(target, Some(&app), &state).await?;
    if !provider.api_key.starts_with("kr://") {
        provider.api_key = String::new();
    }
    crate::gateway::service::emit_status(&app);
    Ok(provider)
}

#[tauri::command]
pub async fn unbind_smart_gateway(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let binding = state
        .db
        .with_read_conn(|conn| crate::database::dao::gateway::binding_for_target(conn, target))?;

    if let Some(binding) = binding {
        if binding.mode == "direct" {
            return Err(AppError::Config(
                "当前Agent使用直连，请在Agent连接中切换".into(),
            ));
        }
        let is_gw = state
            .db
            .with_read_conn(|conn| Ok(crate::database::dao::gateway::is_gateway_connection(conn, target)))
            .unwrap_or(false);
        if is_gw {
            crate::commands::providers::switch_to_official_for_target(target, Some(&app), &state).await?;
        } else {
            state
                .db
                .with_conn(|conn| crate::database::dao::gateway::delete_binding(conn, target))?;
            crate::catalog::invalidate_view_cache();
        }
    }
    crate::gateway::service::emit_status(&app);
    Ok(())
}


pub use crate::database::dao::proxy_logs::UpstreamDailyUsageStat;

#[tauri::command]
pub async fn list_upstream_daily_usage_stats(
    since: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<UpstreamDailyUsageStat>> {
    let since = since.unwrap_or_else(crate::database::dao::proxy_logs::local_midnight_millis);
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        db.with_read_conn(|conn| {
            crate::database::dao::proxy_logs::list_upstream_daily_usage_stats(conn, since)
        })
    })
    .await
    .map_err(|e| AppError::Database(format!("list upstream daily usage stats task failed: {e}")))?
}


#[tauri::command]
pub fn list_gateway_upstream_health(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::gateway::health::UpstreamHealth>> {
    let ids = state
        .db
        .with_read_conn(|conn| list_upstream_providers(conn, true))?
        .into_iter()
        .map(|provider| provider.id)
        .collect::<Vec<_>>();
    Ok(crate::gateway::health::list_for_upstreams(&ids))
}

#[tauri::command]
pub fn get_gateway_upstream_policy(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::upstream_limits::UpstreamLimitPolicy> {
    state.db.with_read_conn(|conn| {
        if get_upstream_provider(conn, &id)?.is_none() {
            return Err(AppError::Config("上游不存在".into()));
        }
        Ok(crate::gateway::upstream_limits::load_policy(conn, &id)?.unwrap_or_default())
    })
}

#[tauri::command]
pub async fn set_gateway_upstream_policy(
    id: String,
    policy: crate::gateway::upstream_limits::UpstreamLimitPolicy,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::upstream_limits::UpstreamLimitPolicy> {
    let _guard = crate::commands::providers::agent_connection_lock().lock().await;
    state.db.with_conn(|conn| crate::gateway::upstream_limits::persist_policy(conn, &id, &policy))?;
    state.db.gateway_upstream_limiter.apply(&id, policy.clone());
    Ok(policy)
}

#[tauri::command]
pub fn list_gateway_upstream_pressure(
    state: tauri::State<'_, AppState>,
) -> Vec<crate::gateway::upstream_limits::UpstreamLimitSnapshot> {
    state.db.gateway_upstream_limiter.snapshot_all()
}


#[tauri::command]
pub fn get_smart_gateway_inbound_limits(
    state: tauri::State<'_, AppState>,
) -> crate::gateway::inbound::InboundLimitSettings {
    crate::gateway::inbound::load_from_db(&state.db)
}

#[tauri::command]
pub fn set_smart_gateway_inbound_limits(
    settings: crate::gateway::inbound::InboundLimitSettings,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::inbound::InboundLimitSettings> {
    crate::gateway::inbound::persist(&state.db, &settings)
}


#[tauri::command]
pub fn get_smart_gateway_health_probe_secs(state: tauri::State<'_, AppState>) -> u64 {
    crate::gateway::health::probe_interval_secs(&state.db)
}

#[tauri::command]
pub fn set_smart_gateway_health_probe_secs(
    secs: u64,
    state: tauri::State<'_, AppState>,
) -> AppResult<u64> {
    crate::gateway::health::persist_probe_interval(&state.db, secs)
}
