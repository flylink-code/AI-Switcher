//! Claude Code custom agents under `~/.claude/agents/`.
//!
//! Agents are markdown files with YAML frontmatter. Known fields (`name`,
//! `description`, `tools`, `disallowedTools`, `model`, `permissionMode`,
//! `maxTurns`, `skills`, `memory`, `isolation`) are parsed and rewritten in a
//! fixed order; unrecognized lines (including nested blocks like `hooks`) are
//! preserved as-is. Enabled files use a `.md` suffix; disabled files are
//! renamed to `.md.disabled` so Claude Code stops loading them without
//! deleting content.

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
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDraft {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub isolation: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    tools: Vec<String>,
    disallowed_tools: Vec<String>,
    permission_mode: Option<String>,
    max_turns: Option<u32>,
    skills: Vec<String>,
    memory: Option<String>,
    isolation: Option<String>,
    extra_lines: Vec<String>,
}

struct AgentFileContent {
    name: String,
    description: String,
    model: Option<String>,
    tools: Vec<String>,
    disallowed_tools: Vec<String>,
    permission_mode: Option<String>,
    max_turns: Option<u32>,
    skills: Vec<String>,
    memory: Option<String>,
    isolation: Option<String>,
    extra_lines: Vec<String>,
    body: String,
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
        agents.push(agent_from_content(&path, enabled, &stem, &content));
    }
    Ok(())
}

fn agent_from_content(path: &Path, enabled: bool, stem: &str, content: &str) -> Agent {
    let meta = parse_frontmatter(content);
    let name = meta
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| stem.to_string());
    Agent {
        name,
        path: path.to_string_lossy().into_owned(),
        enabled,
        description: meta.description.unwrap_or_default(),
        body: strip_frontmatter(content),
        model: meta.model,
        tools: meta.tools,
        disallowed_tools: meta.disallowed_tools,
        permission_mode: meta.permission_mode,
        max_turns: meta.max_turns,
        skills: meta.skills,
        memory: meta.memory,
        isolation: meta.isolation,
    }
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

    let path = if let Ok(existing) = find_agent(&name) {
        PathBuf::from(existing.path)
    } else if enabled_path.exists() {
        enabled_path
    } else if disabled_path.exists() {
        disabled_path
    } else {
        enabled_path
    };
    let exists = path.exists();
    let existing_content = if exists {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };
    let existing = parse_frontmatter(&existing_content);
    let existing_body = strip_frontmatter(&existing_content);
    let file = merge_draft(draft, &name, &existing, &existing_body, exists);
    write_agent_file(&path, &file)?;
    list_agents()?
        .into_iter()
        .find(|agent| agent.name == name)
        .ok_or_else(|| AppError::Config(format!("保存后未找到 Agent: {name}")))
}

fn merge_draft(
    draft: &AgentDraft,
    name: &str,
    existing: &Frontmatter,
    existing_body: &str,
    exists: bool,
) -> AgentFileContent {
    let body = if draft.body.trim().is_empty() && exists {
        existing_body.to_string()
    } else {
        draft.body.clone()
    };
    AgentFileContent {
        name: name.to_string(),
        description: draft.description.clone(),
        model: merge_opt_string(draft.model.as_deref(), existing.model.as_deref(), exists),
        tools: merge_vec(draft.tools.clone(), &existing.tools, exists),
        disallowed_tools: merge_vec(
            draft.disallowed_tools.clone(),
            &existing.disallowed_tools,
            exists,
        ),
        permission_mode: merge_opt_string(
            draft.permission_mode.as_deref(),
            existing.permission_mode.as_deref(),
            exists,
        ),
        max_turns: match draft.max_turns {
            Some(0) => None,
            Some(value) => Some(value),
            None if exists => existing.max_turns,
            None => None,
        },
        skills: merge_vec(draft.skills.clone(), &existing.skills, exists),
        memory: merge_opt_string(draft.memory.as_deref(), existing.memory.as_deref(), exists),
        isolation: merge_opt_string(
            draft.isolation.as_deref(),
            existing.isolation.as_deref(),
            exists,
        ),
        extra_lines: if exists {
            existing.extra_lines.clone()
        } else {
            Vec::new()
        },
        body,
    }
}

fn merge_opt_string(incoming: Option<&str>, existing: Option<&str>, exists: bool) -> Option<String> {
    match incoming {
        Some(value) if value.trim().is_empty() => None,
        Some(value) => Some(value.trim().to_string()),
        None if exists => existing.map(str::to_string),
        None => None,
    }
}

