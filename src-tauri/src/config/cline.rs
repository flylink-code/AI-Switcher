//! Cline CLI config writer (OpenAI Responses via the local proxy).

use serde_json::json;

use crate::config::atomic::{ensure_dir_with_context, write_json_file};
use crate::config::paths::get_home_dir;
use crate::error::AppResult;
use crate::provider::Provider;

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

fn sync_managed_cline_providers_to(dir: &std::path::Path, entries: &[(Provider, Vec<String>)]) -> AppResult<()> {
    ensure_dir_with_context(dir)?;
    let Some((provider, models)) = entries.first() else {
        return Ok(());
    };
    let model = provider.model.trim();
    let mut catalog = serde_json::Map::new();
    for id in std::iter::once(model.to_string()).chain(models.iter().cloned()) {
        if id.trim().is_empty() {
            continue;
        }
        catalog.insert(
            id.clone(),
            json!({
                "name": id,
                "apiFormat": "openai-responses",
                "capabilities": ["streaming", "tools", "reasoning"]
            }),
        );
    }
    let settings = json!({
        "version": 1,
        "provider": "openai-native",
        "apiKey": "__AI_SWITCHER__",
        "model": model,
        "protocol": "openai-responses",
        "baseUrl": provider.base_url,
        "models": catalog,
    });
    write_json_file(&dir.join("ai-switcher.json"), &settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProtocolType, Provider, ProviderKind, ProviderTarget};

    fn sample() -> Provider {
        Provider {
            id: "p_cline".into(),
            name: "Local".into(),
            base_url: "http://127.0.0.1:15821/v1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "gpt-5.4".into(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: Default::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
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
        let provider = sample();
        sync_managed_cline_providers_to(&dir, &[(provider, vec!["gpt-5.4".into()])]).unwrap();
        assert!(dir.join("ai-switcher.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
