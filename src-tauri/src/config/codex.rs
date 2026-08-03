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

use crate::config::{atomic_write, get_backup_dir, get_codex_auth_path, get_codex_config_dir, get_codex_config_path};
use crate::error::{AppError, AppResult};
use crate::provider::{
    effective_model_context_window, validate_target_protocol, ClaudeModelMapping, LiveProviderInfo,
    ProtocolType, Provider, ProviderTarget,
};
use crate::mcp::McpServer;

/// Stable Codex model_provider id for every AI-Switcher managed third-party
/// provider. Keeping this fixed prevents Codex from hiding historical sessions
/// when the user switches between our providers.
pub const MANAGED_PROVIDER_ID: &str = "ai_switcher";
const LEGACY_MANAGED_PROVIDER_PREFIX: &str = "ai_switcher_";
const CONFIG_BACKUP: &str = "codex-original-config.toml";
const AUTH_BACKUP: &str = "codex-original-auth.json";
const PROXY_MANAGED_API_KEY: &str = "PROXY_MANAGED";
const MODEL_CATALOG_FILENAME: &str = "ai-switcher-model-catalog.json";

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

/// Ensure `doc[key]` is a table without probing via `doc[key].is_table()`.
///
/// `toml_edit` treats `doc["missing"]` as an IndexMut insert of a vacant entry;
/// a subsequent nested write then panics with `index not found`. Always probe
/// with `DocumentMut::get` (or `entry`) before creating the table.
fn ensure_table<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Table {
    let needs_table = doc.get(key).map(|item| !item.is_table()).unwrap_or(true);
    if needs_table {
        doc[key] = Item::Table(Table::new());
    }
    doc[key]
        .as_table_mut()
        .expect("table was just ensured")
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

/// Apply a Codex model provider. When `proxy_port` is set, rewrite live
/// `base_url` to the local OpenAI-compatible proxy; the real upstream URL stays
/// on the DB provider row for the forwarder.
pub fn apply_provider(provider: &Provider, api_key: &str, proxy_port: Option<u16>) -> AppResult<()> {
    validate_target_protocol(ProviderTarget::Codex, provider.protocol_type)?;
    let config_path = get_codex_config_path();
    let auth_path = get_codex_auth_path();
    backup_once(&config_path, CONFIG_BACKUP)?;
    backup_once(&auth_path, AUTH_BACKUP)?;

    let mut doc = load_document(&config_path)?;
    write_managed_provider(&mut doc, provider, proxy_port)?;
    atomic_write(&config_path, doc.to_string().as_bytes())?;

    let auth_key = if proxy_port.is_some() {
        PROXY_MANAGED_API_KEY
    } else {
        api_key
    };
    write_auth_api_key(&auth_path, auth_key)
}

fn write_auth_api_key(path: &Path, api_key: &str) -> AppResult<()> {
    let mut auth = if path.exists() {
        serde_json::from_slice::<Value>(&fs::read(path)?)
            .map_err(|error| AppError::Config(format!("Codex auth.json 格式无效：{error}")))?
    } else {
        Value::Object(Map::new())
    };
    let object = auth
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Codex auth.json 必须是 JSON 对象".to_string()))?;
    object.insert(
        "OPENAI_API_KEY".to_string(),
        Value::String(api_key.to_string()),
    );
    atomic_write(path, serde_json::to_string_pretty(&auth)?.as_bytes())
}

fn auth_value_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

/// True when auth carries material Codex authenticates with ahead of the
/// API-key fallback (OAuth tokens / PAT / agent identity / Bedrock). Pure
/// metadata such as `auth_mode`, `last_refresh`, or `tokens.account_id` does
/// not count — it must not shield a stale third-party key from cleanup.
fn auth_has_credential_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };
    if ["personal_access_token", "agent_identity", "bedrock_api_key"]
        .iter()
        .any(|key| obj.get(*key).is_some_and(auth_value_present))
    {
        return true;
    }
    obj.get("tokens")
        .and_then(Value::as_object)
        .is_some_and(|tokens| {
            ["id_token", "access_token", "refresh_token"]
                .iter()
                .any(|key| tokens.get(*key).is_some_and(auth_value_present))
        })
}

