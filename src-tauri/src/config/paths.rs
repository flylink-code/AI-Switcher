//! Path discovery for Claude Code, Claude Desktop, and this app's own data dir.
//!
//! Conventions follow cc-switch: the home dir is resolved via [`dirs::home_dir`]
//! rather than the raw `HOME` environment variable, which can be injected by
//! Git/MSYS/Cygwin shells and point somewhere unexpected on Windows.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// Application data folder name under the home directory.
pub const APP_DIR_NAME: &str = ".claude-switcher";
/// SQLite database file name.
pub const APP_DB_NAME: &str = "app.db";
/// Backup subdirectory name.
pub const BACKUP_DIR_NAME: &str = "backups";
const DATA_ROOT_CONFIG_FILE: &str = "data-root.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRootConfig {
    pub version: u8,
    pub data_root: String,
}

/// Resolve the user home directory with a last-resort fallback.
///
/// Windows: `dirs::home_dir()` calls `SHGetKnownFolderPath(FOLDERID_Profile)`,
/// yielding the real profile path (e.g. `C:\Users\Alice`). We deliberately avoid
/// reading `HOME` directly because Git Bash / MSYS export their own `HOME`.
pub fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        log::warn!("无法获取用户主目录，回退到当前工作目录");
        PathBuf::from(".")
    })
}

/// `~/.claude` — the Claude Code config directory.
pub fn get_claude_config_dir() -> PathBuf {
    get_home_dir().join(".claude")
}

/// `~/.claude/settings.json` — where Claude Code reads env overrides.
pub fn get_claude_settings_path() -> PathBuf {
    get_claude_config_dir().join("settings.json")
}

/// `~/.claude.json` — Claude Code MCP servers + project roots.
pub fn get_claude_json_path() -> PathBuf {
    get_home_dir().join(".claude.json")
}

/// `~/.claude-switcher` — this app's own data directory.
pub fn get_app_config_dir() -> PathBuf {
    configured_data_root().unwrap_or_else(get_legacy_app_config_dir)
}

/// The immutable bootstrap location. It deliberately stays in the original
/// profile so the chosen library can be found before the application database
/// is opened on the next launch.
pub fn get_legacy_app_config_dir() -> PathBuf {
    get_home_dir().join(APP_DIR_NAME)
}

pub fn data_root_config_path() -> PathBuf {
    get_legacy_app_config_dir().join(DATA_ROOT_CONFIG_FILE)
}

pub fn configured_data_root() -> Option<PathBuf> {
    let path = data_root_config_path();
    let text = fs::read_to_string(path).ok()?;
    let config = serde_json::from_str::<DataRootConfig>(&text).ok()?;
    usable_local_absolute_path(&config.data_root)
}

/// True for Windows drive / UNC / `\\?\` paths (including mixed `/` separators).
pub fn looks_like_windows_abspath(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let stripped = strip_windows_verbatim_prefix(trimmed);
    if stripped.starts_with(r"\\") || stripped.starts_with("//") {
        return true;
    }
    let bytes = stripped.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn looks_like_unix_abspath(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('/') && !looks_like_windows_abspath(trimmed)
}

fn strip_windows_verbatim_prefix(value: &str) -> &str {
    value
        .strip_prefix(r"\\?\")
        .or_else(|| value.strip_prefix(r"\\.\"))
        .or_else(|| value.strip_prefix("//?/"))
        .or_else(|| value.strip_prefix("//./"))
        .unwrap_or(value)
}

/// Absolute path that belongs on this OS. Drive-letter / UNC strings are
/// rejected on Unix; Unix-root strings are rejected on Windows. Relative
/// paths are never usable as a library or backup directory.
pub fn usable_local_absolute_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if cfg!(unix) && looks_like_windows_abspath(trimmed) {
        return None;
    }
    if cfg!(windows) && looks_like_unix_abspath(trimmed) {
        return None;
    }
    let path = PathBuf::from(trimmed);
    path.is_absolute().then_some(path)
}

pub fn write_data_root_config(root: &Path) -> AppResult<()> {
    fs::create_dir_all(get_legacy_app_config_dir())?;
    let config = DataRootConfig {
        version: 1,
        data_root: root.to_string_lossy().into_owned(),
    };
    let body = serde_json::to_vec_pretty(&config)?;
    crate::config::atomic_write(&data_root_config_path(), &body)
}

/// `~/.claude-switcher/app.db` — main SQLite database.
pub fn get_app_db_path() -> PathBuf {
    get_app_config_dir().join(APP_DB_NAME)
}

/// `~/.claude-switcher/backups` — rotated backups.
pub fn get_backup_dir() -> PathBuf {
    get_app_config_dir().join(BACKUP_DIR_NAME)
}

/// `~/.claude/skills` — Claude Code skill directory managed by this app.
pub fn get_claude_skills_dir() -> PathBuf {
    get_claude_config_dir().join("skills")
}

/// `~/.claude/agents` — Claude Code custom subagent markdown files.
pub fn get_claude_agents_dir() -> PathBuf {
    get_claude_config_dir().join("agents")
}

/// `~/.claude/plugins` — Claude Code plugin marketplace installs and cache.
pub fn get_claude_plugins_dir() -> PathBuf {
    get_claude_config_dir().join("plugins")
}

/// `~/.claude/plugins/cache` — versioned plugin install cache.
pub fn get_claude_plugins_cache_dir() -> PathBuf {
    get_claude_plugins_dir().join("cache")
}

/// `~/.claude/plugins/installed_plugins.json` — install manifest (scope/path/version).
pub fn get_claude_installed_plugins_path() -> PathBuf {
    get_claude_plugins_dir().join("installed_plugins.json")
}

/// `~/.claude/plugins/known_marketplaces.json` — configured marketplace sources.
pub fn get_claude_known_marketplaces_path() -> PathBuf {
    get_claude_plugins_dir().join("known_marketplaces.json")
}

/// `~/.claude/plugins/marketplaces` — cloned marketplace checkouts.
pub fn get_claude_marketplaces_dir() -> PathBuf {
    get_claude_plugins_dir().join("marketplaces")
}

/// Codex's configuration root. Respect CODEX_HOME so test and portable
/// installations never accidentally modify the user's default profile.
pub fn get_codex_config_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".codex"))
}

pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

pub fn get_codex_skills_dir() -> PathBuf {
    get_codex_config_dir().join("skills")
}

pub fn get_codex_plugins_cache_dir() -> PathBuf {
    get_codex_config_dir().join("plugins").join("cache")
}

/// OpenCode 配置目录（CLI 与 Desktop 应用共享）：`~/.config/opencode`。
pub fn get_opencode_config_dir() -> PathBuf {
    get_home_dir().join(".config").join("opencode")
}

/// OpenCode 生效配置文件。优先级：`OPENCODE_CONFIG` 环境变量（指向文件）>
/// 若 `opencode.json` 与 `opencode.jsonc` 同时存在，优先含用户自有 `provider` 的那份
/// （避免空的 json 盖住有内容的 jsonc）> 已存在的 `opencode.json` > `opencode.jsonc` > 默认 `opencode.json`。
/// 写回一律走该生效路径，避免双文件分叉。
pub fn get_opencode_config_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("OPENCODE_CONFIG").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(custom);
        if path.is_absolute() {
            return path;
        }
    }
    let dir = get_opencode_config_dir();
    let json = dir.join("opencode.json");
    let jsonc = dir.join("opencode.jsonc");
    let json_exists = json.is_file();
    let jsonc_exists = jsonc.is_file();
    if json_exists && jsonc_exists {
        let json_has = opencode_file_has_user_providers(&json);
        let jsonc_has = opencode_file_has_user_providers(&jsonc);
        return match (json_has, jsonc_has) {
            (true, false) => json,
            (false, true) => jsonc,
            (true, true) => pick_newer_opencode_config(&json, &jsonc),
            (false, false) => json,
        };
    }
    if json_exists {
        return json;
    }
    if jsonc_exists {
        return jsonc;
    }
    json
}

fn pick_newer_opencode_config(json: &Path, jsonc: &Path) -> PathBuf {
    let json_mtime = fs::metadata(json).and_then(|m| m.modified()).ok();
    let jsonc_mtime = fs::metadata(jsonc).and_then(|m| m.modified()).ok();
    match (json_mtime, jsonc_mtime) {
        (Some(a), Some(b)) if b > a => jsonc.to_path_buf(),
        _ => json.to_path_buf(),
    }
}

/// 配置文件是否含至少一个可导入的用户自有 provider（有 baseURL，且非托管槽）。
fn opencode_file_has_user_providers(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = json5::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(providers) = value.get("provider").and_then(serde_json::Value::as_object) else {
        return false;
    };
    providers.iter().any(|(provider_id, entry)| {
        !is_managed_opencode_provider_id(provider_id)
            && opencode_provider_entry_has_base_url(entry)
    })
}

fn is_managed_opencode_provider_id(provider_id: &str) -> bool {
    provider_id == "ai-switcher" || provider_id.starts_with("aisw-")
}

