//! Claude Code `settings.json` switching.
//!
//! Unlike a full-file overwrite, this module performs an **env-block in-place
//! merge**: it only touches env vars prefixed with `ANTHROPIC_`, preserving the
//! user's other settings (top-level keys like `model`/`enabledPlugins` and env
//! vars like `ENABLE_TOOL_SEARCH`). A timestamped backup of the original file is
//! written before every change.
//!
//! See task.md §2: "切换前自动备份原配置".

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::backup::backup_file_named;
use crate::config::{atomic_write, get_claude_settings_path, sort_json_keys};
use crate::error::AppResult;
use crate::provider::{LiveProviderInfo, Provider};

/// The exact Claude Code fields owned by this application. Other
/// `ANTHROPIC_*` variables may be user-managed and must be left untouched.
const MANAGED_ENV_KEYS: [&str; 4] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
];
/// Max backups of `settings.json` to retain.
const SETTINGS_BACKUP_KEEP: usize = 10;

/// Activate `provider` by writing its env vars into `settings.json`.
///
/// Steps: read existing file (or start fresh) → back up → replace only the
/// fields owned by this application → write atomically.
pub fn apply_provider_to_settings(provider: &Provider) -> AppResult<()> {
    apply_provider_to_settings_at(provider, &get_claude_settings_path())
}

/// Activate `provider` in Claude Code by pointing it at the local proxy.
/// `ANTHROPIC_BASE_URL` becomes `http://127.0.0.1:port`; the proxy injects the
/// real upstream key and maps the model name.
pub fn apply_provider_to_settings_via_proxy(
    provider: &Provider,
    proxy_port: u16,
) -> AppResult<()> {
    apply_provider_to_settings_via_proxy_at(provider, proxy_port, &get_claude_settings_path())
}

/// Path-injected variant for tests. Backs up to the app backup dir, then mutates
/// the given file in place using the env-block merge strategy.
pub fn apply_provider_to_settings_at(provider: &Provider, path: &Path) -> AppResult<()> {
    let mut settings = read_or_init_settings_at(path)?;

    // Snapshot before mutating, for safety/rollback.
    backup_settings(path)?;

    let env = ensure_env_object(&mut settings);
    remove_managed_keys(env);
    inject_provider_env(env, provider);

    write_settings(path, &settings)
}

/// Path-injected proxy variant for tests.
pub fn apply_provider_to_settings_via_proxy_at(
    provider: &Provider,
    proxy_port: u16,
    path: &Path,
) -> AppResult<()> {
    let mut settings = read_or_init_settings_at(path)?;
    backup_settings(path)?;

    let env = ensure_env_object(&mut settings);
    remove_managed_keys(env);
    set_str(env, "ANTHROPIC_BASE_URL", &format!("http://127.0.0.1:{proxy_port}"));
    set_str(env, "ANTHROPIC_AUTH_TOKEN", "local-proxy-code");
    set_str(env, "ANTHROPIC_MODEL", &provider.model);

    write_settings(path, &settings)
}

/// Switch to "official login" mode: remove only fields previously owned by this
/// application. Unrelated user-managed Anthropic settings are preserved.
pub fn clear_provider_from_settings() -> AppResult<()> {
    clear_provider_from_settings_at(&get_claude_settings_path())
}

/// Path-injected variant for tests.
pub fn clear_provider_from_settings_at(path: &Path) -> AppResult<()> {
    let mut settings = read_or_init_settings_at(path)?;
    backup_settings(path)?;
    let env = ensure_env_object(&mut settings);
    remove_managed_keys(env);
    write_settings(path, &settings)
}

/// Restore the exact values captured before this app first managed its fixed
/// provider fields. A `None` value means the field did not originally exist.
pub fn restore_managed_fields(
    values: &std::collections::BTreeMap<String, Option<Value>>,
) -> AppResult<()> {
    let path = get_claude_settings_path();
    let mut settings = read_or_init_settings_at(&path)?;
    backup_settings(&path)?;
    let env = ensure_env_object(&mut settings);
    remove_managed_keys(env);
    for (key, value) in values {
        if let Some(value) = value {
            env.insert(key.clone(), value.clone());
        }
    }
    write_settings(&path, &settings)
}

/// Parse the currently-live provider from `settings.json`'s env block, if any.
pub fn read_current_live_provider() -> AppResult<Option<LiveProviderInfo>> {
    let path = get_claude_settings_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path)?;
    let value: Value = serde_json::from_slice(&raw)?;
    let env = value.get("env").and_then(Value::as_object);
    let Some(env) = env else { return Ok(None) };

    let base_url = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Token may live under either key depending on provider.
    let auth_token = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let model = env
        .get("ANTHROPIC_MODEL")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // No base_url = nothing configured (official login).
    if base_url.is_empty() && auth_token.is_empty() && model.is_empty() {
        return Ok(None);
    }
    Ok(Some(LiveProviderInfo {
        base_url,
        auth_token,
        model,
    }))
}

/// Resolve the settings path. Public so commands can show it in the UI.
#[allow(dead_code)]
pub fn settings_path() -> PathBuf {
    get_claude_settings_path()
}

// ---- internals -------------------------------------------------------------

