//! Local Skill discovery, GitHub repository installation, and enablement.
//!
//! Skills are installed into `~/.claude/skills/<name>`. An enabled marker is
//! represented by a `SKILL.md` file in that directory, the convention consumed
//! by Claude Code. Installation accepts GitHub repository URLs only and uses a
//! repository archive download rather than executing arbitrary Git commands.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use chrono::Utc;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::coding::pi::config::get_pi_dir;
use crate::config::{
    get_app_config_dir, get_claude_skills_dir, get_codex_skills_dir, get_home_dir, read_json_file,
    write_json_file,
};
use crate::error::{AppError, AppResult};

const SKILL_FILE: &str = "SKILL.md";
const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
const GITHUB_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
const GITHUB_HARD_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SKILL_REPOSITORY: &str = "https://github.com/anthropics/skills";
const SKILL_REPOSITORY_CONFIG_FILE: &str = "skills.json";
const SKILL_SOURCES_CONFIG_FILE: &str = "skill-sources.json";
const CODEX_SKILL_SOURCES_CONFIG_FILE: &str = "codex-skill-sources.json";
const PI_SKILL_SOURCES_CONFIG_FILE: &str = "pi-skill-sources.json";

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    #[default]
    ClaudeCode,
    Codex,
    #[serde(rename = "pi")]
    Pi,
}

fn skill_root(target: SkillTarget) -> PathBuf {
    match target {
        SkillTarget::ClaudeCode => get_claude_skills_dir(),
        SkillTarget::Codex => get_codex_skills_dir(),
        SkillTarget::Pi => get_pi_dir().join("skills"),
    }
}

