//! Prompt (CLAUDE.md) preset commands.

use crate::error::AppResult;
use crate::prompts::{self, LivePrompt, PromptDetail, PromptInfo, PromptTarget};

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
