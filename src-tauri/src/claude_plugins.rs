//! Claude Code plugins discovery and enable/disable via settings.json.
//!
//! Enable state lives in `~/.claude/settings.json`:
//! ```json
//! { "enabledPlugins": { "name@marketplace": true } }
//! ```
//! Install records: `~/.claude/plugins/installed_plugins.json`
//! Cache layout: `~/.claude/plugins/cache/<marketplace>/<name>/<version>/`
//! Marketplaces: `~/.claude/plugins/known_marketplaces.json` (+ CLI add/remove)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::{
    get_claude_installed_plugins_path, get_claude_known_marketplaces_path,
    get_claude_marketplaces_dir, get_claude_plugins_cache_dir, get_claude_plugins_dir,
    get_claude_settings_path, read_json_file, write_json_file,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePlugin {
    pub plugin_id: String,
    pub name: String,
    pub marketplace: String,
    pub version: Option<String>,
    pub enabled: bool,
    pub installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginsSnapshot {
    pub plugins: Vec<ClaudePlugin>,
    pub config_path: String,
    pub cache_path: String,
    pub config_plugin_count: usize,
    pub cache_plugin_count: usize,
    pub parse_ok: bool,
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeMarketplace {
    pub name: String,
    pub root: Option<String>,
    pub source: Option<String>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeMarketplaceListResult {
    pub marketplaces: Vec<ClaudeMarketplace>,
    pub raw_output: String,
    pub used_json: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCatalogPlugin {
    pub plugin_id: String,
    pub name: String,
    pub marketplace: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginCatalog {
    pub plugins: Vec<ClaudeCatalogPlugin>,
    pub marketplaces_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginCommandResult {
    pub ok: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginUpdateStatus {
    pub plugin_id: String,
    pub status: String,
    pub message: String,
    pub local_version: Option<String>,
    pub remote_version: Option<String>,
}

pub fn list_plugins() -> AppResult<Vec<ClaudePlugin>> {
    Ok(list_plugins_snapshot()?.plugins)
}

pub fn list_plugins_snapshot() -> AppResult<ClaudePluginsSnapshot> {
    list_plugins_snapshot_at(
        &get_claude_settings_path(),
        &get_claude_installed_plugins_path(),
        &get_claude_plugins_cache_dir(),
    )
}

pub fn list_plugins_snapshot_at(
    settings_path: &Path,
    installed_path: &Path,
    cache_path: &Path,
) -> AppResult<ClaudePluginsSnapshot> {
    let (enabled_map, parse_ok, parse_error) = read_enabled_map_tolerant(settings_path);
    let config_plugin_count = enabled_map.len();

    let mut by_id: BTreeMap<String, ClaudePlugin> = BTreeMap::new();
    for (plugin_id, enabled) in enabled_map {
        let (name, marketplace) = split_plugin_id(&plugin_id);
        by_id.insert(
            plugin_id.clone(),
            ClaudePlugin {
                plugin_id,
                name,
                marketplace,
                version: None,
                enabled,
                installed: false,
                path: None,
            },
        );
    }

    let installed = read_installed_plugins(installed_path);
    let mut cache_plugin_count = installed.len();
    for item in installed {
        merge_discovered(&mut by_id, item);
    }

    // Fallback / supplement: scan cache tree (same shape as Codex).
    let cached = scan_cache(cache_path)?;
    if cache_plugin_count == 0 {
        cache_plugin_count = cached.len();
    }
    for item in cached {
        merge_discovered(&mut by_id, item);
    }

    let mut plugins: Vec<ClaudePlugin> = by_id.into_values().collect();
    plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    Ok(ClaudePluginsSnapshot {
        plugins,
        config_path: settings_path.to_string_lossy().into_owned(),
        cache_path: cache_path.to_string_lossy().into_owned(),
        config_plugin_count,
        cache_plugin_count,
        parse_ok,
        parse_error,
    })
}

pub fn set_plugin_enabled(plugin_id: &str, enabled: bool) -> AppResult<()> {
    set_plugin_enabled_at(&get_claude_settings_path(), plugin_id, enabled)
}

pub fn set_plugin_enabled_at(
    settings_path: &Path,
    plugin_id: &str,
    enabled: bool,
) -> AppResult<()> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Claude 插件 ID: {plugin_id}"
        )));
    }
    let mut settings =
        read_json_file::<Value>(settings_path)?.unwrap_or_else(|| Value::Object(Map::new()));
    let object = settings
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Claude Code settings.json 必须是 JSON 对象".into()))?;
    let plugins = object
        .entry("enabledPlugins".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let plugins_obj = plugins
        .as_object_mut()
        .ok_or_else(|| AppError::Config("settings.json enabledPlugins 必须是对象".into()))?;
    plugins_obj.insert(plugin_id.to_string(), Value::Bool(enabled));
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json_file(settings_path, &settings)
}

/// List marketplaces from `known_marketplaces.json` (no CLI required).
pub fn list_marketplaces() -> AppResult<ClaudeMarketplaceListResult> {
    list_marketplaces_at(&get_claude_known_marketplaces_path())
}

pub fn list_marketplaces_at(path: &Path) -> AppResult<ClaudeMarketplaceListResult> {
    if !path.is_file() {
        return Ok(ClaudeMarketplaceListResult {
            marketplaces: Vec::new(),
            raw_output: String::new(),
            used_json: true,
        });
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| AppError::Other(format!("读取 marketplace 失败: {e}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::Config(format!("known_marketplaces.json 无效: {e}")))?;
    let mut marketplaces = Vec::new();
    if let Some(map) = value.as_object() {
        for (name, entry) in map {
            let obj = entry.as_object();
            let source = obj.and_then(|o| o.get("source")).and_then(|s| {
                s.as_str().map(str::to_string).or_else(|| {
                    s.as_object().and_then(|src| {
                        src.get("url")
                            .or_else(|| src.get("path"))
                            .or_else(|| src.get("repo"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
            });
            let root = obj
                .and_then(|o| o.get("installLocation").or_else(|| o.get("path")))
                .and_then(Value::as_str)
                .map(str::to_string);
            marketplaces.push(ClaudeMarketplace {
                name: name.clone(),
                root,
                source,
                raw: Some(entry.to_string()),
            });
        }
    }
    marketplaces.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ClaudeMarketplaceListResult {
        marketplaces,
        raw_output: raw,
        used_json: true,
    })
}

/// List installable plugins declared by cloned marketplaces (`marketplace.json`).
pub fn list_plugin_catalog() -> AppResult<ClaudePluginCatalog> {
    list_plugin_catalog_at(&get_claude_marketplaces_dir())
}

pub fn list_plugin_catalog_at(marketplaces_dir: &Path) -> AppResult<ClaudePluginCatalog> {
    let mut plugins = Vec::new();
    if !marketplaces_dir.is_dir() {
        return Ok(ClaudePluginCatalog {
            plugins,
            marketplaces_dir: marketplaces_dir.to_string_lossy().into_owned(),
        });
    }
    for entry in fs::read_dir(marketplaces_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let marketplace = entry.file_name().to_string_lossy().into_owned();
        if marketplace.starts_with("temp_") || marketplace.ends_with(".bak") {
            continue;
        }
        let manifest = entry.path().join(".claude-plugin").join("marketplace.json");
        let Ok(Some(Value::Object(root))) = read_json_file::<Value>(&manifest) else {
            continue;
        };
        let marketplace_name = root
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&marketplace)
            .to_string();
        let Some(items) = root.get("plugins").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(name) = obj.get("name").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            plugins.push(ClaudeCatalogPlugin {
                plugin_id: format!("{name}@{marketplace_name}"),
                name: name.to_string(),
                marketplace: marketplace_name.clone(),
                description: obj
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                category: obj
                    .get("category")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                version: obj
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| resolve_plugin_manifest_version(&entry.path(), name)),
            });
        }
    }
    plugins.sort_by(|a, b| {
        a.marketplace
            .cmp(&b.marketplace)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(ClaudePluginCatalog {
        plugins,
        marketplaces_dir: marketplaces_dir.to_string_lossy().into_owned(),
    })
}

pub fn add_marketplace(executable: &Path, source: &str) -> AppResult<ClaudePluginCommandResult> {
    let source = normalize_marketplace_source(source.trim());
    if source.is_empty() {
        return Err(AppError::Config("marketplace 源不能为空".into()));
    }
    let output = run_claude_plugin_args(
        executable,
        &["plugin", "marketplace", "add", "--scope", "user", &source],
    )?;
    let result = command_result(output, "已添加 marketplace");
    result.map_err(enrich_git_path_error)
}

pub fn remove_marketplace(executable: &Path, name: &str) -> AppResult<ClaudePluginCommandResult> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("marketplace 名称不能为空".into()));
    }
    // Prefer scoped remove; fall back without --scope if the CLI rejects it.
    let output = run_claude_plugin_args(
        executable,
        &["plugin", "marketplace", "remove", "--scope", "user", name],
    )?;
    if output.status.success() {
        return command_result(output, "已移除 marketplace");
    }
    let fallback = run_claude_plugin_args(executable, &["plugin", "marketplace", "remove", name])?;
    command_result(fallback, "已移除 marketplace")
}

pub fn uninstall_plugin(
    executable: &Path,
    plugin_id: &str,
) -> AppResult<ClaudePluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Claude 插件 ID: {plugin_id}"
        )));
    }
    let output = run_claude_plugin_args(
        executable,
        &["plugin", "uninstall", "--scope", "user", plugin_id],
    )?;
    let result = if output.status.success() {
        command_result(output, "已卸载插件")?
    } else {
        let fallback = run_claude_plugin_args(
            executable,
            &["plugin", "remove", "--scope", "user", plugin_id],
        )?;
        if fallback.status.success() {
            command_result(fallback, "已卸载插件")?
        } else {
            let bare = run_claude_plugin_args(executable, &["plugin", "uninstall", plugin_id])?;
            command_result(bare, "已卸载插件")?
        }
    };
    // Best-effort: drop enable entry if CLI left it behind.
    let _ = remove_enabled_entry(&get_claude_settings_path(), plugin_id);
    Ok(result)
}

/// Install a plugin via `claude plugin install --scope user <name@marketplace>`.
pub fn install_plugin(executable: &Path, plugin_id: &str) -> AppResult<ClaudePluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Claude 插件 ID: {plugin_id}（格式应为 name@marketplace，例如 claude-hud@claude-hud）"
        )));
    }
    let output = run_claude_plugin_args(
        executable,
        &["plugin", "install", "--scope", "user", plugin_id],
    )?;
    let result = if output.status.success() {
        command_result(output, "已安装插件").map_err(enrich_git_path_error)?
    } else {
        let bare = run_claude_plugin_args(executable, &["plugin", "install", plugin_id])?;
        command_result(bare, "已安装插件").map_err(enrich_git_path_error)?
    };
    // Ensure enabled so it shows as on after install (CLI usually writes this already).
    let _ = set_plugin_enabled(plugin_id, true);
    Ok(result)
}