fn merge_vec(incoming: Option<Vec<String>>, existing: &[String], exists: bool) -> Vec<String> {
    match incoming {
        Some(value) => value
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        None if exists => existing.to_vec(),
        None => Vec::new(),
    }
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
            &file_from_frontmatter(&agent_name, meta, strip_frontmatter(&content)),
        )?;
        installed.push(agent_from_content(
            &dest,
            true,
            &agent_name,
            &fs::read_to_string(&dest).unwrap_or_default(),
        ));
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

fn file_from_frontmatter(name: &str, meta: Frontmatter, body: String) -> AgentFileContent {
    AgentFileContent {
        name: name.to_string(),
        description: meta.description.unwrap_or_default(),
        model: meta.model,
        tools: meta.tools,
        disallowed_tools: meta.disallowed_tools,
        permission_mode: meta.permission_mode,
        max_turns: meta.max_turns,
        skills: meta.skills,
        memory: meta.memory,
        isolation: meta.isolation,
        extra_lines: meta.extra_lines,
        body,
    }
}

fn write_agent_file(path: &Path, file: &AgentFileContent) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = fs::File::create(path)?;
    writeln!(out, "---")?;
    writeln!(out, "name: {}", yaml_escape(&file.name))?;
    writeln!(
        out,
        "description: {}",
        yaml_escape(file.description.trim())
    )?;
    write_yaml_list(&mut out, "tools", &file.tools)?;
    write_yaml_list(&mut out, "disallowedTools", &file.disallowed_tools)?;
    write_yaml_opt(&mut out, "model", file.model.as_deref())?;
    write_yaml_opt(&mut out, "permissionMode", file.permission_mode.as_deref())?;
    if let Some(max_turns) = file.max_turns {
        writeln!(out, "maxTurns: {max_turns}")?;
    }
    write_yaml_list(&mut out, "skills", &file.skills)?;
    write_yaml_opt(&mut out, "memory", file.memory.as_deref())?;
    write_yaml_opt(&mut out, "isolation", file.isolation.as_deref())?;
    for line in &file.extra_lines {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "---")?;
    let body = file.body.trim();
    if !body.is_empty() {
        writeln!(out)?;
        write!(out, "{body}")?;
        if !body.ends_with('\n') {
            writeln!(out)?;
        }
    }
    Ok(())
}

fn write_yaml_opt(out: &mut fs::File, key: &str, value: Option<&str>) -> std::io::Result<()> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    writeln!(out, "{key}: {}", yaml_escape(value))
}

fn write_yaml_list(out: &mut fs::File, key: &str, items: &[String]) -> std::io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    if items.len() == 1 {
        return writeln!(out, "{key}: {}", yaml_escape(&items[0]));
    }
    writeln!(out, "{key}:")?;
    for item in items {
        writeln!(out, "  - {}", yaml_escape(item))?;
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

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_list_value(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner
        .split(',')
        .map(unquote)
        .filter(|item| !item.is_empty())
        .collect()
}

fn is_yaml_list_item(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "-" || trimmed.starts_with("- ")
}

fn is_indented(line: &str) -> bool {
    if line.trim().is_empty() {
        return true;
    }
    line.starts_with(' ') || line.starts_with('\t')
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
    let lines: Vec<&str> = block.lines().collect();
    let mut meta = Frontmatter::default();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            if !trimmed_line.is_empty() {
                meta.extra_lines.push(line.to_string());
            }
            index += 1;
            continue;
        }
        if is_indented(line) {
            meta.extra_lines.push(line.to_string());
            index += 1;
            continue;
        }
        let Some((key, rest_value)) = trimmed_line.split_once(':') else {
            meta.extra_lines.push(line.to_string());
            index += 1;
            continue;
        };
        let key = key.trim();
        let rest_value = rest_value.trim();
        index += 1;
        match key {
            "name" => meta.name = Some(unquote(rest_value)).filter(|value| !value.is_empty()),
            "description" => {
                meta.description = Some(unquote(rest_value));
            }
            "tools" => {
                meta.tools = take_list(&lines, &mut index, rest_value);
            }
            "disallowedTools" | "disallowed_tools" => {
                meta.disallowed_tools = take_list(&lines, &mut index, rest_value);
            }
            "skills" => {
                meta.skills = take_list(&lines, &mut index, rest_value);
            }
            "model" => {
                meta.model = Some(unquote(rest_value)).filter(|value| !value.is_empty());
            }
            "permissionMode" | "permission_mode" => {
                meta.permission_mode = Some(unquote(rest_value)).filter(|value| !value.is_empty());
            }
            "maxTurns" | "max_turns" => {
                meta.max_turns = unquote(rest_value).parse().ok();
            }
            "memory" => {
                meta.memory = Some(unquote(rest_value)).filter(|value| !value.is_empty());
            }
            "isolation" => {
                meta.isolation = Some(unquote(rest_value)).filter(|value| !value.is_empty());
            }
            _ => {
                meta.extra_lines.push(line.to_string());
                while index < lines.len() && is_indented(lines[index]) {
                    meta.extra_lines.push(lines[index].to_string());
                    index += 1;
                }
            }
        }
    }
    meta
}

