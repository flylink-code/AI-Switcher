//! Codex Agent Plugins discovery, marketplace catalog, and install/remove.
//!
//! Installed plugins live under `~/.codex/plugins/cache/<marketplace>/<name>/<version>/`.
//! Enable state is stored as:
//! ```toml
//! [plugins."name@marketplace"]
//! enabled = true
//! ```
//! Install uses `codex plugin add name@marketplace`. Catalog is scanned from
//! each marketplace snapshot's `marketplace.json` (`.agents/plugins/…` or legacy paths).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use toml_edit::{value, DocumentMut, Item, Table};

use crate::config::{atomic_write, get_codex_config_path, get_codex_plugins_cache_dir};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPlugin {
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
pub struct CodexPluginsSnapshot {
    pub plugins: Vec<CodexPlugin>,
    pub config_path: String,
    pub cache_path: String,
    pub config_plugin_count: usize,
    pub cache_plugin_count: usize,
    pub parse_ok: bool,
    pub parse_error: Option<String>,
}

pub fn list_plugins() -> AppResult<Vec<CodexPlugin>> {
    Ok(list_plugins_snapshot()?.plugins)
}

pub fn list_plugins_snapshot() -> AppResult<CodexPluginsSnapshot> {
    let config_path = get_codex_config_path();
    let cache_path = get_codex_plugins_cache_dir();
    let (doc, parse_ok, parse_error) = load_config_document_tolerant(&config_path);
    let config_entries = read_enabled_map(&doc);
    let config_plugin_count = config_entries.len();

    let mut by_id: BTreeMap<String, CodexPlugin> = BTreeMap::new();
    for (plugin_id, enabled) in config_entries {
        let (name, marketplace) = split_plugin_id(&plugin_id);
        by_id.insert(
            plugin_id.clone(),
            CodexPlugin {
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

    let cached = scan_cache(&cache_path)?;
    let cache_plugin_count = cached.len();
    for item in cached {
        let entry = by_id
            .entry(item.plugin_id.clone())
            .or_insert_with(|| CodexPlugin {
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

    let mut plugins: Vec<CodexPlugin> = by_id.into_values().collect();
    plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    Ok(CodexPluginsSnapshot {
        plugins,
        config_path: config_path.to_string_lossy().into_owned(),
        cache_path: cache_path.to_string_lossy().into_owned(),
        config_plugin_count,
        cache_plugin_count,
        parse_ok,
        parse_error,
    })
}

pub fn set_plugin_enabled(plugin_id: &str, enabled: bool) -> AppResult<()> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Codex 插件 ID: {plugin_id}"
        )));
    }
    let path = get_codex_config_path();
    let mut doc = load_config_document()?;
    let plugins = ensure_table(&mut doc, "plugins");
    let key = plugin_id.to_string();
    let entry = plugins
        .entry(&key)
        .or_insert_with(|| Item::Table(Table::new()));
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    entry["enabled"] = value(enabled);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&path, doc.to_string().as_bytes())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMarketplace {
    pub name: String,
    pub root: Option<String>,
    pub source: Option<String>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMarketplaceListResult {
    pub marketplaces: Vec<CodexMarketplace>,
    pub raw_output: String,
    pub used_json: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPluginCommandResult {
    pub ok: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

/// List configured plugin marketplaces via `codex plugin marketplace list`.
pub fn list_marketplaces() -> AppResult<CodexMarketplaceListResult> {
    let json_attempt = run_codex_plugin_args(&["plugin", "marketplace", "list", "--json"]);
    match json_attempt {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let marketplaces = parse_marketplace_json(&stdout);
            if !marketplaces.is_empty()
                || stdout.trim().starts_with('[')
                || stdout.trim().starts_with('{')
            {
                return Ok(CodexMarketplaceListResult {
                    marketplaces,
                    raw_output: stdout,
                    used_json: true,
                });
            }
        }
        _ => {}
    }

    let output = run_codex_plugin_args(&["plugin", "marketplace", "list"])?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(AppError::Other(format!(
            "codex plugin marketplace list 失败: {}",
            first_nonempty(&stderr, &stdout)
        )));
    }
    Ok(CodexMarketplaceListResult {
        marketplaces: parse_marketplace_text(&stdout),
        raw_output: stdout,
        used_json: false,
    })
}

pub fn add_marketplace(source: &str) -> AppResult<CodexPluginCommandResult> {
    let source = source.trim();
    if source.is_empty() {
        return Err(AppError::Config("marketplace 源不能为空".into()));
    }
    let output = run_codex_plugin_args(&["plugin", "marketplace", "add", source])?;
    command_result(output, "已添加 marketplace")
}

pub fn remove_marketplace(name: &str) -> AppResult<CodexPluginCommandResult> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("marketplace 名称不能为空".into()));
    }
    let output = run_codex_plugin_args(&["plugin", "marketplace", "remove", name])?;
    command_result(output, "已移除 marketplace")
}

