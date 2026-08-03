//! Workspace configuration snapshot (profile) rows.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const CURRENT_PROFILE_SETTING_KEY: &str = "current_profile_id";

/// Per-application scope stored inside a profile payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileScopePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub mcp_ids: Vec<String>,
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
}

/// Full profile payload: null scope = not included on apply.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_code: Option<ProfileScopePayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_desktop: Option<ProfileScopePayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex: Option<ProfileScopePayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub payload: ProfilePayload,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshotScopes {
    pub claude_code: bool,
    pub claude_desktop: bool,
    pub codex: bool,
}

const PROFILE_SELECT: &str =
    "SELECT id, name, payload_json, sort_order, created_at, updated_at";

pub fn list_profiles(conn: &Connection) -> AppResult<Vec<Profile>> {
    let mut stmt = conn.prepare(&format!(
        "{PROFILE_SELECT} FROM profiles ORDER BY sort_order ASC, created_at ASC;"
    ))?;
    let rows = stmt.query_map([], row_to_profile)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_profile(conn: &Connection, id: &str) -> AppResult<Option<Profile>> {
    let mut stmt = conn.prepare(&format!("{PROFILE_SELECT} FROM profiles WHERE id = ?;"))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_profile(row)?)),
        None => Ok(None),
    }
}

pub fn create_profile(
    conn: &Connection,
    name: &str,
    payload: &ProfilePayload,
) -> AppResult<Profile> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("配置快照名称不能为空".to_string()));
    }
    if get_by_name(conn, trimmed)?.is_some() {
        return Err(AppError::Config(format!("配置快照名称已存在: {trimmed}")));
    }
    let id = format!("profile_{}", uuid_v8());
    let now = Utc::now().timestamp_millis();
    let sort_order = next_sort_order(conn)?;
    let payload_json = serde_json::to_string(payload)?;
    conn.execute(
        "INSERT INTO profiles (id, name, payload_json, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?);",
        params![id, trimmed, payload_json, sort_order, now, now],
    )?;
    get_profile(conn, &id)?.ok_or_else(|| AppError::Config("插入后未能读回配置快照".to_string()))
}

pub fn update_profile(
    conn: &Connection,
    id: &str,
    name: Option<&str>,
    payload: Option<&ProfilePayload>,
) -> AppResult<Profile> {
    let existing = get_profile(conn, id)?
        .ok_or_else(|| AppError::Config(format!("配置快照不存在: {id}")))?;
    let next_name = match name {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(AppError::Config("配置快照名称不能为空".to_string()));
            }
            if let Some(other) = get_by_name(conn, trimmed)? {
                if other.id != id {
                    return Err(AppError::Config(format!("配置快照名称已存在: {trimmed}")));
                }
            }
            trimmed.to_string()
        }
        None => existing.name,
    };
    let next_payload = payload.cloned().unwrap_or(existing.payload);
    let payload_json = serde_json::to_string(&next_payload)?;
    let now = Utc::now().timestamp_millis();
    let changed = conn.execute(
        "UPDATE profiles SET name = ?, payload_json = ?, updated_at = ? WHERE id = ?;",
        params![next_name, payload_json, now, id],
    )?;
    if changed == 0 {
        return Err(AppError::Config(format!("配置快照不存在: {id}")));
    }
    get_profile(conn, id)?.ok_or_else(|| AppError::Config(format!("配置快照不存在: {id}")))
}

/// Rewrite `prompt_id` references after a Prompt preset rename.
pub fn rewrite_prompt_id(
    conn: &Connection,
    scope: PromptRenameScope,
    old_name: &str,
    new_name: &str,
) -> AppResult<usize> {
    let mut updated = 0usize;
    for mut profile in list_profiles(conn)? {
        let changed = match scope {
            PromptRenameScope::ClaudeCode => rewrite_scope_prompt(&mut profile.payload.claude_code, old_name, new_name),
            PromptRenameScope::Codex => rewrite_scope_prompt(&mut profile.payload.codex, old_name, new_name),
        };
        if !changed {
            continue;
        }
        update_profile(conn, &profile.id, None, Some(&profile.payload))?;
        updated += 1;
    }
    Ok(updated)
}

