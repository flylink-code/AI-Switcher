//! Claude Desktop configuration directory discovery.
//!
//! Claude Desktop stores its per-provider gateway configs under a `configLibrary`
//! folder inside its install/support directory. The candidate order mirrors the
//! reference implementation in `examples/cc-proxy-master/claude_config.py`:
//!
//! - Windows: `%LOCALAPPDATA%\Claude`, then `%LOCALAPPDATA%\ClaudeZhCN`
//!   (the Chinese-locale folder name), then `%APPDATA%\Claude`.
//! - macOS: `~/Library/Application Support/Claude`.
//!
//! Linux is unsupported (Claude Desktop does not ship there); detection returns
//! `None`.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::backup::backup_file_named;
use crate::config::{read_json_file, write_json_file};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{ClaudeModelMapping, ClaudeModelRole, Provider};

/// Subdirectory inside the Claude install dir that holds provider configs.
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
/// The registry file listing available provider entries + the applied one.
const META_FILE: &str = "_meta.json";

/// All Claude Desktop paths relevant to config writing. Fields are `None` when
/// the platform is unsupported or Claude Desktop is not installed.
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeDesktopPaths {
    /// Install/support base dir (e.g. `%LOCALAPPDATA%\Claude`).
    pub base: Option<PathBuf>,
    /// `configLibrary` dir inside base.
    pub config_library: Option<PathBuf>,
    /// `configLibrary/_meta.json`.
    pub meta_path: Option<PathBuf>,
}

impl ClaudeDesktopPaths {
    fn not_detected() -> Self {
        ClaudeDesktopPaths {
            base: None,
            config_library: None,
            meta_path: None,
        }
    }
}

/// Whether Claude Desktop config management is supported on this OS.
pub fn is_supported_platform() -> bool {
    cfg!(target_os = "windows") || cfg!(target_os = "macos")
}

/// Probe candidate directories and return the first that exists, along with its
/// `configLibrary` and `_meta.json` paths.
pub fn detect_claude_desktop() -> ClaudeDesktopPaths {
    if !is_supported_platform() {
        return ClaudeDesktopPaths::not_detected();
    }

    for candidate in candidate_base_dirs() {
        if candidate.is_dir() {
            let config_library = candidate.join(CONFIG_LIBRARY_DIR);
            let meta_path = config_library.join(META_FILE);
            return ClaudeDesktopPaths {
                base: Some(candidate),
                config_library: Some(config_library),
                meta_path: Some(meta_path),
            };
        }
    }
    ClaudeDesktopPaths::not_detected()
}

/// Ordered list of base directories to probe for the current platform.
fn candidate_base_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let roaming_app_data = std::env::var_os("APPDATA").map(PathBuf::from);
        if let Some(lad) = local_app_data {
            out.push(lad.join("Claude"));
            out.push(lad.join("ClaudeZhCN"));
        }
        if let Some(rad) = roaming_app_data {
            out.push(rad.join("Claude"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            out.push(home.join("Library/Application Support/Claude"));
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Unsupported platform; no candidates. Nothing to add.
    }

    out
}

/// Stable profile id for this app's Claude Desktop gateway config.
const PROFILE_ID: &str = "claude-switcher";
const PROFILE_NAME: &str = "Claude Switcher";
const GATEWAY_TOKEN_KEY: &str = "claude_desktop_gateway_token";
const MAX_BACKUPS: usize = 10;
const DESKTOP_ROLE_ROUTES: [(&str, ClaudeModelRole); 4] = [
    ("claude-sonnet-5", ClaudeModelRole::Sonnet),
    ("claude-opus-4-8", ClaudeModelRole::Opus),
    ("claude-haiku-4-5", ClaudeModelRole::Haiku),
    ("claude-fable-5", ClaudeModelRole::Fable),
];

/// Activate a provider for Claude Desktop by writing the configLibrary profile
/// and updating `_meta.json`.
///
/// - `Anthropic` protocol: point Desktop directly at the provider's base URL.
/// - `Proxy` protocol: point Desktop at the local proxy (`http://127.0.0.1:port`).
pub fn apply_provider(provider: &Provider, proxy_port: u16) -> AppResult<()> {
    let paths = detect_claude_desktop();
    let config_library = paths
        .config_library
        .ok_or_else(|| AppError::Config("未检测到 Claude Desktop 配置目录".to_string()))?;
    let meta_path = paths
        .meta_path
        .ok_or_else(|| AppError::Config("未检测到 Claude Desktop _meta.json".to_string()))?;

    std::fs::create_dir_all(&config_library)?;
    backup_file_named(&meta_path,
        "_meta.json",
        MAX_BACKUPS,
    )?;

    let profile_path = config_library.join(format!("{PROFILE_ID}.json"));
    if profile_path.exists() {
        backup_file_named(&profile_path,
            &format!("{PROFILE_ID}.json"),
            MAX_BACKUPS,
        )?;
    }

    let profile = build_profile(provider, proxy_port)?;
    write_json_file(&profile_path, &profile)?;

    write_meta(&meta_path,
        Some(PROFILE_ID),
        Some(PROFILE_NAME),
    )?;

    Ok(())
}

/// Restore official login mode for Claude Desktop: remove our profile and clear
/// our applied id in `_meta.json`.
pub fn clear_provider() -> AppResult<()> {
    let paths = detect_claude_desktop();
    let config_library = match paths.config_library {
        Some(p) => p,
        None => return Ok(()),
    };
    let meta_path = match paths.meta_path {
        Some(p) => p,
        None => return Ok(()),
    };

    let profile_path = config_library.join(format!("{PROFILE_ID}.json"));
    if profile_path.exists() {
        backup_file_named(
            &profile_path,
            &format!("{PROFILE_ID}.json"),
            MAX_BACKUPS,
        )?;
        std::fs::remove_file(&profile_path)?;
    }

    write_meta(&meta_path,
        None,
        Some(PROFILE_NAME),
    )?;
    Ok(())
}

/// Clear this application's profile and restore the previously applied profile
/// id, when P7 ownership tracking captured one before the first switch.
pub fn clear_provider_restoring_applied_id(previous: Option<String>) -> AppResult<()> {
    clear_provider()?;
    let Some(previous) = previous else { return Ok(()); };
    let paths = detect_claude_desktop();
    let Some(meta_path) = paths.meta_path else { return Ok(()); };
    let mut value = read_json_file::<Value>(&meta_path)?.unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({});
    }
    value["appliedId"] = Value::String(previous);
    write_json_file(&meta_path, &value)
}

