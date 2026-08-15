//! DeepSeek Harness (`dsh`) 配置同步器。
//!
//! 将 AI-Switcher 托管的供应商写回 `~/.dsh/settings.yaml` 与 `~/.dsh/.credentials.yaml`。

use std::sync::Mutex;
use serde_json::{json, Value};

use crate::config::atomic::ensure_dir_with_context;
use crate::config::paths::{get_dsh_credentials_path, get_dsh_settings_path};
use crate::error::{AppError, AppResult};
use crate::provider::{ProtocolType, Provider};

const DSH_MANAGED_KEY: &str = "ai_switcher_managed";

fn dsh_config_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

fn lock_dsh_config() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    dsh_config_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("DeepSeek Harness 配置锁已中毒: {error}")))
}

fn managed_env_key(provider_id: &str) -> String {
    format!("AISW_{}", provider_id.to_ascii_uppercase().replace('-', "_"))
}

fn dsh_api_protocol(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "anthropic-messages",
        ProtocolType::OpenAiChat => "openai-completions",
        ProtocolType::OpenAiResponses => "openai-responses",
        ProtocolType::Proxy => "openai-completions",
    }
}

fn dsh_model_entry(model_id: &str, context_window: Option<u64>) -> Value {
    let mut entry = json!({
        "id": model_id,
        "input": ["text", "image"],
        "reasoningEfforts": {
            "off": null,
            "low": "low",
            "medium": "medium",
            "high": "high"
        }
    });
    if let Some(window) = context_window.filter(|value| *value > 0) {
        entry["contextWindow"] = json!(window);
    }
    entry
}

/// 将 AI-Switcher 管理的 DeepSeek Harness 供应商同步写入 `settings.yaml` 和 `.credentials.yaml`。
pub fn sync_managed_dsh_providers(entries: &[(Provider, Vec<String>)]) -> AppResult<()> {
    let _guard = lock_dsh_config()?;
    let settings_path = get_dsh_settings_path();
    let credentials_path = get_dsh_credentials_path();

    if let Some(parent) = settings_path.parent() {
        ensure_dir_with_context(parent)?;
    }

    // 1. 读取或初始化 settings.yaml，保留用户自有 provider 与其他设置。
    let mut settings: Value = if settings_path.is_file() {
        let content = std::fs::read_to_string(&settings_path)
            .map_err(|e| AppError::Config(format!("无法读取 settings.yaml: {e}")))?;
        serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("无法解析 settings.yaml: {e}")))?
    } else {
        json!({})
    };

    if !settings.is_object() {
        settings = json!({});
    }

    // 2. 读取或初始化 .credentials.yaml
    let mut credentials: Value = if credentials_path.is_file() {
        let content = std::fs::read_to_string(&credentials_path)
            .map_err(|e| AppError::Config(format!("无法读取 .credentials.yaml: {e}")))?;
        serde_yaml::from_str(&content)
            .map_err(|e| AppError::Config(format!("无法解析 .credentials.yaml: {e}")))?
    } else {
        json!({})
    };

    if !credentials.is_object() {
        credentials = json!({});
    }

    let creds_map = credentials.as_object_mut().unwrap();

    // 追踪先前的托管列表，以便清理废弃供应商
    let settings_obj = settings.as_object_mut().unwrap();
    let previously_managed: Vec<String> = settings_obj
        .get(DSH_MANAGED_KEY)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let llm_pi_ai = settings_obj
        .entry("llm-pi-ai".to_string())
        .or_insert_with(|| json!({}));
    if !llm_pi_ai.is_object() {
        *llm_pi_ai = json!({});
    }

    let providers_val = llm_pi_ai
        .as_object_mut()
        .unwrap()
        .entry("providers".to_string())
        .or_insert_with(|| json!({}));
    if !providers_val.is_object() {
        *providers_val = json!({});
    }

    let providers_map = providers_val.as_object_mut().unwrap();

    // 清理废弃的旧托管 Key 与 Credentials 环境变量名
    let current_ids: Vec<String> = entries.iter().map(|(p, _)| p.id.clone()).collect();
    for old_id in &previously_managed {
        if !current_ids.contains(old_id) {
            providers_map.remove(old_id);
            let env_key = managed_env_key(old_id);
            creds_map.remove(&env_key);
        }
    }

    // 写入当前最新供应商配置与 Credentials
    for (provider, extra_models) in entries {
        let env_key = managed_env_key(&provider.id);
        if !provider.api_key.trim().is_empty() {
            creds_map.insert(env_key.clone(), json!(provider.api_key.trim()));
        }

        let mut models_list: Vec<Value> = Vec::new();
        if !provider.model.trim().is_empty() {
            models_list.push(dsh_model_entry(
                provider.model.trim(),
                provider.model_context_window,
            ));
        }
        for m in extra_models {
            if m.trim() != provider.model.trim() && !m.trim().is_empty() {
                models_list.push(dsh_model_entry(m.trim(), provider.model_context_window));
            }
        }

        let mut provider_cfg = serde_json::Map::new();
        provider_cfg.insert("displayName".to_string(), json!(provider.name));
        provider_cfg.insert("apiKeyEnv".to_string(), json!(env_key));
        provider_cfg.insert("api".to_string(), json!(dsh_api_protocol(provider.protocol_type)));
        provider_cfg.insert("baseURL".to_string(), json!(provider.base_url));
        provider_cfg.insert("defaultInput".to_string(), json!(["text", "image"]));
        if !models_list.is_empty() {
            provider_cfg.insert("models".to_string(), Value::Array(models_list));
        }

        providers_map.insert(provider.id.clone(), Value::Object(provider_cfg));
    }

    settings_obj.insert(DSH_MANAGED_KEY.to_string(), json!(current_ids));

    // 写回 YAML，Dsh 会对运行中的 settings/credentials 改动热加载。
    let new_settings_str = serde_yaml::to_string(&settings)
        .map_err(|e| AppError::Config(format!("序列化 settings.yaml 失败: {e}")))?;
    std::fs::write(&settings_path, new_settings_str)
        .map_err(|e| AppError::Config(format!("写回 settings.yaml 失败: {e}")))?;

    let new_creds_str = serde_yaml::to_string(&credentials)
        .map_err(|e| AppError::Config(format!("序列化 .credentials.yaml 失败: {e}")))?;
    std::fs::write(&credentials_path, new_creds_str)
        .map_err(|e| AppError::Config(format!("写回 .credentials.yaml 失败: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_model_entry_declares_vision_and_reasoning_efforts() {
        let entry = dsh_model_entry("qwen3.6-plus", Some(200_000));
        assert_eq!(entry["id"], "qwen3.6-plus");
        assert_eq!(entry["input"], json!(["text", "image"]));
        assert_eq!(entry["reasoningEfforts"]["high"], "high");
        assert!(entry["reasoningEfforts"]["off"].is_null());
        assert_eq!(entry["contextWindow"], 200_000);
    }

    #[test]
    fn dsh_model_entry_omits_zero_context_window() {
        let entry = dsh_model_entry("custom-id", Some(0));
        assert!(entry.get("contextWindow").is_none());
    }
}
