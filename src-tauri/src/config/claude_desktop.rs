//! Claude Desktop configuration directory discovery and profile management.
//!
//! Third-party gateway profiles live under `configLibrary` inside the **3p**
//! install directory (`Claude-3p` on Windows/macOS/Linux). On Windows, Store /
//! MSIX installs may virtualize config under `Packages\\Claude_*\\LocalCache`.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::backup::backup_file_named;
use crate::config::{get_home_dir, read_json_file, write_json_file};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    ClaudeModelMapping, ClaudeModelRole, Provider, CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID,
    CLAUDE_OPUS_ROLE_ID, CLAUDE_SONNET_ROLE_ID,
};

const CONFIG_LIBRARY_DIR: &str = "configLibrary";
const CONFIG_FILE: &str = "claude_desktop_config.json";
const META_FILE: &str = "_meta.json";

/// All Claude Desktop paths relevant to config writing.
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeDesktopPaths {
    /// 1p (official) install dir, e.g. `%LOCALAPPDATA%\Claude`.
    pub base: Option<PathBuf>,
    /// 3p (third-party) install dir, e.g. `%LOCALAPPDATA%\Claude-3p`.
    pub threep_base: Option<PathBuf>,
    /// `Claude-3p/configLibrary`.
    pub config_library: Option<PathBuf>,
    /// `configLibrary/_meta.json`.
    pub meta_path: Option<PathBuf>,
    /// `Claude/claude_desktop_config.json`.
    pub normal_config_path: Option<PathBuf>,
    /// `Claude-3p/claude_desktop_config.json`.
    pub threep_config_path: Option<PathBuf>,
}

impl ClaudeDesktopPaths {
    fn unsupported() -> Self {
        Self {
            base: None,
            threep_base: None,
            config_library: None,
            meta_path: None,
            normal_config_path: None,
            threep_config_path: None,
        }
    }
}

/// Whether Claude Desktop config management is supported on this OS.
pub fn is_supported_platform() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux"))
}

/// Resolve platform paths. On supported platforms this always returns concrete
/// paths (directories may not exist yet — callers create them on write).
pub fn detect_claude_desktop() -> ClaudeDesktopPaths {
    if !is_supported_platform() {
        return ClaudeDesktopPaths::unsupported();
    }

    #[cfg(target_os = "macos")]
    {
        let home = get_home_dir();
        let app_support = home.join("Library").join("Application Support");
        return paths_from_dirs(app_support.join("Claude"), app_support.join("Claude-3p"));
    }

    #[cfg(target_os = "linux")]
    {
        let config = linux_config_home();
        return paths_from_dirs(config.join("Claude"), config.join("Claude-3p"));
    }

    #[cfg(windows)]
    {
        let local_app_data = windows_local_app_data_dir();
        let roaming_app_data = windows_roaming_app_data_dir();
        let normal_dir = pick_windows_claude_dir(&local_app_data, &roaming_app_data, false)
            .unwrap_or_else(|| local_app_data.join("Claude"));
        let threep_dir = pick_windows_claude_dir(&local_app_data, &roaming_app_data, true)
            .unwrap_or_else(|| local_app_data.join("Claude-3p"));
        return paths_from_dirs(normal_dir, threep_dir);
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    ClaudeDesktopPaths::unsupported()
}

#[cfg(target_os = "linux")]
fn linux_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| get_home_dir().join(".config"))
}

#[cfg(windows)]
fn windows_local_app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join("AppData").join("Local"))
}

#[cfg(windows)]
fn windows_roaming_app_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join("AppData").join("Roaming"))
}