/// Return the currently selected Desktop configuration profile, if readable.
pub fn current_applied_id() -> AppResult<Option<String>> {
    let paths = detect_claude_desktop();
    let Some(meta_path) = paths.meta_path else { return Ok(None); };
    let value = read_json_file::<Value>(&meta_path)?.unwrap_or_else(|| serde_json::json!({}));
    Ok(value.get("appliedId").and_then(Value::as_str).map(str::to_string))
}
fn build_profile(provider: &Provider, proxy_port: u16) -> AppResult<Value> {
    let role_routes = provider.requires_local_proxy();
    let (base_url, api_key) = if role_routes {
        let token = get_or_create_gateway_token()?;
        (format!("http://127.0.0.1:{proxy_port}"), token)
    } else {
        (provider.base_url.clone(), provider.api_key.clone())
    };

    let mut profile = serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": base_url.trim_end_matches('/'),
        "inferenceGatewayApiKey": api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "disableDeploymentModeChooser": true,
    });

    if role_routes {
        profile["inferenceModels"] = Value::Array(desktop_inference_models(provider));
    } else if !provider.model.trim().is_empty() {
        profile["inferenceModels"] = serde_json::json!([
            { "name": provider.model.trim(), "supports1m": true }
        ]);
    }

    Ok(profile)
}

fn desktop_inference_models(provider: &Provider) -> Vec<Value> {
    DESKTOP_ROLE_ROUTES
        .iter()
        .map(|(route_id, role)| {
            let upstream = provider
                .model_mapping
                .for_role(*role, provider.model.trim());
            serde_json::json!({
                "name": route_id,
                "labelOverride": upstream,
                "supports1m": true,
            })
        })
        .collect()
}

