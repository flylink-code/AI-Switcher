//! Commands for Claude Code custom agents under `~/.claude/agents/`.

use crate::agents::{
    delete_agent as remove_agent, install_zip_agent as install_from_zip, list_agents as list_local,
    save_agent as write_agent, set_agent_enabled as set_enabled, Agent, AgentDraft,
};
use crate::error::AppResult;

#[tauri::command]
pub fn list_agents() -> AppResult<Vec<Agent>> {
    list_local()
}

#[tauri::command]
pub fn save_agent(draft: AgentDraft) -> AppResult<Agent> {
    write_agent(&draft)
}

#[tauri::command]
pub fn set_agent_enabled(name: String, enabled: bool) -> AppResult<()> {
    set_enabled(&name, enabled)
}

#[tauri::command]
pub fn delete_agent(name: String) -> AppResult<()> {
    remove_agent(&name)
}

#[tauri::command]
pub fn install_zip_agent(path: String) -> AppResult<Vec<Agent>> {
    install_from_zip(std::path::Path::new(&path))
}