/// Read existing `settings.json` (as a JSON object) or start from an empty object.
fn read_or_init_settings() -> AppResult<(Value, PathBuf)> {
    let path = get_claude_settings_path();
    let value = read_or_init_settings_at(&path)?;
    Ok((value, path))
}

/// Path-injected reader. Returns an empty object when the file does not exist.
fn read_or_init_settings_at(path: &Path) -> AppResult<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let raw = std::fs::read(path)?;
    let mut value: Value = serde_json::from_slice(&raw)?;
    if !value.is_object() {
        log::warn!("settings.json 不是 JSON 对象，将以空对象为基础重建");
        value = Value::Object(Map::new());
    }
    Ok(value)
}

/// Ensure `settings.env` is an object and return a mutable reference to it.
fn ensure_env_object(settings: &mut Value) -> &mut Map<String, Value> {
    if settings.get("env").is_none() || !settings["env"].is_object() {
        settings["env"] = Value::Object(Map::new());
    }
    settings
        .get_mut("env")
        .and_then(Value::as_object_mut)
        .expect("env is an object (just ensured)")
}

/// Remove only the explicit provider fields managed by Claude Switcher.
fn remove_managed_keys(env: &mut Map<String, Value>) {
    for key in MANAGED_ENV_KEYS {
        env.remove(key);
    }
}

/// Write the provider's fields into the env object. Only non-empty values are set,
/// so presets with an empty token don't overwrite a possibly-useful value.
fn inject_provider_env(env: &mut Map<String, Value>, provider: &Provider) {
    set_str(env, "ANTHROPIC_BASE_URL", &provider.base_url);
    set_str(env, "ANTHROPIC_AUTH_TOKEN", &provider.api_key);
    set_str(env, "ANTHROPIC_MODEL", &provider.model);
}

fn set_str(env: &mut Map<String, Value>, key: &str, value: &str) {
    if !value.is_empty() {
        env.insert(key.to_string(), Value::String(value.to_string()));
    }
}

/// Atomic, key-sorted pretty write of the settings object.
fn write_settings(path: &std::path::Path, settings: &Value) -> AppResult<()> {
    // sort_json_keys mutates in place; operate on a clone to avoid surprising callers.
    let mut sorted = settings.clone();
    sort_json_keys(&mut sorted);
    let bytes = serde_json::to_vec_pretty(&sorted)?;
    atomic_write(path, &bytes)
}

/// Back up `settings.json` into the app's backup dir under a distinct name.
fn backup_settings(path: &std::path::Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    backup_file_named(path, "settings.json", SETTINGS_BACKUP_KEEP)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderTarget, ProtocolType};
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    fn sample_provider() -> Provider {
        Provider {
            id: "p1".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/anthropic".into(),
            api_key: "sk-deepseek".into(),
            api_key_set: true,
            model: "deepseek-v4-pro".into(),
            protocol_type: ProtocolType::Anthropic,
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            sort_index: 0,
            is_current: true,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
        }
    }

    /// Apply replaces owned fields while preserving unrelated settings.
    #[test]
    fn apply_replaces_anthropic_and_preserves_rest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Pre-existing file with a different provider + unrelated settings.
        fs::write(
            &path,
            json!({
                "model": "default",
                "enabledPlugins": ["x"],
                "env": {
                    "ENABLE_TOOL_SEARCH": "true",
                    "DISABLE_AUTOUPDATER": "1",
                    "ANTHROPIC_BASE_URL": "https://old.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-old",
                    "ANTHROPIC_MODEL": "old-model",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "old-sonnet"
                }
            })
            .to_string(),
        )
        .unwrap();

        apply_provider_to_settings_at(&sample_provider(), &path).unwrap();

        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let env = written.get("env").unwrap().as_object().unwrap();

        // New provider values present.
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.deepseek.com/anthropic");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-deepseek");
        assert_eq!(env["ANTHROPIC_MODEL"], "deepseek-v4-pro");
        // A user-owned Anthropic setting is retained.
        assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "old-sonnet");
        // Non-anthropic env preserved.
        assert_eq!(env["ENABLE_TOOL_SEARCH"], "true");
        assert_eq!(env["DISABLE_AUTOUPDATER"], "1");
        // Top-level unrelated keys preserved.
        assert_eq!(written["model"], "default");
        assert_eq!(written["enabledPlugins"][0], "x");
    }

    /// Switching back removes only fields managed by this application.
    #[test]
    fn apply_then_clear_removes_all_anthropic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, json!({"model": "default", "env": {}}).to_string()).unwrap();

        apply_provider_to_settings_at(&sample_provider(), &path).unwrap();
        let after_apply: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(after_apply["env"]["ANTHROPIC_BASE_URL"].is_string());

        clear_provider_from_settings_at(&path).unwrap();
        let after_clear: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let env = after_clear.get("env").unwrap().as_object().unwrap();
        assert!(env.keys().all(|k| !k.starts_with("ANTHROPIC_")), "anthropic keys remain");
        assert_eq!(after_clear["model"], "default", "top-level key preserved");
    }

    /// Applying to a non-existent file creates it with just the env block.
    #[test]
    fn apply_creates_file_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        apply_provider_to_settings_at(&sample_provider(), &path).unwrap();
        assert!(path.exists());
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["env"]["ANTHROPIC_BASE_URL"], "https://api.deepseek.com/anthropic");
    }
}