fn skill_sources_file(target: SkillTarget) -> &'static str {
    match target {
        SkillTarget::ClaudeCode => SKILL_SOURCES_CONFIG_FILE,
        SkillTarget::Codex => CODEX_SKILL_SOURCES_CONFIG_FILE,
        SkillTarget::Pi => PI_SKILL_SOURCES_CONFIG_FILE,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub description: String,
    pub description_zh: Option<String>,
    pub source: Option<InstalledSkillSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkillSource {
    pub kind: String,
    pub source_url: Option<String>,
    pub revision: Option<String>,
    pub repository_path: Option<String>,
    pub installed_at: i64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateStatus {
    pub name: String,
    pub status: String,
    pub message: String,
    pub local_modified: bool,
    pub local_revision: Option<String>,
    pub remote_revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySkill {
    pub name: String,
    pub path: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillRepositoryConfig {
    repository_url: String,
    #[serde(default)]
    fetched_at: Option<i64>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    skills: Vec<RepositorySkill>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillRepositoriesConfig {
    #[serde(default)]
    repositories: Vec<SkillRepositorySnapshot>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRepositorySnapshot {
    pub repository_url: String,
    pub fetched_at: Option<i64>,
    pub revision: Option<String>,
    pub skills: Vec<RepositorySkill>,
}

fn read_skill_repositories_config() -> AppResult<Vec<SkillRepositorySnapshot>> {
    let path = get_app_config_dir().join(SKILL_REPOSITORY_CONFIG_FILE);
    if !path.exists() {
        return Ok(vec![SkillRepositorySnapshot {
            repository_url: DEFAULT_SKILL_REPOSITORY.to_string(),
            fetched_at: None,
            revision: None,
            skills: Vec::new(),
        }]);
    }

    if let Ok(Some(config)) = read_json_file::<SkillRepositoriesConfig>(&path) {
        if !config.repositories.is_empty() {
            return Ok(config.repositories);
        }
    }

    if let Ok(Some(old_config)) = read_json_file::<SkillRepositoryConfig>(&path) {
        if !old_config.repository_url.is_empty() {
            let repository_url = normalize_github_url(&old_config.repository_url).unwrap_or(old_config.repository_url);
            return Ok(vec![SkillRepositorySnapshot {
                repository_url,
                fetched_at: old_config.fetched_at,
                revision: old_config.revision,
                skills: old_config.skills,
            }]);
        }
    }

    Ok(vec![SkillRepositorySnapshot {
        repository_url: DEFAULT_SKILL_REPOSITORY.to_string(),
        fetched_at: None,
        revision: None,
        skills: Vec::new(),
    }])
}

fn write_skill_repositories_config(repos: &[SkillRepositorySnapshot]) -> AppResult<()> {
    let path = get_app_config_dir().join(SKILL_REPOSITORY_CONFIG_FILE);
    write_json_file(
        &path,
        &SkillRepositoriesConfig {
            repositories: repos.to_vec(),
        },
    )
}

pub fn list_skill_repositories() -> AppResult<Vec<SkillRepositorySnapshot>> {
    read_skill_repositories_config()
}

pub async fn add_skill_repository(url: &str) -> AppResult<SkillRepositorySnapshot> {
    let repository_url = normalize_github_url(url)?;
    let mut repos = read_skill_repositories_config()?;
    if let Some(existing) = repos.iter().find(|r| r.repository_url == repository_url) {
        return Ok(existing.clone());
    }

    let snapshot = match download_github_archive(&repository_url).await {
        Ok((archive, repo)) => {
            let mut skills: Vec<_> = repository_skill_entries(&archive, &repo)?
                .into_iter()
                .map(|(skill, _)| skill)
                .collect();
            skills.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
            SkillRepositorySnapshot {
                repository_url: repository_url.clone(),
                fetched_at: Some(Utc::now().timestamp_millis()),
                revision: archive_revision(&archive),
                skills,
            }
        }
        Err(_) => SkillRepositorySnapshot {
            repository_url: repository_url.clone(),
            fetched_at: None,
            revision: None,
            skills: Vec::new(),
        },
    };
    repos.push(snapshot.clone());
    write_skill_repositories_config(&repos)?;
    Ok(snapshot)
}

pub fn remove_skill_repository(url: &str) -> AppResult<()> {
    let repository_url = normalize_github_url(url).unwrap_or_else(|_| url.trim().to_string());
    let mut repos = read_skill_repositories_config()?;
    repos.retain(|r| r.repository_url != repository_url);
    write_skill_repositories_config(&repos)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillSourcesConfig {
    #[serde(default = "skill_sources_version")]
    version: u8,
    #[serde(default)]
    skills: BTreeMap<String, InstalledSkillSource>,
}

fn skill_sources_version() -> u8 { 1 }

pub fn list_skills(target: SkillTarget) -> AppResult<Vec<Skill>> {
    let root = skill_root(target);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let sources = skill_sources(target)?;
    let mut skills = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join(SKILL_FILE);
        let disabled_file = path.join("SKILL.md.disabled");
        let content = fs::read_to_string(if skill_file.is_file() { &skill_file } else { &disabled_file }).unwrap_or_default();
        let description = skill_description(&content).unwrap_or_default();
        let description_zh = skill_description_zh(&content);
        let name = entry.file_name().to_string_lossy().to_string();
        skills.push(Skill {
            source: sources.skills.get(&name).cloned(),
            name,
            path: path.to_string_lossy().to_string(),
            enabled: skill_file.is_file(),
            description,
            description_zh,
        });
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(skills)
}

pub fn get_skill_repository() -> AppResult<String> {
    let repos = read_skill_repositories_config()?;
    Ok(repos.first().map(|r| r.repository_url.clone()).unwrap_or_else(|| DEFAULT_SKILL_REPOSITORY.to_string()))
}

pub fn get_skill_repository_snapshot() -> AppResult<SkillRepositorySnapshot> {
    let repos = read_skill_repositories_config()?;
    Ok(repos.into_iter().next().unwrap_or_else(|| SkillRepositorySnapshot {
        repository_url: DEFAULT_SKILL_REPOSITORY.to_string(),
        fetched_at: None,
        revision: None,
        skills: Vec::new(),
    }))
}

pub fn set_skill_repository(url: &str) -> AppResult<String> {
    let repository_url = normalize_github_url(url)?;
    let mut repos = read_skill_repositories_config()?;
    if !repos.iter().any(|r| r.repository_url == repository_url) {
        repos.push(SkillRepositorySnapshot {
            repository_url: repository_url.clone(),
            fetched_at: None,
            revision: None,
            skills: Vec::new(),
        });
        write_skill_repositories_config(&repos)?;
    }
    Ok(repository_url)
}

pub async fn refresh_github_repository_skills(url: &str) -> AppResult<SkillRepositorySnapshot> {
    let repository_url = normalize_github_url(url)?;
    let (archive, repo) = download_github_archive(&repository_url).await?;
    let mut skills: Vec<_> = repository_skill_entries(&archive, &repo)?
        .into_iter()
        .map(|(skill, _)| skill)
        .collect();
    skills.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    let snapshot = SkillRepositorySnapshot {
        repository_url: repository_url.clone(),
        fetched_at: Some(Utc::now().timestamp_millis()),
        revision: archive_revision(&archive),
        skills,
    };
    let mut repos = read_skill_repositories_config()?;
    if let Some(pos) = repos.iter().position(|r| r.repository_url == repository_url) {
        repos[pos] = snapshot.clone();
    } else {
        repos.push(snapshot.clone());
    }
    write_skill_repositories_config(&repos)?;
    Ok(snapshot)
}

pub async fn list_github_repository_skills(url: &str) -> AppResult<Vec<RepositorySkill>> {
    let (archive, repo) = download_github_archive(url).await?;
    let mut skills: Vec<_> = repository_skill_entries(&archive, &repo)?
        .into_iter()
        .map(|(skill, _)| skill)
        .collect();
    skills.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(skills)
}

pub async fn install_github_repository_skills(url: &str, paths: &[String], target: SkillTarget) -> AppResult<Vec<Skill>> {
    if paths.is_empty() {
        return Err(AppError::Config("请至少选择一个 Skill".to_string()));
    }
    let (archive, repo) = download_github_archive(url).await?;
    let entries = repository_skill_entries(&archive, &repo)?;
    let entry_map: BTreeMap<_, _> = entries.into_iter()
        .map(|(skill, index)| (skill.path.clone(), (skill, index)))
        .collect();
    let selected: BTreeSet<_> = paths.iter().map(|path| path.trim().to_string()).collect();
    if selected.len() != paths.len() || selected.iter().any(|path| path.is_empty()) {
        return Err(AppError::Config("Skill 选择无效".to_string()));
    }

    let mut selected_indexes = Vec::with_capacity(selected.len());
    let mut installed_names = BTreeSet::new();
    for path in selected {
        let (repository_skill, index) = entry_map.get(&path)
            .ok_or_else(|| AppError::Config(format!("仓库中未找到 Skill: {path}")))?;
        let install_name = sanitize_skill_name(&repository_skill.name)?;
        if !installed_names.insert(install_name.clone()) {
            return Err(AppError::Config(format!("选择的 Skills 名称冲突: {install_name}")));
        }
        selected_indexes.push(*index);
    }

    let mut installed = Vec::with_capacity(selected_indexes.len());
    for index in selected_indexes {
        let path = entry_map.iter()
            .find_map(|(path, (_, entry_index))| (*entry_index == index).then_some(path.clone()))
            .unwrap_or_default();
        let skill = install_archive_skill_at(&archive, index, Some(&repo), target)?;
        installed.push(record_github_source(skill, url, archive_revision(&archive), Some(path), target)?);
    }
    Ok(installed)
}

/// Install a skill from a public GitHub repository. The repo must include one
/// `SKILL.md`, either at its root or under a skill subdirectory.
pub async fn install_github_skill(url: &str, target: SkillTarget) -> AppResult<Skill> {
    let (archive, repo) = download_github_archive(url).await?;
    let skill = install_archive(&archive, Some(&repo), target)?;
    record_github_source(skill, url, archive_revision(&archive), None, target)
}

/// Install from a user-selected ZIP archive. Archives are protected from path
/// traversal and must contain a SKILL.md file.
pub fn install_zip_skill(path: &Path, target: SkillTarget) -> AppResult<Skill> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(AppError::Config("Skill ZIP 超过 100 MB 限制".to_string()));
    }
    let archive = fs::read(path)?;
    let fallback = path.file_stem().and_then(|n| n.to_str());
    let skill = install_archive(&archive, fallback, target)?;
    record_source(skill, InstalledSkillSource {
        kind: "zip".to_string(),
        source_url: Some(path.to_string_lossy().into_owned()),
        revision: None,
        repository_path: None,
        installed_at: Utc::now().timestamp_millis(),
        content_sha256: String::new(),
    }, target)
}

pub fn set_skill_enabled(name: &str, enabled: bool, target: SkillTarget) -> AppResult<()> {
    let path = skill_path(name, target)?;
    let skill_file = path.join(SKILL_FILE);
    let disabled = path.join("SKILL.md.disabled");
    if enabled {
        if skill_file.is_file() {
            return Ok(());
        }
        if !disabled.is_file() {
            return Err(AppError::Config("无法启用：Skill 缺少 SKILL.md".to_string()));
        }
        fs::rename(disabled, skill_file)?;
    } else {
        if disabled.is_file() {
            return Ok(());
        }
        if !skill_file.is_file() {
            return Err(AppError::Config("无法停用：Skill 缺少 SKILL.md".to_string()));
        }
        // Keep the installed source intact. A disabled marker makes the state
        // explicit without destructive deletion; Claude Code only loads SKILL.md.
        fs::rename(skill_file, disabled)?;
    }
    Ok(())
}

pub fn delete_skill(name: &str, target: SkillTarget) -> AppResult<()> {
    let path = skill_path(name, target)?;
    fs::remove_dir_all(path)?;
    let mut sources = skill_sources(target)?;
    sources.skills.remove(name);
    write_skill_sources(&sources, target)?;
    Ok(())
}

pub async fn check_skill_update(name: &str, target: SkillTarget) -> AppResult<SkillUpdateStatus> {
    let skill = list_skills(target)?.into_iter().find(|skill| skill.name == name)
        .ok_or_else(|| AppError::Config(format!("Skill 不存在: {name}")))?;
    match classify_skill_for_update(&skill) {
        SkillCheckPlan::Ready(status) => Ok(status),
        SkillCheckPlan::Github { source_url, source } => {
            let (archive, repo) = download_github_archive(&source_url).await?;
            Ok(compare_installed_github_skill(&skill, &source, Arc::new(archive), &repo).await)
        }
    }
}

pub async fn check_skill_updates(target: SkillTarget) -> AppResult<Vec<SkillUpdateStatus>> {
    let skills = list_skills(target)?;
    let order: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    let mut statuses: HashMap<String, SkillUpdateStatus> = HashMap::new();
    let mut by_repo: BTreeMap<String, Vec<Skill>> = BTreeMap::new();

    for skill in skills {
        match classify_skill_for_update(&skill) {
            SkillCheckPlan::Ready(status) => {
                statuses.insert(skill.name.clone(), status);
            }
            SkillCheckPlan::Github { source_url, .. } => {
                match normalize_github_url(&source_url) {
                    Ok(repo_url) => by_repo.entry(repo_url).or_default().push(skill),
                    Err(error) => {
                        let name = skill.name.clone();
                        statuses.insert(name.clone(), skill_check_error(&name, skill.source.as_ref(), error));
                    }
                }
            }
        }
    }

    for (repo_url, group) in by_repo {
        log::info!("检查 Skill 更新: 下载 {}（{} 个）", repo_url, group.len());
        match download_github_archive(&repo_url).await {
            Ok((bytes, repo)) => {
                let archive = Arc::new(bytes);
                for skill in group {
                    let source = skill.source.clone().expect("github Skill 必有来源记录");
                    let status = compare_installed_github_skill(&skill, &source, archive.clone(), &repo).await;
                    statuses.insert(skill.name.clone(), status);
                }
            }
            Err(error) => {
                log::warn!("检查 Skill 更新失败 {repo_url}: {error}");
                let message = error.to_string();
                for skill in group {
                    statuses.insert(
                        skill.name.clone(),
                        skill_check_error(&skill.name, skill.source.as_ref(), &message),
                    );
                }
            }
        }
    }

    Ok(order.into_iter().filter_map(|name| statuses.remove(&name)).collect())
}

enum SkillCheckPlan {
    Ready(SkillUpdateStatus),
    Github {
        source_url: String,
        source: InstalledSkillSource,
    },
}

fn classify_skill_for_update(skill: &Skill) -> SkillCheckPlan {
    let Some(source) = skill.source.clone() else {
        return SkillCheckPlan::Ready(SkillUpdateStatus {
            name: skill.name.clone(),
            status: "untracked".to_string(),
            message: "此 Skill 没有可检查的 GitHub 来源记录".to_string(),
            local_modified: false,
            local_revision: None,
            remote_revision: None,
        });
    };
    if source.kind != "github" {
        return SkillCheckPlan::Ready(SkillUpdateStatus {
            name: skill.name.clone(),
            status: "unsupported".to_string(),
            message: "仅 GitHub 来源的 Skill 支持更新检查".to_string(),
            local_modified: false,
            local_revision: source.revision,
            remote_revision: None,
        });
    }
    let Some(source_url) = source.source_url.clone().filter(|url| !url.trim().is_empty()) else {
        return SkillCheckPlan::Ready(skill_check_error(
            &skill.name,
            Some(&source),
            "Skill 来源记录缺少地址",
        ));
    };
    SkillCheckPlan::Github { source_url, source }
}

fn skill_check_error(
    name: &str,
    source: Option<&InstalledSkillSource>,
    error: impl std::fmt::Display,
) -> SkillUpdateStatus {
    SkillUpdateStatus {
        name: name.to_string(),
        status: "error".to_string(),
        message: error.to_string(),
        local_modified: false,
        local_revision: source.and_then(|item| item.revision.clone()),
        remote_revision: None,
    }
}

async fn compare_installed_github_skill(
    skill: &Skill,
    source: &InstalledSkillSource,
    archive: Arc<Vec<u8>>,
    repo: &str,
) -> SkillUpdateStatus {
    let local_hash = match skill_content_hash(Path::new(&skill.path)) {
        Ok(hash) => hash,
        Err(error) => return skill_check_error(&skill.name, Some(source), error),
    };
    let local_modified = local_hash != source.content_sha256;
    let remote_revision = archive_revision(&archive);
    let repository_path = source.repository_path.clone();
    let repo_owned = repo.to_string();
    let hash_archive = archive.clone();
    let remote_hash = match tokio::task::spawn_blocking(move || {
        archive_skill_hash(&hash_archive, repository_path.as_deref(), &repo_owned)
    })
    .await
    {
        Ok(Ok(hash)) => hash,
        Ok(Err(error)) => return skill_check_error(&skill.name, Some(source), error),
        Err(error) => return skill_check_error(&skill.name, Some(source), error),
    };
    let status = if local_modified {
        "local_modified"
    } else if remote_hash == local_hash {
        "up_to_date"
    } else {
        "update_available"
    };
    let message = match status {
        "local_modified" => "检测到本地修改，已保留原文件且不会自动覆盖",
        "up_to_date" => "Skill 已是最新版本",
        _ => "发现可用更新，请确认后重新安装以应用",
    }
    .to_string();
    SkillUpdateStatus {
        name: skill.name.clone(),
        status: status.to_string(),
        message,
        local_modified,
        local_revision: source.revision.clone(),
        remote_revision,
    }
}

pub async fn update_github_skills(names: &[String], target: SkillTarget) -> AppResult<Vec<Skill>> {
    if names.is_empty() {
        return Err(AppError::Config("请至少选择一个可更新的 Skill".to_string()));
    }
    let installed = list_skills(target)?;
    let mut updated = Vec::with_capacity(names.len());
    for name in names {
        let skill = installed.iter().find(|skill| skill.name == *name)
            .ok_or_else(|| AppError::Config(format!("Skill 不存在: {name}")))?;
        let status = check_skill_update(name, target).await?;
        if status.status != "update_available" {
            return Err(AppError::Config(format!("Skill 不可安全更新: {name}（{}）", status.message)));
        }
        let source = skill.source.as_ref()
            .ok_or_else(|| AppError::Config(format!("Skill 缺少来源记录: {name}")))?;
        let source_url = source.source_url.as_deref()
            .ok_or_else(|| AppError::Config(format!("Skill 来源记录缺少地址: {name}")))?;
        if let Some(repository_path) = source.repository_path.as_ref() {
            let mut skills = install_github_repository_skills(source_url, &[repository_path.clone()], target).await?;
            updated.append(&mut skills);
        } else {
            updated.push(install_github_skill(source_url, target).await?);
        }
    }
    Ok(updated)
}

fn record_github_source(skill: Skill, url: &str, revision: Option<String>, repository_path: Option<String>, target: SkillTarget) -> AppResult<Skill> {
    record_source(skill, InstalledSkillSource {
        kind: "github".to_string(),
        source_url: Some(normalize_github_url(url)?),
        revision,
        repository_path,
        installed_at: Utc::now().timestamp_millis(),
        content_sha256: String::new(),
    }, target)
}

fn record_source(mut skill: Skill, mut source: InstalledSkillSource, target: SkillTarget) -> AppResult<Skill> {
    let skill_file = PathBuf::from(&skill.path).join(SKILL_FILE);
    let disabled = PathBuf::from(&skill.path).join("SKILL.md.disabled");
    source.content_sha256 = skill_content_hash_from_files(&skill_file, &disabled)?;
    let mut sources = skill_sources(target)?;
    sources.skills.insert(skill.name.clone(), source.clone());
    write_skill_sources(&sources, target)?;
    skill.source = Some(source);
    Ok(skill)
}

fn skill_content_hash(skill_dir: &Path) -> AppResult<String> {
    skill_content_hash_from_files(&skill_dir.join(SKILL_FILE), &skill_dir.join("SKILL.md.disabled"))
}

fn skill_content_hash_from_files(skill_file: &Path, disabled_file: &Path) -> AppResult<String> {
    let content = fs::read(if skill_file.is_file() { skill_file } else { disabled_file })?;
    Ok(hex::encode(Sha256::digest(content)))
}

fn archive_skill_hash(bytes: &[u8], repository_path: Option<&str>, fallback_name: &str) -> AppResult<String> {
    let entries = repository_skill_entries(bytes, fallback_name)?;
    let (_, index) = if let Some(path) = repository_path {
        entries.into_iter().find(|(skill, _)| skill.path == path)
            .ok_or_else(|| AppError::Config("远端仓库已找不到原 Skill 路径".to_string()))?
    } else {
        entries.into_iter().next()
            .ok_or_else(|| AppError::Config("远端仓库未找到 SKILL.md".to_string()))?
    };
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| AppError::Config(format!("无效的 Skill ZIP 文件: {e}")))?;
    let mut content = Vec::new();
    archive.by_index(index)
        .map_err(|e| AppError::Config(format!("读取 ZIP 条目失败: {e}")))?
        .read_to_end(&mut content)?;
    Ok(hex::encode(Sha256::digest(content)))
}

fn skill_sources(target: SkillTarget) -> AppResult<SkillSourcesConfig> {
    let path = get_app_config_dir().join(skill_sources_file(target));
    Ok(read_json_file::<SkillSourcesConfig>(&path)?.unwrap_or_default())
}

fn write_skill_sources(sources: &SkillSourcesConfig, target: SkillTarget) -> AppResult<()> {
    write_json_file(&get_app_config_dir().join(skill_sources_file(target)), sources)
}

fn archive_revision(bytes: &[u8]) -> Option<String> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let entry = archive.by_index(0).ok()?;
    entry.enclosed_name()?.components().next()?.as_os_str().to_str()
        .and_then(|root| root.rsplit('-').next())
        .filter(|revision| revision.len() >= 7)
        .map(ToOwned::to_owned)
}

fn install_archive(bytes: &[u8], fallback_name: Option<&str>, target: SkillTarget) -> AppResult<Skill> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::Config(format!("无效的 Skill ZIP 文件: {e}")))?;
    let skill_index = (0..archive.len()).find(|&index| {
        archive.by_index(index).ok().is_some_and(|entry| {
            entry.enclosed_name().is_some_and(|path| {
                path.file_name().and_then(|name| name.to_str()) == Some(SKILL_FILE)
            })
        })
    }).ok_or_else(|| AppError::Config("ZIP 中未找到 SKILL.md".to_string()))?;
    drop(archive);
    install_archive_skill_at(bytes, skill_index, fallback_name, target)
}

fn install_archive_skill_at(bytes: &[u8], skill_index: usize, fallback_name: Option<&str>, target: SkillTarget) -> AppResult<Skill> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::Config(format!("无效的 Skill ZIP 文件: {e}")))?;
    let root = skill_root(target);
    fs::create_dir_all(&root)?;

    let skill_path_in_archive = archive.by_index(skill_index)
        .map_err(|e| AppError::Config(format!("读取 ZIP 条目失败: {e}")))?
        .enclosed_name()
        .ok_or_else(|| AppError::Config("ZIP 包含不安全路径".to_string()))?
        .to_path_buf();
    let skill_parent = skill_path_in_archive.parent().unwrap_or(Path::new(""));
    let candidate = skill_parent
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|name| !name.is_empty())
        .or(fallback_name)
        .unwrap_or("skill");
    let name = sanitize_skill_name(candidate)?;
    let target_dir = root.join(&name);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)
            .map_err(|e| AppError::Config(format!("读取 ZIP 条目失败: {e}")))?;
        let enclosed = entry.enclosed_name()
            .ok_or_else(|| AppError::Config("ZIP 包含不安全路径".to_string()))?
            .to_path_buf();
        if !enclosed.starts_with(skill_parent) {
            continue;
        }
        let relative = enclosed.strip_prefix(skill_parent).unwrap_or(&enclosed);
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = target_dir.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = fs::File::create(destination)?;
            std::io::copy(&mut entry, &mut output)?;
        }
    }
    list_skills(target)?.into_iter().find(|skill| skill.name == name)
        .ok_or_else(|| AppError::Other("Skill 安装后未能读取".to_string()))
}