/// Shape left behind after a third-party switch with no prior official login:
/// a non-empty `OPENAI_API_KEY` (optionally with metadata) and no real login
/// credential beside it.
fn live_auth_is_stale_third_party_residue(live_auth: &Value) -> bool {
    if auth_has_credential_login_material(live_auth) {
        return false;
    }
    live_auth
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|key| !key.is_empty())
}

/// Delete an API-key-only `auth.json` so Codex can show its login screen.
/// Deleting (not writing `{}`) matches Codex logout: an empty object errors
/// at bootstrap, while a missing file yields NotAuthenticated.
fn clear_stale_third_party_auth_if_needed(auth_path: &Path) -> AppResult<bool> {
    if !auth_path.exists() {
        return Ok(false);
    }
    let live_auth: Value = serde_json::from_slice(&fs::read(auth_path)?)
        .map_err(|error| AppError::Config(format!("Codex auth.json 格式无效：{error}")))?;
    if !live_auth_is_stale_third_party_residue(&live_auth) {
        return Ok(false);
    }
    fs::remove_file(auth_path)?;
    Ok(true)
}

fn write_managed_provider(doc: &mut DocumentMut, provider: &Provider, proxy_port: Option<u16>) -> AppResult<()> {
    let provider_id = MANAGED_PROVIDER_ID;
    let model = provider.model.trim();
    let context_window = effective_model_context_window(provider);
    let anthropic_upstream = provider.protocol_type == ProtocolType::Anthropic;
    write_model_catalog(provider, anthropic_upstream)?;
    doc["model"] = value(model);
    doc["model_provider"] = value(provider_id);
    doc["model_context_window"] = value(context_window as i64);
    doc["model_catalog_json"] = value(MODEL_CATALOG_FILENAME);
    // Advertise Fast-mode UI (/fast) when the model supports it. Do not force
    // service_tier=fast — that doubles API cost; users opt in via Codex.
    if model_supports_codex_fast(model) {
        ensure_table(doc, "features")["fast_mode"] = value(true);
    }
    ensure_table(doc, "model_providers");
    remove_legacy_managed_providers(doc);
    let entry = &mut ensure_table(doc, "model_providers")[provider_id];
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    entry["name"] = value(provider.name.trim());
    let base_url = if let Some(port) = proxy_port {
        format!("http://127.0.0.1:{port}/v1")
    } else {
        provider.base_url.trim().to_string()
    };
    entry["base_url"] = value(base_url);
    // Local proxy always speaks Responses to Codex clients; upstream conversion
    // is handled by the forwarder based on the DB protocol.
    entry["wire_api"] = value(if proxy_port.is_some() || anthropic_upstream {
        "responses"
    } else {
        wire_api(provider.protocol_type)
    });
    entry["requires_openai_auth"] = value(true);
    if let Some(table) = entry.as_table_mut() {
        table.remove("env_key");
    }
    Ok(())
}

/// Models that Codex / ChatGPT Fast mode currently supports (catalog-driven).
fn model_supports_codex_fast(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if m == "gpt-5.6" || m == "gpt-5.6-sol" || m.starts_with("gpt-5.5") {
        return true;
    }
    if m == "gpt-5.4" || m.starts_with("gpt-5.4-pro") {
        return true;
    }
    false
}

fn write_model_catalog(provider: &Provider, anthropic_upstream: bool) -> AppResult<()> {
    let model = provider.model.trim();
    if model.is_empty() {
        return Err(AppError::Config("Codex 默认模型不能为空".to_string()));
    }
    let context_window = effective_model_context_window(provider);
    let web_search_enabled =
        !anthropic_upstream && provider.web_search_enabled.unwrap_or(true);
    let catalog = serde_json::json!({
        "models": [codex_model_catalog_entry(
            model,
            context_window,
            anthropic_upstream,
            web_search_enabled,
        )],
    });
    let dir = get_codex_config_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(MODEL_CATALOG_FILENAME);
    atomic_write(&path, serde_json::to_string_pretty(&catalog)?.as_bytes())
}

