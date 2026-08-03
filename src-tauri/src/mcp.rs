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

use std::fs;
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
    pub sort_index: i64,
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
/// keyed by name in `sort_index` order.
fn enabled_map(servers: &[McpServer], target: McpTarget) -> Map<String, Value> {
    servers
        .iter()
        .filter(|s| match target {
            McpTarget::ClaudeCode => s.enabled_claude_code,
            McpTarget::ClaudeDesktop => s.enabled_claude_desktop,
            McpTarget::Codex => s.enabled_codex,
        })
        .map(|s| (s.name.clone(), s.server_config.clone()))
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

/// Desktop Connectors / `.mcpb` / `.dxt` coexistence notice (does not parse private formats).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDesktopConflictStatus {
    pub desktop_installed: bool,
    pub managed_desktop_servers: usize,
    pub live_desktop_servers: usize,
    pub extension_artifacts: Vec<String>,
    pub conflict_likely: bool,
    pub message: Option<String>,
}

/// Detect whether managed `mcpServers` may collide with Desktop Connectors / extensions.
pub fn get_desktop_connector_status(servers: &[McpServer]) -> AppResult<McpDesktopConflictStatus> {
    let desktop = detect_claude_desktop();
    let Some(base) = desktop.base.as_ref() else {
        return Ok(McpDesktopConflictStatus {
            desktop_installed: false,
            managed_desktop_servers: 0,
            live_desktop_servers: 0,
            extension_artifacts: Vec::new(),
            conflict_likely: false,
            message: None,
        });
    };

    let managed_desktop_servers = servers
        .iter()
        .filter(|server| server.enabled_claude_desktop)
        .count();
    let live_desktop_servers = read_desktop_mcp_servers()?.len();
    let extension_artifacts = collect_extension_artifacts(base);
    let conflict_likely =
        !extension_artifacts.is_empty() && (managed_desktop_servers > 0 || live_desktop_servers > 0);
    let message = if conflict_likely {
        Some(
            "检测到 Claude Desktop Connectors / 扩展包（.mcpb/.dxt）与 JSON mcpServers 可能并存。官方扩展与手写 mcpServers 可能互相覆盖或导致工具不可见；本应用不会复刻 Connectors UI，建议只保留一侧来源。"
                .into(),
        )
    } else if !extension_artifacts.is_empty() {
        Some(
            "已检测到 Claude Desktop 扩展 / Connectors 制品；若之后再启用 JSON mcpServers，可能发生冲突。"
                .into(),
        )
    } else {
        None
    };

    Ok(McpDesktopConflictStatus {
        desktop_installed: true,
        managed_desktop_servers,
        live_desktop_servers,
        extension_artifacts,
        conflict_likely,
        message,
    })
}

fn collect_extension_artifacts(base: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let candidate_dirs = [
        base.join("Claude Extensions"),
        base.join("Extensions"),
        base.join("extensions"),
        base.join("Local Extension Settings"),
    ];
    for dir in candidate_dirs {
        push_extension_names(&dir, &mut found);
    }
    // Shallow scan of the Desktop base for packaged extensions.
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".mcpb") || lower.ends_with(".dxt") {
                found.push(name.to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn push_extension_names(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if path.is_dir() || lower.ends_with(".mcpb") || lower.ends_with(".dxt") {
            found.push(name.to_string());
        }
    }
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
            sort_index: 0,
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