async fn download_github_archive(url: &str) -> AppResult<(Vec<u8>, String)> {
    let (owner, repo) = parse_github_url(url)?;
    let archive_url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/HEAD");
    Ok((download_github_archive_bytes(&archive_url).await?, repo.to_string()))
}

async fn download_github_archive_bytes(archive_url: &str) -> AppResult<Vec<u8>> {
    let mut last_error = None;
    for attempt in 1..=2 {
        match download_github_archive_once(archive_url).await {
            Ok(archive) => return Ok(archive),
            Err(error) => last_error = Some((attempt, error)),
        }
    }
    let (attempts, error) = last_error.expect("download attempts are non-empty");
    Err(AppError::Other(format!("GitHub Skill 下载失败（已重试 {attempts} 次）: {error}")))
}

async fn download_github_archive_once(archive_url: &str) -> AppResult<Vec<u8>> {
    let request = async {
        let mut response = reqwest::Client::builder()
            .connect_timeout(GITHUB_CONNECT_TIMEOUT)
            .timeout(GITHUB_REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| AppError::Other(format!("创建 GitHub 请求客户端失败: {e}")))?
            .get(archive_url)
            .header("User-Agent", "Claude-Switcher")
            // Some proxies add a Content-Encoding header that this minimal client does not
            // decode. Asking for identity keeps the GitHub ZIP bytes intact end-to-end.
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|e| AppError::Other(format!("GitHub 请求阶段失败: {e}")))?;
        if !response.status().is_success() {
            return Err(AppError::Config(format!("GitHub 下载失败（HTTP {}）", response.status())));
        }
        if response.content_length().is_some_and(|size| size > MAX_ARCHIVE_BYTES) {
            return Err(AppError::Config("GitHub Skill 压缩包超过 100 MB 限制".to_string()));
        }
        let mut archive = Vec::new();
        while let Some(chunk) = response.chunk().await
            .map_err(|e| AppError::Other(format!("GitHub 响应体读取失败: {e}")))?
        {
            let next_len = archive.len() as u64 + chunk.len() as u64;
            if next_len > MAX_ARCHIVE_BYTES {
                return Err(AppError::Config("GitHub Skill 压缩包超过 100 MB 限制".to_string()));
            }
            archive.extend_from_slice(&chunk);
        }
        Ok(archive)
    };
    tokio::time::timeout(GITHUB_HARD_TIMEOUT, request)
        .await
        .map_err(|_| AppError::Other("GitHub Skill 下载超时（30s）".into()))?
}

