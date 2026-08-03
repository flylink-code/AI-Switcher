//! MCP server management commands.

use crate::database::dao::mcp as dao;
use crate::error::AppResult;
use crate::mcp::{
    self, McpDesktopConflictStatus, McpImportSummary, McpServer, McpServerInput, McpTarget,
};
use crate::mcp_oauth::{
    clear_mcp_oauth as clear_oauth_entries, get_mcp_oauth_status as read_oauth_status,
    ClearMcpOauthInput, McpOauthStatus,
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

/// Reorder MCP servers by id list.
#[tauri::command]
pub fn reorder_mcp_servers(
    ordered_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| dao::reorder_mcp_servers(conn, &ordered_ids))?;
    sync_all(&state)?;
    Ok(())
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
    let codex = crate::config::codex::read_mcp_servers()?;

    state.db.with_conn(|conn| {
        let mut imported = 0i64;
        let mut updated = 0i64;

        for (name, cfg) in &code {
            let in_desktop = desktop.contains_key(name);
            if dao::import_mcp_entry(conn, name, cfg, true, in_desktop, codex.contains_key(name))? {
                imported += 1;
            } else {
                updated += 1;
            }
        }
        for (name, cfg) in &desktop {
            if code.contains_key(name) {
                continue; // already handled above
            }
            if dao::import_mcp_entry(conn, name, cfg, false, true, codex.contains_key(name))? {
                imported += 1;
            } else {
                updated += 1;
            }
        }
        for (name, cfg) in &codex {
            if code.contains_key(name) || desktop.contains_key(name) { continue; }
            if dao::import_mcp_entry(conn, name, cfg, false, false, true)? { imported += 1; } else { updated += 1; }
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
        enabled_codex: false,
    };
    let saved = state.db.with_conn(|conn| dao::upsert_mcp_server(conn, &input))?;
    sync_all(&state)?;
    Ok(saved)
}

/// Claude Code MCP OAuth credential status (names only; no tokens).
#[tauri::command]
pub fn get_mcp_oauth_status() -> AppResult<McpOauthStatus> {
    read_oauth_status()
}

/// Clear Claude Code MCP OAuth entries from the credentials file.
#[tauri::command]
pub fn clear_mcp_oauth(input: ClearMcpOauthInput) -> AppResult<McpOauthStatus> {
    clear_oauth_entries(input)
}

/// Desktop Connectors / `.mcpb` coexistence notice for the MCP page.
#[tauri::command]
pub fn get_mcp_desktop_conflict_status(
    state: tauri::State<'_, AppState>,
) -> AppResult<McpDesktopConflictStatus> {
    let servers = state.db.with_conn(|conn| dao::list_mcp_servers(conn))?;
    mcp::get_desktop_connector_status(&servers)
}

/// Load all servers from the DB and write the enabled subsets to both apps.
pub fn sync_all(state: &AppState) -> AppResult<()> {
    let servers = state.db.with_conn(|conn| dao::list_mcp_servers(conn))?;
    mcp::sync_to_files(&servers)?;
    crate::config::codex::sync_mcp_servers(&servers)
}