/// Refresh marketplace clone(s): `claude plugin marketplace update [name]`.
pub fn update_marketplace(
    executable: &Path,
    name: Option<&str>,
) -> AppResult<ClaudePluginCommandResult> {
    let mut args = vec!["plugin", "marketplace", "update"];
    let owned;
    if let Some(name) = name.map(str::trim).filter(|s| !s.is_empty()) {
        owned = name.to_string();
        args.push(owned.as_str());
    }
    let output = run_claude_plugin_args(executable, &args)?;
    command_result(output, "已更新 marketplace").map_err(enrich_git_path_error)
}

/// Update an installed plugin: `claude plugin update --scope user <id>`.
pub fn update_plugin(executable: &Path, plugin_id: &str) -> AppResult<ClaudePluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Claude 插件 ID: {plugin_id}"
        )));
    }
    // Refresh that marketplace first so update sees latest catalog.
    let (_, marketplace) = split_plugin_id(plugin_id);
    if !marketplace.is_empty() {
        let _ = update_marketplace(executable, Some(&marketplace));
    }
    let output = run_claude_plugin_args(
        executable,
        &["plugin", "update", "--scope", "user", plugin_id],
    )?;
    let result = if output.status.success() {
        command_result(output, "已更新插件").map_err(enrich_git_path_error)?
    } else {
        let bare = run_claude_plugin_args(executable, &["plugin", "update", plugin_id])?;
        command_result(bare, "已更新插件").map_err(enrich_git_path_error)?
    };
    Ok(result)
}