fn opencode_provider_entry_has_base_url(entry: &serde_json::Value) -> bool {
    entry
        .pointer("/options/baseURL")
        .or_else(|| entry.pointer("/options/baseUrl"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

/// 旧版全局配置 `~/.config/opencode/config.json`（扫描导入时与 opencode.json 合并）。
pub fn get_opencode_legacy_config_path() -> PathBuf {
    get_opencode_config_dir().join("config.json")
}

/// OpenCode `/connect` 写入的密钥文件：`~/.local/share/opencode/auth.json`。
pub fn get_opencode_auth_path() -> PathBuf {
    get_opencode_data_dir().join("auth.json")
}

/// OpenCode 数据目录（会话/用量存储）。OpenCode 遵循 XDG basedir，
/// 所有平台默认落在 `~/.local/share/opencode`。
pub fn get_opencode_data_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("opencode");
    }
    get_home_dir().join(".local").join("share").join("opencode")
}

/// OpenCode SQLite 数据库路径：`OPENCODE_DB` 环境变量 > 数据目录下 `opencode.db`。
pub fn get_opencode_db_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("OPENCODE_DB").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(custom);
        if path.is_absolute() {
            return path;
        }
        return get_opencode_data_dir().join(path);
    }
    get_opencode_data_dir().join("opencode.db")
}

/// DeepSeek Harness 配置目录：`$DSH_HOME` > `~/.dsh`。
pub fn get_dsh_config_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("DSH_HOME").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(custom);
        if path.is_absolute() {
            return path;
        }
    }
    get_home_dir().join(".dsh")
}

/// DeepSeek Harness 基础配置文件 `settings.yaml`。
pub fn get_dsh_settings_path() -> PathBuf {
    get_dsh_config_dir().join("settings.yaml")
}

/// DeepSeek Harness 密钥文件 `.credentials.yaml`。
pub fn get_dsh_credentials_path() -> PathBuf {
    get_dsh_config_dir().join(".credentials.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_paths_are_nested_under_home() {
        let home = get_home_dir();
        assert_eq!(get_legacy_app_config_dir(), home.join(APP_DIR_NAME));
        assert_eq!(
            get_app_db_path(),
            home.join(APP_DIR_NAME).join(APP_DB_NAME)
        );
        assert_eq!(get_claude_settings_path(), home.join(".claude").join("settings.json"));
    }

    #[test]
    fn opencode_config_path_prefers_jsonc_when_json_has_no_providers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let json = temp.path().join("opencode.json");
        let jsonc = temp.path().join("opencode.jsonc");
        fs::write(
            &json,
            r#"{"$schema":"https://opencode.ai/config.json","plugin":["./p.js"]}"#,
        )
        .expect("write json");
        fs::write(
            &jsonc,
            r#"{
  "provider": {
    "acme": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://acme.test/v1" }
    }
  }
}"#,
        )
        .expect("write jsonc");
        assert!(opencode_file_has_user_providers(&jsonc));
        assert!(!opencode_file_has_user_providers(&json));
    }

    #[test]
    fn windows_drive_and_unc_paths_are_detected() {
        assert!(looks_like_windows_abspath(r"J:\Temp\aiswitcher"));
        assert!(looks_like_windows_abspath(r"J:/Temp/aiswitcher"));
        assert!(looks_like_windows_abspath(r"\\?\J:\Temp\aiswitcher"));
        assert!(looks_like_windows_abspath(r"//?/J:\Temp\aiswitcher"));
        assert!(looks_like_windows_abspath(r"\\server\share\lib"));
        assert!(!looks_like_windows_abspath("/home/user/.claude-switcher"));
        assert!(!looks_like_windows_abspath("relative/path"));
        assert!(!looks_like_windows_abspath(""));
    }

    #[test]
    fn usable_local_path_rejects_foreign_os_and_relative() {
        assert!(usable_local_absolute_path("relative").is_none());
        assert!(usable_local_absolute_path("").is_none());
        #[cfg(unix)]
        {
            assert!(usable_local_absolute_path(r"J:\Temp\aiswitcher").is_none());
            assert!(usable_local_absolute_path(r"J:\Temp\aiswitcher\session-backups").is_none());
            assert!(usable_local_absolute_path(r"\\?\J:\Temp\aiswitcher").is_none());
            assert!(usable_local_absolute_path("/tmp/backups").is_some());
        }
        #[cfg(windows)]
        {
            assert!(usable_local_absolute_path(r"C:\Users\admin\.claude-switcher").is_some());
            assert!(usable_local_absolute_path("/home/user/.claude-switcher").is_none());
        }
    }
}