fn repository_skill_entries(bytes: &[u8], fallback_name: &str) -> AppResult<Vec<(RepositorySkill, usize)>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::Config(format!("无效的 Skill ZIP 文件: {e}")))?;
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)
            .map_err(|e| AppError::Config(format!("读取 ZIP 条目失败: {e}")))?;
        let enclosed = entry.enclosed_name()
            .ok_or_else(|| AppError::Config("ZIP 包含不安全路径".to_string()))?
            .to_path_buf();
        if enclosed.file_name().and_then(|name| name.to_str()) != Some(SKILL_FILE) {
            continue;
        }
        let skill_parent = enclosed.parent().unwrap_or(Path::new(""));
        let candidate = skill_parent.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(fallback_name);
        let mut content = String::new();
        let _ = entry.read_to_string(&mut content);
        let path = repository_skill_path(skill_parent);
        entries.entry(path.clone()).or_insert_with(|| (
            RepositorySkill {
                name: candidate.to_string(),
                path,
                description: skill_description(&content).unwrap_or_default(),
            },
            index,
        ));
    }
    if entries.is_empty() {
        return Err(AppError::Config("仓库中未找到 SKILL.md".to_string()));
    }
    Ok(entries.into_values().collect())
}

fn repository_skill_path(skill_parent: &Path) -> String {
    let mut components = skill_parent.components();
    let _ = components.next(); // GitHub archive root directory
    components.as_path().components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_github_url(url: &str) -> AppResult<(&str, &str)> {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = trimmed.strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .ok_or_else(|| AppError::Config("仅支持 https://github.com/<owner>/<repo> 地址".to_string()))?;
    let mut parts = path.split('/');
    let owner = parts.next().filter(|value| !value.is_empty());
    let repo = parts.next().filter(|value| !value.is_empty());
    if owner.is_none() || repo.is_none() || parts.next().is_some() {
        return Err(AppError::Config("GitHub 地址必须为 https://github.com/<owner>/<repo>".to_string()));
    }
    Ok((owner.unwrap(), repo.unwrap()))
}

fn normalize_github_url(url: &str) -> AppResult<String> {
    let (owner, repo) = parse_github_url(url)?;
    Ok(format!("https://github.com/{owner}/{repo}"))
}

fn skill_path(name: &str, target: SkillTarget) -> AppResult<PathBuf> {
    let clean = sanitize_skill_name(name)?;
    let path = skill_root(target).join(clean);
    if !path.is_dir() {
        return Err(AppError::Config(format!("Skill 不存在: {name}")));
    }
    Ok(path)
}

fn sanitize_skill_name(value: &str) -> AppResult<String> {
    let cleaned = value.trim().to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return Err(AppError::Config("无效的 Skill 名称".to_string()));
    }
    Ok(cleaned)
}