/// Check update status for one installed plugin (refreshes its marketplace first).
pub fn check_plugin_update(
    executable: &Path,
    plugin_id: &str,
) -> AppResult<ClaudePluginUpdateStatus> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Claude 插件 ID: {plugin_id}"
        )));
    }
    let (_, marketplace) = split_plugin_id(plugin_id);
    if !marketplace.is_empty() {
        let _ = update_marketplace(executable, Some(&marketplace));
    }
    Ok(evaluate_claude_update_status(plugin_id))
}

/// Refresh all marketplaces, then check every installed plugin.
pub fn check_plugin_updates(executable: &Path) -> AppResult<Vec<ClaudePluginUpdateStatus>> {
    log::info!("检查 Claude 插件更新: 刷新 marketplace");
    if let Err(error) = update_marketplace(executable, None) {
        log::warn!("刷新 Claude marketplace 失败（仍用本地目录比较版本）: {error}");
    }
    let snap = list_plugins_snapshot()?;
    let mut out = Vec::new();
    for plugin in snap.plugins.into_iter().filter(|p| p.installed) {
        out.push(evaluate_claude_update_status(&plugin.plugin_id));
    }
    log::info!("检查 Claude 插件更新完成: {} 个已安装插件", out.len());
    Ok(out)
}

fn evaluate_claude_update_status(plugin_id: &str) -> ClaudePluginUpdateStatus {
    let snap = list_plugins_snapshot().ok();
    let local = snap
        .as_ref()
        .and_then(|s| s.plugins.iter().find(|p| p.plugin_id == plugin_id))
        .cloned();
    let local_version = local.as_ref().and_then(|p| p.version.clone());
    let catalog = list_plugin_catalog().ok();
    let remote_version = catalog
        .as_ref()
        .and_then(|c| c.plugins.iter().find(|p| p.plugin_id == plugin_id))
        .and_then(|p| p.version.clone());

    match (local_version.as_deref(), remote_version.as_deref()) {
        (Some(local), Some(remote)) if versions_equal(local, remote) => ClaudePluginUpdateStatus {
            plugin_id: plugin_id.to_string(),
            status: "up_to_date".into(),
            message: format!("已是最新（{local}）"),
            local_version,
            remote_version,
        },
        (Some(local), Some(remote)) => ClaudePluginUpdateStatus {
            plugin_id: plugin_id.to_string(),
            status: "update_available".into(),
            message: format!("可更新：{local} → {remote}"),
            local_version,
            remote_version,
        },
        (local_v, remote_v) => {
            let message = if local.as_ref().is_some_and(|p| p.installed) {
                "无法比较版本（市场或安装记录缺少 version）；仍可尝试更新".to_string()
            } else {
                "插件未安装或不在本地清单中".to_string()
            };
            ClaudePluginUpdateStatus {
                plugin_id: plugin_id.to_string(),
                status: if local.as_ref().is_some_and(|p| p.installed) {
                    "unknown".into()
                } else {
                    "not_installed".into()
                },
                message,
                local_version: local_v.map(str::to_string),
                remote_version: remote_v.map(str::to_string),
            }
        }
    }
}

