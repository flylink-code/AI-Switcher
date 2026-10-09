//! Gateway profiles, agent connections, and upstream mirrors (Schema 29).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use ts_rs::TS;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::provider::{ProtocolType, Provider, ProviderInput, ProviderKind, ProviderTarget};
use crate::secrets;

use super::settings::{get_setting, set_setting};

const CATALOG_CODE_KEY: &str = "gateway_catalog_claude_code";
const CATALOG_CODEX_KEY: &str = "gateway_catalog_codex";
const CATALOG_CODE_SUBAGENT_KEY: &str = "gateway_catalog_claude_code_subagent";
const CATALOG_CODEX_SUBAGENT_KEY: &str = "gateway_catalog_codex_subagent";
const CATALOG_HIDE_CODE_KEY: &str = "gateway_catalog_hide_official_claude_code";
const CATALOG_HIDE_CODEX_KEY: &str = "gateway_catalog_hide_official_codex";
const CATALOG_OPUSPLAN_KEY: &str = "gateway_catalog_claude_code_opusplan";
const CATALOG_PLAN_KEY: &str = "gateway_catalog_claude_code_plan";
const CATALOG_EXECUTE_KEY: &str = "gateway_catalog_claude_code_execute";

pub const DEFAULT_PROFILE_PREFIX: &str = "gprof_";
pub const SHARED_PROFILE_ID: &str = "gprof_shared";
/// New cloned profiles store this in `target_app` (not per-agent).
pub const SHARED_PROFILE_TARGET: &str = "shared";
/// New long-context rows and leftover `threshold = 1` (matches almost every request).
pub const DEFAULT_LONG_CONTEXT_THRESHOLD: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    External,
    Gateway,
}

impl ConnectionType {
    pub fn from_str_lossy(value: &str) -> Self {
        if value == "gateway" {
            ConnectionType::Gateway
        } else {
            ConnectionType::External
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct GatewayProfile {
    pub id: String,
    pub name: String,
    pub target_app: ProviderTarget,
    pub default_model: String,
    pub plan_model: String,
    pub execute_model: String,
    pub subagent_model: String,
    pub allowed_upstream_ids: Vec<String>,
    pub role_routing_enabled: bool,
    pub explicit_fallback_enabled: bool,
    pub fallback_mode: String,
    pub fallback_models: Vec<String>,
    pub hide_official: bool,
    #[serde(skip_serializing)]
    #[cfg_attr(test, ts(skip))]
    pub entry_token: String,
    pub entry_token_set: bool,
    pub plan_fallback: Vec<String>,
    pub execute_fallback: Vec<String>,
    pub subagent_fallback: Vec<String>,
    pub long_context_model: String,
    pub long_context_tokens: i64,
    pub web_search_model: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProfilePatch {
    pub name: Option<String>,
    pub default_model: Option<String>,
    pub plan_model: Option<String>,
    pub execute_model: Option<String>,
    pub subagent_model: Option<String>,
    pub allowed_upstream_ids: Option<Vec<String>>,
    pub role_routing_enabled: Option<bool>,
    pub explicit_fallback_enabled: Option<bool>,
    pub fallback_mode: Option<String>,
    pub fallback_models: Option<Vec<String>>,
    pub hide_official: Option<bool>,
    pub plan_fallback: Option<Vec<String>>,
    pub execute_fallback: Option<Vec<String>>,
    pub subagent_fallback: Option<Vec<String>>,
    pub long_context_model: Option<String>,
    pub long_context_tokens: Option<i64>,
    pub web_search_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUpstreamImportItem {
    pub provider_id: String,
    pub name: String,
    pub upstream_id: Option<String>,
    pub skipped: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUpstreamImportResult {
    pub imported: i64,
    pub skipped: i64,
    pub items: Vec<GatewayUpstreamImportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUpstreamModelRow {
    pub model_id: String,
    pub visible: bool,
    #[serde(default)]
    pub display_name: String,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
    #[serde(default)]
    pub capabilities: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectionView {
    pub target: ProviderTarget,
    pub connection_type: ConnectionType,
    pub upstream_id: Option<String>,
    pub profile: Option<GatewayProfile>,
}

/// Copy legacy providers/settings into gateway tables. Idempotent.
pub fn seed_from_legacy(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    let providers_exist: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='providers';",
        [],
        |row| row.get(0),
    )?;
    if providers_exist == 0 {
        return Ok(());
    }
    copy_providers_to_upstreams(conn)?;
    seed_targets(conn)?;
    Ok(())
}

fn copy_providers_to_upstreams(conn: &Connection) -> AppResult<()> {
    let has_kind: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'provider_kind';",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let filter = if has_kind > 0 {
        "WHERE COALESCE(provider_kind, 'standard') != 'smart_gateway'"
    } else {
        ""
    };
    conn.execute_batch(&format!(
        "INSERT OR IGNORE INTO upstreams (
            id, name, base_url, api_key, model, protocol_type, notes, sort_index, created_at
         )
         SELECT id, name, base_url, api_key, model, protocol_type, notes, sort_index, created_at
         FROM providers
         {filter};
         INSERT OR IGNORE INTO gateway_id_map (old_provider_id, upstream_id)
         SELECT id, id FROM providers
         {filter};"
    ))?;
    copy_optional_provider_column(conn, "provider_kind", "UPDATE upstreams SET provider_kind = (SELECT provider_kind FROM providers WHERE providers.id = upstreams.id) WHERE provider_kind = 'standard';")?;
    copy_optional_provider_column(conn, "auth_binding", "UPDATE upstreams SET auth_binding = COALESCE((SELECT auth_binding FROM providers WHERE providers.id = upstreams.id), '');")?;
    copy_optional_provider_column(conn, "model_context_window", "UPDATE upstreams SET model_context_window = (SELECT model_context_window FROM providers WHERE providers.id = upstreams.id);")?;
    copy_optional_provider_column(conn, "web_search_enabled", "UPDATE upstreams SET web_search_enabled = (SELECT web_search_enabled FROM providers WHERE providers.id = upstreams.id);")?;
    copy_optional_provider_column(conn, "auto_review_model_override", "UPDATE upstreams SET auto_review_model_override = (SELECT auto_review_model_override FROM providers WHERE providers.id = upstreams.id);")?;
    copy_optional_provider_column(conn, "failover_group", "UPDATE upstreams SET failover_group = COALESCE((SELECT failover_group FROM providers WHERE providers.id = upstreams.id), 0);")?;
    copy_optional_provider_column(conn, "failover_models", "UPDATE upstreams SET failover_models = COALESCE((SELECT failover_models FROM providers WHERE providers.id = upstreams.id), '[]');")?;
    copy_optional_provider_column(conn, "hidden_models_json", "UPDATE upstreams SET hidden_models_json = COALESCE((SELECT hidden_models_json FROM providers WHERE providers.id = upstreams.id), '[]');")?;
    copy_optional_provider_column(conn, "thinking_config_json", "UPDATE upstreams SET thinking_config_json = COALESCE((SELECT thinking_config_json FROM providers WHERE providers.id = upstreams.id), '{}');")?;
    copy_optional_provider_column(conn, "custom_headers_json", "UPDATE upstreams SET custom_headers_json = COALESCE((SELECT custom_headers_json FROM providers WHERE providers.id = upstreams.id), '{}');")?;

    let model_cache_exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='provider_models';",
        [],
        |row| row.get(0),
    )?;
    if model_cache_exists == 0 {
        return Ok(());
    }
    let mut stmt = conn.prepare("SELECT provider_id, models_json FROM provider_models;")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (provider_id, json) = row?;
        let models: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
        for model in models {
            let model = model.trim();
            if model.is_empty() {
                continue;
            }
            let id = format!("{provider_id}:{model}");
            conn.execute(
                "INSERT OR IGNORE INTO upstream_models
                    (id, upstream_id, model_id, verified_status, visible)
                 VALUES (?, ?, ?, 'declared', 1);",
                params![id, provider_id, model],
            )?;
        }
    }
    Ok(())
}

fn copy_optional_provider_column(conn: &Connection, column: &str, sql: &str) -> AppResult<()> {
    let has: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('providers') WHERE name = ?;",
        params![column],
        |row| row.get(0),
    )?;
    if has > 0 {
        conn.execute_batch(sql)?;
    }
    Ok(())
}

fn seed_targets(conn: &Connection) -> AppResult<()> {
    let has_target: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'target_app';",
        [],
        |row| row.get(0),
    )?;
    if has_target == 0 {
        return Ok(());
    }
    let mut targets: Vec<String> = Vec::new();
    let mut stmt = conn.prepare("SELECT DISTINCT target_app FROM providers;")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        let target = row?;
        if !target.trim().is_empty() && !targets.contains(&target) {
            targets.push(target);
        }
    }
    for extra in ["claude_code", "codex"] {
        if catalog_setting_true(conn, extra) && !targets.iter().any(|item| item == extra) {
            targets.push(extra.to_string());
        }
    }
    let failover_on = get_setting(conn, "proxy_failover_enabled")?
        .as_deref()
        == Some("true");
    let now = chrono::Utc::now().timestamp_millis();
    for target in targets {
        ensure_default_profile(conn, &target, failover_on, now)?;
        ensure_external_connections(conn, &target, now)?;
        ensure_gateway_connection(conn, &target, now)?;
        if catalog_setting_true(conn, &target) {
            set_current_connection_type(conn, &target, ConnectionType::Gateway)?;
        }
    }
    ensure_shared_profile(conn, now)?;
    Ok(())
}

fn catalog_setting_true(conn: &Connection, target: &str) -> bool {
    let key = match target {
        "claude_code" => CATALOG_CODE_KEY,
        "codex" => CATALOG_CODEX_KEY,
        _ => return false,
    };
    get_setting(conn, key)
        .ok()
        .flatten()
        .as_deref()
        == Some("true")
}

fn default_profile_id(target: &str) -> String {
    format!("{DEFAULT_PROFILE_PREFIX}{target}")
}

fn ensure_default_profile(
    conn: &Connection,
    target: &str,
    failover_on: bool,
    now: i64,
) -> AppResult<()> {
    let id = default_profile_id(target);
    let existing: i64 = conn.query_row(
        "SELECT count(*) FROM gateway_profiles WHERE id = ?;",
        params![id],
        |row| row.get(0),
    )?;
    if existing > 0 {
        return Ok(());
    }
    let mut allowed: Vec<String> = Vec::new();
    let mut stmt =
        conn.prepare("SELECT id FROM providers WHERE target_app = ? ORDER BY sort_index, created_at;")?;
    let rows = stmt.query_map(params![target], |row| row.get::<_, String>(0))?;
    for row in rows {
        allowed.push(row?);
    }
    let subagent = setting_or_empty(conn, subagent_key(target));
    let hide_official = setting_true(conn, hide_official_key(target));
    let (role_routing, plan, execute) = if target == "claude_code" {
        (
            setting_true(conn, Some(CATALOG_OPUSPLAN_KEY)),
            setting_or_empty(conn, Some(CATALOG_PLAN_KEY)),
            setting_or_empty(conn, Some(CATALOG_EXECUTE_KEY)),
        )
    } else {
        (false, String::new(), String::new())
    };
    let fallback_mode = if failover_on { "retry" } else { "off" };
    let token = format!("gwt_{}", Uuid::new_v4().simple());
    let name = match target {
        "claude_code" => "Claude Code 默认档案",
        "claude_desktop" => "Claude Desktop 默认档案",
        "codex" => "Codex 默认档案",
        "opencode" => "OpenCode 默认档案",
        "pi" => "Pi 默认档案",
        "dsh" => "DSH 默认档案",
        "cline" => "Cline 默认档案",
        other => other,
    };
    conn.execute(
        "INSERT INTO gateway_profiles (
            id, name, target_app, default_model, plan_model, execute_model, subagent_model,
            allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
            fallback_mode, fallback_models_json, hide_official, entry_token,
            plan_fallback_json, execute_fallback_json, subagent_fallback_json,
            created_at, updated_at
         ) VALUES (?, ?, ?, '', ?, ?, ?, ?, ?, 0, ?, '[]', ?, ?, '[]', '[]', '[]', ?, ?);",
        params![
            id,
            name,
            target,
            plan,
            execute,
            subagent,
            serde_json::to_string(&allowed)?,
            if role_routing { 1 } else { 0 },
            fallback_mode,
            if hide_official { 1 } else { 0 },
            token,
            now,
            now,
        ],
    )?;
    Ok(())
}

fn ensure_external_connections(conn: &Connection, target: &str, now: i64) -> AppResult<()> {
    if !table_exists(conn, "agent_connections") || !table_exists(conn, "providers") {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT id, is_current FROM providers WHERE target_app = ?;",
    )?;
    let rows = stmt.query_map(params![target], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (provider_id, is_current) = row?;
        let conn_id = format!("aconn_ext_{provider_id}");
        conn.execute(
            "INSERT OR IGNORE INTO agent_connections
                (id, target_app, connection_type, upstream_id, profile_id, is_current, created_at)
             VALUES (?, ?, 'external', ?, NULL, ?, ?);",
            params![conn_id, target, provider_id, is_current, now],
        )?;
    }
    Ok(())
}

fn ensure_gateway_connection(conn: &Connection, target: &str, now: i64) -> AppResult<()> {
    if !table_exists(conn, "agent_connections") {
        return Ok(());
    }
    let id = format!("aconn_gw_{target}");
    conn.execute(
        "INSERT OR IGNORE INTO agent_connections
            (id, target_app, connection_type, upstream_id, profile_id, is_current, created_at)
         VALUES (?, ?, 'gateway', NULL, ?, 0, ?);",
        params![id, target, SHARED_PROFILE_ID, now],
    )?;
    conn.execute(
        "UPDATE agent_connections SET profile_id = ? WHERE id = ?;",
        params![SHARED_PROFILE_ID, id],
    )?;
    Ok(())
}

fn setting_true(conn: &Connection, key: Option<&str>) -> bool {
    let Some(key) = key else {
        return false;
    };
    get_setting(conn, key).ok().flatten().as_deref() == Some("true")
}

fn setting_or_empty(conn: &Connection, key: Option<&str>) -> String {
    let Some(key) = key else {
        return String::new();
    };
    get_setting(conn, key)
        .ok()
        .flatten()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn subagent_key(target: &str) -> Option<&'static str> {
    match target {
        "claude_code" => Some(CATALOG_CODE_SUBAGENT_KEY),
        "codex" => Some(CATALOG_CODEX_SUBAGENT_KEY),
        _ => None,
    }
}

fn hide_official_key(target: &str) -> Option<&'static str> {
    match target {
        "claude_code" => Some(CATALOG_HIDE_CODE_KEY),
        "codex" => Some(CATALOG_HIDE_CODEX_KEY),
        _ => None,
    }
}

fn current_provider_is_smart_gateway(conn: &Connection, target: ProviderTarget) -> bool {
    conn.query_row(
        "SELECT count(*) FROM providers
         WHERE target_app = ? AND is_current = 1
           AND COALESCE(provider_kind, '') = 'smart_gateway';",
        params![target.as_str()],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

pub fn is_gateway_connection(conn: &Connection, target: ProviderTarget) -> bool {
    if let Some(binding) = binding_for_target(conn, target).ok().flatten() {
        if binding.mode == BINDING_MODE_DIRECT {
            return false;
        }
        // Catalog agents keep Auto as an extra entry; binding alone means catalog-on.
        // Code / Desktop / Codex only route through the gateway when Auto is current.
        if target.is_catalog_target() {
            return true;
        }
        return current_provider_is_smart_gateway(conn, target);
    }
    if table_exists(conn, "agent_connections") {
        let has_rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM agent_connections WHERE target_app = ?;",
                params![target.as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if has_rows > 0 {
            return conn
                .query_row(
                    "SELECT count(*) FROM agent_connections
                     WHERE target_app = ? AND connection_type = 'gateway' AND is_current = 1;",
                    params![target.as_str()],
                    |row| row.get(0),
                )
                .unwrap_or(0)
                > 0;
        }
    }
    let key = match target {
        ProviderTarget::ClaudeCode => Some(CATALOG_CODE_KEY),
        ProviderTarget::Codex => Some(CATALOG_CODEX_KEY),
        _ => None,
    };
    key.and_then(|key| get_setting(conn, key).ok().flatten())
        .as_deref()
        == Some("true")
}

pub fn current_profile(conn: &Connection, target: ProviderTarget) -> AppResult<Option<GatewayProfile>> {
    get_profile(conn, &profile_id_for_target(conn, target)?)
}

pub fn profile_id_for_target(conn: &Connection, target: ProviderTarget) -> AppResult<String> {
    if let Some(binding) = binding_for_target(conn, target)? {
        let id = binding.profile_id.trim();
        if !id.is_empty() && get_profile(conn, id)?.is_some() {
            return Ok(id.to_string());
        }
    }
    Ok(SHARED_PROFILE_ID.to_string())
}

pub fn resolve_profile_id(conn: &Connection, profile_id: Option<&str>) -> AppResult<String> {
    let id = profile_id.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(SHARED_PROFILE_ID);
    if get_profile(conn, id)?.is_some() {
        Ok(id.to_string())
    } else {
        Ok(SHARED_PROFILE_ID.to_string())
    }
}

pub fn get_profile(conn: &Connection, id: &str) -> AppResult<Option<GatewayProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, target_app, default_model, plan_model, execute_model, subagent_model,
                allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
                fallback_mode, fallback_models_json, hide_official, entry_token,
                plan_fallback_json, execute_fallback_json, subagent_fallback_json,
                long_context_model, long_context_tokens, web_search_model,
                created_at, updated_at
         FROM gateway_profiles WHERE id = ?;",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_profile(row)?)),
        None => Ok(None),
    }
}

pub fn list_profiles(conn: &Connection) -> AppResult<Vec<GatewayProfile>> {
    if !table_exists(conn, "gateway_profiles") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, name, target_app, default_model, plan_model, execute_model, subagent_model,
                allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
                fallback_mode, fallback_models_json, hide_official, entry_token,
                plan_fallback_json, execute_fallback_json, subagent_fallback_json,
                long_context_model, long_context_tokens, web_search_model,
                created_at, updated_at
         FROM gateway_profiles
         WHERE id = ? OR target_app = ?
         ORDER BY CASE WHEN id = ? THEN 0 ELSE 1 END, created_at ASC, name ASC;",
    )?;
    let rows = stmt.query_map(
        params![SHARED_PROFILE_ID, SHARED_PROFILE_TARGET, SHARED_PROFILE_ID],
        row_to_profile,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn create_profile(
    conn: &Connection,
    name: &str,
    clone_from: Option<&str>,
) -> AppResult<GatewayProfile> {
    let now = chrono::Utc::now().timestamp_millis();
    ensure_shared_profile(conn, now)?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("档案名称不能为空".into()));
    }
    let source_id = clone_from
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(SHARED_PROFILE_ID);
    let source = get_profile(conn, source_id)?
        .ok_or_else(|| AppError::Config(format!("网关档案不存在: {source_id}")))?;
    let id = format!("{DEFAULT_PROFILE_PREFIX}{}", Uuid::new_v4().simple());
    insert_cloned_profile(conn, &id, trimmed, &source, now)?;
    copy_route_modes(conn, &source.id, &id)?;
    copy_route_rules(conn, &source.id, &id)?;
    let created = get_profile(conn, &id)?
        .ok_or_else(|| AppError::Config("档案创建失败".into()))?;
    seed_route_modes_from_profile(conn, &created)?;
    get_profile(conn, &id)?.ok_or_else(|| AppError::Config("档案创建失败".into()))
}

