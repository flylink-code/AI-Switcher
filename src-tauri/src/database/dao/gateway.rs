//! Gateway profiles, agent connections, and upstream mirrors (Schema 29).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::provider::{Provider, ProviderTarget};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    External,
    Gateway,
}

impl ConnectionType {
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionType::External => "external",
            ConnectionType::Gateway => "gateway",
        }
    }

    pub fn from_str_lossy(value: &str) -> Self {
        if value == "gateway" {
            ConnectionType::Gateway
        } else {
            ConnectionType::External
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub entry_token: String,
    pub entry_token_set: bool,
    pub plan_fallback: Vec<String>,
    pub execute_fallback: Vec<String>,
    pub subagent_fallback: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnection {
    pub id: String,
    pub target_app: ProviderTarget,
    pub connection_type: ConnectionType,
    pub upstream_id: Option<String>,
    pub profile_id: Option<String>,
    pub is_current: bool,
    pub created_at: i64,
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
    conn.execute_batch(
        "INSERT OR IGNORE INTO upstreams (
            id, name, base_url, api_key, model, protocol_type, notes, sort_index, created_at
         )
         SELECT id, name, base_url, api_key, model, protocol_type, notes, sort_index, created_at
         FROM providers;
         INSERT OR IGNORE INTO gateway_id_map (old_provider_id, upstream_id)
         SELECT id, id FROM providers;",
    )?;
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
    let id = format!("aconn_gw_{target}");
    let profile_id = default_profile_id(target);
    conn.execute(
        "INSERT OR IGNORE INTO agent_connections
            (id, target_app, connection_type, upstream_id, profile_id, is_current, created_at)
         VALUES (?, ?, 'gateway', NULL, ?, 0, ?);",
        params![id, target, profile_id, now],
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

pub fn is_gateway_connection(conn: &Connection, target: ProviderTarget) -> bool {
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
    let id: Option<String> = conn
        .query_row(
            "SELECT profile_id FROM agent_connections
             WHERE target_app = ? AND connection_type = 'gateway'
             ORDER BY is_current DESC, created_at ASC LIMIT 1;",
            params![target.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    let profile_id = id.unwrap_or_else(|| default_profile_id(target.as_str()));
    get_profile(conn, &profile_id)
}

pub fn get_profile(conn: &Connection, id: &str) -> AppResult<Option<GatewayProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, target_app, default_model, plan_model, execute_model, subagent_model,
                allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
                fallback_mode, fallback_models_json, hide_official, entry_token,
                plan_fallback_json, execute_fallback_json, subagent_fallback_json,
                created_at, updated_at
         FROM gateway_profiles WHERE id = ?;",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_profile(row)?)),
        None => Ok(None),
    }
}

pub fn list_profiles(conn: &Connection, target: ProviderTarget) -> AppResult<Vec<GatewayProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, target_app, default_model, plan_model, execute_model, subagent_model,
                allowed_upstream_ids_json, role_routing_enabled, explicit_fallback_enabled,
                fallback_mode, fallback_models_json, hide_official, entry_token,
                plan_fallback_json, execute_fallback_json, subagent_fallback_json,
                created_at, updated_at
         FROM gateway_profiles WHERE target_app = ? ORDER BY created_at ASC;",
    )?;
    let rows = stmt.query_map(params![target.as_str()], row_to_profile)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
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
    ensure_default_profile(conn, target.as_str(), false, now)?;
    ensure_gateway_connection(conn, target.as_str(), now)?;
    get_profile(conn, &default_profile_id(target.as_str()))?
        .ok_or_else(|| AppError::Config("未能创建网关档案".to_string()))
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
    if let Some(value) = patch.role_routing_enabled {
        profile.role_routing_enabled = value;
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
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE gateway_profiles SET
            name = ?, default_model = ?, plan_model = ?, execute_model = ?, subagent_model = ?,
            allowed_upstream_ids_json = ?, role_routing_enabled = ?, explicit_fallback_enabled = ?,
            fallback_mode = ?, fallback_models_json = ?, hide_official = ?,
            plan_fallback_json = ?, execute_fallback_json = ?, subagent_fallback_json = ?,
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
        set_setting(
            conn,
            CATALOG_OPUSPLAN_KEY,
            if profile.role_routing_enabled {
                "true"
            } else {
                "false"
            },
        )?;
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
            if let Some(key) = match target {
                "claude_code" => Some(CATALOG_CODE_KEY),
                "codex" => Some(CATALOG_CODEX_KEY),
                _ => None,
            } {
                set_setting(conn, key, "true")?;
            }
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
            if let Some(key) = match target {
                "claude_code" => Some(CATALOG_CODE_KEY),
                "codex" => Some(CATALOG_CODEX_KEY),
                _ => None,
            } {
                set_setting(conn, key, "false")?;
            }
        }
    }
    Ok(())
}

pub fn sync_upstream_from_provider(conn: &Connection, provider: &Provider) -> AppResult<()> {
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
    if has_profile > 0 {
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
    let gateway_current: i64 = conn.query_row(
        "SELECT count(*) FROM agent_connections
         WHERE connection_type = 'gateway' AND is_current = 1
           AND profile_id IN (SELECT id FROM gateway_profiles);",
        [],
        |row| row.get(0),
    )?;
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
                    let in_use: i64 = conn.query_row(
                        "SELECT count(*) FROM agent_connections
                         WHERE profile_id = ? AND connection_type = 'gateway' AND is_current = 1;",
                        params![profile_id],
                        |row| row.get(0),
                    )?;
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
    conn.execute("DELETE FROM agent_connections WHERE upstream_id = ?;", params![id])?;
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
