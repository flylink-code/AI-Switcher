//! Prompt (CLAUDE.md) preset commands.

use crate::error::AppResult;
use crate::prompts::{self, LivePrompt, PromptDetail, PromptInfo};

/// List all stored presets (names + mtimes, no content).
#[tauri::command]
pub fn list_prompts() -> AppResult<Vec<PromptInfo>> {
    prompts::list_prompts()
}

/// Read one preset's content for the editor.
#[tauri::command]
pub fn read_prompt(name: String) -> AppResult<PromptDetail> {
    prompts::read_prompt(&name)
}

/// Create or overwrite a preset.
#[tauri::command]
pub fn save_prompt(name: String, content: String) -> AppResult<()> {
    prompts::save_prompt(&name, &content)
}

/// Delete a preset (idempotent).
#[tauri::command]
pub fn delete_prompt(name: String) -> AppResult<()> {
    prompts::delete_prompt(&name)
}

/// Activate a preset: overwrite the live `~/.claude/CLAUDE.md` (backup first).
#[tauri::command]
pub fn activate_prompt(name: String) -> AppResult<()> {
    prompts::activate_prompt(&name)
}

/// Read the current live `CLAUDE.md`, or `None` when absent.
#[tauri::command]
pub fn read_live_prompt() -> AppResult<Option<LivePrompt>> {
    prompts::read_live_prompt()
}

/// Import the live `CLAUDE.md` into the preset library under `name`.
#[tauri::command]
pub fn import_live_prompt(name: String) -> AppResult<()> {
    prompts::import_live_prompt(&name)
}