fn versions_equal(a: &str, b: &str) -> bool {
    normalize_version(a) == normalize_version(b)
}

fn normalize_version(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_ascii_lowercase()
}

fn resolve_plugin_manifest_version(market_root: &Path, plugin_name: &str) -> Option<String> {
    let candidates = [
        market_root
            .join("plugins")
            .join(plugin_name)
            .join(".claude-plugin")
            .join("plugin.json"),
        market_root
            .join(plugin_name)
            .join(".claude-plugin")
            .join("plugin.json"),
        market_root
            .join("plugins")
            .join(plugin_name)
            .join("plugin.json"),
        market_root.join(plugin_name).join("plugin.json"),
    ];
    for path in candidates {
        if let Ok(Some(Value::Object(obj))) = read_json_file::<Value>(&path) {
            if let Some(version) = obj.get("version").and_then(Value::as_str) {
                let trimmed = version.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

fn remove_enabled_entry(settings_path: &Path, plugin_id: &str) -> AppResult<()> {
    let Some(mut settings) = read_json_file::<Value>(settings_path)? else {
        return Ok(());
    };
    let Some(object) = settings.as_object_mut() else {
        return Ok(());
    };
    if let Some(Value::Object(plugins)) = object.get_mut("enabledPlugins") {
        plugins.remove(plugin_id);
        write_json_file(settings_path, &settings)?;
    }
    Ok(())
}

fn command_result(output: Output, success_message: &str) -> AppResult<ClaudePluginCommandResult> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(ClaudePluginCommandResult {
            ok: true,
            message: if stdout.is_empty() {
                success_message.to_string()
            } else {
                stdout.clone()
            },
            stdout,
            stderr,
        })
    } else {
        Err(AppError::Other(first_nonempty(&stderr, &stdout)))
    }
}

fn first_nonempty(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if !primary.is_empty() {
        primary.to_string()
    } else {
        fallback.trim().to_string()
    }
}

/// Accept `owner/repo`, HTTPS GitHub URLs, or `.git` suffixes.
fn normalize_marketplace_source(source: &str) -> String {
    let trimmed = source.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "ssh://git@github.com/",
    ] {
        if let Some(rest) = without_git.strip_prefix(prefix) {
            let owner_repo = rest.trim_matches('/');
            if owner_repo
                .split('/')
                .filter(|part| !part.is_empty())
                .count()
                >= 2
            {
                let mut parts = owner_repo.split('/');
                let owner = parts.next().unwrap_or("");
                let repo = parts.next().unwrap_or("");
                if !owner.is_empty() && !repo.is_empty() {
                    return format!("{owner}/{repo}");
                }
            }
        }
    }
    without_git.to_string()
}

fn enrich_git_path_error(error: AppError) -> AppError {
    let message = error.to_string();
    if message.to_ascii_lowercase().contains("git")
        && (message.contains("not found") || message.contains("unsafe location"))
    {
        AppError::Other(format!(
            "{message}\n提示：GUI 启动时可能找不到 Git。请确认已安装 Git for Windows，或把 Git\\cmd 加入系统 PATH 后重启 AI-Switcher。"
        ))
    } else {
        error
    }
}

fn run_claude_plugin_args(executable: &Path, args: &[&str]) -> AppResult<Output> {
    run_claude_plugin_args_timeout(executable, args, crate::process_util::CLI_COMMAND_TIMEOUT)
}

fn run_claude_plugin_args_timeout(
    executable: &Path,
    args: &[&str],
    timeout: Duration,
) -> AppResult<Output> {
    let path_env = plugin_cli_path_env(executable.parent());
    let work_dir = crate::config::get_home_dir();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let extension = executable
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let mut command = if extension.eq_ignore_ascii_case("cmd")
            || extension.eq_ignore_ascii_case("bat")
            || extension.is_empty()
        {
            let mut line = format!("call {}", quote_cmd_arg(&executable.to_string_lossy()));
            for arg in args {
                line.push(' ');
                line.push_str(&quote_cmd_arg(arg));
            }
            let mut command = Command::new("cmd.exe");
            command
                .args(["/D", "/S", "/C"])
                .arg(line)
                .current_dir(&work_dir)
                .env("PATH", &path_env)
                .creation_flags(CREATE_NO_WINDOW);
            command
        } else {
            let mut command = Command::new(executable);
            command
                .args(args)
                .current_dir(&work_dir)
                .env("PATH", &path_env)
                .creation_flags(CREATE_NO_WINDOW);
            command
        };
        crate::process_util::output_with_timeout(&mut command, timeout)
            .map_err(|e| AppError::Other(format!("启动 Claude Code 失败: {e}")))
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(&work_dir)
            .env("PATH", &path_env);
        crate::process_util::output_with_timeout(&mut command, timeout)
            .map_err(|e| AppError::Other(format!("启动 Claude Code 失败: {e}")))
    }
}

