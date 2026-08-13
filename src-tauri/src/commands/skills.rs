//! Commands for discovering and managing Claude Code and Codex Skills.

use crate::error::AppResult;
use crate::skills::{
    delete_skill as remove_skill, get_skill_repository as get_repository,
    ignore_unmanaged_skill as ignore_unmanaged,
    install_github_repository_skills as install_from_repository,
    install_github_skill as install_from_github, install_zip_skill as install_from_zip,
    list_github_repository_skills as list_repository_skills, list_skills as list_local_skills,
    register_unmanaged_skill as register_unmanaged,
    scan_unmanaged_skills as scan_unmanaged,
    set_skill_enabled as set_enabled, set_skill_repository as set_repository,
    check_skill_update as check_update, check_skill_updates as check_updates,
    get_skill_repository_snapshot as get_repository_snapshot,
    list_skill_repositories as list_repositories,
    add_skill_repository as add_repository,
    remove_skill_repository as remove_repository,
    refresh_github_repository_skills as refresh_repository_skills,
    update_github_skills as update_skills,
    RepositorySkill, Skill, SkillRepositorySnapshot, SkillTarget, SkillUpdateStatus, UnmanagedSkill,
};
use crate::store::AppState;

#[tauri::command]
pub fn list_skill_repositories() -> AppResult<Vec<SkillRepositorySnapshot>> {
    list_repositories()
}

#[tauri::command]
pub async fn add_skill_repository(url: String, _state: tauri::State<'_, AppState>) -> AppResult<SkillRepositorySnapshot> {
    add_repository(&url).await
}

#[tauri::command]
pub fn remove_skill_repository(url: String) -> AppResult<()> {
    remove_repository(&url)
}

#[tauri::command]
pub fn list_skills(target: Option<SkillTarget>) -> AppResult<Vec<Skill>> {
    list_local_skills(target.unwrap_or_default())
}

#[tauri::command]
pub fn get_skill_repository() -> AppResult<String> {
    get_repository()
}

#[tauri::command]
pub fn get_skill_repository_snapshot() -> AppResult<SkillRepositorySnapshot> {
    get_repository_snapshot()
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
pub async fn refresh_github_repository_skills(url: String, _state: tauri::State<'_, AppState>) -> AppResult<SkillRepositorySnapshot> {
    refresh_repository_skills(&url).await
}

#[tauri::command]
pub async fn install_github_repository_skills(
    url: String,
    paths: Vec<String>,
    target: Option<SkillTarget>,
    _state: tauri::State<'_, AppState>,
) -> AppResult<Vec<Skill>> {
    install_from_repository(&url, &paths, target.unwrap_or_default()).await
}

#[tauri::command]
pub async fn install_github_skill(url: String, target: Option<SkillTarget>, _state: tauri::State<'_, AppState>) -> AppResult<Skill> {
    install_from_github(&url, target.unwrap_or_default()).await
}

#[tauri::command]
pub fn install_zip_skill(path: String, target: Option<SkillTarget>) -> AppResult<Skill> {
    install_from_zip(std::path::Path::new(&path), target.unwrap_or_default())
}

#[tauri::command]
pub fn set_skill_enabled(name: String, enabled: bool, target: Option<SkillTarget>) -> AppResult<()> {
    set_enabled(&name, enabled, target.unwrap_or_default())
}

#[tauri::command]
pub fn delete_skill(name: String, target: Option<SkillTarget>) -> AppResult<()> {
    remove_skill(&name, target.unwrap_or_default())
}

#[tauri::command]
pub async fn check_skill_update(name: String, target: Option<SkillTarget>, _state: tauri::State<'_, AppState>) -> AppResult<SkillUpdateStatus> {
    check_update(&name, target.unwrap_or_default()).await
}

#[tauri::command]
pub async fn check_skill_updates(target: Option<SkillTarget>, _state: tauri::State<'_, AppState>) -> AppResult<Vec<SkillUpdateStatus>> {
    check_updates(target.unwrap_or_default()).await
}

#[tauri::command]
pub async fn update_github_skills(names: Vec<String>, target: Option<SkillTarget>, _state: tauri::State<'_, AppState>) -> AppResult<Vec<Skill>> {
    update_skills(&names, target.unwrap_or_default()).await
}

#[tauri::command]
pub fn scan_unmanaged_skills(target: Option<SkillTarget>) -> AppResult<Vec<UnmanagedSkill>> {
    scan_unmanaged(target.unwrap_or_default())
}

#[tauri::command]
pub fn register_unmanaged_skill(path: String, target: Option<SkillTarget>) -> AppResult<Skill> {
    register_unmanaged(&path, target.unwrap_or_default())
}

#[tauri::command]
pub fn ignore_unmanaged_skill(path: String) -> AppResult<()> {
    ignore_unmanaged(&path)
}
