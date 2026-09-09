//! Agent connection + gateway profile commands.

use serde::Serialize;

use crate::commands::providers::sync_live_after_connection_change;
use crate::database::dao::gateway::{
    current_connection_view, current_profile, delete_upstream, ensure_profile_for_target,
    import_providers_as_upstreams, list_profiles, list_upstream_models, list_upstream_providers,
    patch_profile, replace_upstream_models, set_current_connection_type, set_upstream_model_visible,
    upsert_upstream, AgentConnectionView, ConnectionType, GatewayProfile, GatewayProfilePatch,
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
    state
        .db
        .with_conn(|conn| current_connection_view(conn, target))
}

#[tauri::command]
pub async fn set_agent_connection(
    target: ProviderTarget,
    connection_type: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<AgentConnectionView> {
    let kind = ConnectionType::from_str_lossy(&connection_type);
    state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, target)?;
        set_current_connection_type(conn, target.as_str(), kind)
    })?;
    sync_live_after_connection_change(target, kind == ConnectionType::Gateway, &app, &state).await?;
    crate::commands::proxy::publish_target_status(&app, &state, target).await;
    state
        .db
        .with_conn(|conn| current_connection_view(conn, target))
}

#[tauri::command]
pub fn get_gateway_profile(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Option<GatewayProfile>> {
    state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, target)?;
        current_profile(conn, target)
    })
}

#[tauri::command]
pub fn list_gateway_profiles(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayProfile>> {
    state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, target)?;
        list_profiles(conn, target)
    })
}

#[tauri::command]
pub async fn update_gateway_profile(
    target: ProviderTarget,
    patch: GatewayProfilePatch,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<GatewayProfile> {
    let profile = state.db.with_conn(|conn| {
        let profile = ensure_profile_for_target(conn, target)?;
        patch_profile(conn, &profile.id, &patch)
    })?;
    let _ = crate::config::claude_code::apply_opusplan_model(false);
    for agent in crate::provider::ProviderTarget::ALL {
        if crate::catalog::enabled(state.db.as_ref(), agent) {
            sync_live_after_connection_change(agent, true, &app, &state).await?;
        }
    }
    Ok(profile)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRouteLog {
    pub id: String,
    pub created_at: i64,
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub route_reason: Option<String>,
    pub profile_id: Option<String>,
    pub upstream_id: Option<String>,
    pub provider_name: Option<String>,
    pub attempt_index: i64,
    pub status_code: Option<i64>,
}

#[tauri::command]
pub fn list_gateway_route_logs(
    target: Option<ProviderTarget>,
    limit: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayRouteLog>> {
    let cap = limit.unwrap_or(30).clamp(1, 200);
    state.db.with_conn(|conn| {
        let sql = if target.is_some() {
            "SELECT id, created_at, requested_model, model, route_reason, profile_id, upstream_id,
                    provider_name, attempt_index, status_code
             FROM proxy_request_logs
             WHERE target_app = ? AND COALESCE(data_source, 'proxy') = 'proxy'
             ORDER BY created_at DESC LIMIT ?;"
        } else {
            "SELECT id, created_at, requested_model, model, route_reason, profile_id, upstream_id,
                    provider_name, attempt_index, status_code
             FROM proxy_request_logs
             WHERE COALESCE(data_source, 'proxy') = 'proxy'
               AND (hop IS NULL OR hop IN ('smart_gateway', 'agent_proxy', 'antigravity'))
               AND route_reason IS NOT NULL AND trim(route_reason) != ''
             ORDER BY created_at DESC LIMIT ?;"
        };
        let mut stmt = conn.prepare(sql)?;
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(GatewayRouteLog {
                id: row.get(0)?,
                created_at: row.get(1)?,
                requested_model: row.get(2)?,
                model: row.get(3)?,
                route_reason: row.get(4)?,
                profile_id: row.get(5)?,
                upstream_id: row.get(6)?,
                provider_name: row.get(7)?,
                attempt_index: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                status_code: row.get(9)?,
            })
        };
        if let Some(target) = target {
            let rows = stmt.query_map(rusqlite::params![target.as_str(), cap], map_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let rows = stmt.query_map(rusqlite::params![cap], map_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    })
}

#[tauri::command]
pub fn list_gateway_upstreams(state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state
        .db
        .with_conn(|conn| list_upstream_providers(conn, true))
}

#[tauri::command]
pub fn upsert_gateway_upstream(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.provider_kind == ProviderKind::SmartGateway {
        return Err(AppError::Config("托管 Auto 卡不能加入上游池".to_string()));
    }
    state.db.with_conn(|conn| upsert_upstream(conn, &input))
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
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    let rows = state
        .db
        .with_conn(|conn| set_upstream_model_visible(conn, &id, &model_id, visible))?;
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
pub fn delete_gateway_upstream(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| delete_upstream(conn, &id))
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
    state.db.with_conn(|conn| upsert_upstream(conn, &input))
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
    state.db.with_conn(crate::database::dao::gateway::list_bindings)
}

#[tauri::command]
pub async fn bind_smart_gateway(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let enabled = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::list_upstream_providers(conn, false))?;
    if enabled.is_empty() {
        return Err(AppError::Config("请先在上游池中添加至少一个供应商".into()));
    }
    crate::gateway::service::mark_enabled(state.db.as_ref())?;
    if !crate::gateway::service::current_status().running {
        crate::gateway::service::start_via_state(&state, None).await?;
    }
    let provider_id = crate::gateway::smart_gateway_provider_id(target);
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::upsert_binding(conn, target, &provider_id)
    })?;
    let provider = crate::commands::providers::ensure_smart_gateway_provider_row(&state, target)?;
    crate::commands::providers::switch_provider_for_target(&provider.id, target, Some(&app), &state).await?;
    crate::commands::providers::sync_live_after_connection_change(target, true, &app, &state).await?;
    crate::gateway::service::emit_status(&app);
    Ok(provider)
}

#[tauri::command]
pub async fn unbind_smart_gateway(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| crate::database::dao::gateway::delete_binding(conn, target))?;
    crate::commands::providers::sync_live_after_connection_change(target, false, &app, &state).await?;
    crate::gateway::service::emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn list_route_modes(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::database::dao::gateway::RouteMode>> {
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        crate::database::dao::gateway::list_route_modes(
            conn,
            crate::database::dao::gateway::SHARED_PROFILE_ID,
        )
    })
}

#[tauri::command]
pub async fn update_route_mode(
    id: String,
    patch: crate::database::dao::gateway::RouteModePatch,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::database::dao::gateway::RouteMode> {
    let mode = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::patch_route_mode(conn, &id, &patch))?;
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(mode)
}

#[tauri::command]
pub fn list_route_rules(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::database::dao::gateway::RouteRule>> {
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::list_route_rules(
            conn,
            crate::database::dao::gateway::SHARED_PROFILE_ID,
        )
    })
}