pub fn rename_profile(conn: &Connection, id: &str, name: &str) -> AppResult<GatewayProfile> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("档案名称不能为空".into()));
    }
    patch_profile(
        conn,
        id,
        &GatewayProfilePatch {
            name: Some(trimmed.to_string()),
            ..GatewayProfilePatch::default()
        },
    )
}

pub fn delete_profile(conn: &Connection, id: &str) -> AppResult<()> {
    if id == SHARED_PROFILE_ID {
        return Err(AppError::Config("不能删除默认档案".into()));
    }
    if get_profile(conn, id)?.is_none() {
        return Err(AppError::Config(format!("网关档案不存在: {id}")));
    }
    if table_exists(conn, "gateway_bindings") && bindings_have_profile_id(conn) {
        conn.execute(
            "UPDATE gateway_bindings SET profile_id = ? WHERE profile_id = ?;",
            params![SHARED_PROFILE_ID, id],
        )?;
    }
    if table_exists(conn, "route_modes") {
        conn.execute("DELETE FROM route_modes WHERE profile_id = ?;", params![id])?;
    }
    if table_exists(conn, "route_rules") {
        conn.execute("DELETE FROM route_rules WHERE profile_id = ?;", params![id])?;
    }
    conn.execute("DELETE FROM gateway_profiles WHERE id = ?;", params![id])?;
    Ok(())
}

/// Repoint an existing binding at another profile. Picking a profile must never
/// create the binding itself: a fresh row makes catalog agents look connected
/// and pushes the Auto entry into their live config.
pub fn set_binding_profile(
    conn: &Connection,
    target: ProviderTarget,
    profile_id: &str,
) -> AppResult<GatewayBinding> {
    let requested = profile_id.trim();
    if !requested.is_empty() && get_profile(conn, requested)?.is_none() {
        return Err(AppError::Config(format!("网关档案不存在: {requested}")));
    }
    let Some(binding) = binding_for_target(conn, target)? else {
        return Err(AppError::Config(
            "请先绑定智能网关，再选择路由档案".to_string(),
        ));
    };
    if binding.mode == BINDING_MODE_DIRECT {
        return Err(AppError::Config(
            "当前处于直连模式，无法切换网关档案；请先切换为智能网关模式".to_string(),
        ));
    }
    let resolved = resolve_profile_id(conn, Some(requested))?;
    if bindings_have_profile_id(conn) {
        conn.execute(
            "UPDATE gateway_bindings SET profile_id = ? WHERE target_app = ?;",
            params![resolved, target.as_str()],
        )?;
    }
    binding_for_target(conn, target)?.ok_or_else(|| AppError::Config("绑定写入失败".into()))
}

fn insert_cloned_profile(
    conn: &Connection,
    id: &str,
    name: &str,
    source: &GatewayProfile,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO gateway_profiles (
            id, name, target_app, default_model, plan_model, execute_model, subagent_model,
            allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
            fallback_mode, fallback_models_json, hide_official, entry_token,
            plan_fallback_json, execute_fallback_json, subagent_fallback_json,
            long_context_model, long_context_tokens, web_search_model,
            created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?, '', ?, ?, ?, ?, ?, ?, ?, ?);",
        params![
            id,
            name,
            SHARED_PROFILE_TARGET,
            source.default_model,
            source.plan_model,
            source.execute_model,
            source.subagent_model,
            serde_json::to_string(&source.allowed_upstream_ids).unwrap_or_else(|_| "[]".into()),
            if source.explicit_fallback_enabled { 1 } else { 0 },
            source.fallback_mode,
            serde_json::to_string(&source.fallback_models).unwrap_or_else(|_| "[]".into()),
            if source.hide_official { 1 } else { 0 },
            serde_json::to_string(&source.plan_fallback).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&source.execute_fallback).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&source.subagent_fallback).unwrap_or_else(|_| "[]".into()),
            source.long_context_model,
            source.long_context_tokens,
            source.web_search_model,
            now,
            now,
        ],
    )?;
    Ok(())
}

fn copy_route_modes(conn: &Connection, from_id: &str, to_id: &str) -> AppResult<()> {
    if !table_exists(conn, "route_modes") {
        return Ok(());
    }
    for mode in list_route_modes(conn, from_id)? {
        conn.execute(
            "INSERT OR REPLACE INTO route_modes
                (id, profile_id, enabled, model, thinking_config_json, fallback_models_json, threshold, sort_index)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?);",
            params![
                mode.id,
                to_id,
                if mode.enabled { 1 } else { 0 },
                mode.model,
                mode.thinking_config_json,
                serde_json::to_string(&mode.fallback_models).unwrap_or_else(|_| "[]".into()),
                mode.threshold,
                mode.sort_index
            ],
        )?;
    }
    Ok(())
}