/// Uninstall an installed plugin via `codex plugin remove`, then refresh local state.
pub fn uninstall_plugin(plugin_id: &str) -> AppResult<CodexPluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Codex 插件 ID: {plugin_id}"
        )));
    }
    let output = run_codex_plugin_args(&["plugin", "remove", plugin_id])?;
    let result = command_result(output, "已卸载插件")?;
    // Best-effort: drop enable entry if CLI left it behind.
    if let Ok(mut doc) = load_config_document() {
        if let Some(plugins) = doc.get_mut("plugins").and_then(Item::as_table_mut) {
            plugins.remove(plugin_id);
            let _ = atomic_write(&get_codex_config_path(), doc.to_string().as_bytes());
        }
    }
    Ok(result)
}

/// Install a plugin via `codex plugin add <name@marketplace>`.
pub fn install_plugin(plugin_id: &str) -> AppResult<CodexPluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Codex 插件 ID: {plugin_id}（格式应为 name@marketplace）"
        )));
    }
    let output = run_codex_plugin_args(&["plugin", "add", plugin_id])?;
    let result = if output.status.success() {
        command_result(output, "已安装插件")?
    } else {
        // Older CLIs accepted `plugin install`.
        let fallback = run_codex_plugin_args(&["plugin", "install", plugin_id])?;
        command_result(fallback, "已安装插件")?
    };
    let _ = set_plugin_enabled(plugin_id, true);
    Ok(result)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCatalogPlugin {
    pub plugin_id: String,
    pub name: String,
    pub marketplace: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPluginCatalog {
    pub plugins: Vec<CodexCatalogPlugin>,
    pub marketplaces_dir: String,
}

/// List installable plugins declared by configured marketplace snapshots.
pub fn list_plugin_catalog() -> AppResult<CodexPluginCatalog> {
    let listed = list_marketplaces().unwrap_or(CodexMarketplaceListResult {
        marketplaces: Vec::new(),
        raw_output: String::new(),
        used_json: false,
    });
    let mut catalog = list_plugin_catalog_from_marketplaces(&listed.marketplaces)?;
    if catalog.plugins.is_empty() {
        // CLI list may omit usable roots in some environments; fall back to known local trees.
        let discovered = discover_local_marketplace_roots();
        if !discovered.is_empty() {
            catalog = list_plugin_catalog_from_marketplaces(&discovered)?;
        }
    }
    Ok(catalog)
}

pub fn list_plugin_catalog_from_marketplaces(
    marketplaces: &[CodexMarketplace],
) -> AppResult<CodexPluginCatalog> {
    let mut plugins = Vec::new();
    let mut roots_seen = Vec::new();
    for market in marketplaces {
        let root = resolve_marketplace_root(market);
        let Some(root) = root else { continue };
        let root_display = root.to_string_lossy().into_owned();
        if roots_seen.iter().any(|seen: &String| seen == &root_display) {
            continue;
        }
        roots_seen.push(root_display);
        let Some(manifest) = find_marketplace_manifest(&root) else {
            continue;
        };
        append_catalog_from_manifest(&root, &manifest, &market.name, &mut plugins);
    }
    plugins.sort_by(|a, b| {
        a.marketplace
            .cmp(&b.marketplace)
            .then_with(|| a.name.cmp(&b.name))
    });
    let marketplaces_dir = roots_seen.first().cloned().unwrap_or_else(|| {
        get_codex_config_path()
            .parent()
            .map(|p| p.join("plugins").to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    Ok(CodexPluginCatalog {
        plugins,
        marketplaces_dir,
    })
}

fn resolve_marketplace_root(market: &CodexMarketplace) -> Option<PathBuf> {
    if let Some(root) = market
        .root
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = normalize_fs_path(root);
        if path.is_dir() {
            return Some(path);
        }
    }
    let name = market.name.trim();
    if name.is_empty() {
        return None;
    }
    let codex_home = get_codex_config_path().parent()?.to_path_buf();
    let candidates = [
        codex_home.join("plugins").join(name),
        codex_home.join("plugins").join("marketplaces").join(name),
        codex_home.join(".tmp").join("marketplaces").join(name),
        codex_home.join(".tmp").join("plugins"),
        codex_home.join(".agents").join("plugins").join(name),
    ];
    candidates.into_iter().find(|path| path.is_dir())
}

fn normalize_fs_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    let without_verbatim = trimmed
        .strip_prefix(r"\\?\")
        .or_else(|| trimmed.strip_prefix("//?/"))
        .unwrap_or(trimmed);
    PathBuf::from(without_verbatim)
}

fn discover_local_marketplace_roots() -> Vec<CodexMarketplace> {
    let Some(codex_home) = get_codex_config_path().parent().map(Path::to_path_buf) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let user_home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| codex_home.clone());
    let candidates = [
        ("openai-curated", codex_home.join(".tmp").join("plugins")),
        (
            "openai-primary-runtime",
            user_home
                .join(".cache")
                .join("codex-runtimes")
                .join("codex-primary-runtime")
                .join("plugins")
                .join("openai-primary-runtime"),
        ),
    ];
    for (name, root) in candidates {
        if root.is_dir() && find_marketplace_manifest(&root).is_some() {
            out.push(CodexMarketplace {
                name: name.to_string(),
                root: Some(root.to_string_lossy().into_owned()),
                source: None,
                raw: None,
            });
        }
    }
    let markets_dir = codex_home.join(".tmp").join("marketplaces");
    if let Ok(entries) = fs::read_dir(&markets_dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let root = entry.path();
            if find_marketplace_manifest(&root).is_some() {
                out.push(CodexMarketplace {
                    name,
                    root: Some(root.to_string_lossy().into_owned()),
                    source: None,
                    raw: None,
                });
            }
        }
    }
    out
}

fn find_marketplace_manifest(root: &Path) -> Option<PathBuf> {
    let candidates = [
        root.join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        root.join(".claude-plugin").join("marketplace.json"),
        root.join("marketplace.json"),
        root.join("plugins").join("marketplace.json"),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn append_catalog_from_manifest(
    market_root: &Path,
    manifest: &Path,
    fallback_marketplace: &str,
    plugins: &mut Vec<CodexCatalogPlugin>,
) {
    let Ok(raw) = fs::read_to_string(manifest) else {
        return;
    };
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let marketplace_name = root
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_marketplace)
        .to_string();
    let Some(items) = root.get("plugins").and_then(Value::as_array) else {
        return;
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
        let description = obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                obj.get("interface")
                    .and_then(Value::as_object)
                    .and_then(|iface| {
                        iface
                            .get("displayName")
                            .or_else(|| iface.get("description"))
                    })
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        let source_path = obj.get("source").and_then(|source| {
            source.as_str().map(str::to_string).or_else(|| {
                source
                    .as_object()
                    .and_then(|o| o.get("path"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        });
        plugins.push(CodexCatalogPlugin {
            plugin_id: format!("{name}@{marketplace_name}"),
            name: name.to_string(),
            marketplace: marketplace_name.clone(),
            description,
            category: obj
                .get("category")
                .and_then(Value::as_str)
                .map(str::to_string),
            version: obj
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    resolve_codex_plugin_manifest_version(market_root, name, source_path.as_deref())
                }),
        });
    }
}

fn resolve_codex_plugin_manifest_version(
    market_root: &Path,
    plugin_name: &str,
    source_path: Option<&str>,
) -> Option<String> {
    let mut dirs = Vec::new();
    if let Some(rel) = source_path.map(str::trim).filter(|s| !s.is_empty()) {
        let cleaned = rel.trim_start_matches("./");
        dirs.push(market_root.join(cleaned));
    }
    dirs.push(market_root.join("plugins").join(plugin_name));
    dirs.push(market_root.join(plugin_name));
    for dir in dirs {
        let candidates = [
            dir.join(".codex-plugin").join("plugin.json"),
            dir.join(".claude-plugin").join("plugin.json"),
            dir.join("plugin.json"),
        ];
        for path in candidates {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&raw) {
                    if let Some(version) = obj.get("version").and_then(Value::as_str) {
                        let trimmed = version.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPluginUpdateStatus {
    pub plugin_id: String,
    pub status: String,
    pub message: String,
    pub local_version: Option<String>,
    pub remote_version: Option<String>,
}

/// Refresh marketplace snapshot(s): `codex plugin marketplace upgrade [name]`.
pub fn upgrade_marketplace(name: Option<&str>) -> AppResult<CodexPluginCommandResult> {
    let mut args = vec!["plugin", "marketplace", "upgrade"];
    let owned;
    if let Some(name) = name.map(str::trim).filter(|s| !s.is_empty()) {
        owned = name.to_string();
        args.push(owned.as_str());
    }
    let output = run_codex_plugin_args(&args)?;
    command_result(output, "已刷新 marketplace")
}

/// Reinstall/update a plugin from the latest marketplace snapshot via `codex plugin add`.
pub fn update_plugin(plugin_id: &str) -> AppResult<CodexPluginCommandResult> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Codex 插件 ID: {plugin_id}"
        )));
    }
    let (_, marketplace) = split_plugin_id(plugin_id);
    if !marketplace.is_empty() {
        let _ = upgrade_marketplace(Some(&marketplace));
    }
    install_plugin(plugin_id)
}

pub fn check_plugin_update(plugin_id: &str) -> AppResult<CodexPluginUpdateStatus> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !plugin_id.contains('@') {
        return Err(AppError::Config(format!(
            "无效的 Codex 插件 ID: {plugin_id}"
        )));
    }
    let (_, marketplace) = split_plugin_id(plugin_id);
    if !marketplace.is_empty() {
        let _ = upgrade_marketplace(Some(&marketplace));
    }
    Ok(evaluate_codex_update_status(plugin_id))
}

pub fn check_plugin_updates() -> AppResult<Vec<CodexPluginUpdateStatus>> {
    log::info!("检查 Codex 插件更新: 刷新 marketplace");
    if let Err(error) = upgrade_marketplace(None) {
        log::warn!("刷新 Codex marketplace 失败（仍用本地目录比较版本）: {error}");
    }
    let snap = list_plugins_snapshot()?;
    let mut out = Vec::new();
    for plugin in snap.plugins.into_iter().filter(|p| p.installed) {
        out.push(evaluate_codex_update_status(&plugin.plugin_id));
    }
    log::info!("检查 Codex 插件更新完成: {} 个已安装插件", out.len());
    Ok(out)
}

fn evaluate_codex_update_status(plugin_id: &str) -> CodexPluginUpdateStatus {
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
        (Some(local), Some(remote)) if versions_equal(local, remote) => CodexPluginUpdateStatus {
            plugin_id: plugin_id.to_string(),
            status: "up_to_date".into(),
            message: format!("已是最新（{local}）"),
            local_version,
            remote_version,
        },
        (Some(local), Some(remote)) => CodexPluginUpdateStatus {
            plugin_id: plugin_id.to_string(),
            status: "update_available".into(),
            message: format!("可更新：{local} → {remote}"),
            local_version,
            remote_version,
        },
        (local_v, remote_v) => CodexPluginUpdateStatus {
            plugin_id: plugin_id.to_string(),
            status: if local.as_ref().is_some_and(|p| p.installed) {
                "unknown".into()
            } else {
                "not_installed".into()
            },
            message: if local.as_ref().is_some_and(|p| p.installed) {
                "无法比较版本（市场或安装记录缺少 version）；仍可尝试更新".to_string()
            } else {
                "插件未安装或不在本地清单中".to_string()
            },
            local_version: local_v.map(str::to_string),
            remote_version: remote_v.map(str::to_string),
        },
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

fn command_result(
    output: std::process::Output,
    success_message: &str,
) -> AppResult<CodexPluginCommandResult> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(CodexPluginCommandResult {
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

fn run_codex_plugin_args(args: &[&str]) -> AppResult<std::process::Output> {
    // GUI sessions often omit npm global dirs from PATH; resolve like doctor/tools.
    crate::commands::tools::run_codex_cli_timeout(args, crate::process_util::CLI_COMMAND_TIMEOUT)
}

fn parse_marketplace_json(stdout: &str) -> Vec<CodexMarketplace> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let items = match value {
        Value::Array(items) => items,
        Value::Object(map) => map
            .get("marketplaces")
            .or_else(|| map.get("items"))
            .cloned()
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            if let Some(name) = item.as_str() {
                return Some(CodexMarketplace {
                    name: name.to_string(),
                    root: None,
                    source: None,
                    raw: Some(name.to_string()),
                });
            }
            let obj = item.as_object()?;
            let name = obj
                .get("name")
                .or_else(|| obj.get("marketplace"))
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)?
                .to_string();
            Some(CodexMarketplace {
                name,
                root: obj
                    .get("root")
                    .or_else(|| obj.get("path"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source: obj
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        obj.get("marketplaceSource")
                            .and_then(Value::as_object)
                            .and_then(|source| source.get("source"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .or_else(|| obj.get("url").and_then(Value::as_str).map(str::to_string)),
                raw: Some(item.to_string()),
            })
        })
        .collect()
}

fn parse_marketplace_text(stdout: &str) -> Vec<CodexMarketplace> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Common formats: "name  /path" or "- name (path)"
        let cleaned = line.trim_start_matches(['-', '*', '•']).trim();
        if cleaned.is_empty() {
            continue;
        }
        let (name, rest) = cleaned
            .split_once(|ch: char| ch.is_whitespace())
            .map(|(name, rest)| (name.trim(), rest.trim()))
            .unwrap_or((cleaned, ""));
        if name.eq_ignore_ascii_case("name") || name.eq_ignore_ascii_case("marketplace") {
            continue;
        }
        out.push(CodexMarketplace {
            name: name.trim_matches(|ch| ch == '"' || ch == '\'').to_string(),
            root: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
            source: None,
            raw: Some(line.to_string()),
        });
    }
    out
}

fn load_config_document() -> AppResult<DocumentMut> {
    let path = get_codex_config_path();
    let (doc, parse_ok, parse_error) = load_config_document_tolerant(&path);
    if parse_ok {
        Ok(doc)
    } else {
        Err(AppError::Config(
            parse_error.unwrap_or_else(|| "Codex config.toml 格式无效".into()),
        ))
    }
}

fn load_config_document_tolerant(path: &Path) -> (DocumentMut, bool, Option<String>) {
    if !path.exists() {
        return (DocumentMut::new(), true, None);
    }
    match fs::read_to_string(path) {
        Ok(raw) => match raw.parse::<DocumentMut>() {
            Ok(doc) => (doc, true, None),
            Err(error) => (
                DocumentMut::new(),
                false,
                Some(format!("Codex config.toml 格式无效：{error}")),
            ),
        },
        Err(error) => (
            DocumentMut::new(),
            false,
            Some(format!("无法读取 Codex config.toml：{error}")),
        ),
    }
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Table {
    let needs_table = doc.get(key).map(|item| !item.is_table()).unwrap_or(true);
    if needs_table {
        doc[key] = Item::Table(Table::new());
    }
    doc[key].as_table_mut().expect("table was just ensured")
}

fn read_enabled_map(doc: &DocumentMut) -> Vec<(String, bool)> {
    let Some(plugins) = doc.get("plugins").and_then(Item::as_table) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, item) in plugins.iter() {
        let enabled = item
            .as_table()
            .and_then(|table| table.get("enabled"))
            .and_then(Item::as_bool)
            .unwrap_or(false);
        out.push((key.to_string(), enabled));
    }
    out
}

fn split_plugin_id(plugin_id: &str) -> (String, String) {
    match plugin_id.rsplit_once('@') {
        Some((name, marketplace)) => (name.to_string(), marketplace.to_string()),
        None => (plugin_id.to_string(), String::new()),
    }
}

#[derive(Debug)]
struct CachedPlugin {
    plugin_id: String,
    name: String,
    marketplace: String,
    version: Option<String>,
    path: Option<String>,
}

fn scan_cache(cache_root: &Path) -> AppResult<Vec<CachedPlugin>> {
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
            out.push(CachedPlugin {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn split_plugin_id_uses_last_at() {
        let (name, market) = split_plugin_id("google-calendar@openai-curated");
        assert_eq!(name, "google-calendar");
        assert_eq!(market, "openai-curated");
    }

    #[test]
    fn list_and_toggle_plugins_from_config_and_cache() {
        let _guard = env_lock().lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", root.path());

        let cache = root
            .path()
            .join("plugins")
            .join("cache")
            .join("openai-curated")
            .join("slack")
            .join("1.0.0");
        fs::create_dir_all(&cache).unwrap();
        fs::write(
            root.path().join("config.toml"),
            "[plugins.\"slack@openai-curated\"]\nenabled = true\n",
        )
        .unwrap();

        let listed = list_plugins().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].plugin_id, "slack@openai-curated");
        assert!(listed[0].enabled);
        assert!(listed[0].installed);
        assert_eq!(listed[0].version.as_deref(), Some("1.0.0"));

        set_plugin_enabled("slack@openai-curated", false).unwrap();
        let listed = list_plugins().unwrap();
        assert!(!listed[0].enabled);

        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn list_snapshot_reads_quoted_plugin_table_keys() {
        let _guard = env_lock().lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", root.path());

        let cache = root
            .path()
            .join("plugins")
            .join("cache")
            .join("openai-curated")
            .join("google-calendar")
            .join("0.2.1");
        fs::create_dir_all(&cache).unwrap();
        fs::write(
            root.path().join("config.toml"),
            r#"
[plugins."google-calendar@openai-curated"]
enabled = true

[plugins."slack@openai-curated"]
enabled = false
"#,
        )
        .unwrap();

        let snap = list_plugins_snapshot().unwrap();
        assert!(snap.parse_ok);
        assert_eq!(snap.config_plugin_count, 2);
        assert_eq!(snap.cache_plugin_count, 1);
        assert_eq!(snap.plugins.len(), 2);
        let calendar = snap
            .plugins
            .iter()
            .find(|p| p.plugin_id == "google-calendar@openai-curated")
            .expect("calendar plugin");
        assert!(calendar.enabled);
        assert!(calendar.installed);

        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn catalog_reads_agents_marketplace_json() {
        let _guard = env_lock().lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let market_root = root.path().join("openai-curated");
        let manifest_dir = market_root.join(".agents").join("plugins");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("marketplace.json"),
            r#"{
              "name": "openai-curated",
              "plugins": [
                {
                  "name": "slack",
                  "description": "Slack helpers",
                  "category": "Productivity",
                  "source": { "source": "local", "path": "./plugins/slack" }
                },
                {
                  "name": "google-calendar",
                  "category": "Productivity",
                  "source": "./plugins/google-calendar"
                }
              ]
            }"#,
        )
        .unwrap();

        let catalog = list_plugin_catalog_from_marketplaces(&[CodexMarketplace {
            name: "openai-curated".into(),
            root: Some(market_root.to_string_lossy().into_owned()),
            source: None,
            raw: None,
        }])
        .unwrap();

        assert_eq!(catalog.plugins.len(), 2);
        assert_eq!(
            catalog.plugins[0].plugin_id,
            "google-calendar@openai-curated"
        );
        assert_eq!(catalog.plugins[1].plugin_id, "slack@openai-curated");
        assert_eq!(
            catalog.plugins[1].description.as_deref(),
            Some("Slack helpers")
        );
    }

    #[test]
    fn catalog_reads_claude_plugin_marketplace_layout() {
        let root = tempfile::tempdir().unwrap();
        let market_root = root.path().join("caveman");
        let manifest_dir = market_root.join(".claude-plugin");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("marketplace.json"),
            r#"{
              "name": "caveman",
              "plugins": [{ "name": "caveman", "description": "Caveman mode" }]
            }"#,
        )
        .unwrap();

        let catalog = list_plugin_catalog_from_marketplaces(&[CodexMarketplace {
            name: "caveman".into(),
            root: Some(market_root.to_string_lossy().into_owned()),
            source: None,
            raw: None,
        }])
        .unwrap();

        assert_eq!(catalog.plugins.len(), 1);
        assert_eq!(catalog.plugins[0].plugin_id, "caveman@caveman");
    }

    #[test]
    fn normalize_fs_path_strips_windows_verbatim_prefix() {
        let path = normalize_fs_path(r"\\?\C:\Users\admin\.codex\.tmp\plugins");
        assert_eq!(path, PathBuf::from(r"C:\Users\admin\.codex\.tmp\plugins"));
    }
}