#[tauri::command]
pub async fn upsert_route_rule(
    rule: crate::database::dao::gateway::RouteRule,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::database::dao::gateway::RouteRule> {
    let saved = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::upsert_route_rule(conn, &rule))?;
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(saved)
}

#[tauri::command]
pub async fn delete_route_rule(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| crate::database::dao::gateway::delete_route_rule(conn, &id))?;
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteModeUsageStat {
    pub mode_id: String,
    pub request_count: i64,
    pub estimated_cost: f64,
}

#[tauri::command]
pub fn list_route_mode_usage_stats(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<RouteModeUsageStat>> {
    use crate::database::dao::proxy_logs::{EFFECTIVE_USAGE_FILTER, ROW_COST_SQL};
    let since = chrono::Utc::now().timestamp_millis() - 7 * 24 * 60 * 60 * 1000;
    state.db.with_conn(|conn| {
        let sql = format!(
            "SELECT COALESCE(l.route_reason, ''), COUNT(*), COALESCE(SUM({ROW_COST_SQL}), 0)
             FROM proxy_request_logs l
             LEFT JOIN model_pricing p ON lower(p.model) = lower(COALESCE(l.model, ''))
             WHERE l.created_at >= ? AND COALESCE(l.data_source, 'proxy') = 'proxy'
             {EFFECTIVE_USAGE_FILTER}
             GROUP BY l.route_reason;"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        let mut merged: std::collections::BTreeMap<String, (i64, f64)> =
            std::collections::BTreeMap::new();
        for row in rows {
            let (reason, count, cost) = row?;
            let mode_id = if reason.contains("规划") || reason == "plan" {
                "plan"
            } else if reason.contains("改内容") || reason == "edit" {
                "edit"
            } else if reason.contains("后台") || reason == "background" || reason == "role_subagent" {
                "background"
            } else if reason.contains("思考") || reason == "think" {
                "think"
            } else if reason.contains("长上下文") || reason == "long_context" {
                "long_context"
            } else if reason.contains("联网") || reason == "web_search" {
                "web_search"
            } else if reason.contains("视觉") || reason == "vision" {
                "vision"
            } else if reason.contains("图像") || reason == "image_gen" {
                "image_gen"
            } else if reason.contains("默认") || reason == "auto" || reason == "profile_default" {
                "default"
            } else {
                continue;
            };
            let entry = merged.entry(mode_id.to_string()).or_insert((0, 0.0));
            entry.0 += count;
            entry.1 += cost;
        }
        Ok(merged
            .into_iter()
            .map(|(mode_id, (request_count, estimated_cost))| RouteModeUsageStat {
                mode_id,
                request_count,
                estimated_cost,
            })
            .collect())
    })
}