fn copy_route_rules(conn: &Connection, from_id: &str, to_id: &str) -> AppResult<()> {
    if !table_exists(conn, "route_rules") {
        return Ok(());
    }
    for rule in list_route_rules(conn, from_id)? {
        let new_id = format!("rule_{}", Uuid::new_v4().simple());
        conn.execute(
            "INSERT INTO route_rules
                (id, profile_id, enabled, sort_index, rule_type, condition_json, pattern, target_model,
                 thinking_config_json, rewrites_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            params![
                new_id,
                to_id,
                if rule.enabled { 1 } else { 0 },
                rule.sort_index,
                rule.rule_type,
                rule.condition_json,
                rule.pattern,
                rule.target_model,
                rule.thinking_config_json,
                rule.rewrites_json
            ],
        )?;
    }
    Ok(())
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<GatewayProfile> {
    let token: String = row.get(13)?;
    Ok(GatewayProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        target_app: ProviderTarget::from_str_lossy(&row.get::<_, String>(2)?),
        default_model: row.get(3)?,
        plan_model: row.get(4)?,
        execute_model: row.get(5)?,
        subagent_model: row.get(6)?,
        allowed_upstream_ids: parse_string_list(row.get(7)?),
        role_routing_enabled: row.get::<_, i64>(8)? != 0,
        explicit_fallback_enabled: row.get::<_, i64>(9)? != 0,
        fallback_mode: row.get(10)?,
        fallback_models: parse_string_list(row.get(11)?),
        hide_official: row.get::<_, i64>(12)? != 0,
        entry_token_set: !token.trim().is_empty(),
        entry_token: token,
        plan_fallback: parse_string_list(row.get(14)?),
        execute_fallback: parse_string_list(row.get(15)?),
        subagent_fallback: parse_string_list(row.get(16)?),
        long_context_model: row.get(17).unwrap_or_default(),
        long_context_tokens: row.get(18).unwrap_or(0),
        web_search_model: row.get(19).unwrap_or_default(),
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

fn parse_string_list(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

pub fn ensure_profile_for_target(conn: &Connection, target: ProviderTarget) -> AppResult<GatewayProfile> {
    let now = chrono::Utc::now().timestamp_millis();
    let profile = ensure_shared_profile(conn, now)?;
    let _ = seed_route_modes_from_profile(conn, &profile);
    if table_exists(conn, "agent_connections") {
        ensure_gateway_connection(conn, target.as_str(), now)?;
    }
    Ok(profile)
}

fn profile_has_slots(profile: &GatewayProfile) -> bool {
    !profile.default_model.trim().is_empty()
        || !profile.subagent_model.trim().is_empty()
        || !profile.long_context_model.trim().is_empty()
        || !profile.web_search_model.trim().is_empty()
}

fn pick_legacy_profile_to_copy(conn: &Connection) -> AppResult<Option<GatewayProfile>> {
    if let Some(profile) = get_profile(conn, "gprof_claude_code")? {
        if profile_has_slots(&profile) {
            return Ok(Some(profile));
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id FROM gateway_profiles WHERE id != ? ORDER BY created_at ASC;",
    )?;
    let ids = stmt.query_map(params![SHARED_PROFILE_ID], |row| row.get::<_, String>(0))?;
    let mut first: Option<GatewayProfile> = None;
    for id in ids {
        let id = id?;
        let Some(profile) = get_profile(conn, &id)? else {
            continue;
        };
        if profile_has_slots(&profile) {
            return Ok(Some(profile));
        }
        if first.is_none() {
            first = Some(profile);
        }
    }
    Ok(first)
}

fn insert_shared_profile(conn: &Connection, source: Option<&GatewayProfile>, now: i64) -> AppResult<()> {
    let token = source
        .map(|profile| profile.entry_token.trim().to_string())
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| format!("gwt_{}", Uuid::new_v4().simple()));
    let default_model = source.map(|p| p.default_model.as_str()).unwrap_or("");
    let subagent = source.map(|p| p.subagent_model.as_str()).unwrap_or("");
    let allowed = source
        .map(|p| serde_json::to_string(&p.allowed_upstream_ids).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| "[]".into());
    let explicit = source.map(|p| p.explicit_fallback_enabled).unwrap_or(false);
    let fallback_mode = source
        .map(|p| p.fallback_mode.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("off");
    let fallback_models = source
        .map(|p| serde_json::to_string(&p.fallback_models).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| "[]".into());
    let hide = source.map(|p| p.hide_official).unwrap_or(false);
    let subagent_fallback = source
        .map(|p| serde_json::to_string(&p.subagent_fallback).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| "[]".into());
    let long_context_model = source.map(|p| p.long_context_model.as_str()).unwrap_or("");
    let long_context_tokens = source.map(|p| p.long_context_tokens).unwrap_or(0);
    let web_search_model = source.map(|p| p.web_search_model.as_str()).unwrap_or("");
    conn.execute(
        "INSERT INTO gateway_profiles (
            id, name, target_app, default_model, plan_model, execute_model, subagent_model,
            allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
            fallback_mode, fallback_models_json, hide_official, entry_token,
            plan_fallback_json, execute_fallback_json, subagent_fallback_json,
            long_context_model, long_context_tokens, web_search_model,
            created_at, updated_at
         ) VALUES (?, '默认', 'shared', ?, '', '', ?, ?, 0, ?, ?, ?, ?, ?, '[]', '[]', ?, ?, ?, ?, ?, ?);",
        params![
            SHARED_PROFILE_ID,
            default_model,
            subagent,
            allowed,
            if explicit { 1 } else { 0 },
            fallback_mode,
            fallback_models,
            if hide { 1 } else { 0 },
            token,
            subagent_fallback,
            long_context_model,
            long_context_tokens,
            web_search_model,
            now,
            now,
        ],
    )?;
    Ok(())
}

fn relink_gateway_connections_to_shared(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "agent_connections") {
        return Ok(());
    }
    conn.execute(
        "UPDATE agent_connections SET profile_id = ?
         WHERE connection_type = 'gateway'
           AND (profile_id IS NULL OR profile_id != ?);",
        params![SHARED_PROFILE_ID, SHARED_PROFILE_ID],
    )?;
    Ok(())
}

fn sync_smart_gateway_provider_tokens(conn: &Connection, token: &str) -> AppResult<()> {
    if token.trim().is_empty() || !table_exists(conn, "providers") {
        return Ok(());
    }
    let has_kind: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'provider_kind';",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if has_kind == 0 {
        return Ok(());
    }
    conn.execute(
        "UPDATE providers SET api_key = ?
         WHERE COALESCE(provider_kind, '') = 'smart_gateway';",
        params![token],
    )?;
    Ok(())
}

pub fn ensure_shared_profile(conn: &Connection, now: i64) -> AppResult<GatewayProfile> {
    if get_profile(conn, SHARED_PROFILE_ID)?.is_none() {
        let source = pick_legacy_profile_to_copy(conn)?;
        insert_shared_profile(conn, source.as_ref(), now)?;
        if let Some(profile) = get_profile(conn, SHARED_PROFILE_ID)? {
            sync_smart_gateway_provider_tokens(conn, &profile.entry_token)?;
        }
    }
    conn.execute(
        "UPDATE gateway_profiles SET role_routing_enabled = 0 WHERE id = ? AND role_routing_enabled != 0;",
        params![SHARED_PROFILE_ID],
    )?;
    relink_gateway_connections_to_shared(conn)?;
    let mut profile = get_profile(conn, SHARED_PROFILE_ID)?
        .ok_or_else(|| AppError::Config("未能创建共享网关档案".to_string()))?;
    seed_route_modes_from_profile(conn, &profile)?;
    if profile.entry_token.trim().is_empty() {
        profile.entry_token = format!("gwt_{}", Uuid::new_v4().simple());
        profile.entry_token_set = true;
        conn.execute(
            "UPDATE gateway_profiles SET entry_token = ?, updated_at = ? WHERE id = ?;",
            params![profile.entry_token, now, SHARED_PROFILE_ID],
        )?;
        sync_smart_gateway_provider_tokens(conn, &profile.entry_token)?;
    }
    Ok(profile)
}

pub fn patch_profile(conn: &Connection, id: &str, patch: &GatewayProfilePatch) -> AppResult<GatewayProfile> {
    let mut profile = get_profile(conn, id)?
        .ok_or_else(|| AppError::Config(format!("网关档案不存在: {id}")))?;
    if let Some(name) = &patch.name {
        profile.name = name.trim().to_string();
    }
    if let Some(value) = &patch.default_model {
        profile.default_model = value.trim().to_string();
    }
    if let Some(value) = &patch.plan_model {
        profile.plan_model = value.trim().to_string();
    }
    if let Some(value) = &patch.execute_model {
        profile.execute_model = value.trim().to_string();
    }
    if let Some(value) = &patch.subagent_model {
        profile.subagent_model = value.trim().to_string();
    }
    if let Some(value) = &patch.allowed_upstream_ids {
        profile.allowed_upstream_ids = value.clone();
    }
    if let Some(_value) = patch.role_routing_enabled {
        profile.role_routing_enabled = false;
    }
    if let Some(value) = patch.explicit_fallback_enabled {
        profile.explicit_fallback_enabled = value;
    }
    if let Some(value) = &patch.fallback_mode {
        profile.fallback_mode = normalize_fallback_mode(value);
    }
    if let Some(value) = &patch.fallback_models {
        profile.fallback_models = value.clone();
    }
    if let Some(value) = patch.hide_official {
        profile.hide_official = value;
    }
    if let Some(value) = &patch.plan_fallback {
        profile.plan_fallback = value.clone();
    }
    if let Some(value) = &patch.execute_fallback {
        profile.execute_fallback = value.clone();
    }
    if let Some(value) = &patch.subagent_fallback {
        profile.subagent_fallback = value.clone();
    }
    if let Some(value) = &patch.long_context_model {
        profile.long_context_model = value.trim().to_string();
    }
    if let Some(value) = patch.long_context_tokens {
        profile.long_context_tokens = value.max(0);
    }
    if let Some(value) = &patch.web_search_model {
        profile.web_search_model = value.trim().to_string();
    }
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE gateway_profiles SET
            name = ?, default_model = ?, plan_model = ?, execute_model = ?, subagent_model = ?,
            allowed_upstream_ids_json = ?, role_routing_enabled = ?, explicit_fallback_enabled = ?,
            fallback_mode = ?, fallback_models_json = ?, hide_official = ?,
            plan_fallback_json = ?, execute_fallback_json = ?, subagent_fallback_json = ?,
            long_context_model = ?, long_context_tokens = ?, web_search_model = ?,
            updated_at = ?
         WHERE id = ?;",
        params![
            profile.name,
            profile.default_model,
            profile.plan_model,
            profile.execute_model,
            profile.subagent_model,
            serde_json::to_string(&profile.allowed_upstream_ids)?,
            if profile.role_routing_enabled { 1 } else { 0 },
            if profile.explicit_fallback_enabled { 1 } else { 0 },
            profile.fallback_mode,
            serde_json::to_string(&profile.fallback_models)?,
            if profile.hide_official { 1 } else { 0 },
            serde_json::to_string(&profile.plan_fallback)?,
            serde_json::to_string(&profile.execute_fallback)?,
            serde_json::to_string(&profile.subagent_fallback)?,
            profile.long_context_model,
            profile.long_context_tokens,
            profile.web_search_model,
            now,
            id,
        ],
    )?;
    sync_profile_legacy_settings(conn, &profile)?;
    get_profile(conn, id)?.ok_or_else(|| AppError::Config(format!("网关档案不存在: {id}")))
}

fn normalize_fallback_mode(value: &str) -> String {
    match value.trim() {
        "retry" | "model_chain" => value.trim().to_string(),
        _ => "off".to_string(),
    }
}

fn sync_profile_legacy_settings(conn: &Connection, profile: &GatewayProfile) -> AppResult<()> {
    let target = profile.target_app.as_str();
    if let Some(key) = subagent_key(target) {
        set_setting(conn, key, &profile.subagent_model)?;
    }
    if let Some(key) = hide_official_key(target) {
        set_setting(conn, key, if profile.hide_official { "true" } else { "false" })?;
    }
    if target == "claude_code" {
        set_setting(conn, CATALOG_OPUSPLAN_KEY, "false")?;
        set_setting(conn, CATALOG_PLAN_KEY, &profile.plan_model)?;
        set_setting(conn, CATALOG_EXECUTE_KEY, &profile.execute_model)?;
    }
    Ok(())
}

pub fn current_connection_view(
    conn: &Connection,
    target: ProviderTarget,
) -> AppResult<AgentConnectionView> {
    ensure_profile_for_target(conn, target)?;
    let gateway = is_gateway_connection(conn, target);
    let profile = if gateway {
        current_profile(conn, target)?
    } else {
        None
    };
    let upstream_id = if gateway {
        None
    } else {
        conn.query_row(
            "SELECT id FROM providers WHERE target_app = ? AND is_current = 1 LIMIT 1;",
            params![target.as_str()],
            |row| row.get(0),
        )
        .ok()
    };
    Ok(AgentConnectionView {
        target,
        connection_type: if gateway {
            ConnectionType::Gateway
        } else {
            ConnectionType::External
        },
        upstream_id,
        profile,
    })
}

pub fn set_current_connection_type(
    conn: &Connection,
    target: &str,
    kind: ConnectionType,
) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    ensure_default_profile(conn, target, false, now)?;
    let target_enum = ProviderTarget::from_str_lossy(target);
    match kind {
        ConnectionType::Gateway => {
            let provider_id = crate::gateway::smart_gateway_provider_id(target_enum);
            upsert_binding(conn, target_enum, &provider_id)?;
            if let Some(key) = match target {
                "claude_code" => Some(CATALOG_CODE_KEY),
                "codex" => Some(CATALOG_CODEX_KEY),
                _ => None,
            } {
                set_setting(conn, key, "true")?;
            }
        }
        ConnectionType::External => {
            delete_binding(conn, target_enum)?;
            if let Some(key) = match target {
                "claude_code" => Some(CATALOG_CODE_KEY),
                "codex" => Some(CATALOG_CODEX_KEY),
                _ => None,
            } {
                set_setting(conn, key, "false")?;
            }
        }
    }
    if table_exists(conn, "agent_connections") {
        ensure_gateway_connection(conn, target, now)?;
        conn.execute(
            "UPDATE agent_connections SET is_current = 0 WHERE target_app = ?;",
            params![target],
        )?;
        match kind {
            ConnectionType::Gateway => {
                conn.execute(
                    "UPDATE agent_connections SET is_current = 1
                     WHERE target_app = ? AND connection_type = 'gateway';",
                    params![target],
                )?;
            }
            ConnectionType::External => {
                conn.execute(
                    "UPDATE agent_connections SET is_current = 1
                     WHERE id = (
                        SELECT 'aconn_ext_' || id FROM providers
                        WHERE target_app = ? AND is_current = 1 LIMIT 1
                     );",
                    params![target],
                )?;
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub fn sync_upstream_from_provider(conn: &Connection, provider: &Provider) -> AppResult<()> {
    if provider.is_smart_gateway() {
        let _ = conn.execute("DELETE FROM upstreams WHERE id = ?;", params![provider.id]);
        return Ok(());
    }
    conn.execute(
        "INSERT INTO upstreams (
            id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
            notes, sort_index, enabled, model_context_window, web_search_enabled,
            auto_review_model_override, failover_group, failover_models, hidden_models_json,
            thinking_config_json, custom_headers_json, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            base_url = excluded.base_url,
            api_key = excluded.api_key,
            model = excluded.model,
            protocol_type = excluded.protocol_type,
            provider_kind = excluded.provider_kind,
            auth_binding = excluded.auth_binding,
            notes = excluded.notes,
            sort_index = excluded.sort_index,
            model_context_window = excluded.model_context_window,
            web_search_enabled = excluded.web_search_enabled,
            auto_review_model_override = excluded.auto_review_model_override,
            failover_group = excluded.failover_group,
            failover_models = excluded.failover_models,
            hidden_models_json = excluded.hidden_models_json,
            thinking_config_json = excluded.thinking_config_json,
            custom_headers_json = excluded.custom_headers_json;",
        params![
            provider.id,
            provider.name,
            provider.base_url,
            provider.api_key,
            provider.model,
            provider.protocol_type.as_str(),
            provider.provider_kind.as_str(),
            provider.auth_binding,
            provider.notes,
            provider.sort_index,
            provider.model_context_window.map(|value| value as i64),
            provider.web_search_enabled.map(|value| if value { 1 } else { 0 }),
            provider.auto_review_model_override,
            provider.failover_group,
            serde_json::to_string(&provider.failover_models)?,
            serde_json::to_string(&provider.hidden_models)?,
            serde_json::to_string(provider.thinking_config.as_ref().unwrap_or(
                &crate::provider::ThinkingConfig::default()
            ))?,
            serde_json::to_string(provider.custom_headers.as_ref().unwrap_or(&Default::default()))?,
            provider.created_at,
        ],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO gateway_id_map (old_provider_id, upstream_id) VALUES (?, ?);",
        params![provider.id, provider.id],
    )?;
    let has_profile: i64 = conn.query_row(
        "SELECT count(*) FROM gateway_profiles WHERE target_app = ?;",
        params![provider.target_app.as_str()],
        |row| row.get(0),
    )?;
    if has_profile > 0 && table_exists(conn, "agent_connections") {
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT OR IGNORE INTO agent_connections
                (id, target_app, connection_type, upstream_id, profile_id, is_current, created_at)
             VALUES (?, ?, 'external', ?, NULL, 0, ?);",
            params![
                format!("aconn_ext_{}", provider.id),
                provider.target_app.as_str(),
                provider.id,
                now,
            ],
        )?;
        add_upstream_to_default_allowlist(conn, provider.target_app.as_str(), &provider.id)?;
    }
    Ok(())
}

fn add_upstream_to_default_allowlist(conn: &Connection, target: &str, upstream_id: &str) -> AppResult<()> {
    let profile_id = default_profile_id(target);
    let Some(mut profile) = get_profile(conn, &profile_id)? else {
        return Ok(());
    };
    if profile.allowed_upstream_ids.iter().any(|id| id == upstream_id) {
        return Ok(());
    }
    profile.allowed_upstream_ids.push(upstream_id.to_string());
    conn.execute(
        "UPDATE gateway_profiles SET allowed_upstream_ids_json = ?, updated_at = ? WHERE id = ?;",
        params![
            serde_json::to_string(&profile.allowed_upstream_ids)?,
            chrono::Utc::now().timestamp_millis(),
            profile_id,
        ],
    )?;
    Ok(())
}

pub fn assert_upstream_deletable(conn: &Connection, id: &str) -> AppResult<()> {
    if table_exists(conn, "gateway_bindings") && bindings_have_v34(conn) {
        let mut stmt = conn.prepare(
            "SELECT target_app FROM gateway_bindings WHERE mode = 'direct' AND direct_upstream_id = ?;",
        )?;
        let rows = stmt.query_map(params![id], |row| row.get::<_, String>(0))?;
        let mut targets = Vec::new();
        for row in rows {
            targets.push(row?);
        }
        if !targets.is_empty() {
            let joined = targets.join(", ");
            return Err(AppError::Config(format!(
                "该上游正被 Agent ({joined}) 直连使用，无法删除；请先切换对应 Agent 的连接模式"
            )));
        }
    }
    let gateway_current: i64 = if table_exists(conn, "gateway_bindings") {
        conn.query_row("SELECT count(*) FROM gateway_bindings;", [], |row| row.get(0))?
    } else if table_exists(conn, "agent_connections") {
        conn.query_row(
            "SELECT count(*) FROM agent_connections
             WHERE connection_type = 'gateway' AND is_current = 1
               AND profile_id IN (SELECT id FROM gateway_profiles);",
            [],
            |row| row.get(0),
        )?
    } else {
        0
    };
    if gateway_current > 0 {
        let referenced: i64 = conn.query_row(
            "SELECT count(*) FROM gateway_profiles
             WHERE allowed_upstream_ids_json LIKE '%' || ? || '%';",
            params![id],
            |row| row.get(0),
        )?;
        // LIKE is a hint; confirm with parsed lists below when referenced > 0.
        if referenced > 0 {
            let mut stmt = conn.prepare(
                "SELECT id, allowed_upstream_ids_json FROM gateway_profiles;",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (profile_id, json) = row?;
                let ids = parse_string_list(json);
                if ids.iter().any(|item| item == id) {
                    let in_use: i64 = if table_exists(conn, "gateway_bindings") {
                        conn.query_row("SELECT count(*) FROM gateway_bindings;", [], |row| row.get(0))?
                    } else if table_exists(conn, "agent_connections") {
                        conn.query_row(
                        "SELECT count(*) FROM agent_connections
                         WHERE profile_id = ? AND connection_type = 'gateway' AND is_current = 1;",
                        params![profile_id],
                        |row| row.get(0),
                    )?
                    } else {
                        0
                    };
                    if in_use > 0 {
                        return Err(AppError::Config(
                            "该上游正被当前网关档案使用，请先从档案允许列表中移除".to_string(),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn delete_upstream_mirror(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM upstream_models WHERE upstream_id = ?;", params![id])?;
    if table_exists(conn, "agent_connections") {
        conn.execute("DELETE FROM agent_connections WHERE upstream_id = ?;", params![id])?;
    }
    conn.execute("DELETE FROM gateway_id_map WHERE old_provider_id = ? OR upstream_id = ?;", params![id, id])?;
    conn.execute("DELETE FROM upstreams WHERE id = ?;", params![id])?;
    strip_upstream_from_profiles(conn, id)?;
    Ok(())
}

fn strip_upstream_from_profiles(conn: &Connection, id: &str) -> AppResult<()> {
    let mut stmt = conn.prepare("SELECT id, allowed_upstream_ids_json FROM gateway_profiles;")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let now = chrono::Utc::now().timestamp_millis();
    for row in rows {
        let (profile_id, json) = row?;
        let mut ids = parse_string_list(json);
        let before = ids.len();
        ids.retain(|item| item != id);
        if ids.len() != before {
            conn.execute(
                "UPDATE gateway_profiles SET allowed_upstream_ids_json = ?, updated_at = ? WHERE id = ?;",
                params![serde_json::to_string(&ids)?, now, profile_id],
            )?;
        }
    }
    Ok(())
}

pub fn profile_entry_token(conn: &Connection, target: ProviderTarget) -> AppResult<Option<String>> {
    if let Some(token) = binding_token(conn, target) {
        return Ok(Some(token));
    }
    Ok(current_profile(conn, target)?
        .map(|profile| profile.entry_token)
        .filter(|token| !token.trim().is_empty()))
}

pub fn profile_allows_upstream(profile: &GatewayProfile, upstream_id: &str) -> bool {
    profile.allowed_upstream_ids.is_empty()
        || profile
            .allowed_upstream_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(upstream_id))
}

fn parse_health_latency_ms(detail: Option<&str>) -> Option<u64> {
    let detail = detail?;
    let marker = "latency_ms=";
    let start = detail.find(marker)? + marker.len();
    let rest = &detail[start..];
    let end = rest.find('|').unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

/// Global upstream pool (excludes the managed Auto card).
pub fn list_upstream_providers(conn: &Connection, include_disabled: bool) -> AppResult<Vec<Provider>> {
    let has_model_mapping = upstreams_have_model_mapping(conn);
    let sql = match (include_disabled, has_model_mapping) {
        (true, true) => {
            "SELECT id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                    notes, sort_index, enabled, model_context_window, web_search_enabled,
                    auto_review_model_override, failover_group, failover_models, hidden_models_json,
                    thinking_config_json, custom_headers_json, created_at, model_mapping_json,
                    (SELECT status FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT checked_at FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT detail FROM provider_health WHERE provider_id = upstreams.id)
             FROM upstreams
             WHERE COALESCE(provider_kind, 'standard') != 'smart_gateway'
             ORDER BY sort_index ASC, created_at ASC;"
        }
        (true, false) => {
            "SELECT id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                    notes, sort_index, enabled, model_context_window, web_search_enabled,
                    auto_review_model_override, failover_group, failover_models, hidden_models_json,
                    thinking_config_json, custom_headers_json, created_at,
                    (SELECT status FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT checked_at FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT detail FROM provider_health WHERE provider_id = upstreams.id)
             FROM upstreams
             WHERE COALESCE(provider_kind, 'standard') != 'smart_gateway'
             ORDER BY sort_index ASC, created_at ASC;"
        }
        (false, true) => {
            "SELECT id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                    notes, sort_index, enabled, model_context_window, web_search_enabled,
                    auto_review_model_override, failover_group, failover_models, hidden_models_json,
                    thinking_config_json, custom_headers_json, created_at, model_mapping_json,
                    (SELECT status FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT checked_at FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT detail FROM provider_health WHERE provider_id = upstreams.id)
             FROM upstreams
             WHERE COALESCE(enabled, 1) = 1
               AND COALESCE(provider_kind, 'standard') != 'smart_gateway'
             ORDER BY sort_index ASC, created_at ASC;"
        }
        (false, false) => {
            "SELECT id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                    notes, sort_index, enabled, model_context_window, web_search_enabled,
                    auto_review_model_override, failover_group, failover_models, hidden_models_json,
                    thinking_config_json, custom_headers_json, created_at,
                    (SELECT status FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT checked_at FROM provider_health WHERE provider_id = upstreams.id),
                    (SELECT detail FROM provider_health WHERE provider_id = upstreams.id)
             FROM upstreams
             WHERE COALESCE(enabled, 1) = 1
               AND COALESCE(provider_kind, 'standard') != 'smart_gateway'
             ORDER BY sort_index ASC, created_at ASC;"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| row_to_upstream_provider(row, has_model_mapping))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn row_to_upstream_provider(
    row: &rusqlite::Row<'_>,
    has_model_mapping: bool,
) -> rusqlite::Result<Provider> {
    let protocol = ProtocolType::from_str_lossy(&row.get::<_, String>(5)?);
    let kind = ProviderKind::from_str_lossy(&row.get::<_, String>(6)?);
    let failover_models = parse_string_list(row.get::<_, String>(15).unwrap_or_else(|_| "[]".into()));
    let hidden_models = parse_string_list(row.get::<_, String>(16).unwrap_or_else(|_| "[]".into()));
    let thinking_config_json: String = row.get(17).unwrap_or_else(|_| "{}".into());
    let thinking_config = serde_json::from_str(&thinking_config_json)
        .ok()
        .filter(|cfg: &crate::provider::ThinkingConfig| !cfg.is_empty());
    let custom_headers_json: String = row.get(18).unwrap_or_else(|_| "{}".into());
    let custom_headers = serde_json::from_str(&custom_headers_json).ok();
    let window: Option<i64> = row.get(11)?;
    let web_search: Option<i64> = row.get(12)?;
    let api_key: String = row.get(3)?;
    let model_mapping = if has_model_mapping {
        let mapping_json: String = row.get(20).unwrap_or_else(|_| "{}".into());
        serde_json::from_str(&mapping_json).unwrap_or_default()
    } else {
        crate::provider::ClaudeModelMapping::default()
    };
    let (health_status, health_checked_at, health_detail) = if has_model_mapping {
        (
            row.get::<_, Option<String>>(21)?,
            row.get::<_, Option<i64>>(22)?,
            row.get::<_, Option<String>>(23)?,
        )
    } else {
        (
            row.get::<_, Option<String>>(20)?,
            row.get::<_, Option<i64>>(21)?,
            row.get::<_, Option<String>>(22)?,
        )
    };
    let health_latency_ms = parse_health_latency_ms(health_detail.as_deref());
    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        api_key_set: !api_key.trim().is_empty(),
        api_key,
        model: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        protocol_type: protocol,
        target_app: ProviderTarget::ClaudeCode,
        notes: row.get(8)?,
        created_at: row.get(19)?,
        sort_index: row.get(9)?,
        is_current: false,
        model_mapping,
        model_context_window: window.and_then(|value| u64::try_from(value).ok()),
        web_search_enabled: web_search.map(|value| value != 0),
        auto_review_model_override: row.get(13)?,
        provider_kind: kind,
        auth_binding: row.get(7)?,
        failover_group: row.get(14).unwrap_or(0),
        failover_models,
        hidden_models,
        thinking_config,
        custom_headers,
        health_status,
        health_checked_at,
        health_latency_ms,
    })
}

fn next_upstream_sort_index(conn: &Connection) -> AppResult<i64> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM upstreams;",
        [],
        |row| row.get(0),
    )?;
    Ok(next)
}

pub fn get_upstream_provider(conn: &Connection, id: &str) -> AppResult<Option<Provider>> {
    Ok(list_upstream_providers(conn, true)?
        .into_iter()
        .find(|provider| provider.id == id))
}

pub fn provider_from_upstream(
    conn: &Connection,
    id: &str,
    target: ProviderTarget,
) -> AppResult<Provider> {
    let upstream = get_upstream_provider(conn, id)?
        .ok_or_else(|| AppError::Config(format!("上游不存在: {id}")))?;
    let mut provider = upstream;
    provider.id = id.to_string();
    provider.target_app = target;
    if !provider.api_key.trim().is_empty() && !secrets::is_keyring_ref(&provider.api_key) {
        provider.api_key = secrets::keyring_ref(id);
    }
    provider.api_key_set = !provider.api_key.trim().is_empty();
    provider.is_current = true;
    Ok(provider)
}

pub fn upsert_upstream(conn: &Connection, input: &ProviderInput) -> AppResult<Provider> {
    crate::gateway::assert_not_managed_gateway_kind(input.provider_kind)?;
    if input.provider_kind == ProviderKind::SmartGateway {
        return Err(AppError::Config("托管 Auto 卡不能加入上游池".to_string()));
    }
    if input.name.trim().is_empty() {
        return Err(AppError::Config("上游名称不能为空".to_string()));
    }
    if input.base_url.trim().is_empty() {
        return Err(AppError::Config("API 地址不能为空".to_string()));
    }
    if input.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空".to_string()));
    }
    let protocol_type = if input.provider_kind == ProviderKind::CodexOauth {
        ProtocolType::OpenAiResponses
    } else {
        input.protocol_type
    };
    let norm_target = match protocol_type {
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses => ProviderTarget::Codex,
        _ => ProviderTarget::ClaudeCode,
    };
    let base_url = if input.provider_kind == ProviderKind::CodexOauth {
        crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string()
    } else {
        crate::provider::normalize_provider_base_url(norm_target, protocol_type, &input.base_url)?
    };
    crate::gateway::assert_not_self_referential(&base_url)?;
    let now = chrono::Utc::now().timestamp_millis();
    let existing = input
        .id
        .as_ref()
        .map(|id| get_upstream_provider(conn, id))
        .transpose()?
        .flatten();
    let id = if let Some(existing) = existing.as_ref() {
        existing.id.clone()
    } else {
        input
            .id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("up_{}", Uuid::new_v4().simple()))
    };
    let api_key_col = if input.provider_kind == ProviderKind::CodexOauth {
        String::new()
    } else if input.clear_api_key {
        String::new()
    } else if !input.api_key.trim().is_empty() {
        secrets::store_key(&id, input.api_key.trim())?;
        secrets::keyring_ref(&id)
    } else if let Some(existing) = existing.as_ref() {
        existing.api_key.clone()
    } else {
        String::new()
    };
    if input.clear_api_key {
        let _ = secrets::delete_key(&id);
    }
    let sort_index = existing
        .as_ref()
        .map(|provider| provider.sort_index)
        .unwrap_or(next_upstream_sort_index(conn)?);
    let created_at = existing.as_ref().map(|provider| provider.created_at).unwrap_or(now);
    let thinking_config_json = serde_json::to_string(
        input
            .thinking_config
            .as_ref()
            .filter(|cfg| !cfg.is_empty())
            .unwrap_or(&crate::provider::ThinkingConfig::default()),
    )?;
    let custom_headers_json = serde_json::to_string(
        input
            .custom_headers
            .as_ref()
            .filter(|headers| !headers.is_empty())
            .unwrap_or(&std::collections::HashMap::new()),
    )?;
    let model_mapping_json = serde_json::to_string(&input.model_mapping)?;
    let has_model_mapping = upstreams_have_model_mapping(conn);

    if has_model_mapping {
        conn.execute(
            "INSERT INTO upstreams (
                id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                notes, sort_index, enabled, model_context_window, web_search_enabled,
                auto_review_model_override, failover_group, failover_models, hidden_models_json,
                thinking_config_json, custom_headers_json, created_at, model_mapping_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                base_url = excluded.base_url,
                api_key = excluded.api_key,
                model = excluded.model,
                protocol_type = excluded.protocol_type,
                provider_kind = excluded.provider_kind,
                auth_binding = excluded.auth_binding,
                notes = excluded.notes,
                model_context_window = excluded.model_context_window,
                web_search_enabled = excluded.web_search_enabled,
                auto_review_model_override = excluded.auto_review_model_override,
                failover_group = excluded.failover_group,
                failover_models = excluded.failover_models,
                hidden_models_json = excluded.hidden_models_json,
                thinking_config_json = excluded.thinking_config_json,
                custom_headers_json = excluded.custom_headers_json,
                model_mapping_json = excluded.model_mapping_json;",
            params![
                id,
                input.name.trim(),
                base_url,
                api_key_col,
                input.model.trim(),
                protocol_type.as_str(),
                input.provider_kind.as_str(),
                input.auth_binding.trim(),
                input.notes,
                sort_index,
                input.model_context_window.map(|value| value as i64),
                input.web_search_enabled.map(|value| if value { 1 } else { 0 }),
                input.auto_review_model_override,
                input.failover_group,
                serde_json::to_string(&input.failover_models)?,
                serde_json::to_string(&input.hidden_models)?,
                thinking_config_json,
                custom_headers_json,
                created_at,
                model_mapping_json,
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO upstreams (
                id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding,
                notes, sort_index, enabled, model_context_window, web_search_enabled,
                auto_review_model_override, failover_group, failover_models, hidden_models_json,
                thinking_config_json, custom_headers_json, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                base_url = excluded.base_url,
                api_key = excluded.api_key,
                model = excluded.model,
                protocol_type = excluded.protocol_type,
                provider_kind = excluded.provider_kind,
                auth_binding = excluded.auth_binding,
                notes = excluded.notes,
                model_context_window = excluded.model_context_window,
                web_search_enabled = excluded.web_search_enabled,
                auto_review_model_override = excluded.auto_review_model_override,
                failover_group = excluded.failover_group,
                failover_models = excluded.failover_models,
                hidden_models_json = excluded.hidden_models_json,
                thinking_config_json = excluded.thinking_config_json,
                custom_headers_json = excluded.custom_headers_json;",
            params![
                id,
                input.name.trim(),
                base_url,
                api_key_col,
                input.model.trim(),
                protocol_type.as_str(),
                input.provider_kind.as_str(),
                input.auth_binding.trim(),
                input.notes,
                sort_index,
                input.model_context_window.map(|value| value as i64),
                input.web_search_enabled.map(|value| if value { 1 } else { 0 }),
                input.auto_review_model_override,
                input.failover_group,
                serde_json::to_string(&input.failover_models)?,
                serde_json::to_string(&input.hidden_models)?,
                thinking_config_json,
                custom_headers_json,
                created_at,
            ],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO gateway_id_map (old_provider_id, upstream_id) VALUES (?, ?);",
        params![id, id],
    )?;
    get_upstream_provider(conn, &id)?.ok_or_else(|| AppError::Config("写入上游后无法读取".to_string()))
}

pub fn delete_upstream(conn: &Connection, id: &str) -> AppResult<()> {
    let Some(upstream) = get_upstream_provider(conn, id)? else {
        return Err(AppError::Config(format!("上游不存在: {id}")));
    };
    if upstream.is_smart_gateway() {
        return Err(AppError::Config("不能删除智能网关 Auto 卡".to_string()));
    }
    assert_upstream_deletable(conn, id)?;
    delete_upstream_mirror(conn, id)?;
    if get_provider_row_exists(conn, id)? {
        // Keep the provider card; only drop it from the shared pool.
        return Ok(());
    }
    let _ = secrets::delete_key(id);
    Ok(())
}

fn get_provider_row_exists(conn: &Connection, id: &str) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM providers WHERE id = ?;",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn normalize_upstream_url_for_dedup(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if let Ok(mut parsed) = url::Url::parse(trimmed) {
        let path = parsed.path().trim_end_matches('/').to_string();
        parsed.set_path(&path);
        parsed.to_string().trim_end_matches('/').to_string()
    } else if let Some((scheme, rest)) = trimmed.split_once("://") {
        let scheme_lower = scheme.to_ascii_lowercase();
        if let Some((host_port, path)) = rest.split_once('/') {
            format!("{scheme_lower}://{}/{path}", host_port.to_ascii_lowercase())
        } else {
            format!("{scheme_lower}://{}", rest.to_ascii_lowercase())
        }
    } else {
        trimmed.to_string()
    }
}

pub fn upstream_endpoint_key(base_url: &str, protocol: ProtocolType) -> String {
    format!(
        "{}|{}",
        normalize_upstream_url_for_dedup(base_url),
        protocol.as_str()
    )
}

pub fn upstream_identity_dedup_key(
    base_url: &str,
    protocol: ProtocolType,
    resolved_key: &str,
    provider_kind: ProviderKind,
    auth_binding: &str,
) -> String {
    let norm_url = normalize_upstream_url_for_dedup(base_url);
    let mut hasher = Sha256::new();
    hasher.update(resolved_key.trim().as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());
    format!(
        "{}|{}|{}|{}|{}",
        norm_url,
        protocol.as_str(),
        key_hash,
        provider_kind.as_str(),
        auth_binding.trim()
    )
}

pub fn list_upstream_models(conn: &Connection, upstream_id: &str) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    let mut stmt = conn.prepare(
        "SELECT model_id, visible, COALESCE(display_name, ''), context_window, max_output_tokens,
                COALESCE(reasoning_levels_json, '[]'), COALESCE(capabilities_json, '{}')
         FROM upstream_models WHERE upstream_id = ? ORDER BY model_id COLLATE NOCASE;",
    )?;
    let rows = stmt.query_map(params![upstream_id], |row| {
        let visible: i64 = row.get(1)?;
        let reasoning: String = row.get(5)?;
        let capabilities: String = row.get(6)?;
        Ok(GatewayUpstreamModelRow {
            model_id: row.get(0)?,
            visible: visible != 0,
            display_name: row.get(2)?,
            context_window: row.get(3)?,
            max_output_tokens: row.get(4)?,
            reasoning_levels: serde_json::from_str(&reasoning).unwrap_or_default(),
            capabilities: serde_json::from_str(&capabilities).unwrap_or_else(|_| serde_json::json!({})),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn list_visible_upstream_model_ids(conn: &Connection, upstream_id: &str) -> AppResult<Vec<String>> {
    Ok(list_upstream_models(conn, upstream_id)?
        .into_iter()
        .filter(|row| row.visible)
        .map(|row| row.model_id)
        .collect())
}

fn sync_hidden_models_from_rows(conn: &Connection, upstream_id: &str) -> AppResult<()> {
    let hidden: Vec<String> = list_upstream_models(conn, upstream_id)?
        .into_iter()
        .filter(|row| !row.visible)
        .map(|row| row.model_id)
        .collect();
    conn.execute(
        "UPDATE upstreams SET hidden_models_json = ? WHERE id = ?;",
        params![serde_json::to_string(&hidden)?, upstream_id],
    )?;
    Ok(())
}

pub fn replace_upstream_models(conn: &Connection, upstream_id: &str, models: &[String]) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    let upstream = get_upstream_provider(conn, upstream_id)?
        .ok_or_else(|| AppError::Config(format!("上游不存在: {upstream_id}")))?;
    let existing: std::collections::HashMap<String, bool> = list_upstream_models(conn, upstream_id)?
        .into_iter()
        .map(|row| (row.model_id.to_ascii_lowercase(), row.visible))
        .collect();
    let hidden: std::collections::HashSet<String> = upstream
        .hidden_models
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect();
    let default_key = upstream.model.trim().to_ascii_lowercase();
    conn.execute(
        "DELETE FROM upstream_models WHERE upstream_id = ?;",
        params![upstream_id],
    )?;
    let mut seen = std::collections::HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    if !upstream.model.trim().is_empty() {
        ordered.push(upstream.model.trim().to_string());
        seen.insert(default_key.clone());
    }
    for model in models {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        ordered.push(trimmed.to_string());
    }
    for model in ordered {
        let key = model.to_ascii_lowercase();
        let visible = key == default_key
            || existing.get(&key).copied().unwrap_or(!hidden.contains(&key));
        let inferred = crate::gateway::metadata::infer(&model);
        let row_id = format!("{upstream_id}:{model}");
        conn.execute(
            "INSERT INTO upstream_models (id, upstream_id, model_id, verified_status, visible, display_name, context_window, max_output_tokens, reasoning_levels_json, capabilities_json)
             VALUES (?, ?, ?, 'declared', ?, ?, ?, ?, ?, ?);",
            params![
                row_id,
                upstream_id,
                model,
                if visible { 1 } else { 0 },
                inferred.display_name,
                inferred.context_window,
                inferred.max_output_tokens,
                serde_json::to_string(&inferred.reasoning_levels).unwrap_or_else(|_| "[]".into()),
                inferred.capabilities.to_string()
            ],
        )?;
    }
    sync_hidden_models_from_rows(conn, upstream_id)?;
    list_upstream_models(conn, upstream_id)
}

pub fn set_upstream_model_visible(
    conn: &Connection,
    upstream_id: &str,
    model_id: &str,
    visible: bool,
) -> AppResult<Vec<GatewayUpstreamModelRow>> {
    let upstream = get_upstream_provider(conn, upstream_id)?
        .ok_or_else(|| AppError::Config(format!("上游不存在: {upstream_id}")))?;
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(AppError::Config("模型 id 不能为空".to_string()));
    }
    if !visible && model_id.eq_ignore_ascii_case(upstream.model.trim()) {
        return Err(AppError::Config("默认模型不能关闭".to_string()));
    }
    let row_id = format!("{upstream_id}:{model_id}");
    let updated = conn.execute(
        "UPDATE upstream_models SET visible = ? WHERE upstream_id = ? AND lower(model_id) = lower(?);",
        params![if visible { 1 } else { 0 }, upstream_id, model_id],
    )?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO upstream_models (id, upstream_id, model_id, verified_status, visible)
             VALUES (?, ?, ?, 'declared', ?);",
            params![row_id, upstream_id, model_id, if visible { 1 } else { 0 }],
        )?;
    }
    sync_hidden_models_from_rows(conn, upstream_id)?;
    list_upstream_models(conn, upstream_id)
}

fn append_upstream_to_restricted_allowlist(
    conn: &Connection,
    target: ProviderTarget,
    upstream_id: &str,
) -> AppResult<()> {
    let profile_id = default_profile_id(target.as_str());
    let Some(profile) = get_profile(conn, &profile_id)? else {
        return Ok(());
    };
    if profile.allowed_upstream_ids.is_empty() {
        return Ok(());
    }
    add_upstream_to_default_allowlist(conn, target.as_str(), upstream_id)
}

pub fn import_providers_as_upstreams(
    conn: &Connection,
    source_target: ProviderTarget,
    provider_ids: &[String],
    add_to_allowlist_target: Option<ProviderTarget>,
) -> AppResult<GatewayUpstreamImportResult> {
    let existing = list_upstream_providers(conn, true)?;
    let mut existing_keys: std::collections::HashSet<String> = existing
        .iter()
        .map(|item| upstream_endpoint_key(&item.base_url, item.protocol_type))
        .collect();
    let mut items = Vec::new();
    let mut imported = 0i64;
    let mut skipped = 0i64;
    for raw_id in provider_ids {
        let provider_id = raw_id.trim();
        if provider_id.is_empty() {
            continue;
        }
        let Some(provider) = super::providers::get_provider(conn, provider_id)? else {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider_id.to_string(),
                name: provider_id.to_string(),
                upstream_id: None,
                skipped: true,
                reason: "missing".to_string(),
            });
            continue;
        };
        if provider.target_app != source_target {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id,
                name: provider.name,
                upstream_id: None,
                skipped: true,
                reason: "wrong_target".to_string(),
            });
            continue;
        }
        if provider.is_smart_gateway() {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id,
                name: provider.name,
                upstream_id: None,
                skipped: true,
                reason: "smart_gateway".to_string(),
            });
            continue;
        }
        if crate::gateway::is_self_referential_upstream(&provider.base_url, &crate::gateway::default_gateway_ports()) {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id,
                name: provider.name,
                upstream_id: None,
                skipped: true,
                reason: "self_ref".to_string(),
            });
            continue;
        }
        let key = upstream_endpoint_key(&provider.base_url, provider.protocol_type);
        if existing_keys.contains(&key) {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id,
                name: provider.name,
                upstream_id: None,
                skipped: true,
                reason: "duplicate".to_string(),
            });
            continue;
        }
        let api_key = super::providers::resolve_api_key(conn, &provider.id)?.unwrap_or_default();
        let model = if provider.model.trim().is_empty() {
            super::providers::get_provider_model_cache(conn, &provider.id)?
                .and_then(|cache| cache.models.into_iter().find(|id| !id.trim().is_empty()))
                .unwrap_or_else(|| "default".to_string())
        } else {
            provider.model.clone()
        };
        let input = ProviderInput {
            id: None,
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key,
            clear_api_key: false,
            model,
            model_context_window: provider.model_context_window,
            auto_review_model_override: provider.auto_review_model_override.clone(),
            web_search_enabled: provider.web_search_enabled,
            model_mapping: provider.model_mapping.clone(),
            protocol_type: provider.protocol_type,
            provider_kind: provider.provider_kind,
            auth_binding: provider.auth_binding.clone(),
            target_app: ProviderTarget::ClaudeCode,
            notes: provider.notes.clone(),
            failover_group: provider.failover_group,
            failover_models: provider.failover_models.clone(),
            hidden_models: provider.hidden_models.clone(),
            thinking_config: provider.thinking_config.clone(),
            custom_headers: provider.custom_headers.clone(),
        };
        let created = match upsert_upstream(conn, &input) {
            Ok(created) => created,
            Err(error) => {
                skipped += 1;
                items.push(GatewayUpstreamImportItem {
                    provider_id: provider.id,
                    name: provider.name,
                    upstream_id: None,
                    skipped: true,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        conn.execute(
            "INSERT OR IGNORE INTO gateway_id_map (old_provider_id, upstream_id) VALUES (?, ?);",
            params![provider.id, created.id],
        )?;
        let cached = super::providers::get_provider_model_cache(conn, &provider.id)?
            .map(|cache| cache.models)
            .unwrap_or_default();
        let _ = replace_upstream_models(conn, &created.id, &cached);
        if let Some(target) = add_to_allowlist_target {
            let _ = append_upstream_to_restricted_allowlist(conn, target, &created.id);
        }
        existing_keys.insert(key);
        imported += 1;
        items.push(GatewayUpstreamImportItem {
            provider_id: provider.id,
            name: created.name.clone(),
            upstream_id: Some(created.id),
            skipped: false,
            reason: String::new(),
        });
    }
    Ok(GatewayUpstreamImportResult {
        imported,
        skipped,
        items,
    })
}

pub fn merge_provider_models_into_upstream(
    conn: &Connection,
    upstream_id: &str,
    provider: &Provider,
) -> AppResult<()> {
    let mut candidate_models = Vec::new();
    if !provider.model.trim().is_empty() {
        candidate_models.push(provider.model.trim().to_string());
    }
    for role_model in [
        &provider.model_mapping.sonnet,
        &provider.model_mapping.opus,
        &provider.model_mapping.haiku,
        &provider.model_mapping.fable,
        &provider.model_mapping.subagent,
    ] {
        if !role_model.trim().is_empty() {
            candidate_models.push(role_model.trim().to_string());
        }
    }
    if let Some(ref auto_rev) = provider.auto_review_model_override {
        if !auto_rev.trim().is_empty() {
            candidate_models.push(auto_rev.trim().to_string());
        }
    }
    for m in &provider.failover_models {
        if !m.trim().is_empty() {
            candidate_models.push(m.trim().to_string());
        }
    }
    for m in &provider.hidden_models {
        if !m.trim().is_empty() {
            candidate_models.push(m.trim().to_string());
        }
    }
    if let Some(cache) = super::providers::get_provider_model_cache(conn, &provider.id)? {
        for m in cache.models {
            if !m.trim().is_empty() {
                candidate_models.push(m.trim().to_string());
            }
        }
    }
    let existing_models = list_upstream_models(conn, upstream_id)?;
    for row in &existing_models {
        if !row.model_id.trim().is_empty() {
            candidate_models.push(row.model_id.clone());
        }
    }
    replace_upstream_models(conn, upstream_id, &candidate_models)?;
    // 合并身份相同的旧卡时保留已探测或用户设置的模型元数据。
    for row in existing_models {
        conn.execute(
            "UPDATE upstream_models SET display_name = ?, context_window = ?, max_output_tokens = ?,
                reasoning_levels_json = ?, capabilities_json = ?, visible = ?
             WHERE upstream_id = ? AND model_id = ?;",
            params![row.display_name, row.context_window, row.max_output_tokens,
                serde_json::to_string(&row.reasoning_levels)?, row.capabilities.to_string(),
                i64::from(row.visible), upstream_id, row.model_id],
        )?;
    }
    for hidden in &provider.hidden_models {
        conn.execute(
            "UPDATE upstream_models SET visible = 0 WHERE upstream_id = ?
             AND model_id = ? COLLATE NOCASE
             AND model_id != (SELECT model FROM upstreams WHERE id = ?) COLLATE NOCASE;",
            params![upstream_id, hidden, upstream_id],
        )?;
    }
    sync_hidden_models_from_rows(conn, upstream_id)?;
    Ok(())
}

fn migration_api_key(stored: &str) -> AppResult<String> {
    match super::providers::materialize_api_key(stored)? {
        Some(key) => Ok(key),
        None if stored.trim().is_empty() => Ok(String::new()),
        None => Err(AppError::Config(
            "系统凭据库中缺少旧供应商或上游的 API Key，请恢复凭据后重试迁移".into(),
        )),
    }
}

pub fn import_all_providers_as_upstreams(
    conn: &Connection,
) -> AppResult<GatewayUpstreamImportResult> {
    if !table_exists(conn, "providers") {
        return Ok(GatewayUpstreamImportResult {
            imported: 0,
            skipped: 0,
            items: Vec::new(),
        });
    }

    let existing = list_upstream_providers(conn, true)?;
    let mut existing_names: std::collections::HashSet<String> =
        existing.iter().map(|item| item.name.clone()).collect();
    let mut existing_dedup_keys: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for item in &existing {
        let actual_key = migration_api_key(&item.api_key)?;
        let key = upstream_identity_dedup_key(
            &item.base_url,
            item.protocol_type,
            &actual_key,
            item.provider_kind,
            &item.auth_binding,
        );
        existing_dedup_keys.insert(key, item.id.clone());
    }

    if table_exists(conn, "gateway_id_map") {
        let mut stmt = conn.prepare("SELECT old_provider_id, upstream_id FROM gateway_id_map;")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows {
            let (_old_id, up_id) = row?;
            if let Some(up) = existing.iter().find(|u| u.id == up_id) {
                let actual_key = migration_api_key(&up.api_key)?;
                let key = upstream_identity_dedup_key(
                    &up.base_url,
                    up.protocol_type,
                    &actual_key,
                    up.provider_kind,
                    &up.auth_binding,
                );
                existing_dedup_keys.entry(key).or_insert(up_id);
            }
        }
    }

    let mut stmt = conn.prepare(
        "SELECT id, target_app FROM providers ORDER BY sort_index ASC, created_at ASC;",
    )?;
    let provider_refs: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut items = Vec::new();
    let mut imported = 0i64;
    let mut skipped = 0i64;

    for (provider_id, target_app_str) in provider_refs {
        let target = ProviderTarget::from_str_lossy(&target_app_str);

        // 1. T3 agents (Claude Desktop, DSH) do NOT migrate
        if matches!(target, ProviderTarget::ClaudeDesktop | ProviderTarget::Dsh) {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider_id.clone(),
                name: provider_id.clone(),
                upstream_id: None,
                skipped: true,
                reason: "t3_skipped".to_string(),
            });
            continue;
        }

        let Some(provider) = super::providers::get_provider(conn, &provider_id)? else {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider_id.clone(),
                name: provider_id.clone(),
                upstream_id: None,
                skipped: true,
                reason: "missing".to_string(),
            });
            continue;
        };

        // 2. Skip smart_gateway
        if provider.is_smart_gateway() {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id.clone(),
                name: provider.name.clone(),
                upstream_id: None,
                skipped: true,
                reason: "smart_gateway".to_string(),
            });
            continue;
        }

        // 3. Skip self-referential
        if crate::gateway::is_self_referential_upstream(
            &provider.base_url,
            &crate::gateway::default_gateway_ports(),
        ) {
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id.clone(),
                name: provider.name.clone(),
                upstream_id: None,
                skipped: true,
                reason: "self_ref".to_string(),
            });
            continue;
        }

        let api_key = migration_api_key(&provider.api_key)?;

        let dedup_key = upstream_identity_dedup_key(
            &provider.base_url,
            provider.protocol_type,
            &api_key,
            provider.provider_kind,
            &provider.auth_binding,
        );

        // 4. Check if duplicate of existing upstream
        if let Some(target_upstream_id) = existing_dedup_keys.get(&dedup_key) {
            conn.execute(
                "INSERT OR REPLACE INTO gateway_id_map (old_provider_id, upstream_id) VALUES (?, ?);",
                params![provider.id, target_upstream_id],
            )?;
            merge_provider_models_into_upstream(conn, target_upstream_id, &provider)?;
            skipped += 1;
            items.push(GatewayUpstreamImportItem {
                provider_id: provider.id.clone(),
                name: provider.name.clone(),
                upstream_id: Some(target_upstream_id.clone()),
                skipped: true,
                reason: "duplicate".to_string(),
            });
            continue;
        }

        // 5. Unique name generation (append #2, #3 if name exists)
        let mut final_name = provider.name.clone();
        if existing_names.contains(&final_name) {
            let mut count = 2;
            while existing_names.contains(&format!("{final_name} #{count}")) {
                count += 1;
            }
            final_name = format!("{final_name} #{count}");
        }

        let model = if provider.model.trim().is_empty() {
            super::providers::get_provider_model_cache(conn, &provider.id)?
                .and_then(|cache| cache.models.into_iter().find(|id| !id.trim().is_empty()))
                .unwrap_or_else(|| "default".to_string())
        } else {
            provider.model.clone()
        };

        let input = ProviderInput {
            id: None,
            name: final_name.clone(),
            base_url: provider.base_url.clone(),
            api_key: api_key.clone(),
            clear_api_key: false,
            model,
            model_context_window: provider.model_context_window,
            auto_review_model_override: provider.auto_review_model_override.clone(),
            web_search_enabled: provider.web_search_enabled,
            model_mapping: provider.model_mapping.clone(),
            protocol_type: provider.protocol_type,
            provider_kind: provider.provider_kind,
            auth_binding: provider.auth_binding.clone(),
            target_app: ProviderTarget::ClaudeCode,
            notes: provider.notes.clone(),
            failover_group: provider.failover_group,
            failover_models: provider.failover_models.clone(),
            hidden_models: provider.hidden_models.clone(),
            thinking_config: provider.thinking_config.clone(),
            custom_headers: provider.custom_headers.clone(),
        };

        let created = upsert_upstream(conn, &input)?;

        conn.execute(
            "INSERT OR REPLACE INTO gateway_id_map (old_provider_id, upstream_id) VALUES (?, ?);",
            params![provider.id, created.id],
        )?;

        merge_provider_models_into_upstream(conn, &created.id, &provider)?;

        existing_names.insert(final_name);
        existing_dedup_keys.insert(dedup_key, created.id.clone());
        imported += 1;
        items.push(GatewayUpstreamImportItem {
            provider_id: provider.id.clone(),
            name: created.name.clone(),
            upstream_id: Some(created.id),
            skipped: false,
            reason: String::new(),
        });
    }

    Ok(GatewayUpstreamImportResult {
        imported,
        skipped,
        items,
    })
}

