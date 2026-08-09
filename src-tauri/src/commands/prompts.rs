//! Prompt (CLAUDE.md) preset commands.

use tauri::State;

use crate::database::dao::{self, PromptRenameScope};
use crate::error::AppResult;
use crate::prompts::{self, LivePrompt, PromptDetail, PromptInfo, PromptTarget};
use crate::store::AppState;

/// List all stored presets (names + mtimes, no content).
#[tauri::command]
pub fn list_prompts(target: Option<PromptTarget>) -> AppResult<Vec<PromptInfo>> {
    prompts::list_prompts(target.unwrap_or_default())
}

/// Read one preset's content for the editor.
#[tauri::command]
pub fn read_prompt(name: String, target: Option<PromptTarget>) -> AppResult<PromptDetail> {
    prompts::read_prompt(target.unwrap_or_default(), &name)
}

/// Create or overwrite a preset.
#[tauri::command]
pub fn save_prompt(name: String, content: String, target: Option<PromptTarget>) -> AppResult<()> {
    prompts::save_prompt(target.unwrap_or_default(), &name, &content)
}

/// Rename a preset and cascade profile `prompt_id` references.
#[tauri::command]
pub fn rename_prompt(
    old_name: String,
    new_name: String,
    target: Option<PromptTarget>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let target = target.unwrap_or_default();
    prompts::rename_prompt(target, &old_name, &new_name)?;
    // Profiles 只引用 claude/codex 作用域的 prompt；opencode 预设无需级联。
    let scope = match target {
        PromptTarget::ClaudeCode => Some(PromptRenameScope::ClaudeCode),
        PromptTarget::Codex => Some(PromptRenameScope::Codex),
        PromptTarget::OpenCode => None,
    };
    if let Some(scope) = scope {
        state.db.with_conn(|conn| {
            dao::rewrite_prompt_id(conn, scope, old_name.trim(), new_name.trim())?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Delete a preset (idempotent).
#[tauri::command]
pub fn delete_prompt(name: String, target: Option<PromptTarget>) -> AppResult<()> {
    prompts::delete_prompt(target.unwrap_or_default(), &name)
}

/// Activate a preset: overwrite the live `~/.claude/CLAUDE.md` (backup first).
#[tauri::command]
pub fn activate_prompt(name: String, target: Option<PromptTarget>) -> AppResult<()> {
    prompts::activate_prompt(target.unwrap_or_default(), &name)
}

/// Read the current live `CLAUDE.md`, or `None` when absent.
#[tauri::command]
pub fn read_live_prompt(target: Option<PromptTarget>) -> AppResult<Option<LivePrompt>> {
    prompts::read_live_prompt(target.unwrap_or_default())
}

/// Import the live `CLAUDE.md` into the preset library under `name`.
#[tauri::command]
pub fn import_live_prompt(name: String, target: Option<PromptTarget>) -> AppResult<()> {
    prompts::import_live_prompt(target.unwrap_or_default(), &name)
}