/// Prefer the directory Claude Desktop actually reads.
///
/// Store / MSIX builds may keep a virtualized copy under
/// `Packages\\Claude_*\\LocalCache\\Roaming\\Claude` while Settings → Edit Config
/// opens the non-virtualized `%APPDATA%\\Claude` path.
#[cfg(windows)]
fn pick_windows_claude_dir(
    local_app_data: &Path,
    roaming_app_data: &Path,
    threep: bool,
) -> Option<PathBuf> {
    let exact_name = if threep { "Claude-3p" } else { "Claude" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(local_app_data.join(exact_name));
    candidates.extend(windows_msix_claude_dirs(local_app_data, threep));
    candidates.push(roaming_app_data.join(exact_name));
    if let Some(fuzzy) = windows_fuzzy_local_claude_dirs(local_app_data, threep) {
        candidates.extend(fuzzy);
    }

    let mut seen = std::collections::HashSet::new();
    candidates.retain(|path| seen.insert(normalize_windows_path_key(path)));

    if let Some(path) = candidates
        .iter()
        .find(|path| path.join(CONFIG_FILE).is_file())
        .cloned()
    {
        return Some(path);
    }
    candidates.into_iter().find(|path| path.is_dir())
}

#[cfg(windows)]
fn windows_msix_claude_dirs(local_app_data: &Path, threep: bool) -> Vec<PathBuf> {
    let packages = local_app_data.join("Packages");
    let Ok(entries) = std::fs::read_dir(packages) else {
        return Vec::new();
    };
    let exact_name = if threep { "Claude-3p" } else { "Claude" };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        // Official package id looks like Claude_pzs8sxrjxfjjc; keep the prefix
        // loose so publisher suffixes still match.
        let is_claude_package = name.starts_with("Claude_") || name.starts_with("Claude-");
        let is_threep_package = name.to_ascii_lowercase().contains("3p");
        if !is_claude_package || is_threep_package != threep {
            continue;
        }
        dirs.push(
            path.join("LocalCache")
                .join("Roaming")
                .join(exact_name),
        );
    }
    dirs.sort();
    dirs
}

#[cfg(windows)]
fn windows_fuzzy_local_claude_dirs(local_app_data: &Path, threep: bool) -> Option<Vec<PathBuf>> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(local_app_data)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return false;
            };
            if name.eq_ignore_ascii_case("Packages") {
                return false;
            }
            let starts = name.starts_with("Claude");
            let is_threep = name.contains("-3p");
            starts && is_threep == threep
        })
        .collect();
    candidates.sort();
    Some(candidates)
}

#[cfg(windows)]
fn normalize_windows_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn paths_from_dirs(normal_dir: PathBuf, threep_dir: PathBuf) -> ClaudeDesktopPaths {
    let config_library = threep_dir.join(CONFIG_LIBRARY_DIR);
    ClaudeDesktopPaths {
        base: Some(normal_dir.clone()),
        threep_base: Some(threep_dir.clone()),
        meta_path: Some(config_library.join(META_FILE)),
        normal_config_path: Some(normal_dir.join(CONFIG_FILE)),
        threep_config_path: Some(threep_dir.join(CONFIG_FILE)),
        config_library: Some(config_library),
    }
}

pub const PROFILE_ID: &str = "c765dca5-1e8f-4a6d-9b04-2a76a8b94e31";
pub const LEGACY_PROFILE_ID: &str = "claude-switcher";
const PROFILE_NAME: &str = "Claude Switcher";
const GATEWAY_TOKEN_KEY: &str = "claude_desktop_gateway_token";
/// Claude Desktop appends `/v1/messages` to this base path segment.
pub const CLAUDE_DESKTOP_PROXY_PREFIX: &str = "/claude-desktop";
const MAX_BACKUPS: usize = 10;
const DESKTOP_ROLE_ROUTES: [(&str, ClaudeModelRole); 4] = [
    (CLAUDE_SONNET_ROLE_ID, ClaudeModelRole::Sonnet),
    (CLAUDE_OPUS_ROLE_ID, ClaudeModelRole::Opus),
    (CLAUDE_HAIKU_ROLE_ID, ClaudeModelRole::Haiku),
    (CLAUDE_FABLE_ROLE_ID, ClaudeModelRole::Fable),
];

fn platform_paths() -> AppResult<ClaudeDesktopPaths> {
    let paths = detect_claude_desktop();
    if paths.config_library.is_none() {
        return Err(AppError::Config(
            "当前平台不支持 Claude Desktop 配置管理".to_string(),
        ));
    }
    Ok(paths)
}

/// Local proxy base URL written into the Desktop gateway profile (proxy mode).
pub fn desktop_proxy_gateway_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CLAUDE_DESKTOP_PROXY_PREFIX}")
}

