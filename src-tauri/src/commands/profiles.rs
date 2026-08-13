//! Workspace configuration snapshot commands.

use serde::Serialize;

use crate::agents;
use crate::commands::mcp::sync_all;
use crate::commands::providers::{switch_provider_for_target, switch_to_official_for_target};
use crate::database::dao;
use crate::database::dao::profiles::{
    Profile, ProfilePayload, ProfileScopePayload, ProfileSnapshotScopes,
};
use crate::error::{AppError, AppResult};
use crate::mcp::McpTarget;
use crate::prompts::{self, PromptTarget};
use crate::provider::ProviderTarget;
use crate::skills::{self, SkillTarget};
use crate::store::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProfileResult {
    pub profile: Profile,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum ProfileScope {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
}

impl ProfileScope {
    fn provider_target(self) -> ProviderTarget {
        match self {
            ProfileScope::ClaudeCode => ProviderTarget::ClaudeCode,
            ProfileScope::ClaudeDesktop => ProviderTarget::ClaudeDesktop,
            ProfileScope::Codex => ProviderTarget::Codex,
        }
    }

    fn mcp_target(self) -> McpTarget {
        match self {
            ProfileScope::ClaudeCode => McpTarget::ClaudeCode,
            ProfileScope::ClaudeDesktop => McpTarget::ClaudeDesktop,
            ProfileScope::Codex => McpTarget::Codex,
        }
    }

    fn skill_target(self) -> Option<SkillTarget> {
        match self {
            ProfileScope::ClaudeCode => Some(SkillTarget::ClaudeCode),
            ProfileScope::ClaudeDesktop => None,
            ProfileScope::Codex => Some(SkillTarget::Codex),
        }
    }

    fn prompt_target(self) -> Option<PromptTarget> {
        match self {
            ProfileScope::ClaudeCode => Some(PromptTarget::ClaudeCode),
            ProfileScope::ClaudeDesktop => None,
            ProfileScope::Codex => Some(PromptTarget::Codex),
        }
    }

    fn payload<'a>(self, payload: &'a ProfilePayload) -> Option<&'a ProfileScopePayload> {
        match self {
            ProfileScope::ClaudeCode => payload.claude_code.as_ref(),
            ProfileScope::ClaudeDesktop => payload.claude_desktop.as_ref(),
            ProfileScope::Codex => payload.codex.as_ref(),
        }
    }

    fn set_payload(self, payload: &mut ProfilePayload, scope: ProfileScopePayload) {
        match self {
            ProfileScope::ClaudeCode => payload.claude_code = Some(scope),
            ProfileScope::ClaudeDesktop => payload.claude_desktop = Some(scope),
            ProfileScope::Codex => payload.codex = Some(scope),
        }
    }
}

#[tauri::command]
pub fn list_profiles(state: tauri::State<'_, AppState>) -> AppResult<Vec<Profile>> {
    state.db.with_conn(dao::profiles::list_profiles)
}

#[tauri::command]
pub fn get_current_profile_id(state: tauri::State<'_, AppState>) -> AppResult<Option<String>> {
    state.db.with_conn(dao::profiles::get_current_profile_id)
}

#[tauri::command(rename = "create_workspace_profile")]
pub fn create_workspace_profile(
    name: String,
    scopes: ProfileSnapshotScopes,
    state: tauri::State<'_, AppState>,
) -> AppResult<Profile> {
    let payload = snapshot_current(&state, scopes)?;
    state.db.with_conn(|conn| dao::profiles::create_profile(conn, &name, &payload))
}