/// Build a Codex ≥0.144.5-compatible catalog entry, backfilling parser-required
/// fields when absent from a minimal model list.
fn codex_model_catalog_entry(
    model: &str,
    context_window: u64,
    anthropic_upstream: bool,
    web_search_enabled: bool,
) -> Value {
    let mut entry = serde_json::json!({
        "slug": model,
        "display_name": model,
        "description": model,
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [
            { "effort": "low", "description": "Fast responses with lighter reasoning" },
            { "effort": "medium", "description": "Balances speed and reasoning depth" },
            { "effort": "high", "description": "Greater reasoning depth for complex work" },
            { "effort": "xhigh", "description": "Extra high reasoning depth" }
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1000,
        "base_instructions": "You are Codex, a coding agent. Follow the user's instructions and use tools carefully.",
        "model_messages": {
            "instructions_template": "You are Codex, a coding agent. Follow the user's instructions and use tools carefully.",
            "instructions_variables": {
                "personality_default": "",
                "personality_friendly": "",
                "personality_pragmatic": ""
            }
        },
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "none",
        "support_verbosity": true,
        "default_verbosity": "low",
        "apply_patch_tool_type": if anthropic_upstream { "structured" } else { "freeform" },
        "web_search_tool_type": if web_search_enabled { "text_and_image" } else { "disabled" },
        "truncation_policy": {
            "mode": "tokens",
            "limit": 10000
        },
        "supports_parallel_tool_calls": true,
        "supports_image_detail_original": true,
        "context_window": context_window,
        "max_context_window": context_window,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": web_search_enabled,
        "service_tiers": [],
        "additional_speed_tiers": []
    });
    if model_supports_codex_fast(model) {
        entry["service_tiers"] = serde_json::json!([{
            "id": "fast",
            "name": "Fast",
            "description": "Up to 2.5x speed on Sol (API Fast / Priority tier; higher token rate)"
        }]);
        entry["additional_speed_tiers"] = serde_json::json!(["fast"]);
    }
    entry
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
    } else {
        // No pre-switch auth snapshot: a third-party apply may have created
        // auth.json from scratch. Leaving that vendor key behind sends Codex
        // to the official endpoint with a foreign credential (401) and blocks
        // the login screen because the file still exists.
        clear_stale_third_party_auth_if_needed(&auth_path)?;
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
    let table = ensure_table(&mut doc, "mcp_servers");
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
    use crate::provider::{
        ClaudeModelMapping, ProtocolType, Provider, ProviderKind, ProviderTarget,
    };

    fn sample_codex_provider() -> Provider {
        Provider {
            id: "p1".into(),
            name: "ThirdParty".into(),
            base_url: "https://api.example.com/v1".into(),
            api_key: String::new(),
            api_key_set: true,
            model: "gpt-5".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
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
    fn empty_config_accepts_fast_mode_model_without_panic() {
        let mut provider = sample_codex_provider();
        provider.model = "gpt-5.6-sol".into();
        let mut doc = DocumentMut::new();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", temp.path());
        write_managed_provider(&mut doc, &provider, None).unwrap();
        assert_eq!(doc["features"]["fast_mode"].as_bool(), Some(true));
        assert!(doc["model_providers"][MANAGED_PROVIDER_ID].is_table());
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn empty_config_accepts_standard_model_without_panic() {
        let provider = sample_codex_provider();
        let mut doc = DocumentMut::new();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", temp.path());
        write_managed_provider(&mut doc, &provider, None).unwrap();
        assert!(doc.get("features").is_none());
        assert!(doc["model_providers"][MANAGED_PROVIDER_ID].is_table());
        std::env::remove_var("CODEX_HOME");
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

        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", temp.path());
        write_managed_provider(&mut doc, &provider, None).unwrap();

        let text = doc.to_string();
        assert!(text.contains("requires_openai_auth = true"));
        assert!(!text.contains("env_key"));
        assert!(!text.contains("[model_providers.ai_switcher_old]"));
        assert_eq!(doc["model"].as_str(), Some("gpt-5"));
        assert_eq!(doc["model_provider"].as_str(), Some(MANAGED_PROVIDER_ID));
        assert_eq!(doc["model_catalog_json"].as_str(), Some(MODEL_CATALOG_FILENAME));
        assert_eq!(doc["model_context_window"].as_integer(), Some(272_000));
        let entry = doc["model_providers"][MANAGED_PROVIDER_ID].as_table().unwrap();
        assert_eq!(entry.get("name").and_then(Item::as_str), Some("ThirdParty"));
        assert_eq!(
            entry.get("base_url").and_then(Item::as_str),
            Some("https://api.example.com/v1")
        );
        assert_eq!(entry.get("wire_api").and_then(Item::as_str), Some("responses"));
        assert_eq!(entry.get("requires_openai_auth").and_then(Item::as_bool), Some(true));
        assert!(entry.get("env_key").is_none());

        let catalog_path = temp.path().join(MODEL_CATALOG_FILENAME);
        let catalog: Value = serde_json::from_str(&fs::read_to_string(catalog_path).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["slug"], "gpt-5");
        assert_eq!(catalog["models"][0]["supports_reasoning_summaries"], true);
        assert_eq!(catalog["models"][0]["context_window"], 272_000);
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn catalog_uses_explicit_context_window() {
        let mut provider = sample_codex_provider();
        provider.model_context_window = Some(200_000);
        let entry = codex_model_catalog_entry(
            "gpt-5",
            effective_model_context_window(&provider),
            false,
            true,
        );
        assert_eq!(entry["context_window"], 200_000);
        assert_eq!(entry["max_context_window"], 200_000);
        assert_eq!(entry["supports_reasoning_summaries"], true);
    }

    #[test]
    fn anthropic_upstream_catalog_disables_incompatible_codex_tools() {
        let entry = codex_model_catalog_entry("claude-sonnet", 272_000, true, false);
        assert_eq!(entry["apply_patch_tool_type"], "structured");
        assert_eq!(entry["web_search_tool_type"], "disabled");
        assert_eq!(entry["supports_search_tool"], false);
    }

    #[test]
    fn provider_can_disable_web_search_for_openai_upstream() {
        let mut provider = sample_codex_provider();
        provider.web_search_enabled = Some(false);
        let entry = codex_model_catalog_entry(
            &provider.model,
            effective_model_context_window(&provider),
            false,
            provider.web_search_enabled.unwrap_or(true),
        );
        assert_eq!(entry["web_search_tool_type"], "disabled");
        assert_eq!(entry["supports_search_tool"], false);
    }

    #[test]
    fn anthropic_upstream_via_proxy_keeps_responses_wire_api() {
        let mut provider = sample_codex_provider();
        provider.protocol_type = ProtocolType::Anthropic;
        provider.base_url = "https://api.anthropic.test".into();
        let mut doc = DocumentMut::new();
        doc["model_providers"] = Item::Table(Table::new());
        write_managed_provider(&mut doc, &provider, Some(15823)).unwrap();
        let entry = doc["model_providers"][MANAGED_PROVIDER_ID].as_table().unwrap();
        assert_eq!(entry.get("wire_api").and_then(Item::as_str), Some("responses"));
        assert_eq!(
            entry.get("base_url").and_then(Item::as_str),
            Some("http://127.0.0.1:15823/v1")
        );
    }

    #[test]
    fn catalog_advertises_fast_mode_for_sol() {
        let entry = codex_model_catalog_entry("gpt-5.6-sol", 272_000, false, true);
        assert_eq!(entry["service_tiers"][0]["id"], "fast");
        assert_eq!(entry["additional_speed_tiers"][0], "fast");
        assert!(model_supports_codex_fast("gpt-5.6-sol"));
        assert!(model_supports_codex_fast("gpt-5.5"));
        assert!(model_supports_codex_fast("gpt-5.4"));
        assert!(!model_supports_codex_fast("gpt-5.6-luna"));
        assert!(!model_supports_codex_fast("gpt-5.4-mini"));
    }

    #[test]
    fn recognizes_stable_and_legacy_managed_ids() {
        assert!(is_managed_provider_id("ai_switcher"));
        assert!(is_managed_provider_id("ai_switcher_p1"));
        assert!(!is_managed_provider_id("openai"));
        assert!(!is_managed_provider_id("custom"));
    }

    #[test]
    fn auth_update_preserves_login_tokens_and_mode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(
            &path,
            r#"{"tokens":{"access_token":"keep"},"auth_mode":"chatgpt","OPENAI_API_KEY":"old"}"#,
        )
        .unwrap();

        write_auth_api_key(&path, "new-key").unwrap();

        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["tokens"]["access_token"], "keep");
        assert_eq!(value["auth_mode"], "chatgpt");
        assert_eq!(value["OPENAI_API_KEY"], "new-key");
    }

    #[test]
    fn credential_login_material_only_counts_real_credentials() {
        assert!(auth_has_credential_login_material(&serde_json::json!({
            "tokens": { "access_token": "t" }
        })));
        assert!(auth_has_credential_login_material(&serde_json::json!({
            "tokens": { "refresh_token": "r" }
        })));
        assert!(auth_has_credential_login_material(&serde_json::json!({
            "personal_access_token": "pat"
        })));
        assert!(auth_has_credential_login_material(&serde_json::json!({
            "bedrock_api_key": "bk"
        })));

        assert!(!auth_has_credential_login_material(&serde_json::json!({
            "OPENAI_API_KEY": "sk-x"
        })));
        assert!(!auth_has_credential_login_material(&serde_json::json!({
            "OPENAI_API_KEY": "sk-x",
            "last_refresh": "2026-01-01T00:00:00Z",
            "tokens": { "account_id": "acct-meta-only" }
        })));
        assert!(!auth_has_credential_login_material(&serde_json::json!({})));
    }

    #[test]
    fn stale_third_party_residue_detection() {
        assert!(live_auth_is_stale_third_party_residue(&serde_json::json!({
            "OPENAI_API_KEY": "sk-third-party"
        })));
        assert!(live_auth_is_stale_third_party_residue(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "PROXY_MANAGED"
        })));
        assert!(live_auth_is_stale_third_party_residue(&serde_json::json!({
            "OPENAI_API_KEY": "sk-third-party",
            "last_refresh": "2026-01-01T00:00:00Z",
            "tokens": { "account_id": "acct-meta-only" }
        })));

        assert!(!live_auth_is_stale_third_party_residue(&serde_json::json!({
            "OPENAI_API_KEY": "sk-x",
            "tokens": { "access_token": "t" }
        })));
        assert!(!live_auth_is_stale_third_party_residue(&serde_json::json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": { "access_token": "official-oauth-token" }
        })));
        assert!(!live_auth_is_stale_third_party_residue(&serde_json::json!({})));
        assert!(!live_auth_is_stale_third_party_residue(&serde_json::json!({
            "OPENAI_API_KEY": ""
        })));
    }

    #[test]
    fn clears_api_key_only_auth_but_keeps_oauth() {
        let temp = tempfile::tempdir().unwrap();
        let stale = temp.path().join("stale-auth.json");
        fs::write(&stale, r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-vendor"}"#).unwrap();
        assert!(clear_stale_third_party_auth_if_needed(&stale).unwrap());
        assert!(!stale.exists());

        let oauth = temp.path().join("oauth-auth.json");
        fs::write(
            &oauth,
            r#"{"tokens":{"access_token":"keep"},"auth_mode":"chatgpt","OPENAI_API_KEY":"sk-vendor"}"#,
        )
        .unwrap();
        assert!(!clear_stale_third_party_auth_if_needed(&oauth).unwrap());
        assert!(oauth.exists());
        let value: Value = serde_json::from_slice(&fs::read(&oauth).unwrap()).unwrap();
        assert_eq!(value["tokens"]["access_token"], "keep");
        assert_eq!(value["OPENAI_API_KEY"], "sk-vendor");
    }
}
