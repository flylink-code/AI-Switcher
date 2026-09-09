//! Agent connection + gateway profile commands.

use serde::Serialize;

use crate::commands::providers::sync_live_after_connection_change;
use crate::database::dao::gateway::{
    current_connection_view, current_profile, ensure_profile_for_target, list_profiles,
    patch_profile, set_current_connection_type, AgentConnectionView, ConnectionType,
    GatewayProfile, GatewayProfilePatch,
};
use crate::error::AppResult;
use crate::provider::ProviderTarget;
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
    if crate::catalog::enabled(state.db.as_ref(), target) {
        sync_live_after_connection_change(target, true, &app, &state).await?;
    } else if target == ProviderTarget::ClaudeCode && patch.role_routing_enabled == Some(false) {
        let _ = crate::config::claude_code::apply_opusplan_model(false);
        let _ = crate::wsl_direct::sync_claude_codex_files();
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
    target: ProviderTarget,
    limit: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<GatewayRouteLog>> {
    let cap = limit.unwrap_or(30).clamp(1, 200);
    state.db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, created_at, requested_model, model, route_reason, profile_id, upstream_id,
                    provider_name, attempt_index, status_code
             FROM proxy_request_logs
             WHERE target_app = ? AND COALESCE(data_source, 'proxy') = 'proxy'
             ORDER BY created_at DESC LIMIT ?;",
        )?;
        let rows = stmt.query_map(rusqlite::params![target.as_str(), cap], |row| {
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
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}
