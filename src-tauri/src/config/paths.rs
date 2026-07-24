//! Path discovery for Claude Code, Claude Desktop, and this app's own data dir.
//!
//! Conventions follow cc-switch: the home dir is resolved via [`dirs::home_dir`]
//! rather than the raw `HOME` environment variable, which can be injected by
//! Git/MSYS/Cygwin shells and point somewhere unexpected on Windows.

use std::path::PathBuf;

/// Application data folder name under the home directory.
pub const APP_DIR_NAME: &str = ".claude-switcher";
/// SQLite database file name.
pub const APP_DB_NAME: &str = "app.db";
/// Backup subdirectory name.
pub const BACKUP_DIR_NAME: &str = "backups";

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
    get_home_dir().join(APP_DIR_NAME)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_paths_are_nested_under_home() {
        let home = get_home_dir();
        assert_eq!(get_app_config_dir(), home.join(APP_DIR_NAME));
        assert_eq!(
            get_app_db_path(),
            home.join(APP_DIR_NAME).join(APP_DB_NAME)
        );
        assert_eq!(get_claude_settings_path(), home.join(".claude").join("settings.json"));
    }
}
