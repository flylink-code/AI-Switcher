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
use crate::provider::{validate_target_protocol, ClaudeModelMapping, LiveProviderInfo, ProtocolType, Provider, ProviderTarget};
use crate::mcp::McpServer;

/// Stable Codex model_provider id for every AI-Switcher managed third-party
/// provider. Keeping this fixed prevents Codex from hiding historical sessions
/// when the user switches between our providers.
pub const MANAGED_PROVIDER_ID: &str = "ai_switcher";
const LEGACY_MANAGED_PROVIDER_PREFIX: &str = "ai_switcher_";
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

pub fn managed_provider_id() -> &'static str {
    MANAGED_PROVIDER_ID
}

pub fn is_managed_provider_id(provider_id: &str) -> bool {
    provider_id == MANAGED_PROVIDER_ID || provider_id.starts_with(LEGACY_MANAGED_PROVIDER_PREFIX)
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
///
/// Pure-API shape matches CodexPlusPlus / cc-switch:
/// - `requires_openai_auth = true` so Codex reads `~/.codex/auth.json`
/// - never set `env_key` (that forces a process env var and yields
///   `Missing environment variable: OPENAI_API_KEY` when unset)
pub fn apply_provider(provider: &Provider, api_key: &str) -> AppResult<()> {
    validate_target_protocol(ProviderTarget::Codex, provider.protocol_type)?;
    let config_path = get_codex_config_path();
    let auth_path = get_codex_auth_path();
    backup_once(&config_path, CONFIG_BACKUP)?;
    backup_once(&auth_path, AUTH_BACKUP)?;

    let mut doc = load_document(&config_path)?;
    write_managed_provider(&mut doc, provider);
    atomic_write(&config_path, doc.to_string().as_bytes())?;

    // Keep the foreign file intentionally minimal. Do not merge or expose a
    // previous OAuth document; `switch_to_official` restores its byte backup.
    let auth = serde_json::json!({ "OPENAI_API_KEY": api_key });
    atomic_write(&auth_path, serde_json::to_string_pretty(&auth)?.as_bytes())
}

fn write_managed_provider(doc: &mut DocumentMut, provider: &Provider) {
    let provider_id = MANAGED_PROVIDER_ID;
    doc["model"] = value(provider.model.trim());
    doc["model_provider"] = value(provider_id);
    if !doc["model_providers"].is_table() {
        doc["model_providers"] = Item::Table(Table::new());
    }
    remove_legacy_managed_providers(doc);
    let entry = &mut doc["model_providers"][provider_id];
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    entry["name"] = value(provider.name.trim());
    entry["base_url"] = value(provider.base_url.trim());
    entry["wire_api"] = value(wire_api(provider.protocol_type));
    entry["requires_openai_auth"] = value(true);
    // Drop the legacy mistaken field so re-switching heals old config.toml.
    if let Some(table) = entry.as_table_mut() {
        table.remove("env_key");
    }
}

fn remove_legacy_managed_providers(doc: &mut DocumentMut) {
    let Some(providers) = doc["model_providers"].as_table_mut() else {
        return;
    };
    let legacy_keys: Vec<String> = providers
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| key.starts_with(LEGACY_MANAGED_PROVIDER_PREFIX) && key != MANAGED_PROVIDER_ID)
        .collect();
    for key in legacy_keys {
        providers.remove(&key);
    }
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
    if !is_managed_provider_id(provider_id) {
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
    use crate::provider::{ClaudeModelMapping, ProtocolType, Provider, ProviderTarget};

    fn sample_codex_provider() -> Provider {
        Provider {
            id: "p1".into(),
            name: "ThirdParty".into(),
            base_url: "https://api.example.com/v1".into(),
            api_key: String::new(),
            api_key_set: true,
            model: "gpt-5".into(),
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            target_app: ProviderTarget::Codex,
            notes: String::new(),
            sort_index: 0,
            is_current: true,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
        }
    }

    #[test]
    fn selects_expected_wire_api() {
        assert_eq!(wire_api(ProtocolType::OpenAiResponses), "responses");
        assert_eq!(wire_api(ProtocolType::OpenAiChat), "chat");
        assert_eq!(wire_api(ProtocolType::Anthropic), "anthropic");
    }

    #[test]
    fn pure_api_provider_uses_stable_id_without_env_key() {
        let provider = sample_codex_provider();
        let mut doc = DocumentMut::new();
        doc["model_providers"] = Item::Table(Table::new());
        doc["model_providers"]["ai_switcher_old"] = Item::Table(Table::new());
        doc["model_providers"]["ai_switcher_old"]["env_key"] = value("OPENAI_API_KEY");
        doc["model_providers"]["ai_switcher"] = Item::Table(Table::new());
        doc["model_providers"]["ai_switcher"]["env_key"] = value("OPENAI_API_KEY");

        write_managed_provider(&mut doc, &provider);

        let text = doc.to_string();
        assert!(text.contains("requires_openai_auth = true"));
        assert!(!text.contains("env_key"));
        assert!(!text.contains("[model_providers.ai_switcher_old]"));
        assert_eq!(doc["model"].as_str(), Some("gpt-5"));
        assert_eq!(doc["model_provider"].as_str(), Some(MANAGED_PROVIDER_ID));
        let entry = doc["model_providers"][MANAGED_PROVIDER_ID].as_table().unwrap();
        assert_eq!(entry.get("name").and_then(Item::as_str), Some("ThirdParty"));
        assert_eq!(
            entry.get("base_url").and_then(Item::as_str),
            Some("https://api.example.com/v1")
        );
        assert_eq!(entry.get("wire_api").and_then(Item::as_str), Some("responses"));
        assert_eq!(entry.get("requires_openai_auth").and_then(Item::as_bool), Some(true));
        assert!(entry.get("env_key").is_none());
    }

    #[test]
    fn recognizes_stable_and_legacy_managed_ids() {
        assert!(is_managed_provider_id("ai_switcher"));
        assert!(is_managed_provider_id("ai_switcher_p1"));
        assert!(!is_managed_provider_id("openai"));
        assert!(!is_managed_provider_id("custom"));
    }
}