fn skill_description(content: &str) -> Option<String> {
    frontmatter_value(content, "description")
}

fn skill_description_zh(content: &str) -> Option<String> {
    frontmatter_value(content, "description_zh").or_else(|| frontmatter_value(content, "descriptionZh"))
}

fn frontmatter_value(content: &str, key: &str) -> Option<String> {
    let frontmatter = content.strip_prefix("---")?.split_once("---")?.0;
    frontmatter.lines().find_map(|line| {
        line.trim().strip_prefix(&format!("{key}:")).map(|value| value.trim().trim_matches('"').to_string())
    })
}

// ---- Skills Discovery (unmanaged local skills) --------------------------------

const SKILL_DISCOVERY_IGNORE_FILE: &str = "skill-discovery-ignore.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmanagedSkill {
    pub directory: String,
    pub name: String,
    pub description: String,
    pub found_in: Vec<String>,
    pub path: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillDiscoveryIgnoreConfig {
    #[serde(default = "skill_sources_version")]
    version: u8,
    #[serde(default)]
    ignored: BTreeSet<String>,
}

/// Scan known skill roots outside the managed target directory.
pub fn scan_unmanaged_skills(target: SkillTarget) -> AppResult<Vec<UnmanagedSkill>> {
    let managed_root = skill_root(target);
    let managed_names: BTreeSet<String> = list_skills(target)?
        .into_iter()
        .map(|skill| skill.name)
        .collect();
    let ignored = load_discovery_ignore()?.ignored;

    let mut by_key: BTreeMap<String, UnmanagedSkill> = BTreeMap::new();
    for (scan_dir, label) in discovery_scan_sources(target) {
        if same_path(&scan_dir, &managed_root) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&scan_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let directory = entry.file_name().to_string_lossy().to_string();
            if directory.starts_with('.') {
                continue;
            }
            let skill_md = path.join(SKILL_FILE);
            let disabled = path.join("SKILL.md.disabled");
            let content_path = if skill_md.is_file() {
                skill_md
            } else if disabled.is_file() {
                disabled
            } else {
                continue;
            };
            let ignore_key = discovery_ignore_key(&path);
            if ignored.contains(&ignore_key) || ignored.contains(&directory) {
                continue;
            }
            if managed_names.contains(&directory) {
                continue;
            }
            let content = fs::read_to_string(&content_path).unwrap_or_default();
            let description = skill_description(&content).unwrap_or_default();
            let name = frontmatter_value(&content, "name").unwrap_or_else(|| directory.clone());
            by_key
                .entry(ignore_key)
                .and_modify(|skill| {
                    if !skill.found_in.contains(&label) {
                        skill.found_in.push(label.clone());
                    }
                })
                .or_insert(UnmanagedSkill {
                    directory: directory.clone(),
                    name,
                    description,
                    found_in: vec![label.clone()],
                    path: path.to_string_lossy().into_owned(),
                });
        }
    }

    let mut out: Vec<UnmanagedSkill> = by_key.into_values().collect();
    out.sort_by(|a, b| a.directory.to_lowercase().cmp(&b.directory.to_lowercase()));
    Ok(out)
}

