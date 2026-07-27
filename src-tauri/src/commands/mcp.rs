//! MCP server management commands.

use crate::database::dao::mcp as dao;
use crate::error::AppResult;
use crate::mcp::{
    self, McpImportSummary, McpServer, McpServerInput, McpTarget,
};
use crate::mcp_registry::{
    resolve_mcp_registry_server, search_mcp_registry as search_registry, RegistryMcpServer,
};
use crate::store::AppState;

/// List all unified MCP servers.
#[tauri::command]
pub fn list_mcp_servers(state: tauri::State<'_, AppState>) -> AppResult<Vec<McpServer>> {
    state.db.with_conn(|conn| dao::list_mcp_servers(conn))
}

/// Create or update a server, then write the enabled set back to both apps.
#[tauri::command]
pub fn save_mcp_server(
    input: McpServerInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<McpServer> {
    let saved = state.db.with_conn(|conn| dao::upsert_mcp_server(conn, &input))?;
    sync_all(&state)?;
    Ok(saved)
}

/// Delete a server, then re-sync (removes its key from both apps' files).
#[tauri::command]
pub fn delete_mcp_server(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::delete_mcp_server(conn, &id))?;
    sync_all(&state)
}

/// Toggle one enabled flag, then re-sync.
#[tauri::command]
pub fn toggle_mcp_server(
    id: String,
    target: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let target = McpTarget::from_str_lossy(&target);
    state
        .db
        .with_conn(|conn| dao::set_mcp_enabled(conn, &id, target, enabled))?;
    sync_all(&state)
}

/// Import the union of both apps' live `mcpServers` maps into the DB.
///
/// A server present in an app is enabled for that app; existing rows keep their
/// stored config (DB is the source of truth once managed). Import does NOT sync
/// back — the next edit/toggle will.
#[tauri::command]
pub fn import_mcp_servers(state: tauri::State<'_, AppState>) -> AppResult<McpImportSummary> {
    let code = mcp::read_code_mcp_servers()?;
    let desktop = mcp::read_desktop_mcp_servers()?;

    state.db.with_conn(|conn| {
        let mut imported = 0i64;
        let mut updated = 0i64;

        for (name, cfg) in &code {
            let in_desktop = desktop.contains_key(name);
            if dao::import_mcp_entry(conn, name, cfg, true, in_desktop)? {
                imported += 1;
            } else {
                updated += 1;
            }
        }
        for (name, cfg) in &desktop {
            if code.contains_key(name) {
                continue; // already handled above
            }
            if dao::import_mcp_entry(conn, name, cfg, false, true)? {
                imported += 1;
            } else {
                updated += 1;
            }
        }
        Ok(McpImportSummary { imported, updated })
    })
}

/// Search the public official MCP Registry. Results are read-only metadata;
/// unsupported entries are returned with an explanation instead of a guessed config.
#[tauri::command]
pub async fn search_mcp_registry(query: String) -> AppResult<Vec<RegistryMcpServer>> {
    search_registry(&query).await
}

/// Add one supported Registry entry to the unified local MCP database and sync it.
#[tauri::command]
pub async fn install_mcp_registry_server(
    name: String,
    enabled_claude_code: bool,
    enabled_claude_desktop: bool,
    state: tauri::State<'_, AppState>,
) -> AppResult<McpServer> {
    let server_config = resolve_mcp_registry_server(&name).await?;
    let input = McpServerInput {
        id: None,
        name: name.trim().to_string(),
        server_config,
        enabled_claude_code,
        enabled_claude_desktop,
    };
    let saved = state.db.with_conn(|conn| dao::upsert_mcp_server(conn, &input))?;
    sync_all(&state)?;
    Ok(saved)
}

/// Load all servers from the DB and write the enabled subsets to both apps.
pub fn sync_all(state: &AppState) -> AppResult<()> {
    let servers = state.db.with_conn(|conn| dao::list_mcp_servers(conn))?;
    mcp::sync_to_files(&servers)
}
