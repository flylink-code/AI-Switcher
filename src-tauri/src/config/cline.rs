//! Cline CLI config writer (OpenAI Responses).
//!
//! Independent cards still go through the local proxy on 15827. A bound Auto
//! card points at the smart gateway on 15828. The sidecar lists every provider.

use serde_json::{json, Map, Value};

use crate::config::atomic::{ensure_dir_with_context, write_json_file};
use crate::config::paths::get_home_dir;
use crate::error::AppResult;
use crate::provider::Provider;

const PLACEHOLDER_API_KEY: &str = "__AI_SWITCHER__";

pub fn cline_config_dir() -> std::path::PathBuf {
    get_home_dir().join(".cline")
}

pub fn cline_data_dir() -> std::path::PathBuf {
    std::env::var_os("CLINE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| cline_config_dir().join("data"))
}

pub fn cline_mcp_settings_path() -> std::path::PathBuf {
    cline_data_dir().join("settings").join("cline_mcp_settings.json")
}

pub fn cline_sessions_dir() -> std::path::PathBuf {
    std::env::var_os("CLINE_SESSION_DATA_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| cline_data_dir().join("sessions"))
}

pub fn cline_sessions_db_candidates() -> Vec<std::path::PathBuf> {
    let db_dir = std::env::var_os("CLINE_DB_DATA_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| cline_data_dir().join("db"));
    vec![
        db_dir.join("sessions.db"),
        cline_sessions_dir().join("sessions.db"),
    ]
}

pub fn cline_skills_dir() -> std::path::PathBuf {
    cline_config_dir().join("skills")
}

pub fn cline_rules_dir() -> std::path::PathBuf {
    cline_config_dir().join("rules")
}

pub fn sync_managed_cline_providers(entries: &[(Provider, Vec<String>)]) -> AppResult<()> {
    sync_managed_cline_providers_to(&cline_config_dir(), entries)
}

fn cline_model_catalog(model: &str, models: &[String]) -> Map<String, Value> {
    let mut catalog = Map::new();
    for id in std::iter::once(model.to_string()).chain(models.iter().cloned()) {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        catalog.insert(
            id.to_string(),
            json!({
                "name": id,
                "apiFormat": "openai-responses",
                "capabilities": ["streaming", "tools", "reasoning"]
            }),
        );
    }
    catalog
}

fn resolved_api_key(provider: &Provider) -> &str {
    let key = provider.api_key.trim();
    if key.is_empty() {
        PLACEHOLDER_API_KEY
    } else {
        key
    }
}

fn cline_provider_entry(provider: &Provider, models: &[String]) -> Value {
    let model = provider.model.trim();
    json!({
        "id": provider.id,
        "name": provider.name,
        "apiKey": resolved_api_key(provider),
        "model": model,
        "protocol": "openai-responses",
        "baseUrl": provider.base_url,
        "models": cline_model_catalog(model, models),
    })
}

fn sync_managed_cline_providers_to(dir: &std::path::Path, entries: &[(Provider, Vec<String>)]) -> AppResult<()> {
    ensure_dir_with_context(dir)?;
    if entries.is_empty() {
        return Ok(());
    }
    let primary_idx = entries
        .iter()
        .position(|(provider, _)| provider.is_smart_gateway())
        .unwrap_or(0);
    let (primary, primary_models) = &entries[primary_idx];
    let model = primary.model.trim();
    let providers: Vec<Value> = entries
        .iter()
        .map(|(provider, models)| cline_provider_entry(provider, models))
        .collect();
    let settings = json!({
        "version": 1,
        "provider": "openai-native",
        "apiKey": resolved_api_key(primary),
        "model": model,
        "protocol": "openai-responses",
        "baseUrl": primary.base_url,
        "models": cline_model_catalog(model, primary_models),
        "providers": providers,
    });
    write_json_file(&dir.join("ai-switcher.json"), &settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProtocolType, Provider, ProviderKind, ProviderTarget};

    fn sample(id: &str, name: &str, base_url: &str, model: &str, kind: ProviderKind, api_key: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            api_key_set: !api_key.is_empty(),
            model: model.into(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: Default::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            provider_kind: kind,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::Cline,
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    #[test]
    fn writes_cline_sidecar() {
        let dir = std::env::temp_dir().join(format!("aisw-cline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let provider = sample(
            "p_cline",
            "Local",
            "http://127.0.0.1:15821/v1",
            "gpt-5.4",
            ProviderKind::Standard,
            "",
        );
        sync_managed_cline_providers_to(&dir, &[(provider, vec!["gpt-5.4".into()])]).unwrap();
        assert!(dir.join("ai-switcher.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_all_providers_and_prefers_auto_for_toplevel() {
        let dir = std::env::temp_dir().join(format!("aisw-cline-all-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let independent = sample(
            "p_direct",
            "Direct",
            "https://api.example.test/v1",
            "gpt-5.4",
            ProviderKind::Standard,
            "sk-direct",
        );
        let auto = sample(
            "p_sg_cline",
            "Auto",
            "http://127.0.0.1:15828/v1",
            "auto",
            ProviderKind::SmartGateway,
            "gwt_entry",
        );
        sync_managed_cline_providers_to(
            &dir,
            &[
                (independent, vec!["gpt-5.4".into()]),
                (auto, vec!["auto".into(), "gpt-6-astra".into()]),
            ],
        )
        .unwrap();
        let raw = std::fs::read_to_string(dir.join("ai-switcher.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["baseUrl"], "http://127.0.0.1:15828/v1");
        assert_eq!(value["apiKey"], "gwt_entry");
        assert_eq!(value["model"], "auto");
        let listed = value["providers"].as_array().expect("providers array");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["id"], "p_direct");
        assert_eq!(listed[0]["apiKey"], "sk-direct");
        assert_eq!(listed[1]["id"], "p_sg_cline");
        assert_eq!(listed[1]["apiKey"], "gwt_entry");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