/// Copy an unmanaged skill directory into the managed skills root and record source.
pub fn register_unmanaged_skill(path: &str, target: SkillTarget) -> AppResult<Skill> {
    let source = PathBuf::from(path.trim());
    if !source.is_dir() {
        return Err(AppError::Config(format!("Skill 目录不存在: {path}")));
    }
    let skill_md = source.join(SKILL_FILE);
    let disabled = source.join("SKILL.md.disabled");
    if !skill_md.is_file() && !disabled.is_file() {
        return Err(AppError::Config("目录缺少 SKILL.md".to_string()));
    }
    let directory = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Config("无法解析 Skill 目录名".to_string()))?
        .to_string();
    if directory.starts_with('.') || directory.contains(['/', '\\']) {
        return Err(AppError::Config(format!("无效的 Skill 目录名: {directory}")));
    }
    let dest = skill_root(target).join(&directory);
    if dest.exists() {
        return Err(AppError::Config(format!("目标已存在同名 Skill: {directory}")));
    }
    fs::create_dir_all(skill_root(target))?;
    copy_dir_recursive(&source, &dest)?;
    let content_hash = skill_content_hash(&dest).unwrap_or_default();
    let skill = list_skills(target)?
        .into_iter()
        .find(|skill| skill.name == directory)
        .ok_or_else(|| AppError::Config(format!("登记后未找到 Skill: {directory}")))?;
    record_source(
        skill,
        InstalledSkillSource {
            kind: "local_import".to_string(),
            source_url: Some(source.to_string_lossy().into_owned()),
            revision: None,
            repository_path: None,
            installed_at: Utc::now().timestamp_millis(),
            content_sha256: content_hash,
        },
        target,
    )
}

