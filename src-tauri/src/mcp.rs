//! Unified MCP server management across Claude Code and Claude Desktop.
//!
//! The SQLite `mcp_servers` table is the single source of truth. Each row holds
//! one server definition (`server_config`, the raw JSON entry) plus two enabled
//! flags. Syncing writes the enabled subset back to:
//!
//! - Claude Code: `~/.claude.json` → top-level `mcpServers` object
//! - Claude Desktop: `<base>/claude_desktop_config.json` → top-level `mcpServers`
//!
//! Only the `mcpServers` key is touched; every other key in those files (project
//! roots, UI prefs, ...) is preserved, and a timestamped backup is written before
//! every mutation (see `claude_code.rs` for the same pattern).
//!
//! See task.md §2.5: "统一面板管理 + 双向同步：从现有配置导入，编辑后写回各应用".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::backup::backup_file_named;
use crate::config::claude_desktop::detect_claude_desktop;
use crate::config::{get_claude_json_path, read_json_file, write_json_file};
use crate::error::{AppError, AppResult};

/// Max backups of each MCP-bearing config file to retain.
const MCP_BACKUP_KEEP: usize = 10;

/// Which application an enabled flag targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTarget {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
}

impl McpTarget {
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "claude_desktop" => McpTarget::ClaudeDesktop,
            "codex" => McpTarget::Codex,
            _ => McpTarget::ClaudeCode,
        }
    }
}

/// One MCP server row. `server_config` is the raw JSON entry as it appears under
/// `mcpServers.<name>` (e.g. `{"command": "npx", "args": [...]}` or an SSE/HTTP
/// variant with `url`). Field names are camelCase on the wire for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub server_config: Value,
    pub enabled_claude_code: bool,
    pub enabled_claude_desktop: bool,
    pub enabled_codex: bool,
    pub created_at: i64,
}

/// Input shape for create/update commands. `id` is omitted on create.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub server_config: Value,
    #[serde(default)]
    pub enabled_claude_code: bool,
    #[serde(default)]
    pub enabled_claude_desktop: bool,
    #[serde(default)]
    pub enabled_codex: bool,
}

/// Result of importing the live configs from both applications.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportSummary {
    /// Newly inserted rows.
    pub imported: i64,
    /// Existing rows whose enabled flags were raised.
    pub updated: i64,
}

// ---- file locations ---------------------------------------------------------

/// `~/.claude.json` — Claude Code MCP servers + project roots.
pub fn code_mcp_path() -> PathBuf {
    get_claude_json_path()
}

/// `<base>/claude_desktop_config.json`, or `None` when Claude Desktop is not
/// installed (detection follows the same candidates as the gateway config).
pub fn desktop_mcp_path() -> Option<PathBuf> {
    detect_claude_desktop()
        .base
        .map(|b| b.join("claude_desktop_config.json"))
}

// ---- reading ----------------------------------------------------------------

/// Read Claude Code's `mcpServers` map. Missing file / missing key → empty.
pub fn read_code_mcp_servers() -> AppResult<Map<String, Value>> {
    read_mcp_map(&code_mcp_path())
}

/// Read Claude Desktop's `mcpServers` map. Not installed → empty.
pub fn read_desktop_mcp_servers() -> AppResult<Map<String, Value>> {
    match desktop_mcp_path() {
        Some(path) => read_mcp_map(&path),
        None => Ok(Map::new()),
    }
}

