//! Codex configuration integration.
//!
//! Only AI-Switcher-owned model-provider entries are updated.  `toml_edit`
//! retains comments and unrelated settings in users' config.toml files.

use std::fs;
use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::config::{atomic_write, get_backup_dir, get_codex_auth_path, get_codex_config_path};
use crate::error::{AppError, AppResult};
use crate::provider::{ClaudeModelMapping, LiveProviderInfo, ProtocolType, Provider};
use crate::mcp::McpServer;

const MANAGED_PROVIDER_PREFIX: &str = "ai_switcher_";
const CONFIG_BACKUP: &str = "codex-original-config.toml";
const AUTH_BACKUP: &str = "codex-original-auth.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAuthStatus {
    pub config_path: String,
    pub auth_path: String,
    pub config_exists: bool,
    pub logged_in: bool,
    pub login_command: String,
}

pub fn auth_status() -> CodexAuthStatus {
    let config = get_codex_config_path();
    let auth = get_codex_auth_path();
    CodexAuthStatus {
        config_path: config.to_string_lossy().into_owned(),
        auth_path: auth.to_string_lossy().into_owned(),
        config_exists: config.exists(),
        // The token contents are deliberately never parsed or sent over IPC.
        logged_in: auth.is_file(),
        login_command: "codex login".to_string(),
    }
}

fn managed_provider_id(provider: &Provider) -> String {
    let suffix: String = provider
        .id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    format!("{MANAGED_PROVIDER_PREFIX}{suffix}")
}

fn wire_api(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "anthropic",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => "chat",
        ProtocolType::OpenAiResponses => "responses",
    }
}

fn load_document(path: &Path) -> AppResult<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    fs::read_to_string(path)
        .map_err(AppError::from)?
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Config(format!("Codex config.toml 格式无效：{error}")))
}

fn backup_once(path: &Path, backup_name: &str) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let backup = get_backup_dir().join(backup_name);
    if backup.exists() {
        return Ok(());
    }
    let bytes = fs::read(path)?;
    atomic_write(&backup, &bytes)
}

/// Apply a direct Codex model provider. The API key remains in the OS keyring
/// at rest; Codex needs its selected runtime key in auth.json while active.
pub fn apply_provider(provider: &Provider, api_key: &str) -> AppResult<()> {
    let config_path = get_codex_config_path();
    let auth_path = get_codex_auth_path();
    backup_once(&config_path, CONFIG_BACKUP)?;
    backup_once(&auth_path, AUTH_BACKUP)?;

    let provider_id = managed_provider_id(provider);
    let mut doc = load_document(&config_path)?;
    doc["model"] = value(provider.model.trim());
    doc["model_provider"] = value(provider_id.as_str());
    if !doc["model_providers"].is_table() {
        doc["model_providers"] = Item::Table(Table::new());
    }
    let entry = &mut doc["model_providers"][provider_id.as_str()];
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    entry["name"] = value(provider.name.trim());
    entry["base_url"] = value(provider.base_url.trim());
    entry["wire_api"] = value(wire_api(provider.protocol_type));
    entry["env_key"] = value("OPENAI_API_KEY");
    atomic_write(&config_path, doc.to_string().as_bytes())?;

    // Keep the foreign file intentionally minimal. Do not merge or expose a
    // previous OAuth document; `switch_to_official` restores its byte backup.
    let auth = serde_json::json!({ "OPENAI_API_KEY": api_key });
    atomic_write(&auth_path, serde_json::to_string_pretty(&auth)?.as_bytes())
}

pub fn restore_official() -> AppResult<()> {
    let config_path = get_codex_config_path();
    let auth_path = get_codex_auth_path();
    let config_backup = get_backup_dir().join(CONFIG_BACKUP);
    let auth_backup = get_backup_dir().join(AUTH_BACKUP);
    if config_backup.exists() {
        atomic_write(&config_path, &fs::read(config_backup)?)?;
    }
    if auth_backup.exists() {
        atomic_write(&auth_path, &fs::read(auth_backup)?)?;
    }
    Ok(())
}

pub fn read_current_live_provider() -> AppResult<Option<LiveProviderInfo>> {
    let doc = load_document(&get_codex_config_path())?;
    let provider_id = doc["model_provider"].as_str().unwrap_or_default();
    if !provider_id.starts_with(MANAGED_PROVIDER_PREFIX) {
        return Ok(None);
    }
    let entry = &doc["model_providers"][provider_id];
    let Some(table) = entry.as_table() else { return Ok(None); };
    let base_url = table.get("base_url").and_then(Item::as_str).unwrap_or_default().to_string();
    let model = doc["model"].as_str().unwrap_or_default().to_string();
    if base_url.is_empty() || model.is_empty() {
        return Ok(None);
    }
    Ok(Some(LiveProviderInfo {
        base_url,
        auth_token: String::new(),
        model,
        model_mapping: ClaudeModelMapping::default(),
    }))
}

/// Synchronize the enabled Codex subset into `[mcp_servers.<name>]` while
/// retaining every unrelated config section. Unsupported JSON shapes are left
/// out rather than guessed into an invalid TOML server definition.
pub fn sync_mcp_servers(servers: &[McpServer]) -> AppResult<()> {
    let path = get_codex_config_path();
    let mut doc = load_document(&path)?;
    if !doc["mcp_servers"].is_table() {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let table = doc["mcp_servers"].as_table_mut().expect("table set above");
    for server in servers.iter().filter(|server| server.enabled_codex) {
        let Some(object) = server.server_config.as_object() else { continue; };
        let mut entry = Table::new();
        if let Some(command) = object.get("command").and_then(|value| value.as_str()) {
            entry["command"] = value(command);
        }
        if let Some(url) = object.get("url").and_then(|value| value.as_str()) {
            entry["url"] = value(url);
        }
        if let Some(args) = object.get("args").and_then(|value| value.as_array()) {
            let mut values = Array::new();
            for arg in args.iter().filter_map(|value| value.as_str()) { values.push(arg); }
            entry["args"] = value(values);
        }
        if entry.is_empty() { continue; }
        table[server.name.as_str()] = Item::Table(entry);
    }
    atomic_write(&path, doc.to_string().as_bytes())
}

pub fn read_mcp_servers() -> AppResult<BTreeMap<String, Value>> {
    let doc = load_document(&get_codex_config_path())?;
    let Some(servers) = doc["mcp_servers"].as_table() else { return Ok(BTreeMap::new()); };
    let mut result = BTreeMap::new();
    for (name, item) in servers.iter() {
        let Some(table) = item.as_table() else { continue; };
        let mut config = Map::new();
        if let Some(command) = table.get("command").and_then(Item::as_str) {
            config.insert("command".to_string(), Value::String(command.to_string()));
        }
        if let Some(url) = table.get("url").and_then(Item::as_str) {
            config.insert("url".to_string(), Value::String(url.to_string()));
        }
        if let Some(args) = table.get("args").and_then(Item::as_array) {
            config.insert("args".to_string(), Value::Array(args.iter().filter_map(|value| value.as_str().map(|value| Value::String(value.to_string()))).collect()));
        }
        if !config.is_empty() { result.insert(name.to_string(), Value::Object(config)); }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_expected_wire_api() {
        assert_eq!(wire_api(ProtocolType::OpenAiResponses), "responses");
        assert_eq!(wire_api(ProtocolType::OpenAiChat), "chat");
        assert_eq!(wire_api(ProtocolType::Anthropic), "anthropic");
    }
}