/// Persist an ignore entry so discovery no longer surfaces this skill path.
pub fn ignore_unmanaged_skill(path: &str) -> AppResult<()> {
    let source = PathBuf::from(path.trim());
    if path.trim().is_empty() {
        return Err(AppError::Config("忽略路径不能为空".to_string()));
    }
    let key = discovery_ignore_key(&source);
    let mut config = load_discovery_ignore()?;
    config.ignored.insert(key);
    write_discovery_ignore(&config)
}

fn discovery_scan_sources(target: SkillTarget) -> Vec<(PathBuf, String)> {
    let home = get_home_dir();
    let mut sources = Vec::new();
    match target {
        SkillTarget::ClaudeCode => {
            sources.push((get_codex_skills_dir(), "codex".to_string()));
            sources.push((get_pi_dir().join("skills"), "pi".to_string()));
        }
        SkillTarget::Codex => {
            sources.push((get_claude_skills_dir(), "claude_code".to_string()));
            sources.push((get_pi_dir().join("skills"), "pi".to_string()));
        }
        SkillTarget::Pi => {
            sources.push((get_claude_skills_dir(), "claude_code".to_string()));
            sources.push((get_codex_skills_dir(), "codex".to_string()));
        }
    }
    sources.push((home.join(".agents").join("skills"), "agents".to_string()));
    sources.push((home.join(".codex").join("skills"), "codex".to_string()));
    sources.push((home.join(".claude").join("skills"), "claude_code".to_string()));
    sources.push((home.join(".pi").join("agent").join("skills"), "pi".to_string()));
    // Deduplicate while preserving order.
    let mut seen = BTreeSet::new();
    sources.retain(|(path, _)| seen.insert(normalize_path_key(path)));
    sources
}

fn discovery_ignore_key(path: &Path) -> String {
    normalize_path_key(path)
}

