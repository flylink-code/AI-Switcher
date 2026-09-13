//! Agent connection + gateway profile commands.

use std::sync::Arc;

use serde::Serialize;

use crate::commands::providers::sync_live_after_connection_change;
use crate::database::dao::gateway::{
    current_connection_view, current_profile, delete_upstream, ensure_profile_for_target,
    import_providers_as_upstreams, list_profiles, list_upstream_models, list_upstream_providers,
    patch_profile, replace_upstream_models, set_current_connection_type, set_upstream_model_visible,
    upsert_upstream, AgentConnectionView, ConnectionType, GatewayBinding, GatewayProfile,
    GatewayProfilePatch, GatewayUpstreamImportResult, GatewayUpstreamModelRow,
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
    target: Option<ProviderTarget>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayProfile>> {
    let _ = target;
    let listed = state.db.with_read_conn(list_profiles)?;
    if !listed.is_empty() {
        return Ok(listed);
    }
    state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        list_profiles(conn)
    })
}

#[tauri::command]
pub fn create_gateway_profile(
    name: String,
    clone_from: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<GatewayProfile> {
    let created = state.db.with_conn(|conn| {
        crate::database::dao::gateway::create_profile(conn, &name, clone_from.as_deref())
    })?;
    crate::catalog::invalidate_view_cache();
    Ok(created)
}

#[tauri::command]
pub fn rename_gateway_profile(
    id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<GatewayProfile> {
    state
        .db
        .with_conn(|conn| crate::database::dao::gateway::rename_profile(conn, &id, &name))
}

#[tauri::command]
pub async fn delete_gateway_profile(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| crate::database::dao::gateway::delete_profile(conn, &id))?;
    crate::catalog::invalidate_view_cache();
    for agent in ProviderTarget::ALL {
        let gateway_on = state
            .db
            .with_read_conn(|conn| {
                Ok(crate::database::dao::gateway::is_gateway_connection(conn, agent))
            })
            .unwrap_or(false);
        if gateway_on {
            crate::gateway::sticky::clear_for_target(agent);
            crate::commands::providers::sync_live_after_connection_change(
                agent, true, &app, &state,
            )
            .await?;
        }
    }
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    crate::gateway::service::emit_status(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_gateway_binding_profile(
    target: ProviderTarget,
    profile_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<GatewayBinding> {
    let provider_id = crate::gateway::smart_gateway_provider_id(target);
    let binding = state.db.with_conn(|conn| {
        crate::database::dao::gateway::set_binding_profile(conn, target, &profile_id, &provider_id)
    })?;
    let _ = crate::commands::providers::ensure_smart_gateway_provider_row(&state, target);
    crate::catalog::invalidate_view_cache();
    crate::gateway::sticky::clear_for_target(target);
    let gateway_on = state
        .db
        .with_read_conn(|conn| {
            Ok(crate::database::dao::gateway::is_gateway_connection(conn, target))
        })
        .unwrap_or(false);
    if gateway_on {
        crate::commands::providers::sync_live_after_connection_change(target, true, &app, &state)
            .await?;
    }
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    crate::gateway::service::emit_status(&app);
    Ok(binding)
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
    pub route_mode: Option<String>,
    pub profile_id: Option<String>,
    pub upstream_id: Option<String>,
    pub provider_name: Option<String>,
    pub attempt_index: i64,
    pub status_code: Option<i64>,
    pub duration_ms: i64,
    pub input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub error_category: Option<String>,
    pub stream_outcome: Option<String>,
    pub estimated_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedGatewayRouteLogs {
    pub data: Vec<GatewayRouteLog>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[tauri::command]
pub fn list_gateway_route_logs(
    target: Option<ProviderTarget>,
    limit: Option<i64>,
    offset: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> AppResult<PaginatedGatewayRouteLogs> {
    let cap = limit.unwrap_or(20).clamp(1, 200);
    let skip = offset.unwrap_or(0).max(0);
    let page = skip / cap;
    state.db.with_read_conn(|conn| {
        let cost = crate::database::dao::proxy_logs::ROW_COST_SQL;
        let where_sql = if target.is_some() {
            "l.target_app = ? AND COALESCE(l.data_source, 'proxy') = 'proxy'
               AND l.route_reason IS NOT NULL AND trim(l.route_reason) != ''"
        } else {
            "COALESCE(l.data_source, 'proxy') = 'proxy'
               AND (l.hop IS NULL OR l.hop IN ('smart_gateway', 'agent_proxy', 'antigravity'))
               AND l.route_reason IS NOT NULL AND trim(l.route_reason) != ''"
        };
        let count_sql = format!("SELECT COUNT(*) FROM proxy_request_logs l WHERE {where_sql}");
        let total: i64 = if let Some(target) = target {
            conn.query_row(&count_sql, rusqlite::params![target.as_str()], |row| row.get(0))?
        } else {
            conn.query_row(&count_sql, [], |row| row.get(0))?
        };
        let columns = format!(
            "l.id, l.created_at, l.requested_model, l.model, l.route_reason, l.route_mode,
                    l.profile_id, l.upstream_id, l.provider_name, l.attempt_index, l.status_code,
                    COALESCE(l.duration_ms, 0), COALESCE(l.input_tokens, 0),
                    COALESCE(l.cache_read_input_tokens, 0), COALESCE(l.cache_creation_input_tokens, 0),
                    COALESCE(l.output_tokens, 0), l.error_category, l.stream_outcome,
                    COALESCE({cost}, 0)"
        );
        let sql = format!(
            "SELECT {columns}
             FROM proxy_request_logs l
             LEFT JOIN model_pricing p ON lower(p.model) = lower(COALESCE(l.model, ''))
             WHERE {where_sql}
             ORDER BY l.created_at DESC LIMIT ? OFFSET ?;"
        );
        let mut stmt = conn.prepare(&sql)?;
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(GatewayRouteLog {
                id: row.get(0)?,
                created_at: row.get(1)?,
                requested_model: row.get(2)?,
                model: row.get(3)?,
                route_reason: row.get(4)?,
                route_mode: row.get(5)?,
                profile_id: row.get(6)?,
                upstream_id: row.get(7)?,
                provider_name: row.get(8)?,
                attempt_index: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                status_code: row.get(10)?,
                duration_ms: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
                input_tokens: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                cache_read_input_tokens: row.get::<_, Option<i64>>(13)?.unwrap_or(0),
                cache_creation_input_tokens: row.get::<_, Option<i64>>(14)?.unwrap_or(0),
                output_tokens: row.get::<_, Option<i64>>(15)?.unwrap_or(0),
                error_category: row.get(16)?,
                stream_outcome: row.get(17)?,
                estimated_cost: row.get::<_, Option<f64>>(18)?.unwrap_or(0.0),
            })
        };
        let data = if let Some(target) = target {
            let rows = stmt.query_map(rusqlite::params![target.as_str(), cap, skip], map_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let rows = stmt.query_map(rusqlite::params![cap, skip], map_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        Ok(PaginatedGatewayRouteLogs {
            data,
            total,
            page,
            page_size: cap,
        })
    })
}

#[tauri::command]
pub fn list_gateway_upstreams(state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state
        .db
        .with_read_conn(|conn| list_upstream_providers(conn, true))
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
    crate::catalog::invalidate_view_cache();
    let provider = crate::commands::providers::ensure_smart_gateway_provider_row(&state, target)?;
    if target.is_catalog_target() {
        state
            .db
            .with_conn(|conn| crate::database::dao::clear_current_provider(conn, target))?;
    } else {
        crate::commands::providers::switch_provider_for_target(&provider.id, target, Some(&app), &state).await?;
    }
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
    if target.is_catalog_target() {
        state
            .db
            .with_conn(|conn| crate::database::dao::clear_current_provider(conn, target))?;
    }
    crate::catalog::invalidate_view_cache();
    crate::commands::providers::sync_live_after_connection_change(target, false, &app, &state).await?;
    crate::gateway::service::emit_status(&app);
    Ok(())
}

#[tauri::command]
pub fn list_route_modes(
    profile_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::database::dao::gateway::RouteMode>> {
    state.db.with_conn(|conn| {
        crate::database::dao::gateway::ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        let id = crate::database::dao::gateway::resolve_profile_id(conn, profile_id.as_deref())?;
        if let Some(profile) = crate::database::dao::gateway::get_profile(conn, &id)? {
            crate::database::dao::gateway::seed_route_modes_from_profile(conn, &profile)?;
        }
        crate::database::dao::gateway::list_route_modes(conn, &id)
    })
}

#[tauri::command]
pub async fn update_route_mode(
    id: String,
    patch: crate::database::dao::gateway::RouteModePatch,
    profile_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::database::dao::gateway::RouteMode> {
    let mode = state.db.with_conn(|conn| {
        crate::database::dao::gateway::patch_route_mode(conn, &id, &patch, profile_id.as_deref())
    })?;
    crate::catalog::invalidate_view_cache();
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(mode)
}

#[tauri::command]
pub fn list_route_rules(
    profile_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<crate::database::dao::gateway::RouteRule>> {
    state.db.with_read_conn(|conn| {
        let id = crate::database::dao::gateway::resolve_profile_id(conn, profile_id.as_deref())?;
        crate::database::dao::gateway::list_route_rules(conn, &id)
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
pub async fn list_route_mode_usage_stats(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<RouteModeUsageStat>> {
    let since = chrono::Utc::now().timestamp_millis() - 7 * 24 * 60 * 60 * 1000;
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        db.with_read_conn(|conn| {
            crate::database::dao::proxy_logs::list_route_mode_usage_stats(conn, since)
        })
        .map(|rows| {
            rows.into_iter()
                .map(|row| RouteModeUsageStat {
                    mode_id: row.mode_id,
                    request_count: row.request_count,
                    estimated_cost: row.estimated_cost,
                })
                .collect()
        })
    })
    .await
    .map_err(|e| AppError::Database(format!("route mode usage stats task failed: {e}")))?
}

#[tauri::command]
pub fn simulate_gateway_route(
    input: crate::gateway::simulate::SimulateRouteInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<crate::gateway::simulate::SimulateRouteResult> {
    crate::gateway::simulate::simulate(&state.db, input)
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartGatewayBudgetView {
    pub daily_budget_usd: f64,
    pub action: String,
    pub fallback_model: String,
    pub today_spend_usd: f64,
}

#[tauri::command]
pub fn get_smart_gateway_budget(
    state: tauri::State<'_, AppState>,
) -> SmartGatewayBudgetView {
    let settings = crate::gateway::budget::load_from_db(&state.db);
    SmartGatewayBudgetView {
        daily_budget_usd: settings.daily_budget_usd,
        action: settings.action,
        fallback_model: settings.fallback_model,
        today_spend_usd: crate::gateway::budget::today_spend_usd(&state.db),
    }
}

#[tauri::command]
pub fn set_smart_gateway_budget(
    settings: crate::gateway::budget::BudgetSettings,
    state: tauri::State<'_, AppState>,
) -> AppResult<SmartGatewayBudgetView> {
    crate::gateway::budget::persist(&state.db, &settings)?;
    Ok(SmartGatewayBudgetView {
        daily_budget_usd: settings.daily_budget_usd,
        action: settings.action,
        fallback_model: settings.fallback_model,
        today_spend_usd: crate::gateway::budget::today_spend_usd(&state.db),
    })
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