#[tauri::command(rename = "update_workspace_profile")]
pub fn update_workspace_profile(
    id: String,
    name: Option<String>,
    payload: Option<ProfilePayload>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Profile> {
    state
        .db
        .with_conn(|conn| dao::profiles::update_profile(conn, &id, name.as_deref(), payload.as_ref()))
}

#[tauri::command(rename = "delete_workspace_profile")]
pub fn delete_workspace_profile(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| {
        dao::profiles::delete_profile(conn, &id)?;
        if dao::profiles::get_current_profile_id(conn)?
            .is_some_and(|current| current == id)
        {
            dao::profiles::set_current_profile_id(conn, None)?;
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn apply_profile(
    id: String,
    autosave_previous: Option<bool>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<ApplyProfileResult> {
    apply_profile_for_id(&id, autosave_previous.unwrap_or(true), &app, &state).await
}

pub async fn apply_profile_for_id<R: tauri::Runtime>(
    id: &str,
    autosave_previous: bool,
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> AppResult<ApplyProfileResult> {
    let profile = state
        .db
        .with_conn(|conn| dao::profiles::get_profile(conn, id))?
        .ok_or_else(|| AppError::Config(format!("配置快照不存在: {id}")))?;

    if autosave_previous {
        autosave_current_profile(state, &profile.id).await?;
    }

    let mut warnings = Vec::new();

    for scope in [
        ProfileScope::ClaudeCode,
        ProfileScope::ClaudeDesktop,
        ProfileScope::Codex,
    ] {
        let Some(scope_payload) = scope.payload(&profile.payload) else {
            continue;
        };
        apply_provider(scope, scope_payload, app, state).await?;
    }

    for scope in [
        ProfileScope::ClaudeCode,
        ProfileScope::ClaudeDesktop,
        ProfileScope::Codex,
    ] {
        let Some(scope_payload) = scope.payload(&profile.payload) else {
            continue;
        };
        apply_mcp(scope, scope_payload, state, &mut warnings)?;
    }
    sync_all(state)?;

    for scope in [
        ProfileScope::ClaudeCode,
        ProfileScope::ClaudeDesktop,
        ProfileScope::Codex,
    ] {
        let Some(scope_payload) = scope.payload(&profile.payload) else {
            continue;
        };
        apply_skills(scope, scope_payload, &mut warnings);
    }

    if let Some(scope_payload) = ProfileScope::ClaudeCode.payload(&profile.payload) {
        apply_agents(scope_payload, &mut warnings);
    }

    for scope in [
        ProfileScope::ClaudeCode,
        ProfileScope::ClaudeDesktop,
        ProfileScope::Codex,
    ] {
        let Some(scope_payload) = scope.payload(&profile.payload) else {
            continue;
        };
        apply_prompt(scope, scope_payload, &mut warnings);
    }

    state
        .db
        .with_conn(|conn| dao::profiles::set_current_profile_id(conn, Some(&profile.id)))?;

    let language = crate::commands::system::read_app_language(&state.db)?;
    crate::tray::refresh_tray_menu(app, &language)?;

    Ok(ApplyProfileResult { profile, warnings })
}

async fn autosave_current_profile(state: &AppState, next_profile_id: &str) -> AppResult<()> {
    let previous_id = state.db.with_conn(dao::profiles::get_current_profile_id)?;
    let Some(previous_id) = previous_id.filter(|current| current != next_profile_id) else {
        return Ok(());
    };
    let previous = state
        .db
        .with_conn(|conn| dao::profiles::get_profile(conn, &previous_id))?
        .ok_or_else(|| AppError::Config(format!("当前配置快照不存在: {previous_id}")))?;
    let mut payload = previous.payload.clone();
    if let Some(scope) = previous.payload.claude_code.as_ref() {
        payload.claude_code = Some(snapshot_scope(state, ProfileScope::ClaudeCode)?);
        let _ = scope;
    }
    if previous.payload.claude_desktop.is_some() {
        payload.claude_desktop = Some(snapshot_scope(state, ProfileScope::ClaudeDesktop)?);
    }
    if previous.payload.codex.is_some() {
        payload.codex = Some(snapshot_scope(state, ProfileScope::Codex)?);
    }
    state.db.with_conn(|conn| {
        dao::profiles::update_profile(conn, &previous_id, None, Some(&payload))
    })?;
    Ok(())
}

pub fn snapshot_current(state: &AppState, scopes: ProfileSnapshotScopes) -> AppResult<ProfilePayload> {
    let mut payload = ProfilePayload::default();
    if scopes.claude_code {
        ProfileScope::ClaudeCode.set_payload(&mut payload, snapshot_scope(state, ProfileScope::ClaudeCode)?);
    }
    if scopes.claude_desktop {
        ProfileScope::ClaudeDesktop.set_payload(
            &mut payload,
            snapshot_scope(state, ProfileScope::ClaudeDesktop)?,
        );
    }
    if scopes.codex {
        ProfileScope::Codex.set_payload(&mut payload, snapshot_scope(state, ProfileScope::Codex)?);
    }
    Ok(payload)
}

fn snapshot_scope(state: &AppState, scope: ProfileScope) -> AppResult<ProfileScopePayload> {
    let provider_id = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, scope.provider_target()))?
        .map(|provider| provider.id);

    let servers = state.db.with_conn(dao::mcp::list_mcp_servers)?;
    let mcp_target = scope.mcp_target();
    let mcp_ids = servers
        .iter()
        .filter(|server| server.is_enabled_for(mcp_target))
        .map(|server| server.id.clone())
        .collect();

    let skill_ids = match scope.skill_target() {
        Some(target) => skills::list_skills(target)?
            .into_iter()
            .filter(|skill| skill.enabled)
            .map(|skill| skill.name)
            .collect(),
        None => Vec::new(),
    };

    let agent_ids = if matches!(scope, ProfileScope::ClaudeCode) {
        agents::list_agents()?
            .into_iter()
            .filter(|agent| agent.enabled)
            .map(|agent| agent.name)
            .collect()
    } else {
        Vec::new()
    };

    let prompt_id = match scope.prompt_target() {
        Some(target) => detect_active_prompt(target)?,
        None => None,
    };

    Ok(ProfileScopePayload {
        provider_id,
        mcp_ids,
        skill_ids,
        agent_ids,
        prompt_id,
    })
}

fn detect_active_prompt(target: PromptTarget) -> AppResult<Option<String>> {
    let Some(live) = prompts::read_live_prompt(target)? else {
        return Ok(None);
    };
    for preset in prompts::list_prompts(target)? {
        let detail = prompts::read_prompt(target, &preset.name)?;
        if detail.content == live.content {
            return Ok(Some(preset.name));
        }
    }
    Ok(None)
}

async fn apply_provider<R: tauri::Runtime>(
    scope: ProfileScope,
    scope_payload: &ProfileScopePayload,
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> AppResult<()> {
    let target = scope.provider_target();
    match scope_payload.provider_id.as_deref() {
        Some(id) if !id.is_empty() => {
            switch_provider_for_target(id, target, Some(app), state).await?;
        }
        _ => {
            switch_to_official_for_target(target, Some(app), state).await?;
        }
    }
    Ok(())
}

fn apply_mcp(
    scope: ProfileScope,
    scope_payload: &ProfileScopePayload,
    state: &AppState,
    warnings: &mut Vec<String>,
) -> AppResult<()> {
    let target = scope.mcp_target();
    let enabled: std::collections::BTreeSet<&str> =
        scope_payload.mcp_ids.iter().map(String::as_str).collect();
    let servers = state.db.with_conn(dao::mcp::list_mcp_servers)?;
    for server in &servers {
        let should_enable = enabled.contains(server.id.as_str());
        let currently_enabled = server.is_enabled_for(target);
        if currently_enabled == should_enable {
            continue;
        }
        if let Err(error) = state.db.with_conn(|conn| {
            dao::mcp::set_mcp_enabled(conn, &server.id, target, should_enable)
        }) {
            warnings.push(format!(
                "{} MCP {}: {error}",
                scope_label(scope),
                server.name
            ));
        }
    }
    for id in &scope_payload.mcp_ids {
        if !servers.iter().any(|server| server.id == *id) {
            warnings.push(format!(
                "{} MCP 不存在，已跳过: {id}",
                scope_label(scope)
            ));
        }
    }
    Ok(())
}

fn apply_skills(
    scope: ProfileScope,
    scope_payload: &ProfileScopePayload,
    warnings: &mut Vec<String>,
) {
    let Some(target) = scope.skill_target() else {
        return;
    };
    let enabled: std::collections::BTreeSet<&str> = scope_payload
        .skill_ids
        .iter()
        .map(String::as_str)
        .collect();
    let skills = match skills::list_skills(target) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("{} Skills 读取失败: {error}", scope_label(scope)));
            return;
        }
    };
    for skill in &skills {
        let should_enable = enabled.contains(skill.name.as_str());
        if skill.enabled == should_enable {
            continue;
        }
        if let Err(error) = skills::set_skill_enabled(&skill.name, should_enable, target) {
            warnings.push(format!(
                "{} Skill {}: {error}",
                scope_label(scope),
                skill.name
            ));
        }
    }
    for name in &scope_payload.skill_ids {
        if !skills.iter().any(|skill| skill.name == *name) {
            warnings.push(format!(
                "{} Skill 不存在，已跳过: {name}",
                scope_label(scope)
            ));
        }
    }
}

fn apply_agents(scope_payload: &ProfileScopePayload, warnings: &mut Vec<String>) {
    let enabled: std::collections::BTreeSet<&str> = scope_payload
        .agent_ids
        .iter()
        .map(String::as_str)
        .collect();
    let agents = match agents::list_agents() {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("Claude Code Agents 读取失败: {error}"));
            return;
        }
    };
    for agent in &agents {
        let should_enable = enabled.contains(agent.name.as_str());
        if agent.enabled == should_enable {
            continue;
        }
        if let Err(error) = agents::set_agent_enabled(&agent.name, should_enable) {
            warnings.push(format!("Claude Code Agent {}: {error}", agent.name));
        }
    }
    for name in &scope_payload.agent_ids {
        if !agents.iter().any(|agent| agent.name == *name) {
            warnings.push(format!("Claude Code Agent 不存在，已跳过: {name}"));
        }
    }
}