fn take_list(lines: &[&str], index: &mut usize, inline: &str) -> Vec<String> {
    let mut items = parse_list_value(inline);
    while *index < lines.len() && is_yaml_list_item(lines[*index]) {
        let item = lines[*index].trim().trim_start_matches('-').trim();
        if !item.is_empty() {
            items.push(unquote(item));
        }
        *index += 1;
    }
    items
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

    fn simple_file(name: &str, description: &str, body: &str) -> AgentFileContent {
        AgentFileContent {
            name: name.to_string(),
            description: description.to_string(),
            model: None,
            tools: Vec::new(),
            disallowed_tools: Vec::new(),
            permission_mode: None,
            max_turns: None,
            skills: Vec::new(),
            memory: None,
            isolation: None,
            extra_lines: Vec::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn parse_and_roundtrip_agent_markdown() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reviewer.md");
        write_agent_file(&path, &simple_file("reviewer", "Reviews PRs", "Be thorough.")).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let meta = parse_frontmatter(&content);
        assert_eq!(meta.name.as_deref(), Some("reviewer"));
        assert_eq!(meta.description.as_deref(), Some("Reviews PRs"));
        assert!(strip_frontmatter(&content).contains("Be thorough."));
    }

    #[test]
    fn roundtrip_preserves_tools_model_and_unknown_keys() {
        let original = "---\n\
name: reviewer\n\
description: Reviews PRs\n\
tools:\n\
  - Read\n\
  - Grep\n\
model: sonnet\n\
permissionMode: plan\n\
maxTurns: 8\n\
color: cyan\n\
hooks:\n\
  PreToolUse:\n\
    - matcher: Bash\n\
---\n\n\
Be thorough.\n";
        let meta = parse_frontmatter(original);
        assert_eq!(meta.tools, vec!["Read".to_string(), "Grep".to_string()]);
        assert_eq!(meta.model.as_deref(), Some("sonnet"));
        assert_eq!(meta.permission_mode.as_deref(), Some("plan"));
        assert_eq!(meta.max_turns, Some(8));
        assert!(meta.extra_lines.iter().any(|line| line.contains("color: cyan")));
        assert!(meta.extra_lines.iter().any(|line| line.contains("hooks:")));
        assert!(meta
            .extra_lines
            .iter()
            .any(|line| line.contains("PreToolUse:")));

        let dir = tempdir().unwrap();
        let path = dir.path().join("reviewer.md");
        write_agent_file(
            &path,
            &file_from_frontmatter("reviewer", meta, strip_frontmatter(original)),
        )
        .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        let again = parse_frontmatter(&written);
        assert_eq!(again.tools, vec!["Read".to_string(), "Grep".to_string()]);
        assert_eq!(again.model.as_deref(), Some("sonnet"));
        assert_eq!(again.permission_mode.as_deref(), Some("plan"));
        assert_eq!(again.max_turns, Some(8));
        assert!(written.contains("color: cyan"));
        assert!(written.contains("hooks:"));
        assert!(written.contains("PreToolUse:"));
        assert!(written.contains("matcher: Bash"));
        assert!(strip_frontmatter(&written).contains("Be thorough."));
    }

    #[test]
    fn parse_inline_tools_list() {
        let content = "---\nname: explorer\ndescription: Explore\ntools: Read, Grep, Glob\n---\n\nGo.\n";
        let meta = parse_frontmatter(content);
        assert_eq!(
            meta.tools,
            vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]
        );
    }

    #[test]
    fn merge_draft_preserves_unspecified_fields() {
        let existing = Frontmatter {
            name: Some("reviewer".into()),
            description: Some("old".into()),
            model: Some("sonnet".into()),
            tools: vec!["Read".into()],
            extra_lines: vec!["color: cyan".into()],
            ..Frontmatter::default()
        };
        let draft = AgentDraft {
            name: "reviewer".into(),
            description: "new desc".into(),
            body: String::new(),
            model: None,
            tools: None,
            disallowed_tools: None,
            permission_mode: None,
            max_turns: None,
            skills: None,
            memory: None,
            isolation: None,
        };
        let merged = merge_draft(&draft, "reviewer", &existing, "Keep body.", true);
        assert_eq!(merged.description, "new desc");
        assert_eq!(merged.model.as_deref(), Some("sonnet"));
        assert_eq!(merged.tools, vec!["Read".to_string()]);
        assert_eq!(merged.body, "Keep body.");
        assert_eq!(merged.extra_lines, vec!["color: cyan".to_string()]);
    }
}