/// Validate the bearer token Claude Desktop sends to the local gateway.
pub fn validate_gateway_auth_header(auth_header: Option<&str>) -> AppResult<()> {
    let expected = get_or_create_gateway_token()?;
    let Some(value) = auth_header else {
        return Err(AppError::Config(
            "Claude Desktop gateway 缺少 Authorization 头".to_string(),
        ));
    };
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or(value)
        .trim();
    if token != expected {
        return Err(AppError::Config(
            "Claude Desktop gateway token 无效".to_string(),
        ));
    }
    Ok(())
}

/// Activate a provider for Claude Desktop by writing the configLibrary profile
/// and updating `_meta.json`.
pub fn apply_provider(provider: &Provider, proxy_port: u16) -> AppResult<()> {
    let paths = platform_paths()?;
    let config_library = paths
        .config_library
        .as_ref()
        .ok_or_else(|| AppError::Config("未解析 Claude Desktop configLibrary 路径".to_string()))?;
    let meta_path = paths
        .meta_path
        .as_ref()
        .ok_or_else(|| AppError::Config("未解析 Claude Desktop _meta.json 路径".to_string()))?;

    std::fs::create_dir_all(config_library)?;

    if let Some(normal_config) = &paths.normal_config_path {
        write_deployment_mode(normal_config, "3p")?;
    }
    if let Some(threep_config) = &paths.threep_config_path {
        write_deployment_mode(threep_config, "3p")?;
    }

    if meta_path.exists() {
        backup_file_named(meta_path, "_meta.json", MAX_BACKUPS)?;
    }

    let profile_path = config_library.join(format!("{PROFILE_ID}.json"));
    let legacy_profile_path = config_library.join(format!("{LEGACY_PROFILE_ID}.json"));
    if profile_path.exists() {
        backup_file_named(
            &profile_path,
            &format!("{PROFILE_ID}.json"),
            MAX_BACKUPS,
        )?;
    }
    if legacy_profile_path.exists() {
        backup_file_named(
            &legacy_profile_path,
            &format!("{LEGACY_PROFILE_ID}.json"),
            MAX_BACKUPS,
        )?;
    }

    let profile = build_profile(provider, proxy_port)?;
    write_json_file(&profile_path, &profile)?;
    write_meta(meta_path, Some(PROFILE_ID), Some(PROFILE_NAME))?;
    if legacy_profile_path.exists() {
        std::fs::remove_file(&legacy_profile_path)?;
    }
    Ok(())
}

/// Restore official login mode for Claude Desktop.
pub fn clear_provider() -> AppResult<()> {
    let paths = platform_paths()?;
    let config_library = match paths.config_library {
        Some(p) => p,
        None => return Ok(()),
    };
    let meta_path = match paths.meta_path {
        Some(p) => p,
        None => return Ok(()),
    };

    if let Some(normal_config) = paths.normal_config_path {
        write_deployment_mode(&normal_config, "1p")?;
    }
    if let Some(threep_config) = paths.threep_config_path {
        write_deployment_mode(&threep_config, "1p")?;
    }

    for id in [PROFILE_ID, LEGACY_PROFILE_ID] {
        let profile_path = config_library.join(format!("{id}.json"));
        if profile_path.exists() {
            backup_file_named(
                &profile_path,
                &format!("{id}.json"),
                MAX_BACKUPS,
            )?;
            std::fs::remove_file(&profile_path)?;
        }
    }

    write_meta(&meta_path, None, Some(PROFILE_NAME))?;
    Ok(())
}

pub fn clear_provider_restoring_applied_id(previous: Option<String>) -> AppResult<()> {
    clear_provider()?;
    let Some(previous) = previous.filter(|id| !is_managed_profile_id(id)) else {
        return Ok(());
    };
    let paths = platform_paths()?;
    let Some(meta_path) = paths.meta_path else {
        return Ok(());
    };
    let mut value = read_json_file::<Value>(&meta_path)?.unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({});
    }
    value["appliedId"] = Value::String(previous);
    write_json_file(&meta_path, &value)
}

pub fn current_applied_id() -> AppResult<Option<String>> {
    let paths = detect_claude_desktop();
    let Some(meta_path) = paths.meta_path else {
        return Ok(None);
    };
    if !meta_path.exists() {
        return Ok(None);
    }
    let value = read_json_file::<Value>(&meta_path)?.unwrap_or_else(|| serde_json::json!({}));
    Ok(value
        .get("appliedId")
        .and_then(Value::as_str)
        .map(str::to_string))
}

