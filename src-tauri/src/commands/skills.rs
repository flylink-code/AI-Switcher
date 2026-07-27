//! Commands for discovering and managing Claude Code Skills.

use crate::error::AppResult;
use crate::skills::{
    delete_skill as remove_skill, get_skill_repository as get_repository,
    install_github_repository_skills as install_from_repository,
    install_github_skill as install_from_github, install_zip_skill as install_from_zip,
    list_github_repository_skills as list_repository_skills, list_skills as list_local_skills,
    set_skill_enabled as set_enabled, set_skill_repository as set_repository,
    RepositorySkill, Skill,
};
use crate::store::AppState;

#[tauri::command]
pub fn list_skills() -> AppResult<Vec<Skill>> {
    list_local_skills()
}

#[tauri::command]
pub fn get_skill_repository() -> AppResult<String> {
    get_repository()
}

#[tauri::command]
pub fn set_skill_repository(url: String) -> AppResult<String> {
    set_repository(&url)
}

#[tauri::command]
pub async fn list_github_repository_skills(url: String, _state: tauri::State<'_, AppState>) -> AppResult<Vec<RepositorySkill>> {
    list_repository_skills(&url).await
}

#[tauri::command]
pub async fn install_github_repository_skills(
    url: String,
    paths: Vec<String>,
    _state: tauri::State<'_, AppState>,
) -> AppResult<Vec<Skill>> {
    install_from_repository(&url, &paths).await
}

#[tauri::command]
pub async fn install_github_skill(url: String, _state: tauri::State<'_, AppState>) -> AppResult<Skill> {
    install_from_github(&url).await
}

#[tauri::command]
pub fn install_zip_skill(path: String) -> AppResult<Skill> {
    install_from_zip(std::path::Path::new(&path))
}

#[tauri::command]
pub fn set_skill_enabled(name: String, enabled: bool) -> AppResult<()> {
    set_enabled(&name, enabled)
}

#[tauri::command]
pub fn delete_skill(name: String) -> AppResult<()> {
    remove_skill(&name)
}