#[derive(Debug, Clone, Copy)]
pub enum PromptRenameScope {
    ClaudeCode,
    Codex,
}

fn rewrite_scope_prompt(
    scope: &mut Option<ProfileScopePayload>,
    old_name: &str,
    new_name: &str,
) -> bool {
    let Some(payload) = scope.as_mut() else {
        return false;
    };
    if payload.prompt_id.as_deref() == Some(old_name) {
        payload.prompt_id = Some(new_name.to_string());
        return true;
    }
    false
}

pub fn delete_profile(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM profiles WHERE id = ?;", params![id])?;
    if changed == 0 {
        return Err(AppError::Config(format!("配置快照不存在: {id}")));
    }
    Ok(())
}

pub fn get_current_profile_id(conn: &Connection) -> AppResult<Option<String>> {
    crate::database::dao::settings::get_setting(conn, CURRENT_PROFILE_SETTING_KEY)
}

pub fn set_current_profile_id(conn: &Connection, id: Option<&str>) -> AppResult<()> {
    match id {
        Some(value) => crate::database::dao::settings::set_setting(
            conn,
            CURRENT_PROFILE_SETTING_KEY,
            value,
        ),
        None => {
            conn.execute(
                "DELETE FROM settings WHERE key = ?;",
                params![CURRENT_PROFILE_SETTING_KEY],
            )?;
            Ok(())
        }
    }
}

fn get_by_name(conn: &Connection, name: &str) -> AppResult<Option<Profile>> {
    let mut stmt = conn.prepare(&format!("{PROFILE_SELECT} FROM profiles WHERE name = ?;"))?;
    let mut rows = stmt.query(params![name])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_profile(row)?)),
        None => Ok(None),
    }
}

fn next_sort_order(conn: &Connection) -> AppResult<i64> {
    let max: Option<i64> = conn
        .query_row("SELECT MAX(sort_order) FROM profiles;", [], |row| row.get(0))
        .ok();
    Ok(max.unwrap_or(-1) + 1)
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let payload_json: String = row.get(2)?;
    let payload: ProfilePayload = serde_json::from_str(&payload_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Profile {
        id: row.get(0)?,
        name: row.get(1)?,
        payload,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn uuid_v8() -> String {
    let mut buf = uuid::Uuid::encode_buffer();
    let value = uuid::Uuid::new_v4().simple().encode_upper(&mut buf);
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::schema::{create_tables, migrate};

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn profile_payload_serializes_ids_only() {
        let payload = ProfilePayload {
            claude_code: Some(ProfileScopePayload {
                provider_id: Some("p_ABC".to_string()),
                mcp_ids: vec!["mcp_1".to_string()],
                skill_ids: vec!["commit".to_string()],
                agent_ids: vec!["reviewer".to_string()],
                prompt_id: Some("default".to_string()),
            }),
            claude_desktop: None,
            codex: Some(ProfileScopePayload {
                provider_id: None,
                mcp_ids: vec![],
                skill_ids: vec![],
                agent_ids: vec![],
                prompt_id: None,
            }),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"providerId\":\"p_ABC\""));
        assert!(json.contains("\"mcpIds\":[\"mcp_1\"]"));
        assert!(!json.contains("claude_desktop"));
        let parsed: ProfilePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, payload);
    }

    #[test]
    fn profile_crud_round_trip() {
        let conn = test_conn();
        let payload = ProfilePayload {
            claude_code: Some(ProfileScopePayload::default()),
            ..ProfilePayload::default()
        };
        let created = create_profile(&conn, "Work", &payload).unwrap();
        assert_eq!(created.name, "Work");
        let listed = list_profiles(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        let updated = update_profile(&conn, &created.id, Some("Work 2"), None).unwrap();
        assert_eq!(updated.name, "Work 2");
        delete_profile(&conn, &created.id).unwrap();
        assert!(list_profiles(&conn).unwrap().is_empty());
    }
}