fn apply_prompt(
    scope: ProfileScope,
    scope_payload: &ProfileScopePayload,
    warnings: &mut Vec<String>,
) {
    let Some(target) = scope.prompt_target() else {
        return;
    };
    let Some(prompt_id) = scope_payload.prompt_id.as_deref().filter(|id| !id.is_empty()) else {
        return;
    };
    if let Err(error) = prompts::activate_prompt(target, prompt_id) {
        warnings.push(format!(
            "{} Prompt {prompt_id}: {error}",
            scope_label(scope)
        ));
    }
}

fn scope_label(scope: ProfileScope) -> &'static str {
    match scope {
        ProfileScope::ClaudeCode => "Claude Code",
        ProfileScope::ClaudeDesktop => "Claude Desktop",
        ProfileScope::Codex => "Codex",
    }
}

trait McpServerEnabled {
    fn is_enabled_for(&self, target: McpTarget) -> bool;
}

impl McpServerEnabled for crate::mcp::McpServer {
    fn is_enabled_for(&self, target: McpTarget) -> bool {
        match target {
            McpTarget::ClaudeCode => self.enabled_claude_code,
            McpTarget::ClaudeDesktop => self.enabled_claude_desktop,
            McpTarget::Codex => self.enabled_codex,
            McpTarget::OpenCode => self.enabled_opencode,
            McpTarget::Pi => self.enabled_pi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dao::profiles::CURRENT_PROFILE_SETTING_KEY;
    use crate::database::schema::{create_tables, migrate};
    use crate::store::AppState;
    use std::sync::Arc;

    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn snapshot_shape_uses_null_scopes() {
        let scopes = ProfileSnapshotScopes {
            claude_code: true,
            claude_desktop: false,
            codex: false,
        };
        let db = Arc::new(crate::database::Database::memory().unwrap());
        db.with_conn(|conn| {
            create_tables(conn)?;
            migrate(conn)?;
            Ok(())
        })
        .unwrap();
        let (lifecycle_tx, _lifecycle_rx) = unbounded_channel();
        let state = AppState {
            db: Arc::clone(&db),
            proxy: tokio::sync::Mutex::new(crate::proxy::ProxyManager::new(db, lifecycle_tx)),
            proxy_status: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        };
        let payload = snapshot_current(&state, scopes).unwrap();
        assert!(payload.claude_code.is_some());
        assert!(payload.claude_desktop.is_none());
        assert!(payload.codex.is_none());
        let scope = payload.claude_code.unwrap();
        assert!(scope.mcp_ids.is_empty());
        assert!(scope.skill_ids.is_empty());
    }

    #[test]
    fn current_profile_setting_key_is_stable() {
        assert_eq!(CURRENT_PROFILE_SETTING_KEY, "current_profile_id");
    }
}
