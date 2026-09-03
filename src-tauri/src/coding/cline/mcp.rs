//! Cline MCP 配置：`~/.cline/data/settings/cline_mcp_settings.json`。
//!
//! 只改写顶层 `mcpServers`；保留其余键。

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::backup::backup_file_named;
use crate::config::atomic::{read_json_file, write_json_file};
use crate::config::cline::cline_mcp_settings_path;
use crate::error::AppResult;
use crate::mcp::McpServer;

const MCP_BACKUP_KEEP: usize = 10;

pub fn get_cline_mcp_path() -> PathBuf {
    cline_mcp_settings_path()
}

pub fn read_mcp_servers() -> AppResult<Map<String, Value>> {
    read_mcp_servers_at(&get_cline_mcp_path())
}

fn read_mcp_servers_at(path: &Path) -> AppResult<Map<String, Value>> {
    let Some(value) = read_json_file::<Value>(path)? else {
        return Ok(Map::new());
    };
    Ok(value
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

pub fn sync_mcp_servers(servers: &[McpServer]) -> AppResult<()> {
    sync_mcp_servers_at(&get_cline_mcp_path(), servers)
}

pub fn sync_mcp_servers_at(path: &Path, servers: &[McpServer]) -> AppResult<()> {
    let mut map = Map::new();
    for server in servers.iter().filter(|s| s.enabled_cline) {
        map.insert(server.name.clone(), server.server_config.clone());
    }

    let mut value = read_json_file::<Value>(path)?.unwrap_or_else(|| Value::Object(Map::new()));
    if !value.is_object() {
        log::warn!("{} 不是 JSON 对象，将以空对象为基础重建", path.display());
        value = Value::Object(Map::new());
    }

    if path.exists() {
        backup_file_named(path, "cline_mcp_settings.json", MCP_BACKUP_KEEP)?;
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let obj = value.as_object_mut().expect("normalized to object");
    if map.is_empty() {
        obj.remove("mcpServers");
    } else {
        obj.insert("mcpServers".to_string(), Value::Object(map));
    }
    write_json_file(path, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    fn server(name: &str, enabled_cline: bool) -> McpServer {
        McpServer {
            id: format!("mcp_{name}"),
            name: name.to_string(),
            server_config: json!({"command": "npx", "args": ["-y", name]}),
            enabled_claude_code: false,
            enabled_claude_desktop: false,
            enabled_codex: false,
            enabled_opencode: false,
            enabled_pi: false,
            enabled_cline,
            sort_index: 0,
            created_at: 0,
        }
    }

    #[test]
    fn sync_writes_enabled_and_preserves_other_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cline_mcp_settings.json");
        fs::write(&path, r#"{"keep":1,"mcpServers":{"old":{"command":"x"}}}"#).unwrap();

        sync_mcp_servers_at(&path, &[server("alpha", true), server("beta", false)]).unwrap();

        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["keep"], 1);
        assert!(value["mcpServers"].get("alpha").is_some());
        assert!(value["mcpServers"].get("beta").is_none());
        assert!(value["mcpServers"].get("old").is_none());
    }
}
