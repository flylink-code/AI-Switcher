//! Claude Code MCP OAuth credential status (read / clear, never expose tokens).
//!
//! Storage (Claude Code):
//! - Windows / Linux: `~/.claude/.credentials.json` → top-level `mcpOAuth`
//! - macOS: Keychain item service `Claude Code-credentials` (same JSON shape)
//!
//! Keys under `mcpOAuth` look like `serverName|base64(...)`. We only surface the
//! server name prefix and allow clearing entries without returning secrets.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::error::{AppError, AppResult};

const MCP_OAUTH_KEY: &str = "mcpOAuth";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNTS: &[&str] = &["Claude Code", "credentials", ""];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOauthStatus {
    pub storage: String,
    pub path: Option<String>,
    pub server_names: Vec<String>,
    pub entry_count: usize,
    pub clearable: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearMcpOauthInput {
    /// When empty / omitted, clear all `mcpOAuth` entries.
    #[serde(default)]
    pub server_names: Vec<String>,
}

pub fn get_mcp_oauth_status() -> AppResult<McpOauthStatus> {
    let Some((storage, path, value)) = load_credentials_document()? else {
        return Ok(McpOauthStatus {
            storage: "none".into(),
            path: None,
            server_names: Vec::new(),
            entry_count: 0,
            clearable: false,
            note: Some("未检测到 Claude Code MCP OAuth 凭证存储".into()),
        });
    };

    let names = mcp_oauth_server_names(&value);
    drop(value);
    let clearable = storage == "file";
    let note = if storage == "keychain" {
        Some("macOS Keychain 中的凭证请用 `claude mcp logout` / `/mcp` 管理".into())
    } else if names.is_empty() {
        Some("当前没有已保存的 MCP OAuth 条目".into())
    } else {
        None
    };

    Ok(McpOauthStatus {
        storage,
        path: path.map(|p| p.to_string_lossy().into_owned()),
        entry_count: names.len(),
        server_names: names,
        clearable,
        note,
    })
}

pub fn clear_mcp_oauth(input: ClearMcpOauthInput) -> AppResult<McpOauthStatus> {
    let Some((storage, path, mut value)) = load_credentials_document()? else {
        return get_mcp_oauth_status();
    };

    if storage != "file" {
        return Err(AppError::Config(
            "当前平台通过 Keychain 保存 MCP OAuth，请在 Claude Code 中执行 logout".to_string(),
        ));
    }
    let Some(path) = path else {
        return Err(AppError::Config("未找到凭证文件路径".to_string()));
    };

    let Some(obj) = value.as_object_mut() else {
        return Err(AppError::Config("凭证文件格式无效".to_string()));
    };

    let targets: BTreeSet<String> = input
        .server_names
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    match obj.get_mut(MCP_OAUTH_KEY).and_then(Value::as_object_mut) {
        Some(mcp) => {
            if targets.is_empty() {
                mcp.clear();
            } else {
                let keys: Vec<String> = mcp
                    .keys()
                    .filter(|key| targets.contains(server_name_from_oauth_key(key)))
                    .cloned()
                    .collect();
                for key in keys {
                    mcp.remove(&key);
                }
            }
            if mcp.is_empty() {
                obj.remove(MCP_OAUTH_KEY);
            }
        }
        None => {}
    }

    write_credentials_file(&path, &value)?;
    get_mcp_oauth_status()
}

fn credentials_file_path() -> PathBuf {
    get_claude_config_dir().join(".credentials.json")
}

fn load_credentials_document() -> AppResult<Option<(String, Option<PathBuf>, Value)>> {
    let file_path = credentials_file_path();
    if file_path.exists() {
        let raw = fs::read_to_string(&file_path)?;
        let value: Value = serde_json::from_str(&raw).map_err(|err| {
            AppError::Config(format!("解析 .credentials.json 失败: {err}"))
        })?;
        return Ok(Some(("file".into(), Some(file_path), value)));
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(value) = read_keychain_credentials()? {
            return Ok(Some(("keychain".into(), None, value)));
        }
    }

    Ok(None)
}

fn write_credentials_file(path: &std::path::Path, value: &Value) -> AppResult<()> {
    let pretty = serde_json::to_string_pretty(value)
        .map_err(|err| AppError::Config(format!("序列化凭证失败: {err}")))?;
    fs::write(path, pretty)?;
    Ok(())
}

fn mcp_oauth_server_names(value: &Value) -> Vec<String> {
    let Some(map) = value.get(MCP_OAUTH_KEY).and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for key in map.keys() {
        names.insert(server_name_from_oauth_key(key).to_string());
    }
    names.into_iter().collect()
}

fn server_name_from_oauth_key(key: &str) -> &str {
    key.split_once('|').map(|(name, _)| name).unwrap_or(key)
}

#[cfg(target_os = "macos")]
fn read_keychain_credentials() -> AppResult<Option<Value>> {
    for account in KEYCHAIN_ACCOUNTS {
        let entry = match keyring::Entry::new(KEYCHAIN_SERVICE, account) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        match entry.get_password() {
            Ok(raw) => {
                let value: Value = serde_json::from_str(&raw).map_err(|err| {
                    AppError::Config(format!("解析 Keychain 凭证失败: {err}"))
                })?;
                return Ok(Some(value));
            }
            Err(keyring::Error::NoEntry) => continue,
            Err(err) => {
                log::debug!("读取 Claude Code Keychain 失败 ({account}): {err}");
                continue;
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_server_names_from_oauth_keys() {
        let value = serde_json::json!({
            "mcpOAuth": {
                "notion|YWJj": { "accessToken": "secret" },
                "github|ZGVm": { "accessToken": "secret" },
                "plain": { "accessToken": "secret" }
            }
        });
        assert_eq!(
            mcp_oauth_server_names(&value),
            vec!["github".to_string(), "notion".to_string(), "plain".to_string()]
        );
    }

    #[test]
    fn clearing_targets_preserves_unrelated_keys() {
        let mut value = serde_json::json!({
            "claudeAiOauth": { "accessToken": "keep" },
            "mcpOAuth": {
                "notion|a": { "accessToken": "x" },
                "github|b": { "accessToken": "y" }
            }
        });
        let obj = value.as_object_mut().unwrap();
        let mcp = obj.get_mut("mcpOAuth").unwrap().as_object_mut().unwrap();
        let keys: Vec<_> = mcp
            .keys()
            .filter(|key| server_name_from_oauth_key(key) == "notion")
            .cloned()
            .collect();
        for key in keys {
            mcp.remove(&key);
        }
        assert!(mcp.contains_key("github|b"));
        assert!(!mcp.contains_key("notion|a"));
        assert!(obj.get("claudeAiOauth").is_some());
    }
}