/// Claude Desktop queries this endpoint to populate its model menu.
pub fn model_list_response(_provider: &Provider) -> Value {
    let data = DESKTOP_ROLE_ROUTES
        .iter()
        .map(|(route_id, _)| {
            serde_json::json!({
                "type": "model",
                "id": route_id,
                "created_at": 0,
                "supports1m": true,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "data": data,
        "has_more": false,
    })
}

/// Read the Claude Switcher profile currently applied to Claude Desktop, if any.
pub fn read_current_live_provider() -> AppResult<Option<crate::provider::LiveProviderInfo>> {
    let paths = detect_claude_desktop();
    let Some(config_library) = paths.config_library else {
        return Ok(None);
    };
    let profile_path = config_library.join(format!("{PROFILE_ID}.json"));
    let Some(profile) = read_json_file::<Value>(&profile_path)? else {
        return Ok(None);
    };
    let base_url = profile
        .get("inferenceGatewayBaseUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let auth_token = profile
        .get("inferenceGatewayApiKey")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let inference_models = profile
        .get("inferenceModels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let upstream_name = |item: &Value| {
        item.get("labelOverride")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let model = inference_models
        .first()
        .map(&upstream_name)
        .unwrap_or_default();
    let role_model = |role: &str| {
        inference_models
            .iter()
            .find(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.to_ascii_lowercase().contains(role))
            })
            .map(&upstream_name)
            .unwrap_or_default()
    };
    let model_mapping = ClaudeModelMapping {
        sonnet: role_model("sonnet"),
        opus: role_model("opus"),
        haiku: role_model("haiku"),
        fable: role_model("fable"),
        subagent: String::new(),
    };
    if base_url.is_empty() && auth_token.is_empty() && model.is_empty() {
        return Ok(None);
    }
    Ok(Some(crate::provider::LiveProviderInfo {
        base_url,
        auth_token,
        model,
        model_mapping,
    }))
}

fn get_or_create_gateway_token() -> AppResult<String> {
    let token = crate::database::Database::init()
        .and_then(|db| {
            db.with_conn(|conn| {
                if let Some(existing) = get_setting(conn, GATEWAY_TOKEN_KEY)? {
                    let trimmed = existing.trim();
                    if !trimmed.is_empty() {
                        return Ok(trimmed.to_string());
                    }
                }
                let new = format!("cs-{}", uuid::Uuid::new_v4().simple());
                set_setting(conn, GATEWAY_TOKEN_KEY, &new)?;
                Ok(new)
            })
        })
        .unwrap_or_else(|_| format!("cs-{}", uuid::Uuid::new_v4().simple()));
    Ok(token)
}

fn write_meta(
    path: &Path,
    applied_id: Option<&str>,
    our_name: Option<&str>,
) -> AppResult<()> {
    let mut value = read_json_file::<Value>(path)?.unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({});
    }
    let obj = value.as_object_mut().expect("normalized to object");

    let mut entries = obj
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Remove our previous entry.
    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID));

    if let Some(id) = applied_id {
        entries.push(serde_json::json!({
            "id": PROFILE_ID,
            "name": our_name.unwrap_or(PROFILE_NAME),
        }));
        obj.insert("appliedId".to_string(), Value::String(id.to_string()));
    } else if obj
        .get("appliedId")
        .and_then(Value::as_str)
        .is_some_and(|id| id == PROFILE_ID)
    {
        obj.remove("appliedId");
    }

    obj.insert("entries".to_string(), Value::Array(entries));
    write_json_file(path, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProtocolType, ProviderTarget};

    fn mapped_provider() -> Provider {
        Provider {
            id: "desktop-models".into(),
            name: "Mapped".into(),
            base_url: "https://api.example.test".into(),
            api_key: "secret".into(),
            api_key_set: true,
            model: "default-upstream".into(),
            model_mapping: ClaudeModelMapping {
                sonnet: "upstream-sonnet".into(),
                opus: "upstream-opus".into(),
                haiku: "upstream-haiku".into(),
                fable: "upstream-fable".into(),
                subagent: String::new(),
            },
            protocol_type: ProtocolType::Anthropic,
            target_app: ProviderTarget::ClaudeDesktop,
            notes: String::new(),
            sort_index: 0,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
        }
    }

    #[test]
    fn unsupported_platform_returns_none() {
        // This test is only meaningful on Linux; on win/mac detection may succeed.
        if !is_supported_platform() {
            let p = detect_claude_desktop();
            assert!(p.base.is_none());
        }
    }

    #[test]
    fn candidate_dirs_nonempty_on_supported() {
        if is_supported_platform() {
            assert!(!candidate_base_dirs().is_empty());
        }
    }

    #[test]
    fn desktop_catalog_uses_safe_routes_and_upstream_labels() {
        let provider = mapped_provider();
        assert!(provider.requires_local_proxy());
        let models = desktop_inference_models(&provider);
        assert_eq!(models.len(), 4);
        assert_eq!(models[0]["name"], "claude-sonnet-5");
        assert_eq!(models[0]["labelOverride"], "upstream-sonnet");
        assert_eq!(models[3]["name"], "claude-fable-5");
        assert_eq!(models[3]["labelOverride"], "upstream-fable");

        let response = model_list_response(&provider);
        assert_eq!(response["data"][1]["id"], "claude-opus-4-8");
        assert_eq!(response["data"][2]["id"], "claude-haiku-4-5");
    }
}