pub fn is_managed_profile_id(id: &str) -> bool {
    id == PROFILE_ID || id == LEGACY_PROFILE_ID
}

pub fn current_profile_uses_legacy_role_routes() -> AppResult<bool> {
    let paths = platform_paths()?;
    profile_paths_use_legacy_role_routes(&paths)
}

fn profile_paths_use_legacy_role_routes(paths: &ClaudeDesktopPaths) -> AppResult<bool> {
    let Some(meta_path) = paths.meta_path.as_ref() else {
        return Ok(false);
    };
    if !meta_path.exists() {
        return Ok(false);
    }
    let meta = read_json_file::<Value>(meta_path)?.unwrap_or_else(|| serde_json::json!({}));
    if meta.get("appliedId").and_then(Value::as_str) != Some(PROFILE_ID) {
        return Ok(false);
    }
    let Some(config_library) = paths.config_library.as_ref() else {
        return Ok(false);
    };
    let profile_path = config_library.join(format!("{PROFILE_ID}.json"));
    if !profile_path.exists() {
        return Ok(false);
    }
    let Some(profile) = read_json_file::<Value>(&profile_path)? else {
        return Ok(false);
    };
    Ok(profile_uses_legacy_role_routes(&profile))
}

fn profile_uses_legacy_role_routes(profile: &Value) -> bool {
    profile
        .get("inferenceModels")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                matches!(
                    model.get("name").and_then(Value::as_str),
                    Some("claude-sonnet-4-6" | "claude-opus-4-8")
                )
            })
        })
}

