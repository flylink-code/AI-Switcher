//! Claude Code custom agents under `~/.claude/agents/`.
//!
//! Agents are markdown files with YAML frontmatter (`name`, `description`).
//! Enabled files use a `.md` suffix; disabled files are renamed to `.md.disabled`
//! so Claude Code stops loading them without deleting content.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::config::get_claude_agents_dir;
use crate::error::{AppError, AppResult};

const MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const DISABLED_SUFFIX: &str = ".md.disabled";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDraft {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
}

pub fn list_agents() -> AppResult<Vec<Agent>> {
    let root = get_claude_agents_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut agents = Vec::new();
    collect_agents(&root, &mut agents)?;
    agents.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(agents)
}

fn collect_agents(dir: &Path, agents: &mut Vec<Agent>) -> AppResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_agents(&path, agents)?;
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let (enabled, stem) = if file_name.ends_with(DISABLED_SUFFIX) {
            (
                false,
                file_name
                    .strip_suffix(DISABLED_SUFFIX)
                    .unwrap_or(file_name)
                    .to_string(),
            )
        } else if file_name.ends_with(".md") {
            (
                true,
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or(file_name)
                    .to_string(),
            )
        } else {
            continue;
        };
        let content = fs::read_to_string(&path).unwrap_or_default();
        let meta = parse_frontmatter(&content);
        let name = meta
            .name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(stem);
        agents.push(Agent {
            name,
            path: path.to_string_lossy().into_owned(),
            enabled,
            description: meta.description.unwrap_or_default(),
        });
    }
    Ok(())
}

pub fn set_agent_enabled(name: &str, enabled: bool) -> AppResult<()> {
    let agent = find_agent(name)?;
    let path = PathBuf::from(&agent.path);
    if enabled == agent.enabled {
        return Ok(());
    }
    let target = if enabled {
        enabled_path_for(&path)?
    } else {
        disabled_path_for(&path)?
    };
    if target.exists() {
        return Err(AppError::Config(format!(
            "无法切换 Agent 状态：目标文件已存在 {}",
            target.display()
        )));
    }
    fs::rename(&path, &target)?;
    Ok(())
}

pub fn delete_agent(name: &str) -> AppResult<()> {
    let agent = find_agent(name)?;
    fs::remove_file(&agent.path)?;
    Ok(())
}

pub fn save_agent(draft: &AgentDraft) -> AppResult<Agent> {
    let name = sanitize_agent_name(&draft.name)?;
    let root = get_claude_agents_dir();
    fs::create_dir_all(&root)?;
    let enabled_path = root.join(format!("{name}.md"));
    let disabled_path = root.join(format!("{name}{DISABLED_SUFFIX}"));

    let (path, preserve_body) = if let Ok(existing) = find_agent(&name) {
        (PathBuf::from(existing.path), draft.body.trim().is_empty())
    } else if enabled_path.exists() {
        (enabled_path, draft.body.trim().is_empty())
    } else if disabled_path.exists() {
        (disabled_path, draft.body.trim().is_empty())
    } else {
        (enabled_path, false)
    };

    let body = if preserve_body {
        let existing = fs::read_to_string(&path).unwrap_or_default();
        strip_frontmatter(&existing)
    } else {
        draft.body.clone()
    };
    write_agent_file(&path, &name, &draft.description, &body)?;
    list_agents()?
        .into_iter()
        .find(|agent| agent.name == name)
        .ok_or_else(|| AppError::Config(format!("保存后未找到 Agent: {name}")))
}

pub fn install_zip_agent(path: &Path) -> AppResult<Vec<Agent>> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(AppError::Config("Agent ZIP 超过 50 MB 限制".to_string()));
    }
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Config(format!("无法读取 Agent ZIP: {error}")))?;
    let root = get_claude_agents_dir();
    fs::create_dir_all(&root)?;

    let mut installed = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Config(format!("读取 ZIP 条目失败: {error}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Some(file_name) = name.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".md") || file_name.ends_with(DISABLED_SUFFIX) {
            continue;
        }
        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .map_err(|error| AppError::Io(format!("读取 Agent 内容失败: {error}")))?;
        let meta = parse_frontmatter(&content);
        let stem = Path::new(file_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("agent");
        let agent_name = meta
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(stem);
        let agent_name = sanitize_agent_name(agent_name)?;
        let dest = root.join(format!("{agent_name}.md"));
        write_agent_file(
            &dest,
            &agent_name,
            meta.description.as_deref().unwrap_or(""),
            strip_frontmatter(&content).trim(),
        )?;
        installed.push(Agent {
            name: agent_name,
            path: dest.to_string_lossy().into_owned(),
            enabled: true,
            description: meta.description.unwrap_or_default(),
        });
    }
    if installed.is_empty() {
        return Err(AppError::Config(
            "ZIP 中未找到带 frontmatter 的 Agent Markdown 文件".to_string(),
        ));
    }
    Ok(installed)
}

fn find_agent(name: &str) -> AppResult<Agent> {
    list_agents()?
        .into_iter()
        .find(|agent| agent.name == name)
        .ok_or_else(|| AppError::Config(format!("Agent 不存在: {name}")))
}

fn enabled_path_for(path: &Path) -> AppResult<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Config("无效的 Agent 路径".to_string()))?;
    if let Some(stem) = file_name.strip_suffix(DISABLED_SUFFIX) {
        return Ok(path.with_file_name(format!("{stem}.md")));
    }
    Ok(path.to_path_buf())
}

fn disabled_path_for(path: &Path) -> AppResult<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Config("无效的 Agent 路径".to_string()))?;
    if file_name.ends_with(DISABLED_SUFFIX) {
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::Config("无效的 Agent 文件名".to_string()))?;
    Ok(path.with_file_name(format!("{stem}{DISABLED_SUFFIX}")))
}

fn write_agent_file(path: &Path, name: &str, description: &str, body: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let description = description.trim();
    let body = body.trim();
    let mut file = fs::File::create(path)?;
    writeln!(file, "---")?;
    writeln!(file, "name: {name}")?;
    writeln!(file, "description: {}", yaml_escape(description))?;
    writeln!(file, "---")?;
    if !body.is_empty() {
        writeln!(file)?;
        write!(file, "{body}")?;
        if !body.ends_with('\n') {
            writeln!(file)?;
        }
    }
    Ok(())
}

fn sanitize_agent_name(raw: &str) -> AppResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::Config("Agent 名称不能为空".to_string()));
    }
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(AppError::Config("Agent 名称包含非法字符".to_string()));
    }
    Ok(name.to_string())
}

fn yaml_escape(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value.contains(':') || value.contains('#') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(content: &str) -> Frontmatter {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Frontmatter::default();
    }
    let rest = trimmed.trim_start_matches("---");
    let Some((block, _)) = rest.split_once("\n---") else {
        return Frontmatter::default();
    };
    let mut meta = Frontmatter::default();
    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        match key {
            "name" => meta.name = Some(value),
            "description" => meta.description = Some(value),
            _ => {}
        }
    }
    meta
}

fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }
    let rest = trimmed.trim_start_matches("---");
    if let Some((_, body)) = rest.split_once("\n---") {
        return body.trim_start_matches(['\r', '\n']).to_string();
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_and_roundtrip_agent_markdown() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reviewer.md");
        write_agent_file(&path, "reviewer", "Reviews PRs", "Be thorough.").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let meta = parse_frontmatter(&content);
        assert_eq!(meta.name.as_deref(), Some("reviewer"));
        assert_eq!(meta.description.as_deref(), Some("Reviews PRs"));
        assert!(strip_frontmatter(&content).contains("Be thorough."));
    }
}