fn normalize_path_key(path: &Path) -> String {
    let display = path.to_string_lossy();
    #[cfg(windows)]
    {
        display.trim_start_matches(r"\\?\").replace('/', "\\").to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        display.to_string()
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    normalize_path_key(a) == normalize_path_key(b)
}

fn load_discovery_ignore() -> AppResult<SkillDiscoveryIgnoreConfig> {
    let path = get_app_config_dir().join(SKILL_DISCOVERY_IGNORE_FILE);
    Ok(read_json_file::<SkillDiscoveryIgnoreConfig>(&path)?.unwrap_or_default())
}

fn write_discovery_ignore(config: &SkillDiscoveryIgnoreConfig) -> AppResult<()> {
    let path = get_app_config_dir().join(SKILL_DISCOVERY_IGNORE_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json_file(&path, config)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> AppResult<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn repository_archive() -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        archive.start_file("owner-skills-123/skills/first/SKILL.md", options).unwrap();
        archive.write_all(b"---\ndescription: First skill\n---\n").unwrap();
        archive.start_file("owner-skills-123/skills/second/SKILL.md", options).unwrap();
        archive.write_all(b"---\ndescription: Second skill\n---\n").unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn repository_archive_lists_each_skill_directory() {
        let entries = repository_skill_entries(&repository_archive(), "skills").unwrap();
        let paths: Vec<_> = entries.into_iter().map(|(skill, _)| (skill.path, skill.description)).collect();
        assert_eq!(paths, vec![
            ("skills/first".to_string(), "First skill".to_string()),
            ("skills/second".to_string(), "Second skill".to_string()),
        ]);
    }

    #[test]
    fn github_repository_url_is_canonicalized() {
        assert_eq!(
            normalize_github_url("http://github.com/anthropics/skills.git/").unwrap(),
            "https://github.com/anthropics/skills",
        );
        assert!(normalize_github_url("https://github.com/anthropics/skills/tree/main").is_err());
    }

    #[test]
    fn local_import_skill_is_skipped_without_github_download() {
        let skill = Skill {
            name: "hardware-scheme-diagram".into(),
            path: "/tmp/x".into(),
            enabled: true,
            description: String::new(),
            description_zh: None,
            source: Some(InstalledSkillSource {
                kind: "local_import".into(),
                source_url: None,
                revision: None,
                repository_path: None,
                installed_at: 0,
                content_sha256: String::new(),
            }),
        };
        match classify_skill_for_update(&skill) {
            SkillCheckPlan::Ready(status) => assert_eq!(status.status, "unsupported"),
            SkillCheckPlan::Github { .. } => panic!("local_import must not trigger a GitHub download"),
        }
    }

    #[test]
    fn codex_skill_metadata_is_kept_separate_from_claude_code() {
        assert_eq!(skill_sources_file(SkillTarget::ClaudeCode), SKILL_SOURCES_CONFIG_FILE);
        assert_eq!(skill_sources_file(SkillTarget::Codex), CODEX_SKILL_SOURCES_CONFIG_FILE);
        assert_eq!(skill_sources_file(SkillTarget::Pi), PI_SKILL_SOURCES_CONFIG_FILE);
        assert_ne!(skill_sources_file(SkillTarget::ClaudeCode), skill_sources_file(SkillTarget::Codex));
        assert_ne!(skill_sources_file(SkillTarget::ClaudeCode), skill_sources_file(SkillTarget::Pi));
        assert_eq!(SkillTarget::default(), SkillTarget::ClaudeCode);
    }

    #[test]
    fn unmanaged_skill_scan_skips_managed_and_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let managed = temp.path().join("managed");
        let scatter = temp.path().join("scatter");
        fs::create_dir_all(managed.join("kept")).unwrap();
        fs::write(managed.join("kept").join(SKILL_FILE), "---\ndescription: Kept\n---\n").unwrap();
        fs::create_dir_all(scatter.join("loose")).unwrap();
        fs::write(scatter.join("loose").join(SKILL_FILE), "---\ndescription: Loose\n---\n").unwrap();
        fs::create_dir_all(scatter.join("ignored")).unwrap();
        fs::write(scatter.join("ignored").join(SKILL_FILE), "---\ndescription: Ignored\n---\n").unwrap();

        let managed_names: BTreeSet<String> = ["kept".to_string()].into_iter().collect();
        let mut ignored = BTreeSet::new();
        ignored.insert(normalize_path_key(&scatter.join("ignored")));

        let mut found = Vec::new();
        for entry in fs::read_dir(&scatter).unwrap().flatten() {
            let path = entry.path();
            let directory = entry.file_name().to_string_lossy().to_string();
            if managed_names.contains(&directory) || ignored.contains(&discovery_ignore_key(&path)) {
                continue;
            }
            if path.join(SKILL_FILE).is_file() {
                found.push(directory);
            }
        }
        assert_eq!(found, vec!["loose".to_string()]);
    }

    #[tokio::test]
    async fn github_download_requests_identity_encoding_and_retries() {
        let archive = repository_archive();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let length = stream.read(&mut request).unwrap();
                requests.push(String::from_utf8_lossy(&request[..length]).into_owned());
                if attempt == 0 {
                    stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", archive.len());
                    stream.write_all(headers.as_bytes()).unwrap();
                    stream.write_all(&archive).unwrap();
                }
            }
            requests
        });

        let bytes = download_github_archive_bytes(&format!("http://{address}/archive.zip")).await.unwrap();
        let paths = repository_skill_entries(&bytes, "skills").unwrap()
            .into_iter().map(|(skill, _)| skill.path).collect::<Vec<_>>();
        assert_eq!(paths, vec!["skills/first", "skills/second"]);
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.to_ascii_lowercase().contains("accept-encoding: identity")));
    }
}