/// GUI-launched Tauri often lacks Git/npm on PATH; Claude marketplace clone needs `git`.
fn plugin_cli_path_env(tool_dir: Option<&Path>) -> std::ffi::OsString {
    use std::collections::HashSet;

    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = tool_dir {
        dirs.push(dir.to_path_buf());
    }
    dirs.extend(common_git_bin_dirs());
    dirs.push(PathBuf::from(r"C:\Program Files\nodejs"));
    #[cfg(windows)]
    {
        if let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
            dirs.push(system_root.join("System32"));
            dirs.push(system_root);
        }
        dirs.extend(windows_user_path_dirs_for_plugins());
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
    }

    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    let mut estimated_length = 0usize;
    for path in dirs {
        if path.as_os_str().is_empty() || !seen.insert(path.clone()) {
            continue;
        }
        let added = path.as_os_str().to_string_lossy().len() + 1;
        if estimated_length + added > 6_000 {
            continue;
        }
        estimated_length += added;
        unique.push(path);
    }
    std::env::join_paths(unique).unwrap_or_default()
}

fn common_git_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        for root in [program_files, program_files_x86, local] {
            let Some(root) = root else { continue };
            dirs.push(root.join("Git").join("cmd"));
            dirs.push(root.join("Git").join("bin"));
            dirs.push(root.join("Programs").join("Git").join("cmd"));
            dirs.push(root.join("Programs").join("Git").join("bin"));
        }
        // Hard fallbacks for typical installs when env vars are thin.
        dirs.push(PathBuf::from(r"C:\Program Files\Git\cmd"));
        dirs.push(PathBuf::from(r"C:\Program Files\Git\bin"));
        dirs.push(PathBuf::from(r"C:\Program Files (x86)\Git\cmd"));
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

