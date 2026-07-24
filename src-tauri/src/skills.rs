//! Local Skill discovery, GitHub repository installation, and enablement.
//!
//! Skills are installed into `~/.claude/skills/<name>`. An enabled marker is
//! represented by a `SKILL.md` file in that directory, the convention consumed
//! by Claude Code. Installation accepts GitHub repository URLs only and uses a
//! repository archive download rather than executing arbitrary Git commands.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::ZipArchive;

use crate::config::get_claude_skills_dir;
use crate::error::{AppError, AppResult};

const SKILL_FILE: &str = "SKILL.md";
const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub description: String,
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

/// Install a skill from a public GitHub repository. The repo must include one
/// `SKILL.md`, either at its root or under a skill subdirectory.
pub async fn install_github_skill(url: &str) -> AppResult<Skill> {
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
    let archive = response
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("读取 GitHub Skill 下载内容失败: {e}")))?;
    if archive.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(AppError::Config("GitHub Skill 压缩包超过 100 MB 限制".to_string()));
    }
    install_archive(&archive, Some(repo))
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
    let root = get_claude_skills_dir();
    fs::create_dir_all(&root)?;

    let skill_index = (0..archive.len()).find(|&index| {
        archive.by_index(index).ok().is_some_and(|entry| {
            entry.enclosed_name().is_some_and(|path| {
                path.file_name().and_then(|name| name.to_str()) == Some(SKILL_FILE)
            })
        })
    }).ok_or_else(|| AppError::Config("ZIP 中未找到 SKILL.md".to_string()))?;

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
    let frontmatter = content.strip_prefix("---")?.splitn(2, "---").nth(1)?;
    frontmatter.lines().find_map(|line| {
        line.trim().strip_prefix("description:").map(|value| value.trim().trim_matches('"').to_string())
    })
}
