//! Codex Agent Plugins discovery and enable/disable via config.toml.
//!
//! Installed plugins live under `~/.codex/plugins/cache/<marketplace>/<name>/<version>/`.
//! Enable state is stored as:
//! ```toml
//! [plugins."name@marketplace"]
//! enabled = true
//! ```
//! No marketplace/store install in this first slice.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
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
        let entry = by_id.entry(item.plugin_id.clone()).or_insert_with(|| CodexPlugin {
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
        return Err(AppError::Config(format!("无效的 Codex 插件 ID: {plugin_id}")));
    }
    let path = get_codex_config_path();
    let mut doc = load_config_document()?;
    let plugins = ensure_table(&mut doc, "plugins");
    let key = plugin_id.to_string();
    let entry = plugins.entry(&key).or_insert_with(|| Item::Table(Table::new()));
    if !entry.is_table() {
        *entry = Item::Table(Table::new());
    }
    entry["enabled"] = value(enabled);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&path, doc.to_string().as_bytes())
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
        fs::write(root.path().join("config.toml"), "[plugins.\"slack@openai-curated\"]\nenabled = true\n").unwrap();

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
}
