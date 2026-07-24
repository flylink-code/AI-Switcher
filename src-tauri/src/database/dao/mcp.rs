//! MCP server row queries and mutations (table: `mcp_servers`).

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::mcp::{validate_server_input, McpServer, McpServerInput, McpTarget};

/// List all MCP servers ordered by name.
pub fn list_mcp_servers(conn: &Connection) -> AppResult<Vec<McpServer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, server_config, enabled_claude_code, enabled_claude_desktop, created_at
         FROM mcp_servers ORDER BY name ASC;",
    )?;
    let rows = stmt.query_map([], row_to_server)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Fetch a single server by id.
pub fn get_mcp_server(conn: &Connection, id: &str) -> AppResult<Option<McpServer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, server_config, enabled_claude_code, enabled_claude_desktop, created_at
         FROM mcp_servers WHERE id = ?;",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_server(row)?)),
        None => Ok(None),
    }
}

/// Insert or update a server. On update, renaming is allowed; the next sync
/// removes the old key from both apps' files (sync rewrites the whole set).
pub fn upsert_mcp_server(conn: &Connection, input: &McpServerInput) -> AppResult<McpServer> {
    validate_server_input(input)?;
    let name = input.name.trim();
    let config_str = serde_json::to_string(&input.server_config)?;

    if let Some(id) = input.id.as_ref() {
        if let Some(existing) = get_by_name(conn, name)? {
            if existing.id != *id {
                return Err(AppError::Config(format!("MCP 服务器名称已存在: {name}")));
            }
        }
        let changed = conn.execute(
            "UPDATE mcp_servers SET
                name = ?, server_config = ?, enabled_claude_code = ?, enabled_claude_desktop = ?
             WHERE id = ?;",
            params![
                name,
                config_str,
                input.enabled_claude_code as i64,
                input.enabled_claude_desktop as i64,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::Config(format!("MCP 服务器不存在: {id}")));
        }
        return get_mcp_server(conn, id)?
            .ok_or_else(|| AppError::Config(format!("MCP 服务器不存在: {id}")));
    }

    // Create. Reject duplicate names — the name is the key in the apps' files.
    if get_by_name(conn, name)?.is_some() {
        return Err(AppError::Config(format!("MCP 服务器名称已存在: {name}")));
    }
    let id = format!("mcp_{}", uuid_v8());
    let now = Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO mcp_servers
            (id, name, server_config, enabled_claude_code, enabled_claude_desktop, created_at)
         VALUES (?, ?, ?, ?, ?, ?);",
        params![
            id,
            name,
            config_str,
            input.enabled_claude_code as i64,
            input.enabled_claude_desktop as i64,
            now,
        ],
    )?;
    get_mcp_server(conn, &id)?.ok_or_else(|| AppError::Config("插入后未能读回 MCP 服务器".to_string()))
}

/// Delete a server by id.
pub fn delete_mcp_server(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM mcp_servers WHERE id = ?;", params![id])?;
    if changed == 0 {
        return Err(AppError::Config(format!("MCP 服务器不存在: {id}")));
    }
    Ok(())
}

/// Flip one enabled flag.
pub fn set_mcp_enabled(
    conn: &Connection,
    id: &str,
    target: McpTarget,
    enabled: bool,
) -> AppResult<()> {
    let column = match target {
        McpTarget::ClaudeCode => "enabled_claude_code",
        McpTarget::ClaudeDesktop => "enabled_claude_desktop",
    };
    let changed = conn.execute(
        &format!("UPDATE mcp_servers SET {column} = ? WHERE id = ?;"),
        params![enabled as i64, id],
    )?;
    if changed == 0 {
        return Err(AppError::Config(format!("MCP 服务器不存在: {id}")));
    }
    Ok(())
}

/// Import one server discovered in an application's live config.
///
/// - Name unknown → insert a new row (enabled only where it was found).
/// - Name known → raise the found-in flags, keep the stored config (DB wins).
///
/// Returns `true` when a new row was inserted.
pub fn import_mcp_entry(
    conn: &Connection,
    name: &str,
    config: &Value,
    in_code: bool,
    in_desktop: bool,
) -> AppResult<bool> {
    if let Some(existing) = get_by_name(conn, name)? {
        if in_code {
            set_mcp_enabled(conn, &existing.id, McpTarget::ClaudeCode, true)?;
        }
        if in_desktop {
            set_mcp_enabled(conn, &existing.id, McpTarget::ClaudeDesktop, true)?;
        }
        return Ok(false);
    }
    let input = McpServerInput {
        id: None,
        name: name.to_string(),
        server_config: config.clone(),
        enabled_claude_code: in_code,
        enabled_claude_desktop: in_desktop,
    };
    upsert_mcp_server(conn, &input)?;
    Ok(true)
}

fn get_by_name(conn: &Connection, name: &str) -> AppResult<Option<McpServer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, server_config, enabled_claude_code, enabled_claude_desktop, created_at
         FROM mcp_servers WHERE name = ?;",
    )?;
    let mut rows = stmt.query(params![name])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_server(row)?)),
        None => Ok(None),
    }
}

fn row_to_server(row: &rusqlite::Row<'_>) -> rusqlite::Result<McpServer> {
    let config_str: String = row.get(2)?;
    let server_config: Value =
        serde_json::from_str(&config_str).unwrap_or_else(|_| Value::Object(Default::default()));
    Ok(McpServer {
        id: row.get(0)?,
        name: row.get(1)?,
        server_config,
        enabled_claude_code: row.get::<_, i64>(3)? != 0,
        enabled_claude_desktop: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
    })
}

/// 8-char hex id derived from a UUIDv4 (same convention as providers).
fn uuid_v8() -> String {
    let mut buf = uuid::Uuid::encode_buffer();
    let s = uuid::Uuid::new_v4().simple().encode_upper(&mut buf);
    s[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;

    fn input(name: &str) -> McpServerInput {
        McpServerInput {
            id: None,
            name: name.to_string(),
            server_config: json!({"command": "npx", "args": ["-y", name]}),
            enabled_claude_code: true,
            enabled_claude_desktop: false,
        }
    }

    #[test]
    fn crud_and_toggle_works() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let a = upsert_mcp_server(conn, &input("alpha"))?;
            assert_eq!(list_mcp_servers(conn)?.len(), 1);
            assert!(a.enabled_claude_code && !a.enabled_claude_desktop);

            // duplicate name rejected
            assert!(upsert_mcp_server(conn, &input("alpha")).is_err());

            // toggle desktop flag
            set_mcp_enabled(conn, &a.id, McpTarget::ClaudeDesktop, true)?;
            let after = get_mcp_server(conn, &a.id)?.unwrap();
            assert!(after.enabled_claude_desktop);

            // rename via update
            let mut upd = input("beta");
            upd.id = Some(a.id.clone());
            let renamed = upsert_mcp_server(conn, &upd)?;
            assert_eq!(renamed.name, "beta");

            delete_mcp_server(conn, &a.id)?;
            assert!(list_mcp_servers(conn)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn import_inserts_then_raises_flags() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let cfg = json!({"url": "http://x"});
            // First seen in Claude Code only.
            assert!(import_mcp_entry(conn, "svc", &cfg, true, false)?);
            // Later seen in Desktop too: not a new row, flag raised.
            assert!(!import_mcp_entry(conn, "svc", &cfg, false, true)?);
            let s = list_mcp_servers(conn)?.pop().unwrap();
            assert!(s.enabled_claude_code && s.enabled_claude_desktop);
            Ok(())
        })
        .unwrap();
    }
}