#[cfg(windows)]
fn windows_user_path_dirs_for_plugins() -> Vec<PathBuf> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let Ok(env) = hkcu.open_subkey("Environment") else {
        return Vec::new();
    };
    let Ok(value) = env.get_value::<String, _>("Path") else {
        return Vec::new();
    };
    let expanded = expand_windows_env_for_plugins(&value);
    std::env::split_paths(&expanded).collect()
}

#[cfg(windows)]
fn expand_windows_env_for_plugins(value: &str) -> String {
    let mut result = value.to_string();
    for (key, replacement) in [
        (
            "%USERPROFILE%",
            dirs::home_dir().map(|p| p.to_string_lossy().into_owned()),
        ),
        (
            "%LOCALAPPDATA%",
            dirs::data_local_dir().map(|p| p.to_string_lossy().into_owned()),
        ),
        (
            "%APPDATA%",
            dirs::config_dir().map(|p| p.to_string_lossy().into_owned()),
        ),
    ] {
        if let Some(replacement) = replacement {
            result = result.replace(key, &replacement);
            result = result.replace(&key.to_ascii_lowercase(), &replacement);
        }
    }
    result
}

#[cfg(windows)]
fn quote_cmd_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.contains([' ', '\t', '"', '&', '|', '<', '>', '^', '%']) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\"\""))
}