pub const MIGRATION_DONE_MARKER: &str = "__mig_v34_done__";
pub const SNAPSHOT_PREFIX: &str = "__snap_";

pub const MIGRATION_TARGETS: [ProviderTarget; 5] = [
    ProviderTarget::ClaudeCode,
    ProviderTarget::Codex,
    ProviderTarget::OpenCode,
    ProviderTarget::Pi,
    ProviderTarget::Cline,
];

pub fn is_v34_migration_done(conn: &Connection) -> bool {
    if !table_exists(conn, "upstream_migration_v34") {
        return false;
    }
    conn.query_row(
        "SELECT count(*) FROM upstream_migration_v34 WHERE old_provider_id = ?;",
        params![MIGRATION_DONE_MARKER],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0) > 0
}

pub fn migrate_v33_to_v34(conn: &Connection) -> AppResult<()> {
    if is_v34_migration_done(conn) {
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS upstream_migration_v34 (
            old_provider_id TEXT PRIMARY KEY,
            target_app TEXT NOT NULL,
            upstream_id TEXT NOT NULL,
            was_current INTEGER NOT NULL DEFAULT 0,
            prev_binding_json TEXT NOT NULL DEFAULT '',
            newly_created INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    let now = chrono::Utc::now().timestamp_millis();
    let tx = conn.unchecked_transaction()?;

    // 保存全部 T1/T2 原绑定，再执行迁移。
    if table_exists(&tx, "upstream_migration_v34") {
        for target in MIGRATION_TARGETS {
            let snap_json = if let Some(b) = binding_for_target(&tx, target)? {
                let snap = BindingSnapshot::from_binding(&b);
                serde_json::to_string(&snap)
                    .map_err(|e| AppError::Config(format!("序列化绑定快照失败: {e}")))?
            } else {
                String::new()
            };
            let row_id = format!("{SNAPSHOT_PREFIX}{}", target.as_str());
            tx.execute(
                "INSERT OR REPLACE INTO upstream_migration_v34
                    (old_provider_id, target_app, upstream_id, was_current, prev_binding_json, newly_created)
                 VALUES (?, ?, '', 0, ?, 0);",
                params![row_id, target.as_str(), snap_json],
            )?;
        }
    }

    // 2. Import all legacy T1/T2 providers as upstreams
    let import_res = import_all_providers_as_upstreams(&tx)?;
    let newly_created_set: std::collections::HashSet<String> = import_res
        .items
        .iter()
        .filter(|item| !item.skipped && item.upstream_id.is_some())
        .filter_map(|item| item.upstream_id.clone())
        .collect();

    // 3. Record provider mapping & current state in upstream_migration_v34
    if table_exists(&tx, "upstream_migration_v34") && table_exists(&tx, "providers") {
        let mut stmt = tx.prepare(
            "SELECT id, target_app, is_current FROM providers;",
        )?;
        let prov_rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        })?;
        for prow in prov_rows {
            let (old_id, target_str, is_current) = prow?;
            let target = ProviderTarget::from_str_lossy(&target_str);
            if matches!(target, ProviderTarget::ClaudeDesktop | ProviderTarget::Dsh) {
                continue;
            }
            let mapped_up_id: String = tx.query_row(
                "SELECT upstream_id FROM gateway_id_map WHERE old_provider_id = ?;",
                params![old_id],
                |row| row.get(0),
            ).optional()?.unwrap_or_default();
            let newly_created = if !mapped_up_id.is_empty() && newly_created_set.contains(&mapped_up_id) { 1 } else { 0 };
            tx.execute(
                "INSERT OR REPLACE INTO upstream_migration_v34
                    (old_provider_id, target_app, upstream_id, was_current, prev_binding_json, newly_created)
                 VALUES (?, ?, ?, ?, '', ?);",
                params![old_id, target_str, mapped_up_id, if is_current { 1 } else { 0 }, newly_created],
            )?;
        }
    }

    // 4. Update bindings for T1 agents (ClaudeCode, Codex)
    for target in [ProviderTarget::ClaudeCode, ProviderTarget::Codex] {
        let current_prov: Option<(String, String)> = if table_exists(&tx, "providers") {
            tx.query_row(
                "SELECT id, COALESCE(provider_kind, '') FROM providers WHERE target_app = ? AND is_current = 1 LIMIT 1;",
                params![target.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?
        } else {
            None
        };

        match current_prov {
            Some((_id, kind)) if kind == "smart_gateway" => {
                // Auto is current -> mode = gateway, preserve existing token/profile/provider_id
                let existing = binding_for_target(&tx, target)?;
                match existing {
                    Some(_b) => {
                        tx.execute(
                            "UPDATE gateway_bindings SET mode = 'gateway', direct_upstream_id = '' WHERE target_app = ?;",
                            params![target.as_str()],
                        )?;
                    }
                    None => {
                        let token = format!("gwt_{}", Uuid::new_v4().simple());
                        let provider_id = format!("sgw_{}", target.as_str());
                        tx.execute(
                            "INSERT INTO gateway_bindings
                                (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                             VALUES (?, ?, ?, ?, ?, 'gateway', '');",
                            params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID],
                        )?;
                    }
                }
            }
            Some((id, _kind)) => {
                // Independent custom card is current -> mode = direct, preserve existing token/profile/provider_id
                let mapped_upstream: Option<String> = tx.query_row(
                    "SELECT upstream_id FROM gateway_id_map WHERE old_provider_id = ?;",
                    params![id],
                    |row| row.get(0),
                ).optional()?;

                if let Some(upstream_id) = mapped_upstream {
                    let existing = binding_for_target(&tx, target)?;
                    match existing {
                        Some(_b) => {
                            tx.execute(
                                "UPDATE gateway_bindings SET mode = 'direct', direct_upstream_id = ? WHERE target_app = ?;",
                                params![upstream_id, target.as_str()],
                            )?;
                        }
                        None => {
                            let token = format!("gwt_{}", Uuid::new_v4().simple());
                            let provider_id = format!("sgw_{}", target.as_str());
                            tx.execute(
                                "INSERT INTO gateway_bindings
                                    (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                                 VALUES (?, ?, ?, ?, ?, 'direct', ?);",
                                params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID, upstream_id],
                            )?;
                        }
                    }
                }
            }
            None => {
                // Official login -> do not create binding row; remove existing if any ("官方不绑")
                tx.execute(
                    "DELETE FROM gateway_bindings WHERE target_app = ?;",
                    params![target.as_str()],
                )?;
            }
        }
    }

    // 5. Update bindings for T2 agents (OpenCode, Pi, Cline)
    for target in [ProviderTarget::OpenCode, ProviderTarget::Pi, ProviderTarget::Cline] {
        let existing = binding_for_target(&tx, target)?;
        match existing {
            Some(b) => {
                let mode = if b.mode.trim().is_empty() {
                    BINDING_MODE_GATEWAY.to_string()
                } else {
                    b.mode
                };
                tx.execute(
                    "UPDATE gateway_bindings SET mode = ?, direct_upstream_id = ? WHERE target_app = ?;",
                    params![mode, b.direct_upstream_id, target.as_str()],
                )?;
            }
            None => {
                let token = format!("gwt_{}", Uuid::new_v4().simple());
                let provider_id = format!("sgw_{}", target.as_str());
                tx.execute(
                    "INSERT INTO gateway_bindings
                        (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                     VALUES (?, ?, ?, ?, ?, 'gateway', '');",
                    params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID],
                )?;
            }
        }
    }

    // 6. Record completion marker
    if table_exists(&tx, "upstream_migration_v34") {
        tx.execute(
            "INSERT OR REPLACE INTO upstream_migration_v34
                (old_provider_id, target_app, upstream_id, was_current, prev_binding_json, newly_created)
             VALUES (?, 'global', '', 0, '', 0);",
            params![MIGRATION_DONE_MARKER],
        )?;
    }

    tx.commit()?;
    Ok(())
}

pub fn rollback_v34(conn: &Connection) -> AppResult<()> {
    if !is_v34_migration_done(conn) {
        return Err(AppError::Config("缺少完整的 Schema 34 迁移快照，已停止回滚".into()));
    }
    let tx = conn.unchecked_transaction()?;

    // 1. Restore all T1/T2 bindings from dedicated snapshot rows
    for target in MIGRATION_TARGETS {
        let row_id = format!("{SNAPSHOT_PREFIX}{}", target.as_str());
        let snap_row: Option<String> = match tx.query_row(
            "SELECT prev_binding_json FROM upstream_migration_v34 WHERE old_provider_id = ?;",
            params![row_id],
            |row| row.get(0),
        ) {
            Ok(val) => Some(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(AppError::Config(format!("缺少 {} 原绑定快照，已停止回滚", target.as_str())));
            }
            Err(err) => return Err(err.into()),
        };

        if let Some(snap_json) = snap_row {
            let trimmed = snap_json.trim();
            if !trimmed.is_empty() {
                let snap: BindingSnapshot = serde_json::from_str(trimmed)
                    .map_err(|e| AppError::Config(format!("解析绑定快照失败: {e}")))?;
                tx.execute(
                    "INSERT OR REPLACE INTO gateway_bindings
                        (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                     VALUES (?, ?, ?, ?, ?, ?, ?);",
                    params![
                        snap.target_app,
                        snap.entry_token,
                        snap.provider_id,
                        snap.created_at,
                        snap.profile_id,
                        snap.mode,
                        snap.direct_upstream_id,
                    ],
                )?;
                continue;
            }
        }
        // If snapshot was empty or not found, target had no binding before migration
        tx.execute(
            "DELETE FROM gateway_bindings WHERE target_app = ?;",
            params![target.as_str()],
        )?;
    }

    // 2. Restore providers.is_current
    let mut stmt = tx.prepare(
        "SELECT old_provider_id, target_app, was_current, upstream_id, newly_created
         FROM upstream_migration_v34
         WHERE old_provider_id != ? AND substr(old_provider_id, 1, 7) != ?;",
    )?;
    struct ProvMig {
        old_provider_id: String,
        target_app: String,
        was_current: bool,
        upstream_id: String,
        newly_created: bool,
    }
    let prov_rows: Vec<ProvMig> = stmt.query_map(params![MIGRATION_DONE_MARKER, SNAPSHOT_PREFIX], |row| {
        Ok(ProvMig {
            old_provider_id: row.get(0)?,
            target_app: row.get(1)?,
            was_current: row.get::<_, i64>(2)? != 0,
            upstream_id: row.get(3)?,
            newly_created: row.get::<_, i64>(4)? != 0,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 即使升级前没有旧卡，也要清除升级后生成的当前卡。
    for target in MIGRATION_TARGETS {
        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE target_app = ?;",
            params![target.as_str()],
        )?;
    }
    for row in &prov_rows {
        if row.was_current {
            tx.execute(
                "UPDATE providers SET is_current = 1 WHERE id = ?;",
                params![row.old_provider_id],
            )?;
        }
    }

    // 3. Remove upstreams that were newly created during v34 migration
    let newly_created_upstreams: std::collections::HashSet<String> = prov_rows
        .iter()
        .filter(|r| r.newly_created)
        .map(|r| r.upstream_id.clone())
        .collect();

    for up_id in newly_created_upstreams {
        tx.execute("DELETE FROM upstream_models WHERE upstream_id = ?;", params![up_id])?;
        tx.execute("DELETE FROM gateway_id_map WHERE upstream_id = ?;", params![up_id])?;
        tx.execute("DELETE FROM upstreams WHERE id = ?;", params![up_id])?;
        // 回滚也用于导出副本，不能删除运行库仍在引用的系统凭据。
    }

    // 4. Also clean up any gateway_id_map entries for old_provider_ids in migration
    for row in &prov_rows {
        tx.execute(
            "DELETE FROM gateway_id_map WHERE old_provider_id = ?;",
            params![row.old_provider_id],
        )?;
    }

    // 5. Drop migration table
    tx.execute("DROP TABLE IF EXISTS upstream_migration_v34;", [])?;

    crate::database::schema::set_user_version(&tx, 33)?;
    tx.commit()?;
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name = ?;",
        params![name],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn bindings_have_profile_id(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM pragma_table_info('gateway_bindings') WHERE name = 'profile_id';",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn bindings_have_v34(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM pragma_table_info('gateway_bindings') WHERE name = 'mode';",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn upstreams_have_model_mapping(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM pragma_table_info('upstreams') WHERE name = 'model_mapping_json';",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

pub const BINDING_MODE_GATEWAY: &str = "gateway";
pub const BINDING_MODE_DIRECT: &str = "direct";

fn default_binding_mode() -> String {
    BINDING_MODE_GATEWAY.to_string()
}

fn row_to_binding(
    row: &rusqlite::Row<'_>,
    has_profile: bool,
    has_v34: bool,
) -> rusqlite::Result<GatewayBinding> {
    let token: String = row.get(1)?;
    let profile_id = if has_profile {
        row.get::<_, String>(4)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| SHARED_PROFILE_ID.to_string())
    } else {
        SHARED_PROFILE_ID.to_string()
    };
    let (mode, direct_upstream_id) = if has_v34 {
        let m: String = row.get(5).unwrap_or_else(|_| BINDING_MODE_GATEWAY.to_string());
        let mode = if m.trim().is_empty() {
            BINDING_MODE_GATEWAY.to_string()
        } else {
            m
        };
        let du: String = row.get(6).unwrap_or_default();
        (mode, du)
    } else {
        (BINDING_MODE_GATEWAY.to_string(), String::new())
    };

    Ok(GatewayBinding {
        target_app: ProviderTarget::from_str_lossy(&row.get::<_, String>(0)?),
        entry_token_set: !token.trim().is_empty(),
        entry_token: token,
        provider_id: row.get(2)?,
        created_at: row.get(3)?,
        profile_id,
        mode,
        direct_upstream_id,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct GatewayBinding {
    pub target_app: ProviderTarget,
    #[cfg_attr(test, ts(skip))]
    pub entry_token: String,
    pub entry_token_set: bool,
    pub provider_id: String,
    pub profile_id: String,
    pub created_at: i64,
    #[serde(default = "default_binding_mode")]
    pub mode: String,
    #[serde(default)]
    pub direct_upstream_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSnapshot {
    pub target_app: String,
    pub entry_token: String,
    pub provider_id: String,
    pub created_at: i64,
    pub profile_id: String,
    pub mode: String,
    pub direct_upstream_id: String,
}

impl BindingSnapshot {
    pub fn from_binding(b: &GatewayBinding) -> Self {
        Self {
            target_app: b.target_app.as_str().to_string(),
            entry_token: b.entry_token.clone(),
            provider_id: b.provider_id.clone(),
            created_at: b.created_at,
            profile_id: b.profile_id.clone(),
            mode: b.mode.clone(),
            direct_upstream_id: b.direct_upstream_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct RouteMode {
    pub id: String,
    pub profile_id: String,
    pub enabled: bool,
    pub model: String,
    pub thinking_config_json: String,
    pub fallback_models: Vec<String>,
    pub threshold: i64,
    pub sort_index: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteModePatch {
    pub enabled: Option<bool>,
    pub model: Option<String>,
    pub thinking_config_json: Option<String>,
    pub fallback_models: Option<Vec<String>>,
    pub threshold: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct RouteRule {
    pub id: String,
    pub profile_id: String,
    pub enabled: bool,
    pub sort_index: i64,
    pub rule_type: String,
    pub condition_json: String,
    pub pattern: String,
    pub target_model: String,
    pub thinking_config_json: String,
    pub rewrites_json: String,
}

pub fn migrate_v30_to_v31(conn: &Connection) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    let profile = ensure_shared_profile(conn, now)?;
    seed_route_modes_from_profile(conn, &profile)?;
    migrate_bindings_from_connections(conn, &profile, now)?;
    rewrite_smart_gateway_cards_to_standalone(conn)?;
    if table_exists(conn, "agent_connections") {
        conn.execute_batch("DROP TABLE IF EXISTS agent_connections;")?;
    }
    Ok(())
}

pub(crate) fn seed_route_modes_from_profile(conn: &Connection, profile: &GatewayProfile) -> AppResult<()> {
    let long_context_threshold = if profile.long_context_tokens > 0 {
        profile.long_context_tokens
    } else {
        DEFAULT_LONG_CONTEXT_THRESHOLD
    };
    let seeds: [(&str, bool, &str, i64, i64); 9] = [
        ("default", true, profile.default_model.trim(), 0, 0),
        ("background", !profile.subagent_model.trim().is_empty(), profile.subagent_model.trim(), 0, 1),
        ("plan", false, "", 0, 2),
        ("think", false, "", 0, 3),
        ("edit", false, "", 0, 4),
        (
            "long_context",
            !profile.long_context_model.trim().is_empty(),
            profile.long_context_model.trim(),
            long_context_threshold,
            5,
        ),
        ("web_search", !profile.web_search_model.trim().is_empty(), profile.web_search_model.trim(), 0, 6),
        ("vision", false, "", 0, 7),
        ("image_gen", false, "", 0, 8),
    ];
    for (id, enabled, model, threshold, sort_index) in seeds {
        conn.execute(
            "INSERT OR IGNORE INTO route_modes
                (id, profile_id, enabled, model, thinking_config_json, fallback_models_json, threshold, sort_index)
             VALUES (?, ?, ?, ?, '{}', '[]', ?, ?);",
            params![
                id,
                profile.id,
                if enabled { 1 } else { 0 },
                model,
                threshold.max(0),
                sort_index
            ],
        )?;
    }
    repair_long_context_one_token_threshold(conn)?;
    Ok(())
}

pub(crate) fn repair_long_context_one_token_threshold(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "route_modes") {
        return Ok(());
    }
    conn.execute(
        "UPDATE route_modes SET threshold = ?
         WHERE id = 'long_context' AND threshold = 1;",
        params![DEFAULT_LONG_CONTEXT_THRESHOLD],
    )?;
    Ok(())
}

fn migrate_bindings_from_connections(
    conn: &Connection,
    profile: &GatewayProfile,
    now: i64,
) -> AppResult<()> {
    let mut targets: Vec<String> = Vec::new();
    if table_exists(conn, "agent_connections") {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT target_app FROM agent_connections
             WHERE connection_type = 'gateway' AND is_current = 1;",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            targets.push(row?);
        }
    }
    if table_exists(conn, "providers") {
        let has_kind: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'provider_kind';",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if has_kind > 0 {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT target_app FROM providers
                 WHERE COALESCE(provider_kind, '') = 'smart_gateway' AND is_current = 1;",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let target = row?;
                if !targets.iter().any(|item| item == &target) {
                    targets.push(target);
                }
            }
        }
    }
    for (index, target) in targets.into_iter().enumerate() {
        let token = if index == 0 && !profile.entry_token.trim().is_empty() {
            profile.entry_token.clone()
        } else {
            format!("gwt_{}", Uuid::new_v4().simple())
        };
        let provider_id = format!("sgw_{target}");
        conn.execute(
            "INSERT OR IGNORE INTO gateway_bindings (target_app, entry_token, provider_id, created_at)
             VALUES (?, ?, ?, ?);",
            params![target, token, provider_id, now],
        )?;
    }
    Ok(())
}

fn rewrite_smart_gateway_cards_to_standalone(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "providers") {
        return Ok(());
    }
    let has_kind: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'provider_kind';",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if has_kind == 0 {
        return Ok(());
    }
    let port = crate::gateway::SMART_GATEWAY_PORT;
    let mut stmt = conn.prepare(
        "SELECT id, target_app FROM providers WHERE COALESCE(provider_kind, '') = 'smart_gateway';",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, target_app) in rows {
        let target = ProviderTarget::from_str_lossy(&target_app);
        let (_, base_url) = crate::gateway::smart_gateway_live_endpoint(target, port);
        let token = binding_token(conn, target).unwrap_or_default();
        conn.execute(
            "UPDATE providers SET base_url = ?, api_key = ? WHERE id = ?;",
            params![base_url, token, id],
        )?;
    }
    Ok(())
}

pub fn list_bindings(conn: &Connection) -> AppResult<Vec<GatewayBinding>> {
    if !table_exists(conn, "gateway_bindings") {
        return Ok(Vec::new());
    }
    let has_profile = bindings_have_profile_id(conn);
    let has_v34 = bindings_have_v34(conn);
    let sql = if has_v34 {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id FROM gateway_bindings ORDER BY target_app;"
    } else if has_profile {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id FROM gateway_bindings ORDER BY target_app;"
    } else {
        "SELECT target_app, entry_token, provider_id, created_at FROM gateway_bindings ORDER BY target_app;"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| row_to_binding(row, has_profile, has_v34))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn binding_for_target(conn: &Connection, target: ProviderTarget) -> AppResult<Option<GatewayBinding>> {
    if !table_exists(conn, "gateway_bindings") {
        return Ok(None);
    }
    let has_profile = bindings_have_profile_id(conn);
    let has_v34 = bindings_have_v34(conn);
    let sql = if has_v34 {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id FROM gateway_bindings WHERE target_app = ?;"
    } else if has_profile {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id FROM gateway_bindings WHERE target_app = ?;"
    } else {
        "SELECT target_app, entry_token, provider_id, created_at FROM gateway_bindings WHERE target_app = ?;"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![target.as_str()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row_to_binding(row, has_profile, has_v34)?))
}

pub fn binding_by_token(conn: &Connection, token: &str) -> AppResult<Option<GatewayBinding>> {
    let trimmed = token.trim();
    if trimmed.is_empty() || !table_exists(conn, "gateway_bindings") {
        return Ok(None);
    }
    let has_profile = bindings_have_profile_id(conn);
    let has_v34 = bindings_have_v34(conn);
    let sql = if has_v34 {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id FROM gateway_bindings WHERE entry_token = ?;"
    } else if has_profile {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id FROM gateway_bindings WHERE entry_token = ?;"
    } else {
        "SELECT target_app, entry_token, provider_id, created_at FROM gateway_bindings WHERE entry_token = ?;"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![trimmed])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let binding = row_to_binding(row, has_profile, has_v34)?;
    if binding.mode == BINDING_MODE_DIRECT {
        return Ok(None);
    }
    Ok(Some(binding))
}

fn binding_token(conn: &Connection, target: ProviderTarget) -> Option<String> {
    binding_for_target(conn, target)
        .ok()
        .flatten()
        .map(|binding| binding.entry_token)
        .filter(|token| !token.trim().is_empty())
}

pub fn upsert_binding(conn: &Connection, target: ProviderTarget, provider_id: &str) -> AppResult<GatewayBinding> {
    let now = chrono::Utc::now().timestamp_millis();
    let has_v34 = bindings_have_v34(conn);
    let has_profile = bindings_have_profile_id(conn);
    if let Some(existing) = binding_for_target(conn, target)? {
        let token = if existing.entry_token.trim().is_empty() {
            format!("gwt_{}", Uuid::new_v4().simple())
        } else {
            existing.entry_token.clone()
        };
        if has_v34 {
            conn.execute(
                "UPDATE gateway_bindings
                 SET entry_token = ?, provider_id = ?, mode = 'gateway', direct_upstream_id = ''
                 WHERE target_app = ?;",
                params![token, provider_id, target.as_str()],
            )?;
        } else if existing.provider_id != provider_id {
            conn.execute(
                "UPDATE gateway_bindings SET provider_id = ? WHERE target_app = ?;",
                params![provider_id, target.as_str()],
            )?;
        }
        return binding_for_target(conn, target)?
            .ok_or_else(|| AppError::Config("绑定写入失败".to_string()));
    }
    let token = format!("gwt_{}", Uuid::new_v4().simple());
    if has_v34 {
        conn.execute(
            "INSERT INTO gateway_bindings (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
             VALUES (?, ?, ?, ?, ?, 'gateway', '');",
            params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID],
        )?;
    } else if has_profile {
        conn.execute(
            "INSERT INTO gateway_bindings (target_app, entry_token, provider_id, created_at, profile_id)
             VALUES (?, ?, ?, ?, ?);",
            params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID],
        )?;
    } else {
        conn.execute(
            "INSERT INTO gateway_bindings (target_app, entry_token, provider_id, created_at)
             VALUES (?, ?, ?, ?);",
            params![target.as_str(), token, provider_id, now],
        )?;
    }
    binding_for_target(conn, target)?.ok_or_else(|| AppError::Config("绑定写入失败".to_string()))
}

pub fn set_direct_binding(
    conn: &Connection,
    target: ProviderTarget,
    upstream_id: &str,
) -> AppResult<GatewayBinding> {
    let upstream_id = upstream_id.trim();
    if upstream_id.is_empty() {
        return Err(AppError::Config("直连上游 ID 不能为空".to_string()));
    }
    let upstream = get_upstream_provider(conn, upstream_id)?
        .ok_or_else(|| AppError::Config(format!("上游不存在: {upstream_id}")))?;
    if upstream.is_smart_gateway() {
        return Err(AppError::Config("不能将智能网关 Auto 卡设为直连上游".to_string()));
    }

    let now = chrono::Utc::now().timestamp_millis();
    let has_v34 = bindings_have_v34(conn);
    let has_profile = bindings_have_profile_id(conn);

    if let Some(existing) = binding_for_target(conn, target)? {
        let token = if existing.entry_token.trim().is_empty() {
            format!("gwt_{}", Uuid::new_v4().simple())
        } else {
            existing.entry_token
        };
        if has_v34 {
            conn.execute(
                "UPDATE gateway_bindings
                 SET entry_token = ?, mode = 'direct', direct_upstream_id = ?
                 WHERE target_app = ?;",
                params![token, upstream_id, target.as_str()],
            )?;
        } else {
            conn.execute(
                "UPDATE gateway_bindings SET entry_token = ? WHERE target_app = ?;",
                params![token, target.as_str()],
            )?;
        }
    } else {
        let token = format!("gwt_{}", Uuid::new_v4().simple());
        let provider_id = format!("sgw_{}", target.as_str());
        if has_v34 {
            conn.execute(
                "INSERT INTO gateway_bindings
                    (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                 VALUES (?, ?, ?, ?, ?, 'direct', ?);",
                params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID, upstream_id],
            )?;
        } else if has_profile {
            conn.execute(
                "INSERT INTO gateway_bindings
                    (target_app, entry_token, provider_id, created_at, profile_id)
                 VALUES (?, ?, ?, ?, ?);",
                params![target.as_str(), token, provider_id, now, SHARED_PROFILE_ID],
            )?;
        } else {
            conn.execute(
                "INSERT INTO gateway_bindings (target_app, entry_token, provider_id, created_at)
                 VALUES (?, ?, ?, ?);",
                params![target.as_str(), token, provider_id, now],
            )?;
        }
    }
    binding_for_target(conn, target)?.ok_or_else(|| AppError::Config("直连绑定写入失败".to_string()))
}

pub fn set_gateway_binding(
    conn: &Connection,
    target: ProviderTarget,
) -> AppResult<GatewayBinding> {
    let provider_id = format!("sgw_{}", target.as_str());
    upsert_binding(conn, target, &provider_id)
}

pub fn list_direct_bindings_for_upstream(
    conn: &Connection,
    upstream_id: &str,
) -> AppResult<Vec<GatewayBinding>> {
    if !table_exists(conn, "gateway_bindings") || !bindings_have_v34(conn) {
        return Ok(Vec::new());
    }
    let has_profile = bindings_have_profile_id(conn);
    let sql = if has_profile {
        "SELECT target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id
         FROM gateway_bindings WHERE mode = 'direct' AND direct_upstream_id = ?;"
    } else {
        "SELECT target_app, entry_token, provider_id, created_at, mode, direct_upstream_id
         FROM gateway_bindings WHERE mode = 'direct' AND direct_upstream_id = ?;"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![upstream_id], |row| row_to_binding(row, has_profile, true))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete_binding(conn: &Connection, target: ProviderTarget) -> AppResult<()> {
    if table_exists(conn, "gateway_bindings") {
        conn.execute(
            "DELETE FROM gateway_bindings WHERE target_app = ?;",
            params![target.as_str()],
        )?;
    }
    Ok(())
}

pub fn has_any_binding(conn: &Connection) -> bool {
    if !table_exists(conn, "gateway_bindings") {
        return false;
    }
    conn.query_row("SELECT count(*) FROM gateway_bindings;", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0
}

pub fn list_route_modes(conn: &Connection, profile_id: &str) -> AppResult<Vec<RouteMode>> {
    if !table_exists(conn, "route_modes") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, profile_id, enabled, model, thinking_config_json, fallback_models_json, threshold, sort_index
         FROM route_modes WHERE profile_id = ? ORDER BY sort_index ASC;",
    )?;
    let rows = stmt.query_map(params![profile_id], |row| {
        Ok(RouteMode {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            model: row.get(3)?,
            thinking_config_json: row.get(4)?,
            fallback_models: parse_string_list(row.get(5)?),
            threshold: row.get(6)?,
            sort_index: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn patch_route_mode(
    conn: &Connection,
    mode_id: &str,
    patch: &RouteModePatch,
    profile_id: Option<&str>,
) -> AppResult<RouteMode> {
    let profile_id = resolve_profile_id(conn, profile_id)?;
    let mut modes = list_route_modes(conn, &profile_id)?;
    let Some(mode) = modes.iter_mut().find(|item| item.id == mode_id) else {
        return Err(AppError::Config(format!("未知路由模式: {mode_id}")));
    };
    if let Some(enabled) = patch.enabled {
        mode.enabled = enabled;
    }
    if let Some(model) = &patch.model {
        mode.model = model.trim().to_string();
    }
    if let Some(thinking) = &patch.thinking_config_json {
        mode.thinking_config_json = thinking.clone();
    }
    if let Some(fallback) = &patch.fallback_models {
        mode.fallback_models = fallback.clone();
    }
    if let Some(threshold) = patch.threshold {
        mode.threshold = threshold.max(0);
    }
    conn.execute(
        "UPDATE route_modes SET enabled = ?, model = ?, thinking_config_json = ?, fallback_models_json = ?, threshold = ?
         WHERE profile_id = ? AND id = ?;",
        params![
            if mode.enabled { 1 } else { 0 },
            mode.model,
            mode.thinking_config_json,
            serde_json::to_string(&mode.fallback_models).unwrap_or_else(|_| "[]".into()),
            mode.threshold,
            profile_id,
            mode_id
        ],
    )?;
    if mode_id == "background" {
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE gateway_profiles SET subagent_model = ?, updated_at = ? WHERE id = ?;",
            params![mode.model, now, profile_id],
        )?;
    }
    list_route_modes(conn, &profile_id)?
        .into_iter()
        .find(|item| item.id == mode_id)
        .ok_or_else(|| AppError::Config("路由模式写入失败".to_string()))
}

pub fn list_route_rules(conn: &Connection, profile_id: &str) -> AppResult<Vec<RouteRule>> {
    if !table_exists(conn, "route_rules") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, profile_id, enabled, sort_index, rule_type, condition_json, pattern, target_model,
                thinking_config_json, rewrites_json
         FROM route_rules WHERE profile_id = ? ORDER BY sort_index ASC;",
    )?;
    let rows = stmt.query_map(params![profile_id], |row| {
        Ok(RouteRule {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            sort_index: row.get(3)?,
            rule_type: row.get(4)?,
            condition_json: row.get(5)?,
            pattern: row.get(6)?,
            target_model: row.get(7)?,
            thinking_config_json: row.get(8)?,
            rewrites_json: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_route_rule(conn: &Connection, rule: &RouteRule) -> AppResult<RouteRule> {
    let profile_id = resolve_profile_id(conn, Some(rule.profile_id.as_str()))?;
    conn.execute(
        "INSERT INTO route_rules
            (id, profile_id, enabled, sort_index, rule_type, condition_json, pattern, target_model,
             thinking_config_json, rewrites_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            profile_id = excluded.profile_id,
            enabled = excluded.enabled,
            sort_index = excluded.sort_index,
            rule_type = excluded.rule_type,
            condition_json = excluded.condition_json,
            pattern = excluded.pattern,
            target_model = excluded.target_model,
            thinking_config_json = excluded.thinking_config_json,
            rewrites_json = excluded.rewrites_json;",
        params![
            rule.id,
            profile_id,
            if rule.enabled { 1 } else { 0 },
            rule.sort_index,
            rule.rule_type,
            rule.condition_json,
            rule.pattern,
            rule.target_model,
            rule.thinking_config_json,
            rule.rewrites_json
        ],
    )?;
    list_route_rules(conn, &profile_id)?
        .into_iter()
        .find(|item| item.id == rule.id)
        .ok_or_else(|| AppError::Config("规则写入失败".to_string()))
}

pub fn delete_route_rule(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM route_rules WHERE id = ?;", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use super::{
        assert_upstream_deletable, binding_by_token, binding_for_target, create_profile,
        current_profile, delete_profile, delete_upstream, ensure_profile_for_target,
        is_gateway_connection, get_upstream_provider, list_upstream_models,
        list_direct_bindings_for_upstream, list_profiles, list_route_modes,
        list_route_rules, list_upstream_providers, migrate_v33_to_v34, patch_route_mode,
        provider_from_upstream, repair_long_context_one_token_threshold, rollback_v34,
        set_binding_profile, set_direct_binding, set_gateway_binding, upstream_endpoint_key,
        upstream_identity_dedup_key, upsert_binding, upsert_route_rule, upsert_upstream,
        RouteModePatch, RouteRule, BINDING_MODE_DIRECT, BINDING_MODE_GATEWAY,
        DEFAULT_LONG_CONTEXT_THRESHOLD, SHARED_PROFILE_ID,
    };
    use crate::database::dao::{set_current_provider, upsert_provider};
    use crate::database::Database;
    use crate::provider::{
        ClaudeModelMapping, ProtocolType, ProviderInput, ProviderKind, ProviderTarget,
    };

    #[test]
    fn endpoint_key_ignores_trailing_slash_and_case() {
        let left = upstream_endpoint_key("https://API.example.com/v1/", ProtocolType::Anthropic);
        let right = upstream_endpoint_key("https://api.example.com/v1", ProtocolType::Anthropic);
        assert_eq!(left, right);
        let chat = upstream_endpoint_key("https://api.example.com/v1", ProtocolType::OpenAiChat);
        assert_ne!(left, chat);
    }

    fn provider_input(
        id: Option<&str>,
        target: ProviderTarget,
        kind: ProviderKind,
        protocol: ProtocolType,
        base_url: &str,
        model: &str,
    ) -> ProviderInput {
        ProviderInput {
            id: id.map(str::to_string),
            name: format!("{target:?} {kind:?}"),
            base_url: base_url.to_string(),
            api_key: String::new(),
            clear_api_key: false,
            model: model.to_string(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: protocol,
            provider_kind: kind,
            auth_binding: String::new(),
            target_app: target,
            notes: String::new(),
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
        }
    }

    #[test]
    fn gateway_connection_requires_auto_current_for_exclusive_agents() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let independent = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_indep"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.example.test",
                    "claude-sonnet-4-6",
                ),
            )?;
            set_current_provider(conn, &independent.id)?;
            upsert_binding(conn, ProviderTarget::ClaudeCode, "p_sg_claude_code")?;
            assert!(
                !is_gateway_connection(conn, ProviderTarget::ClaudeCode),
                "bound Code with an independent current must not look like catalog-on"
            );

            let auto = upsert_provider(
                conn,
                &provider_input(
                    Some("p_sg_claude_code"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::SmartGateway,
                    ProtocolType::Anthropic,
                    "http://127.0.0.1:15828",
                    "claude.auto",
                ),
            )?;
            set_current_provider(conn, &auto.id)?;
            assert!(is_gateway_connection(conn, ProviderTarget::ClaudeCode));

            upsert_binding(conn, ProviderTarget::Cline, "p_sg_cline")?;
            assert!(
                is_gateway_connection(conn, ProviderTarget::Cline),
                "catalog agents stay catalog-on from the binding alone"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn bound_independent_code_must_not_look_like_live_gateway() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let independent = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_indep"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.example.test",
                    "gpt-5.6-terra",
                ),
            )?;
            set_current_provider(conn, &independent.id)?;
            upsert_binding(conn, ProviderTarget::ClaudeCode, "sgw_claude_code")?;
            set_binding_profile(conn, ProviderTarget::ClaudeCode, SHARED_PROFILE_ID)?;
            assert!(
                !is_gateway_connection(conn, ProviderTarget::ClaudeCode),
                "editing gateway rules / picking a profile must not treat this as live gateway"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn long_context_threshold_one_is_repaired_to_20000() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            conn.execute(
                "UPDATE route_modes SET threshold = 1 WHERE id = 'long_context';",
                [],
            )?;
            repair_long_context_one_token_threshold(conn)?;
            let modes = list_route_modes(conn, SHARED_PROFILE_ID)?;
            let long_context = modes.iter().find(|mode| mode.id == "long_context").unwrap();
            assert_eq!(long_context.threshold, DEFAULT_LONG_CONTEXT_THRESHOLD);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn list_profiles_returns_cloned_rows() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let listed = list_profiles(conn)?;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, SHARED_PROFILE_ID);
            let cloned = create_profile(conn, "编程", Some(SHARED_PROFILE_ID))?;
            assert_ne!(cloned.id, SHARED_PROFILE_ID);
            assert_eq!(cloned.name, "编程");
            assert!(cloned.entry_token.is_empty());
            let listed = list_profiles(conn)?;
            assert_eq!(listed.len(), 2);
            assert_eq!(listed[0].id, SHARED_PROFILE_ID);
            let cloned_row = listed.iter().find(|profile| profile.id == cloned.id).expect("cloned");
            assert_eq!(cloned_row.name, "编程");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn list_profiles_is_read_only_and_hides_legacy_agent_rows() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            conn.execute(
                "INSERT INTO gateway_profiles (id, name, target_app, created_at, updated_at)
                 VALUES ('gprof_codex', 'Codex 默认档案', 'codex', 1, 1);",
                [],
            )?;
            let cloned = create_profile(conn, "gpt", Some(SHARED_PROFILE_ID))?;
            conn.execute_batch("PRAGMA query_only = ON;")?;
            let listed = list_profiles(conn)?;
            assert!(listed.iter().all(|profile| {
                profile.id == SHARED_PROFILE_ID || profile.id == cloned.id
            }));
            assert_eq!(
                listed.iter().find(|profile| profile.id == cloned.id).unwrap().name,
                "gpt"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn current_profile_and_modes_are_read_only() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let cloned = create_profile(conn, "gpt", Some(SHARED_PROFILE_ID))?;
            upsert_binding(conn, ProviderTarget::ClaudeCode, "p_sg_claude_code")?;
            set_binding_profile(conn, ProviderTarget::ClaudeCode, &cloned.id)?;
            patch_route_mode(
                conn,
                "default",
                &RouteModePatch {
                    model: Some("claude.sub2api.gpt-5.6-terra".into()),
                    enabled: Some(true),
                    ..RouteModePatch::default()
                },
                Some(&cloned.id),
            )?;
            conn.execute_batch("PRAGMA query_only = ON;")?;
            let bound = current_profile(conn, ProviderTarget::ClaudeCode)?.expect("bound profile");
            assert_eq!(bound.id, cloned.id);
            let modes = list_route_modes(conn, &cloned.id)?;
            let default = modes.iter().find(|mode| mode.id == "default").expect("default");
            assert_eq!(default.model, "claude.sub2api.gpt-5.6-terra");
            let unbound = current_profile(conn, ProviderTarget::Codex)?.expect("shared");
            assert_eq!(unbound.id, SHARED_PROFILE_ID);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn clone_profile_copies_modes_and_rules() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            patch_route_mode(
                conn,
                "default",
                &RouteModePatch {
                    model: Some("deepseek.chat".into()),
                    enabled: Some(true),
                    ..RouteModePatch::default()
                },
                Some(SHARED_PROFILE_ID),
            )?;
            upsert_route_rule(
                conn,
                &RouteRule {
                    id: "rule_clone_src".into(),
                    profile_id: SHARED_PROFILE_ID.into(),
                    enabled: true,
                    sort_index: 0,
                    rule_type: "model-prefix".into(),
                    condition_json: "{}".into(),
                    pattern: "gpt-".into(),
                    target_model: "deepseek.chat".into(),
                    thinking_config_json: "{}".into(),
                    rewrites_json: "[]".into(),
                },
            )?;
            let cloned = create_profile(conn, "Astra", Some(SHARED_PROFILE_ID))?;
            let modes = list_route_modes(conn, &cloned.id)?;
            let default = modes.iter().find(|mode| mode.id == "default").unwrap();
            assert_eq!(default.model, "deepseek.chat");
            let rules = list_route_rules(conn, &cloned.id)?;
            assert_eq!(rules.len(), 1);
            assert_ne!(rules[0].id, "rule_clone_src");
            assert_eq!(rules[0].pattern, "gpt-");
            assert_eq!(rules[0].profile_id, cloned.id);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn current_profile_follows_binding_and_delete_falls_back() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let cloned = create_profile(conn, "Astra", Some(SHARED_PROFILE_ID))?;
            upsert_binding(conn, ProviderTarget::ClaudeCode, "p_sg_claude_code")?;
            set_binding_profile(conn, ProviderTarget::ClaudeCode, &cloned.id)?;
            let bound = current_profile(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(bound.id, cloned.id);
            let unbound = current_profile(conn, ProviderTarget::Codex)?.unwrap();
            assert_eq!(unbound.id, SHARED_PROFILE_ID);
            delete_profile(conn, &cloned.id)?;
            let after = current_profile(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(after.id, SHARED_PROFILE_ID);
            let binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(binding.profile_id, SHARED_PROFILE_ID);
            assert!(delete_profile(conn, SHARED_PROFILE_ID).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn picking_a_profile_must_not_create_a_binding() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let cloned = create_profile(conn, "gpt", Some(SHARED_PROFILE_ID))?;
            assert!(set_binding_profile(conn, ProviderTarget::ClaudeCode, &cloned.id).is_err());
            assert!(binding_for_target(conn, ProviderTarget::ClaudeCode)?.is_none());
            assert!(!is_gateway_connection(conn, ProviderTarget::ClaudeCode));
            let rows: i64 = conn.query_row(
                "SELECT count(*) FROM gateway_bindings WHERE target_app = ?;",
                [ProviderTarget::ClaudeCode.as_str()],
                |row| row.get(0),
            )?;
            assert_eq!(rows, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_upstream_identity_dedup_key() {
        let key1 = upstream_identity_dedup_key(
            "https://api.example.com/v1/",
            ProtocolType::Anthropic,
            "secret-123",
            ProviderKind::Standard,
            "",
        );
        let key2 = upstream_identity_dedup_key(
            "https://API.EXAMPLE.COM/v1",
            ProtocolType::Anthropic,
            "secret-123",
            ProviderKind::Standard,
            "",
        );
        assert_eq!(key1, key2, "dedup key should normalize URL case and trailing slash");

        let key_diff_secret = upstream_identity_dedup_key(
            "https://api.example.com/v1",
            ProtocolType::Anthropic,
            "secret-456",
            ProviderKind::Standard,
            "",
        );
        assert_ne!(key1, key_diff_secret, "different keys must have different dedup keys");

        let key_diff_protocol = upstream_identity_dedup_key(
            "https://api.example.com/v1",
            ProtocolType::OpenAiChat,
            "secret-123",
            ProviderKind::Standard,
            "",
        );
        assert_ne!(key1, key_diff_protocol, "different protocols must have different dedup keys");
    }

    #[test]
    fn test_direct_binding_restrictions() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let upstream = upsert_upstream(
                conn,
                &ProviderInput {
                    id: Some("up_test_direct".to_string()),
                    name: "Upstream Direct Test".to_string(),
                    base_url: "https://api.openai.com/v1".to_string(),
                    api_key: String::new(),
                    clear_api_key: false,
                    model: "gpt-4o".to_string(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::OpenAiChat,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 0,
                    failover_models: Vec::new(),
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;

            // 1. set_direct_binding creates/updates direct binding
            let binding = set_direct_binding(conn, ProviderTarget::ClaudeCode, &upstream.id)?;
            assert_eq!(binding.mode, BINDING_MODE_DIRECT);
            assert_eq!(binding.direct_upstream_id, upstream.id);

            // 2. direct does not recognize gateway
            assert!(!is_gateway_connection(conn, ProviderTarget::ClaudeCode));

            // 3. direct token cannot enter gateway
            assert!(binding_by_token(conn, &binding.entry_token)?.is_none());

            // 4. set_binding_profile rejects when in direct mode
            assert!(set_binding_profile(conn, ProviderTarget::ClaudeCode, SHARED_PROFILE_ID).is_err());

            // 5. assert_upstream_deletable blocks deletion of direct-referenced upstream
            assert!(assert_upstream_deletable(conn, &upstream.id).is_err());
            assert!(delete_upstream(conn, &upstream.id).is_err());

            // 6. list_direct_bindings_for_upstream finds the binding
            let direct_bindings = list_direct_bindings_for_upstream(conn, &upstream.id)?;
            assert_eq!(direct_bindings.len(), 1);
            assert_eq!(direct_bindings[0].target_app, ProviderTarget::ClaudeCode);

            // 7. Switching to gateway clears direct
            let gateway_binding = set_gateway_binding(conn, ProviderTarget::ClaudeCode)?;
            assert_eq!(gateway_binding.mode, BINDING_MODE_GATEWAY);
            assert!(gateway_binding.direct_upstream_id.is_empty());

            // Upstream can now be deleted
            assert!(assert_upstream_deletable(conn, &upstream.id).is_ok());

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_provider_from_upstream_field_mapping() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let upstream = upsert_upstream(
                conn,
                &ProviderInput {
                    id: Some("up_source".to_string()),
                    name: "Upstream Source".to_string(),
                    base_url: "https://api.anthropic.com".to_string(),
                    api_key: String::new(),
                    clear_api_key: false,
                    model: "claude-3-7-sonnet-20250219".to_string(),
                    model_context_window: Some(200_000),
                    auto_review_model_override: Some("guardian".to_string()),
                    web_search_enabled: Some(true),
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::Anthropic,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: "test notes".to_string(),
                    failover_group: 1,
                    failover_models: vec!["model-a".to_string()],
                    hidden_models: vec!["model-b".to_string()],
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;

            let provider = provider_from_upstream(conn, &upstream.id, ProviderTarget::Codex)?;
            assert_eq!(provider.id, upstream.id);
            assert_eq!(provider.target_app, ProviderTarget::Codex);
            assert!(provider.is_current);
            assert_eq!(provider.name, upstream.name);
            assert_eq!(provider.base_url, upstream.base_url);
            assert_eq!(provider.model, upstream.model);
            assert_eq!(provider.model_context_window, Some(200_000));
            assert_eq!(provider.web_search_enabled, Some(true));

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn migration_missing_keyring_entry_aborts_without_changing_connections() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            let old = upsert_provider(conn, &provider_input(
                Some("missing_key_old"), ProviderTarget::ClaudeCode, ProviderKind::Standard,
                ProtocolType::Anthropic, "https://missing-key.example.test", "custom-model",
            ))?;
            set_current_provider(conn, &old.id)?;
            // 只引用随机不存在的条目，不写入或删除系统凭据。
            let missing_ref = format!("kr://aisw_missing_test_{}", uuid::Uuid::new_v4());
            conn.execute("UPDATE providers SET api_key = ? WHERE id = ?;", params![missing_ref, old.id])?;
            assert!(migrate_v33_to_v34(conn).is_err());
            assert!(!super::is_v34_migration_done(conn));
            assert!(list_upstream_providers(conn, true)?.is_empty());
            assert!(binding_for_target(conn, ProviderTarget::ClaudeCode)?.is_none());
            assert_eq!(super::super::providers::get_current_provider(conn, ProviderTarget::ClaudeCode)?.unwrap().id, old.id);
            assert_eq!(conn.query_row("SELECT count(*) FROM upstream_migration_v34;", [], |r| r.get::<_, i64>(0))?, 0);
            assert_eq!(conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, u32>(0))?, 33);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn migration_upstream_write_failure_does_not_mark_upgrade_complete() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            let old = upsert_provider(conn, &provider_input(
                Some("failed_import_old"), ProviderTarget::ClaudeCode, ProviderKind::Standard,
                ProtocolType::Anthropic, "https://failed-import.example.test", "custom-model",
            ))?;
            set_current_provider(conn, &old.id)?;
            conn.execute_batch("CREATE TRIGGER reject_migration_upstream BEFORE INSERT ON upstreams
                BEGIN SELECT RAISE(ABORT, 'test upstream write failure'); END;")?;
            assert!(migrate_v33_to_v34(conn).is_err());
            assert!(!super::is_v34_migration_done(conn));
            assert!(list_upstream_providers(conn, true)?.is_empty());
            assert!(binding_for_target(conn, ProviderTarget::ClaudeCode)?.is_none());
            assert_eq!(super::super::providers::get_current_provider(conn, ProviderTarget::ClaudeCode)?.unwrap().id, old.id);
            assert_eq!(conn.query_row("SELECT count(*) FROM upstream_migration_v34;", [], |r| r.get::<_, i64>(0))?, 0);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn rollback_missing_snapshot_fails_without_changing_bindings() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let before = binding_for_target(conn, ProviderTarget::Pi)?.unwrap();
            conn.execute("DELETE FROM upstream_migration_v34 WHERE old_provider_id = '__snap_pi';", [])?;
            assert!(rollback_v34(conn).is_err());
            let after = binding_for_target(conn, ProviderTarget::Pi)?.unwrap();
            assert_eq!(before.entry_token, after.entry_token);
            assert_eq!(conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, u32>(0))?, 34);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn rollback_restores_official_even_after_new_auto_card_was_selected() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            migrate_v33_to_v34(conn)?;
            let auto = upsert_provider(conn, &provider_input(
                Some("new_auto"), ProviderTarget::ClaudeCode, ProviderKind::SmartGateway,
                ProtocolType::Anthropic, "http://127.0.0.1:15828", "claude.auto",
            ))?;
            set_current_provider(conn, &auto.id)?;
            rollback_v34(conn)?;
            assert!(super::super::providers::get_current_provider(conn, ProviderTarget::ClaudeCode)?.is_none());
            assert!(binding_for_target(conn, ProviderTarget::ClaudeCode)?.is_none());
            Ok(())
        }).unwrap();
    }

    fn migration_fixture() -> Database {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            // 新库已迁移完成，清掉标记和默认绑定，模拟升级前的 Schema 33 状态。
            conn.execute("DELETE FROM upstream_migration_v34;", [])?;
            conn.execute("DELETE FROM gateway_bindings;", [])?;
            crate::database::schema::set_user_version(conn, 33)
        }).unwrap();
        db
    }

    #[test]
    fn test_migrate_v33_to_v34_and_idempotence() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            // Setup legacy providers
            let p1 = upsert_provider(
                conn,
                &provider_input(
                    Some("p1"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.anthropic.com",
                    "claude-3-5-sonnet",
                ),
            )?;
            set_current_provider(conn, &p1.id)?;

            let _p2 = upsert_provider(
                conn,
                &provider_input(
                    Some("p2"),
                    ProviderTarget::OpenCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.anthropic.com",
                    "claude-3-5-sonnet",
                ),
            )?;

            let _p3_auto = upsert_provider(
                conn,
                &provider_input(
                    Some("p3_auto"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::SmartGateway,
                    ProtocolType::Anthropic,
                    "http://127.0.0.1:15828",
                    "claude.auto",
                ),
            )?;

            conn.execute(
                "INSERT INTO providers (id, name, base_url, api_key, model, protocol_type, provider_kind, target_app, is_current, created_at)
                 VALUES ('p4_self', 'Self reference', 'http://127.0.0.1:15828/v1', '', 'claude-3-5-sonnet', 'anthropic', 'standard', 'claude_code', 0, 0);",
                [],
            )?;

            let _p5_desktop = upsert_provider(
                conn,
                &provider_input(
                    Some("p5_desktop"),
                    ProviderTarget::ClaudeDesktop,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.anthropic.com",
                    "claude-3-5-sonnet",
                ),
            )?;

            // Run migration
            migrate_v33_to_v34(conn)?;

            // p1 and p2 have same endpoint & empty key -> deduplicated to 1 upstream
            // p3_auto, p4_self_ref, p5_desktop skipped
            let upstreams = list_upstream_providers(conn, true)?;
            assert_eq!(upstreams.len(), 1, "should deduplicate to 1 upstream");

            // T1 (Code) was current with independent p1 -> direct binding
            let code_binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.expect("code binding");
            assert_eq!(code_binding.mode, BINDING_MODE_DIRECT);
            assert_eq!(code_binding.direct_upstream_id, upstreams[0].id);

            // T2 (OpenCode) was unbound -> gateway binding
            let opencode_binding = binding_for_target(conn, ProviderTarget::OpenCode)?.expect("opencode binding");
            assert_eq!(opencode_binding.mode, BINDING_MODE_GATEWAY);

            // T1 (Codex) had no current provider -> official, not bound
            assert!(binding_for_target(conn, ProviderTarget::Codex)?.is_none());

            // Idempotency: run again, nothing breaks or duplicates
            migrate_v33_to_v34(conn)?;
            let upstreams_after = list_upstream_providers(conn, true)?;
            assert_eq!(upstreams_after.len(), 1, "re-running migration must be idempotent");

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_rollback_v34() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            let p_auto = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_auto_roll"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::SmartGateway,
                    ProtocolType::Anthropic,
                    "http://127.0.0.1:15828",
                    "claude.auto",
                ),
            )?;
            set_current_provider(conn, &p_auto.id)?;

            let p1 = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_roll"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.example.com",
                    "claude-3-5-sonnet",
                ),
            )?;

            // Migrate
            migrate_v33_to_v34(conn)?;
            assert_eq!(crate::database::schema::SCHEMA_VERSION, 34);

            let binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(binding.mode, BINDING_MODE_GATEWAY);

            // In v34, user switches current to independent card
            set_current_provider(conn, &p1.id)?;
            let up_id: String = conn.query_row(
                "SELECT upstream_id FROM gateway_id_map WHERE old_provider_id = 'p_code_roll';",
                [],
                |row| row.get(0),
            )?;
            assert!(!up_id.is_empty());
            assert!(get_upstream_provider(conn, &up_id)?.is_some());

            // Rollback
            rollback_v34(conn)?;

            let current_ver: u32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0))?;
            assert_eq!(current_ver, 33, "rollback must set user_version to 33");

            // Check provider current restored (Auto was current, p1 was not)
            let prov_auto = super::super::providers::get_provider(conn, "p_code_auto_roll")?.unwrap();
            assert!(prov_auto.is_current, "Auto card must be restored to is_current=true");
            let prov1 = super::super::providers::get_provider(conn, "p_code_roll")?.unwrap();
            assert!(!prov1.is_current, "p1 must be restored to is_current=false");

            // Newly created upstream must be deleted
            assert!(
                get_upstream_provider(conn, &up_id)?.is_none(),
                "newly created upstream must be deleted on rollback"
            );

            // Can re-migrate cleanly from version 33
            migrate_v33_to_v34(conn)?;
            let binding_re = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(binding_re.mode, BINDING_MODE_GATEWAY);

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_existing_upstream_with_key_hit() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            // 1. Create an existing upstream
            let up = upsert_upstream(
                conn,
                &ProviderInput {
                    id: Some("up_existing_key".to_string()),
                    name: "Existing Upstream".to_string(),
                    base_url: "https://api.upstream.com/v1".to_string(),
                    api_key: "secret_shared_key_123".to_string(),
                    clear_api_key: false,
                    model: "upstream-model".to_string(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::Anthropic,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 0,
                    failover_models: Vec::new(),
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;

            // 2. Create a legacy provider with the exact same base_url and matching secret key,
            //    plus extra role model in model_mapping to verify models merge into existing upstream
            let mut mapping = ClaudeModelMapping::default();
            mapping.haiku = "claude-3-5-haiku-role".to_string();

            let p_legacy = upsert_provider(
                conn,
                &ProviderInput {
                    id: Some("p_legacy_same_key".to_string()),
                    name: "Legacy Same Key".to_string(),
                    base_url: "https://API.UPSTREAM.COM/v1".to_string(),
                    api_key: "secret_shared_key_123".to_string(),
                    clear_api_key: false,
                    model: "upstream-model".to_string(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: mapping,
                    protocol_type: ProtocolType::Anthropic,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 0,
                    failover_models: Vec::new(),
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;

            // Run migration
            migrate_v33_to_v34(conn)?;

            // Check that NO new upstream was created: existing upstream is reused!
            let upstreams = list_upstream_providers(conn, true)?;
            assert_eq!(upstreams.len(), 1, "should hit existing upstream and not create duplicate");
            assert_eq!(upstreams[0].id, up.id);

            // gateway_id_map maps p_legacy to existing up.id
            let mapped: String = conn.query_row(
                "SELECT upstream_id FROM gateway_id_map WHERE old_provider_id = ?;",
                params![p_legacy.id],
                |row| row.get(0),
            )?;
            assert_eq!(mapped, up.id);

            // Verify models were merged into existing upstream
            let models = list_upstream_models(conn, &up.id)?;
            assert!(
                models.iter().any(|m| m.model_id == "claude-3-5-haiku-role"),
                "role model must be merged into existing upstream"
            );

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_auto_migration_preserves_custom_profile_and_token() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            // Create a custom profile
            let custom_profile = create_profile(conn, "Custom Profile", Some(SHARED_PROFILE_ID))?;

            // Pre-create binding with custom profile, token, and provider_id
            conn.execute(
                "INSERT INTO gateway_bindings
                    (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                 VALUES ('claude_code', 'gwt_custom_secret_token', 'sgw_custom_prov', 1234567, ?, 'gateway', '');",
                params![custom_profile.id],
            )?;

            // Insert auto card as current
            let auto = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_auto"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::SmartGateway,
                    ProtocolType::Anthropic,
                    "http://127.0.0.1:15828",
                    "claude.auto",
                ),
            )?;
            set_current_provider(conn, &auto.id)?;

            // Run migration
            migrate_v33_to_v34(conn)?;

            // Check binding: profile_id, entry_token, provider_id, created_at must be preserved!
            let binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.expect("binding");
            assert_eq!(binding.mode, BINDING_MODE_GATEWAY);
            assert_eq!(binding.profile_id, custom_profile.id, "profile_id must be preserved");
            assert_eq!(binding.entry_token, "gwt_custom_secret_token", "entry_token must be preserved");
            assert_eq!(binding.provider_id, "sgw_custom_prov", "provider_id must be preserved");
            assert_eq!(binding.created_at, 1234567, "created_at must be preserved");

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_unbound_t2_agents_get_gateway_binding_on_migration() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            // Ensure no bindings exist
            conn.execute("DELETE FROM gateway_bindings;", [])?;

            // Run migration with no cards for Pi or Cline
            migrate_v33_to_v34(conn)?;

            // T2 (Pi, Cline, OpenCode) must all have gateway bindings created
            for target in [ProviderTarget::OpenCode, ProviderTarget::Pi, ProviderTarget::Cline] {
                let binding = binding_for_target(conn, target)?.expect("T2 binding must be created");
                assert_eq!(binding.mode, BINDING_MODE_GATEWAY);
                assert!(binding.direct_upstream_id.is_empty());
                assert!(!binding.entry_token.is_empty());
            }

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_rerun_migration_then_rollback_and_remigrate() {
        let db = migration_fixture();
        db.with_conn(|conn| {
            // Pre-create a binding for Code with custom profile & token
            let custom_profile = create_profile(conn, "Initial Profile", Some(SHARED_PROFILE_ID))?;
            conn.execute(
                "INSERT INTO gateway_bindings
                    (target_app, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)
                 VALUES ('claude_code', 'gwt_initial_token', 'sgw_initial', 11111, ?, 'gateway', '');",
                params![custom_profile.id],
            )?;

            // Pre-create an auto card (not current) and an independent card (current)
            let p_auto = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_auto"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::SmartGateway,
                    ProtocolType::Anthropic,
                    "http://127.0.0.1:15828",
                    "claude.auto",
                ),
            )?;

            let p1 = upsert_provider(
                conn,
                &provider_input(
                    Some("p_code_indep"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://api.example.com",
                    "claude-3-5-sonnet",
                ),
            )?;
            set_current_provider(conn, &p1.id)?;

            // 1. Run migration
            migrate_v33_to_v34(conn)?;
            let b1 = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(b1.mode, BINDING_MODE_DIRECT);
            let created_up_id = b1.direct_upstream_id.clone();
            assert!(!created_up_id.is_empty());
            assert!(get_upstream_provider(conn, &created_up_id)?.is_some());
            assert_eq!(b1.profile_id, custom_profile.id);
            assert_eq!(b1.entry_token, "gwt_initial_token");

            // 2. Rerun migration multiple times
            migrate_v33_to_v34(conn)?;
            migrate_v33_to_v34(conn)?;

            let b_rerun = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(b_rerun.mode, BINDING_MODE_DIRECT);
            assert_eq!(b_rerun.entry_token, "gwt_initial_token");

            // 3. User alters state in v34 (e.g. switches current to auto)
            set_current_provider(conn, &p_auto.id)?;

            // 4. Rollback
            rollback_v34(conn)?;
            let current_ver: u32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0))?;
            assert_eq!(current_ver, 33);

            // Assert newly created upstream is ACTUALLY DELETED
            assert!(
                get_upstream_provider(conn, &created_up_id)?.is_none(),
                "newly created upstream must be deleted on rollback"
            );

            // Initial binding must be fully restored: token, profile, mode!
            let b_restored = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(b_restored.entry_token, "gwt_initial_token");
            assert_eq!(b_restored.profile_id, custom_profile.id);
            assert_eq!(b_restored.created_at, 11111);
            assert_eq!(b_restored.provider_id, "sgw_initial");
            assert_eq!(b_restored.mode, BINDING_MODE_GATEWAY);

            // Providers is_current must be restored to pre-migration state:
            // p1 (p_code_indep) was current, p_auto was not!
            let prov1 = super::super::providers::get_provider(conn, "p_code_indep")?.unwrap();
            assert!(prov1.is_current, "p_code_indep must be restored to is_current=true");
            let prov_auto = super::super::providers::get_provider(conn, "p_code_auto")?.unwrap();
            assert!(!prov_auto.is_current, "p_code_auto must be restored to is_current=false");

            // T2 bindings that did not exist initially must be removed on rollback
            assert!(binding_for_target(conn, ProviderTarget::Pi)?.is_none());

            // 5. Remigrate from version 33 cleanly
            migrate_v33_to_v34(conn)?;
            let b_remigrated = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(b_remigrated.mode, BINDING_MODE_DIRECT);
            assert_eq!(b_remigrated.entry_token, "gwt_initial_token");

            Ok(())
        })
        .unwrap();
    }
}

