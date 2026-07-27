//! Local Skill discovery, GitHub repository installation, and enablement.
//!
//! Skills are installed into `~/.claude/skills/<name>`. An enabled marker is
//! represented by a `SKILL.md` file in that directory, the convention consumed
//! by Claude Code. Installation accepts GitHub repository URLs only and uses a
//! repository archive download rather than executing arbitrary Git commands.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::config::{get_app_config_dir, get_claude_skills_dir, read_json_file, write_json_file};
use crate::error::{AppError, AppResult};

const SKILL_FILE: &str = "SKILL.md";
const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
const DEFAULT_SKILL_REPOSITORY: &str = "https://github.com/anthropics/skills";
const SKILL_REPOSITORY_CONFIG_FILE: &str = "skills.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
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
}

pub fn list_skills() -> AppResult<Vec<Skill>> {
    let root = get_claude_skills_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join(SKILL_FILE);
        let disabled_file = path.join("SKILL.md.disabled");
        let description = fs::read_to_string(if skill_file.is_file() { &skill_file } else { &disabled_file })
            .ok()
            .and_then(|content| skill_description(&content))
            .unwrap_or_default();
        skills.push(Skill {
            name: entry.file_name().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            enabled: skill_file.is_file(),
            description,
        });
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(skills)
}

pub fn get_skill_repository() -> AppResult<String> {
    let path = get_app_config_dir().join(SKILL_REPOSITORY_CONFIG_FILE);
    let repository_url = read_json_file::<SkillRepositoryConfig>(&path)?
        .map(|config| config.repository_url)
        .unwrap_or_else(|| DEFAULT_SKILL_REPOSITORY.to_string());
    normalize_github_url(&repository_url)
}

pub fn set_skill_repository(url: &str) -> AppResult<String> {
    let repository_url = normalize_github_url(url)?;
    let path = get_app_config_dir().join(SKILL_REPOSITORY_CONFIG_FILE);
    write_json_file(&path, &SkillRepositoryConfig {
        repository_url: repository_url.clone(),
    })?;
    Ok(repository_url)
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

pub async fn install_github_repository_skills(url: &str, paths: &[String]) -> AppResult<Vec<Skill>> {
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
        installed.push(install_archive_skill_at(&archive, index, Some(&repo))?);
    }
    Ok(installed)
}

/// Install a skill from a public GitHub repository. The repo must include one
/// `SKILL.md`, either at its root or under a skill subdirectory.
pub async fn install_github_skill(url: &str) -> AppResult<Skill> {
    let (archive, repo) = download_github_archive(url).await?;
    install_archive(&archive, Some(&repo))
}

/// Install from a user-selected ZIP archive. Archives are protected from path
/// traversal and must contain a SKILL.md file.
pub fn install_zip_skill(path: &Path) -> AppResult<Skill> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(AppError::Config("Skill ZIP 超过 100 MB 限制".to_string()));
    }
    let archive = fs::read(path)?;
    let fallback = path.file_stem().and_then(|n| n.to_str());
    install_archive(&archive, fallback)
}

pub fn set_skill_enabled(name: &str, enabled: bool) -> AppResult<()> {
    let path = skill_path(name)?;
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

pub fn delete_skill(name: &str) -> AppResult<()> {
    let path = skill_path(name)?;
    fs::remove_dir_all(path)?;
    Ok(())
}

fn install_archive(bytes: &[u8], fallback_name: Option<&str>) -> AppResult<Skill> {
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
    install_archive_skill_at(bytes, skill_index, fallback_name)
}

fn install_archive_skill_at(bytes: &[u8], skill_index: usize, fallback_name: Option<&str>) -> AppResult<Skill> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::Config(format!("无效的 Skill ZIP 文件: {e}")))?;
    let root = get_claude_skills_dir();
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
    let target = root.join(&name);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::create_dir_all(&target)?;

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
        let destination = target.join(relative);
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
    list_skills()?.into_iter().find(|skill| skill.name == name)
        .ok_or_else(|| AppError::Other("Skill 安装后未能读取".to_string()))
}

async fn download_github_archive(url: &str) -> AppResult<(Vec<u8>, String)> {
    let (owner, repo) = parse_github_url(url)?;
    let archive_url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/HEAD");
    let response = reqwest::Client::new()
        .get(&archive_url)
        .header("User-Agent", "Claude-Switcher")
        .send()
        .await
        .map_err(|e| AppError::Other(format!("下载 GitHub Skill 失败: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::Config(format!("GitHub 下载失败（HTTP {}）", response.status())));
    }
    if response.content_length().is_some_and(|size| size > MAX_ARCHIVE_BYTES) {
        return Err(AppError::Config("GitHub Skill 压缩包超过 100 MB 限制".to_string()));
    }
    let archive = response.bytes().await
        .map_err(|e| AppError::Other(format!("读取 GitHub Skill 下载内容失败: {e}")))?;
    if archive.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(AppError::Config("GitHub Skill 压缩包超过 100 MB 限制".to_string()));
    }
    Ok((archive.to_vec(), repo.to_string()))
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

fn skill_path(name: &str) -> AppResult<PathBuf> {
    let clean = sanitize_skill_name(name)?;
    let path = get_claude_skills_dir().join(clean);
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
    let frontmatter = content.strip_prefix("---")?.split_once("---")?.0;
    frontmatter.lines().find_map(|line| {
        line.trim().strip_prefix("description:").map(|value| value.trim().trim_matches('"').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
}