fn read_mcp_map(path: &Path) -> AppResult<Map<String, Value>> {
    let Some(value) = read_json_file::<Value>(path)? else {
        return Ok(Map::new());
    };
    Ok(value
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

// ---- writing ----------------------------------------------------------------

/// Write the enabled subsets back to both applications. Desktop is skipped when
/// not installed. Files that exist are backed up first.
pub fn sync_to_files(servers: &[McpServer]) -> AppResult<()> {
    let code_map = enabled_map(servers, McpTarget::ClaudeCode);
    write_mcp_map(&code_mcp_path(), &code_map)?;

    if let Some(path) = desktop_mcp_path() {
        let desktop_map = enabled_map(servers, McpTarget::ClaudeDesktop);
        write_mcp_map(&path, &desktop_map)?;
    }
    Ok(())
}

/// Build the `mcpServers` object for one application: enabled servers only,
/// keyed by name (BTreeMap for deterministic output order).
fn enabled_map(servers: &[McpServer], target: McpTarget) -> Map<String, Value> {
    let sorted: BTreeMap<&str, &Value> = servers
        .iter()
        .filter(|s| match target {
            McpTarget::ClaudeCode => s.enabled_claude_code,
            McpTarget::ClaudeDesktop => s.enabled_claude_desktop,
            McpTarget::Codex => s.enabled_codex,
        })
        .map(|s| (s.name.as_str(), &s.server_config))
        .collect();
    sorted
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// Replace the `mcpServers` key in `path`, preserving all other top-level keys.
/// The key is removed entirely when the set is empty (matches a hand-edited file).
fn write_mcp_map(path: &Path, servers: &Map<String, Value>) -> AppResult<()> {
    let mut value = read_json_file::<Value>(path)?.unwrap_or_else(|| Value::Object(Map::new()));
    if !value.is_object() {
        log::warn!("{} 不是 JSON 对象，将以空对象为基础重建", path.display());
        value = Value::Object(Map::new());
    }

    if path.exists() {
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mcp.json".to_string());
        backup_file_named(path, &stem, MCP_BACKUP_KEEP)?;
    }

    let obj = value.as_object_mut().expect("normalized to object");
    if servers.is_empty() {
        obj.remove("mcpServers");
    } else {
        obj.insert("mcpServers".to_string(), Value::Object(servers.clone()));
    }
    write_json_file(path, &value)
}

/// Validate a server name (used as the key in both apps' `mcpServers` objects).
pub fn validate_server_input(input: &McpServerInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::Config("MCP 服务器名称不能为空".to_string()));
    }
    if !input.server_config.is_object() {
        return Err(AppError::Config(
            "MCP 服务器配置必须是 JSON 对象".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    fn server(name: &str, code: bool, desktop: bool) -> McpServer {
        McpServer {
            id: format!("mcp_{name}"),
            name: name.to_string(),
            server_config: json!({"command": "npx", "args": ["-y", name]}),
            enabled_claude_code: code,
            enabled_claude_desktop: desktop,
            enabled_codex: false,
            created_at: 0,
        }
    }

    #[test]
    fn enabled_map_filters_by_target() {
        let servers = vec![
            server("a", true, false),
            server("b", false, true),
            server("c", true, true),
        ];
        let code = enabled_map(&servers, McpTarget::ClaudeCode);
        let desktop = enabled_map(&servers, McpTarget::ClaudeDesktop);
        assert!(code.contains_key("a") && code.contains_key("c") && !code.contains_key("b"));
        assert!(desktop.contains_key("b") && desktop.contains_key("c") && !desktop.contains_key("a"));
    }

    #[test]
    fn write_mcp_map_preserves_other_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        fs::write(
            &path,
            json!({"projects": {"/x": {}}, "mcpServers": {"old": {"command": "old"}}}).to_string(),
        )
        .unwrap();

        let mut new_map = Map::new();
        new_map.insert("fs".to_string(), json!({"command": "npx"}));
        write_mcp_map(&path, &new_map).unwrap();

        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("projects").is_some(), "unrelated key preserved");
        let servers = written["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("fs") && !servers.contains_key("old"));
    }

    #[test]
    fn write_empty_map_removes_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        fs::write(&path, json!({"mcpServers": {"a": {}}, "keep": 1}).to_string()).unwrap();

        write_mcp_map(&path, &Map::new()).unwrap();
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("mcpServers").is_none());
        assert_eq!(written["keep"], 1);
    }

    #[test]
    fn read_missing_file_returns_empty() {
        let dir = tempdir().unwrap();
        let map = read_mcp_map(&dir.path().join("nope.json")).unwrap();
        assert!(map.is_empty());
    }
}
