//! Prompt preset management for Claude Code and Codex global instructions.
//!
//! Presets are plain Markdown files stored in `~/.claude-switcher/prompts/`.
//! "Activating" a preset copies its content over the live Claude Code prompt
//! file (`~/.claude/CLAUDE.md`) — with a timestamped backup first, per task.md
//! §2.6 ("一键激活写入 live 文件，回填保护"). The current live file can also be
//! imported back into the preset library.
//!
//! Storage is file-based (not SQLite) so presets stay greppable / copyable.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::backup::backup_file_named;
use crate::config::{atomic_write, get_app_config_dir, get_claude_config_dir, get_codex_config_dir};
use crate::error::{AppError, AppResult};

/// Subdirectory of the app data dir holding prompt presets.
const PROMPTS_DIR_NAME: &str = "prompts";
/// Max backups of the live `CLAUDE.md` to retain.
const LIVE_BACKUP_KEEP: usize = 10;
/// Live file name inside `~/.claude`.
const LIVE_FILE_NAME: &str = "CLAUDE.md";
const CODEX_LIVE_FILE_NAME: &str = "AGENTS.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptTarget {
    ClaudeCode,
    Codex,
}

impl Default for PromptTarget {
    fn default() -> Self { Self::ClaudeCode }
}

/// One preset in the library (list view; no content).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInfo {
    pub name: String,
    /// File mtime as unix-epoch milliseconds (0 when unknown).
    pub updated_at: i64,
}

/// A preset with its content (editor view).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptDetail {
    pub name: String,
    pub content: String,
    pub updated_at: i64,
}

/// The live prompt file, when present.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePrompt {
    pub path: String,
    pub content: String,
    pub updated_at: i64,
}

// ---- paths ------------------------------------------------------------------

pub fn prompts_dir(target: PromptTarget) -> PathBuf {
    let base = get_app_config_dir().join(PROMPTS_DIR_NAME);
    match target {
        PromptTarget::ClaudeCode => base,
        PromptTarget::Codex => base.join("codex"),
    }
}

pub fn live_prompt_path(target: PromptTarget) -> PathBuf {
    match target {
        PromptTarget::ClaudeCode => get_claude_config_dir().join(LIVE_FILE_NAME),
        PromptTarget::Codex => get_codex_config_dir().join(CODEX_LIVE_FILE_NAME),
    }
}

fn preset_path(target: PromptTarget, name: &str) -> AppResult<PathBuf> {
    validate_name(name)?;
    Ok(prompts_dir(target).join(format!("{name}.md")))
}

/// Reject names that would escape the prompts dir or produce odd files.
fn validate_name(name: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("Prompt 名称不能为空".to_string()));
    }
    if name.len() > 80 {
        return Err(AppError::Config("Prompt 名称过长（≤80 字符）".to_string()));
    }
    if name
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control())
    {
        return Err(AppError::Config(format!("Prompt 名称含非法字符: {name}")));
    }
    Ok(())
}

// ---- CRUD -------------------------------------------------------------------

/// List all presets sorted by name.
pub fn list_prompts(target: PromptTarget) -> AppResult<Vec<PromptInfo>> {
    let dir = prompts_dir(target);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push(PromptInfo {
            name: stem.to_string(),
            updated_at: mtime_millis(&path),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Read one preset's content.
pub fn read_prompt(target: PromptTarget, name: &str) -> AppResult<PromptDetail> {
    let path = preset_path(target, name)?;
    if !path.exists() {
        return Err(AppError::Config(format!("Prompt 不存在: {name}")));
    }
    let content = fs::read_to_string(&path)?;
    Ok(PromptDetail {
        name: name.to_string(),
        content,
        updated_at: mtime_millis(&path),
    })
}

/// Create or overwrite a preset.
pub fn save_prompt(target: PromptTarget, name: &str, content: &str) -> AppResult<()> {
    let path = preset_path(target, name)?;
    fs::create_dir_all(prompts_dir(target))?;
    atomic_write(&path, content.as_bytes())
}

/// Delete a preset. Missing file is not an error (idempotent for the UI).
pub fn delete_prompt(target: PromptTarget, name: &str) -> AppResult<()> {
    let path = preset_path(target, name)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

// ---- live file --------------------------------------------------------------

/// Activate a preset: back up the live `CLAUDE.md` (if any), then overwrite it
/// with the preset content.
pub fn activate_prompt(target: PromptTarget, name: &str) -> AppResult<()> {
    let detail = read_prompt(target, name)?;
    let live = live_prompt_path(target);
    if live.exists() {
        let backup_name = match target {
            PromptTarget::ClaudeCode => LIVE_FILE_NAME,
            PromptTarget::Codex => CODEX_LIVE_FILE_NAME,
        };
        backup_file_named(&live, backup_name, LIVE_BACKUP_KEEP)?;
    }
    if let Some(parent) = live.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&live, detail.content.as_bytes())
}

/// Read the current live `CLAUDE.md`, or `None` when absent.
pub fn read_live_prompt(target: PromptTarget) -> AppResult<Option<LivePrompt>> {
    let live = live_prompt_path(target);
    if !live.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&live)?;
    Ok(Some(LivePrompt {
        path: live.to_string_lossy().into_owned(),
        content,
        updated_at: mtime_millis(&live),
    }))
}

/// Copy the current live `CLAUDE.md` into the preset library under `name`.
pub fn import_live_prompt(target: PromptTarget, name: &str) -> AppResult<()> {
    let Some(live) = read_live_prompt(target)? else {
        return Err(AppError::Config(
            "未检测到 live CLAUDE.md，无法导入".to_string(),
        ));
    };
    save_prompt(target, name, &live.content)
}

fn mtime_millis(path: &std::path::Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation() {
        assert!(validate_name("default").is_ok());
        assert!(validate_name("中文预设").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..\\evil").is_err());
        assert!(validate_name("x".repeat(81).as_str()).is_err());
    }
}