fn read_enabled_map_tolerant(path: &Path) -> (Vec<(String, bool)>, bool, Option<String>) {
    if !path.exists() {
        return (Vec::new(), true, None);
    }
    match read_json_file::<Value>(path) {
        Ok(Some(Value::Object(map))) => {
            let enabled = map
                .get("enabledPlugins")
                .and_then(Value::as_object)
                .map(|plugins| {
                    plugins
                        .iter()
                        .filter_map(|(id, value)| {
                            value.as_bool().map(|enabled| (id.clone(), enabled))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (enabled, true, None)
        }
        Ok(Some(_)) => (
            Vec::new(),
            false,
            Some("Claude Code settings.json 必须是 JSON 对象".into()),
        ),
        Ok(None) => (Vec::new(), true, None),
        Err(error) => (Vec::new(), false, Some(error.to_string())),
    }
}

#[derive(Debug)]
struct DiscoveredPlugin {
    plugin_id: String,
    name: String,
    marketplace: String,
    version: Option<String>,
    path: Option<String>,
}

fn merge_discovered(by_id: &mut BTreeMap<String, ClaudePlugin>, item: DiscoveredPlugin) {
    let entry = by_id
        .entry(item.plugin_id.clone())
        .or_insert_with(|| ClaudePlugin {
            plugin_id: item.plugin_id.clone(),
            name: item.name.clone(),
            marketplace: item.marketplace.clone(),
            version: None,
            enabled: false,
            installed: false,
            path: None,
        });
    entry.installed = true;
    entry.version = item.version.or(entry.version.clone());
    entry.path = item.path.or(entry.path.clone());
    if entry.name.is_empty() {
        entry.name = item.name;
    }
    if entry.marketplace.is_empty() {
        entry.marketplace = item.marketplace;
    }
}

fn read_installed_plugins(path: &Path) -> Vec<DiscoveredPlugin> {
    let Ok(Some(Value::Object(root))) = read_json_file::<Value>(path) else {
        return Vec::new();
    };
    let Some(plugins) = root.get("plugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (plugin_id, entries) in plugins {
        let (name, marketplace) = split_plugin_id(plugin_id);
        let latest = entries
            .as_array()
            .and_then(|items| items.last())
            .and_then(Value::as_object);
        let version = latest
            .and_then(|o| o.get("version"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let install_path = latest
            .and_then(|o| o.get("installPath"))
            .and_then(Value::as_str)
            .map(str::to_string);
        out.push(DiscoveredPlugin {
            plugin_id: plugin_id.clone(),
            name,
            marketplace,
            version,
            path: install_path,
        });
    }
    out
}

fn scan_cache(cache_root: &Path) -> AppResult<Vec<DiscoveredPlugin>> {
    if !cache_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for marketplace_entry in fs::read_dir(cache_root)? {
        let marketplace_entry = marketplace_entry?;
        if !marketplace_entry.file_type()?.is_dir() {
            continue;
        }
        let marketplace = marketplace_entry.file_name().to_string_lossy().into_owned();
        for plugin_entry in fs::read_dir(marketplace_entry.path())? {
            let plugin_entry = plugin_entry?;
            if !plugin_entry.file_type()?.is_dir() {
                continue;
            }
            let name = plugin_entry.file_name().to_string_lossy().into_owned();
            let (version, path) = newest_version_dir(&plugin_entry.path())?;
            out.push(DiscoveredPlugin {
                plugin_id: format!("{name}@{marketplace}"),
                name,
                marketplace: marketplace.clone(),
                version,
                path: path.map(|p| p.to_string_lossy().into_owned()),
            });
        }
    }
    Ok(out)
}

fn newest_version_dir(plugin_dir: &Path) -> AppResult<(Option<String>, Option<PathBuf>)> {
    let mut best: Option<(String, PathBuf)> = None;
    for entry in fs::read_dir(plugin_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match &best {
            Some((current, _)) if name <= *current => {}
            _ => best = Some((name, entry.path())),
        }
    }
    Ok(match best {
        Some((version, path)) => (Some(version), Some(path)),
        None => (None, None),
    })
}

fn split_plugin_id(plugin_id: &str) -> (String, String) {
    match plugin_id.rsplit_once('@') {
        Some((name, marketplace)) => (name.to_string(), marketplace.to_string()),
        None => (plugin_id.to_string(), String::new()),
    }
}

/// Resolve Claude Code CLI path for plugin commands (native/npm/pnpm installs).
pub fn resolve_claude_executable() -> AppResult<PathBuf> {
    // Prefer the same probe used by tools/localization when available via env PATH candidates.
    let home = crate::config::get_home_dir();
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            candidates.push(local.join("Claude").join("claude.exe"));
            candidates.push(local.join("Programs").join("Claude").join("claude.exe"));
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            candidates.push(PathBuf::from(roaming).join("npm").join("claude.cmd"));
        }
        candidates.push(
            home.join("AppData")
                .join("Roaming")
                .join("npm")
                .join("claude.cmd"),
        );
        candidates.push(
            home.join("AppData")
                .join("Local")
                .join("pnpm")
                .join("claude.cmd"),
        );
    }
    candidates.push(home.join(".local").join("bin").join("claude"));
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            #[cfg(windows)]
            {
                for suffix in ["cmd", "exe", "bat"] {
                    candidates.push(dir.join(format!("claude.{suffix}")));
                }
            }
            candidates.push(dir.join("claude"));
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(AppError::Config(
        "未检测到 Claude Code，无法执行插件 CLI（请先安装 Claude Code）".into(),
    ))
}

#[allow(dead_code)]
pub fn plugins_dir_for_display() -> String {
    get_claude_plugins_dir().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plugin_id_uses_last_at() {
        let (name, market) = split_plugin_id("claude-code-zh-cn@claude-code-zh-cn");
        assert_eq!(name, "claude-code-zh-cn");
        assert_eq!(market, "claude-code-zh-cn");
    }

    #[test]
    fn normalize_github_marketplace_urls() {
        assert_eq!(
            normalize_marketplace_source("https://github.com/jarrodwatts/claude-hud"),
            "jarrodwatts/claude-hud"
        );
        assert_eq!(
            normalize_marketplace_source("https://github.com/jarrodwatts/claude-hud.git"),
            "jarrodwatts/claude-hud"
        );
        assert_eq!(
            normalize_marketplace_source("jarrodwatts/claude-hud"),
            "jarrodwatts/claude-hud"
        );
    }

    #[test]
    fn list_catalog_reads_marketplace_json() {
        let root = tempfile::tempdir().unwrap();
        let market = root.path().join("claude-hud").join(".claude-plugin");
        fs::create_dir_all(&market).unwrap();
        fs::write(
            market.join("marketplace.json"),
            r#"{
              "name": "claude-hud",
              "plugins": [
                { "name": "claude-hud", "description": "HUD", "category": "monitoring" }
              ]
            }"#,
        )
        .unwrap();
        // temp / bak dirs ignored
        fs::create_dir_all(root.path().join("temp_123")).unwrap();

        let catalog = list_plugin_catalog_at(root.path()).unwrap();
        assert_eq!(catalog.plugins.len(), 1);
        assert_eq!(catalog.plugins[0].plugin_id, "claude-hud@claude-hud");
        assert_eq!(catalog.plugins[0].description.as_deref(), Some("HUD"));
    }

    #[test]
    fn list_and_toggle_from_settings_and_installed() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let plugins_dir = root.path().join("plugins");
        let cache = plugins_dir
            .join("cache")
            .join("official")
            .join("figma")
            .join("1.2.3");
        fs::create_dir_all(&cache).unwrap();
        let installed = plugins_dir.join("installed_plugins.json");
        fs::write(
            &settings,
            r#"{ "enabledPlugins": { "figma@official": true } }"#,
        )
        .unwrap();
        fs::write(
            &installed,
            r#"{
              "version": 2,
              "plugins": {
                "figma@official": [
                  { "scope": "user", "installPath": "C:/tmp/figma", "version": "1.2.3" }
                ]
              }
            }"#,
        )
        .unwrap();

        let snap =
            list_plugins_snapshot_at(&settings, &installed, &plugins_dir.join("cache")).unwrap();
        assert!(snap.parse_ok);
        assert_eq!(snap.plugins.len(), 1);
        assert_eq!(snap.plugins[0].plugin_id, "figma@official");
        assert!(snap.plugins[0].enabled);
        assert!(snap.plugins[0].installed);
        assert_eq!(snap.plugins[0].version.as_deref(), Some("1.2.3"));

        set_plugin_enabled_at(&settings, "figma@official", false).unwrap();
        let snap =
            list_plugins_snapshot_at(&settings, &installed, &plugins_dir.join("cache")).unwrap();
        assert!(!snap.plugins[0].enabled);
    }

    #[test]
    fn list_marketplaces_from_known_json() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("known_marketplaces.json");
        fs::write(
            &path,
            r#"{
              "claude-plugins-official": {
                "source": { "source": "git", "url": "https://github.com/anthropics/claude-plugins-official.git" },
                "installLocation": "C:/Users/me/.claude/plugins/marketplaces/claude-plugins-official"
              }
            }"#,
        )
        .unwrap();
        let listed = list_marketplaces_at(&path).unwrap();
        assert!(listed.used_json);
        assert_eq!(listed.marketplaces.len(), 1);
        assert_eq!(listed.marketplaces[0].name, "claude-plugins-official");
        assert!(listed.marketplaces[0]
            .source
            .as_deref()
            .unwrap_or("")
            .contains("github.com"));
    }
}