fn build_profile(provider: &Provider, proxy_port: u16) -> AppResult<Value> {
    let role_routes = provider.requires_local_proxy();
    let (base_url, api_key) = if role_routes {
        let token = get_or_create_gateway_token()?;
        (desktop_proxy_gateway_base_url(proxy_port), token)
    } else {
        (provider.base_url.clone(), provider.api_key.clone())
    };

    let mut profile = serde_json::json!({
        "coworkEgressAllowedHosts": ["*"],
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

pub fn active_profile_uses_local_proxy() -> bool {
    let Ok(applied) = current_applied_id() else {
        return false;
    };
    let Some(applied) = applied.filter(|id| is_managed_profile_id(id)) else {
        return false;
    };
    let paths = detect_claude_desktop();
    let Some(config_library) = paths.config_library else {
        return false;
    };
    let profile_path = config_library.join(format!("{applied}.json"));
    let Ok(Some(profile)) = read_json_file::<Value>(&profile_path) else {
        return false;
    };
    let base_url = profile
        .get("inferenceGatewayBaseUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    base_url.contains("127.0.0.1") && base_url.contains(CLAUDE_DESKTOP_PROXY_PREFIX)
}

pub fn read_current_live_provider() -> AppResult<Option<crate::provider::LiveProviderInfo>> {
    let paths = detect_claude_desktop();
    let Some(config_library) = paths.config_library else {
        return Ok(None);
    };
    let applied = current_applied_id()?;
    let id = applied
        .as_deref()
        .filter(|id| is_managed_profile_id(id))
        .unwrap_or(PROFILE_ID);
    let profile_path = config_library.join(format!("{id}.json"));
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

pub fn get_or_create_gateway_token() -> AppResult<String> {
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

fn read_json_or_empty(path: &Path) -> AppResult<Value> {
    let value = if path.exists() {
        read_json_file(path)?.unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if value.is_object() {
        Ok(value)
    } else {
        Ok(serde_json::json!({}))
    }
}

fn write_deployment_mode(path: &Path, mode: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut value = read_json_or_empty(path)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "deploymentMode".to_string(),
            Value::String(mode.to_string()),
        );
    }
    write_json_file(path, &value)
}

fn write_meta(path: &Path, applied_id: Option<&str>, our_name: Option<&str>) -> AppResult<()> {
    let mut value = read_json_or_empty(path)?;
    let obj = value.as_object_mut().expect("normalized to object");

    let mut entries = obj
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    entries.retain(|entry| {
        !entry
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(is_managed_profile_id)
    });

    if let Some(id) = applied_id {
        entries.push(serde_json::json!({
            "id": PROFILE_ID,
            "name": our_name.unwrap_or(PROFILE_NAME),
        }));
        obj.insert("appliedId".to_string(), Value::String(id.to_string()));
    } else if obj
        .get("appliedId")
        .and_then(Value::as_str)
        .is_some_and(is_managed_profile_id)
    {
        obj.remove("appliedId");
    }

    obj.insert("entries".to_string(), Value::Array(entries));
    write_json_file(path, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProtocolType, ProviderKind, ProviderTarget};
    use tempfile::tempdir;

    fn mapped_provider() -> Provider {
        Provider {
            id: "desktop-models".into(),
            name: "Mapped".into(),
            base_url: "https://api.example.test".into(),
            api_key: "secret".into(),
            api_key_set: true,
            model: "default-upstream".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping {
                sonnet: "upstream-sonnet".into(),
                opus: "upstream-opus".into(),
                haiku: "upstream-haiku".into(),
                fable: "upstream-fable".into(),
                subagent: String::new(),
            },
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
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
        if !is_supported_platform() {
            let p = detect_claude_desktop();
            assert!(p.base.is_none());
        }
    }

    #[test]
    fn supported_platform_paths_are_populated() {
        if is_supported_platform() {
            let p = detect_claude_desktop();
            assert!(p.base.is_some());
            assert!(p.threep_base.is_some());
            assert!(p.config_library.is_some());
            assert!(p.meta_path.is_some());
            assert!(p.normal_config_path.is_some());
            assert!(p.threep_config_path.is_some());
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
        assert_eq!(models[1]["name"], "claude-opus-5");
        assert_eq!(models[1]["labelOverride"], "upstream-opus");
        assert_eq!(models[3]["name"], "claude-fable-5");
        assert_eq!(models[3]["labelOverride"], "upstream-fable");

        let response = model_list_response(&provider);
        assert_eq!(response["data"][1]["id"], "claude-opus-5");
        assert_eq!(response["data"][2]["id"], "claude-haiku-4-5");
        assert!(!response["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "claude-opus-4-8"));
    }

    #[test]
    fn stale_route_detection_only_applies_to_the_managed_uuid_profile() {
        let dir = tempdir().unwrap();
        let paths = paths_from_dirs(dir.path().join("Claude"), dir.path().join("Claude-3p"));
        let config_library = paths.config_library.as_ref().unwrap();
        let meta_path = paths.meta_path.as_ref().unwrap();
        std::fs::create_dir_all(config_library).unwrap();
        let profile_path = config_library.join(format!("{PROFILE_ID}.json"));

        write_json_file(
            meta_path,
            &serde_json::json!({
                "appliedId": PROFILE_ID,
                "entries": [{"id": PROFILE_ID, "name": PROFILE_NAME}]
            }),
        )
        .unwrap();
        write_json_file(
            &profile_path,
            &serde_json::json!({
                "inferenceModels": [
                    {"name": "claude-sonnet-5"},
                    {"name": "claude-opus-4-8"},
                    {"name": "claude-haiku-4-5"},
                    {"name": "claude-fable-5"}
                ]
            }),
        )
        .unwrap();
        assert!(profile_paths_use_legacy_role_routes(&paths).unwrap());

        write_json_file(
            &profile_path,
            &serde_json::json!({
                "inferenceModels": [
                    {"name": "claude-sonnet-5"},
                    {"name": "claude-opus-5"},
                    {"name": "claude-haiku-4-5"},
                    {"name": "claude-fable-5"}
                ]
            }),
        )
        .unwrap();
        assert!(!profile_paths_use_legacy_role_routes(&paths).unwrap());

        write_json_file(
            meta_path,
            &serde_json::json!({
                "appliedId": "user-profile",
                "entries": [{"id": "user-profile", "name": "User profile"}]
            }),
        )
        .unwrap();
        write_json_file(
            &profile_path,
            &serde_json::json!({
                "inferenceModels": [{"name": "claude-opus-4-8"}]
            }),
        )
        .unwrap();
        assert!(!profile_paths_use_legacy_role_routes(&paths).unwrap());
    }

    #[test]
    fn proxy_profile_uses_claude_desktop_gateway_prefix() {
        let provider = Provider {
            id: "proxy-provider".into(),
            name: "Proxy".into(),
            base_url: "https://api.example.test".into(),
            api_key: "secret".into(),
            api_key_set: true,
            model: "deepseek-chat".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping {
                sonnet: "deepseek-chat".into(),
                opus: "deepseek-chat".into(),
                haiku: "deepseek-chat".into(),
                fable: "deepseek-chat".into(),
                subagent: String::new(),
            },
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeDesktop,
            notes: String::new(),
            sort_index: 0,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
        };
        let profile = build_profile(&provider, 15_821).expect("profile");
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            serde_json::json!("http://127.0.0.1:15821/claude-desktop")
        );
        assert_eq!(profile["coworkEgressAllowedHosts"], serde_json::json!(["*"]));
    }

    #[test]
    fn paths_from_dirs_uses_threep_for_config_library() {
        let paths = paths_from_dirs(
            PathBuf::from("/tmp/Claude"),
            PathBuf::from("/tmp/Claude-3p"),
        );
        assert_eq!(paths.base, Some(PathBuf::from("/tmp/Claude")));
        assert_eq!(paths.threep_base, Some(PathBuf::from("/tmp/Claude-3p")));
        assert_eq!(
            paths.config_library,
            Some(PathBuf::from("/tmp/Claude-3p/configLibrary"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_picker_prefers_msix_config_over_empty_local_dir() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("Local");
        let roaming = root.path().join("Roaming");
        let empty_local = local.join("Claude");
        std::fs::create_dir_all(&empty_local).unwrap();

        let msix = local
            .join("Packages")
            .join("Claude_pzs8sxrjxfjjc")
            .join("LocalCache")
            .join("Roaming")
            .join("Claude");
        std::fs::create_dir_all(&msix).unwrap();
        std::fs::write(msix.join(CONFIG_FILE), r#"{"mcpServers":{}}"#).unwrap();

        let picked = pick_windows_claude_dir(&local, &roaming, false).unwrap();
        assert_eq!(picked, msix);
    }

    #[cfg(windows)]
    #[test]
    fn windows_picker_keeps_local_when_it_owns_the_config() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("Local");
        let roaming = root.path().join("Roaming");
        let local_claude = local.join("Claude");
        std::fs::create_dir_all(&local_claude).unwrap();
        std::fs::write(local_claude.join(CONFIG_FILE), r#"{"mcpServers":{}}"#).unwrap();

        let msix = local
            .join("Packages")
            .join("Claude_pzs8sxrjxfjjc")
            .join("LocalCache")
            .join("Roaming")
            .join("Claude");
        std::fs::create_dir_all(&msix).unwrap();
        std::fs::write(msix.join(CONFIG_FILE), r#"{"mcpServers":{"other":{}}}"#).unwrap();

        let picked = pick_windows_claude_dir(&local, &roaming, false).unwrap();
        assert_eq!(picked, local_claude);
    }

    #[test]
    fn managed_profile_id_is_a_stable_uuid() {
        assert!(uuid::Uuid::parse_str(PROFILE_ID).is_ok());
        assert!(is_managed_profile_id(PROFILE_ID));
        assert!(is_managed_profile_id(LEGACY_PROFILE_ID));
    }

    #[test]
    fn write_meta_replaces_legacy_profile_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("_meta.json");
        write_json_file(
            &path,
            &serde_json::json!({
                "appliedId": LEGACY_PROFILE_ID,
                "entries": [
                    { "id": LEGACY_PROFILE_ID, "name": "Legacy" },
                    { "id": "other-profile", "name": "Other" }
                ]
            }),
        )
        .expect("seed meta");

        write_meta(&path, Some(PROFILE_ID), Some(PROFILE_NAME)).expect("write meta");
        let value = read_json_file::<Value>(&path)
            .expect("read meta")
            .expect("meta exists");
        assert_eq!(value["appliedId"], PROFILE_ID);
        let entries = value["entries"].as_array().expect("entries");
        assert!(entries
            .iter()
            .any(|entry| entry["id"] == serde_json::json!(PROFILE_ID)));
        assert!(!entries
            .iter()
            .any(|entry| entry["id"] == serde_json::json!(LEGACY_PROFILE_ID)));
        assert!(entries
            .iter()
            .any(|entry| entry["id"] == serde_json::json!("other-profile")));
    }
}
