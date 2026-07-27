//! Read-only local session discovery for Claude Code and Claude Desktop.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config;
use crate::error::{AppError, AppResult};

const SEARCH_RESULT_LIMIT: usize = 200;
const SUMMARY_LIMIT: usize = 160;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionProvider {
    ClaudeCode,
    ClaudeDesktop,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProviderStatus {
    pub provider: SessionProvider,
    pub status: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider: SessionProvider,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    pub source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionScanResult {
    pub sessions: Vec<SessionMeta>,
    pub providers: Vec<SessionProviderStatus>,
}

pub fn scan_sessions(provider: Option<SessionProvider>) -> AppResult<SessionScanResult> {
    let mut sessions = Vec::new();
    let mut providers = Vec::new();

    if provider.is_none() || provider == Some(SessionProvider::ClaudeCode) {
        let (mut code_sessions, status) = scan_claude_code_sessions()?;
        sessions.append(&mut code_sessions);
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::ClaudeDesktop) {
        providers.push(claude_desktop_status());
    }

    sessions.sort_by(|left, right| {
        let left_time = left.last_active_at.or(left.created_at).unwrap_or(0);
        let right_time = right.last_active_at.or(right.created_at).unwrap_or(0);
        right_time.cmp(&left_time)
    });

    Ok(SessionScanResult {
        sessions,
        providers,
    })
}

pub fn search_session_contents(
    query: &str,
    provider: Option<SessionProvider>,
    limit: usize,
) -> AppResult<SessionScanResult> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err(AppError::Config("搜索内容不能为空".to_string()));
    }

    let mut result = scan_sessions(provider)?;
    let limit = clamp_search_limit(limit);
    result.sessions.retain(|session| {
        if session_metadata_contains(session, &query) {
            return true;
        }
        file_contains(&session.source_path, &query).unwrap_or(false)
    });
    result.sessions.truncate(limit);
    Ok(result)
}

pub fn load_session_messages(
    provider: SessionProvider,
    source_path: &str,
) -> AppResult<Vec<SessionMessage>> {
    match provider {
        SessionProvider::ClaudeCode => {
            let root = claude_code_session_root();
            let source = validate_session_path_in_root(&root, Path::new(source_path))?;
            load_claude_code_messages(&source)
        }
        SessionProvider::ClaudeDesktop => Err(AppError::Config(
            "Claude Desktop 未公开稳定的本地会话格式，当前版本不读取其私有缓存".to_string(),
        )),
    }
}

fn scan_claude_code_sessions() -> AppResult<(Vec<SessionMeta>, SessionProviderStatus)> {
    let root = claude_code_session_root();
    if !root.is_dir() {
        return Ok((
            Vec::new(),
            SessionProviderStatus {
                provider: SessionProvider::ClaudeCode,
                status: "not_found".to_string(),
                detail: "未发现 Claude Code 本地会话目录".to_string(),
                root_path: Some(root.display().to_string()),
            },
        ));
    }

    let mut paths = Vec::new();
    collect_jsonl_files(&root, &mut paths)?;
    let mut sessions = Vec::new();
    for path in paths {
        let path = match validate_session_path_in_root(&root, &path) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("跳过会话根目录外的文件 {}: {error}", path.display());
                continue;
            }
        };
        match parse_claude_code_session(&path) {
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {}
            Err(error) => log::warn!("跳过无法解析的 Claude Code 会话 {}: {error}", path.display()),
        }
    }

    Ok((
        sessions,
        SessionProviderStatus {
            provider: SessionProvider::ClaudeCode,
            status: "available".to_string(),
            detail: "Claude Code 本地会话可用".to_string(),
            root_path: Some(root.display().to_string()),
        },
    ))
}

fn claude_desktop_status() -> SessionProviderStatus {
    let candidates = claude_desktop_roots();
    let detected = candidates.iter().find(|path| path.exists());
    SessionProviderStatus {
        provider: SessionProvider::ClaudeDesktop,
        status: if detected.is_some() {
            "unsupported_format"
        } else {
            "not_found"
        }
        .to_string(),
        detail: if detected.is_some() {
            "已检测到 Claude Desktop；其历史会话没有公开稳定的本地格式，当前仅提供官方入口"
        } else {
            "未检测到 Claude Desktop 数据目录；当前仅提供官方入口"
        }
        .to_string(),
        root_path: detected.map(|path| path.display().to_string()),
    }
}

fn claude_code_session_root() -> PathBuf {
    config::get_claude_config_dir().join("projects")
}

fn claude_desktop_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(value) = std::env::var_os(variable) {
            roots.push(PathBuf::from(value).join("Claude-3p"));
        }
    }
    roots
}

fn collect_jsonl_files(directory: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    let entries = fs::read_dir(directory)
        .map_err(|error| AppError::Io(format!("读取会话目录 {} 失败: {error}", directory.display())))?;
    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                log::warn!("跳过无法读取的会话目录项: {error}");
                continue;
            }
        };
        let path = entry.path();
        if entry
            .file_type()
            .map(|file_type| file_type.is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        if path.is_dir() {
            if let Err(error) = collect_jsonl_files(&path, files) {
                log::warn!("跳过无法扫描的会话子目录 {}: {error}", path.display());
            }
            continue;
        }
        let is_jsonl = path.extension().and_then(|value| value.to_str()) == Some("jsonl");
        let is_agent = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("agent-"));
        if is_jsonl && !is_agent {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_claude_code_session(path: &Path) -> AppResult<Option<SessionMeta>> {
    let file = File::open(path)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", path.display())))?;
    let reader = BufReader::new(file);
    let mut session_id = None;
    let mut project_dir = None;
    let mut created_at = None;
    let mut first_user_message = None;

    for line in reader.lines().take(60) {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        session_id = session_id.or_else(|| {
            value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        project_dir = project_dir.or_else(|| {
            value
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        created_at = created_at.or_else(|| value.get("timestamp").and_then(parse_timestamp));

        if first_user_message.is_none() && message_role(&value).as_deref() == Some("user") {
            let content = message_content(&value);
            let trimmed = content.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('/') {
                first_user_message = Some(truncate(trimmed, SUMMARY_LIMIT));
            }
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string)
    });
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let last_active_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);

    Ok(Some(SessionMeta {
        provider: SessionProvider::ClaudeCode,
        session_id: session_id.clone(),
        title: first_user_message.clone(),
        summary: first_user_message,
        project_dir,
        created_at,
        last_active_at,
        source_path: path.display().to_string(),
        resume_command: resume_command(&session_id),
    }))
}

fn load_claude_code_messages(path: &Path) -> AppResult<Vec<SessionMessage>> {
    let file = File::open(path)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", path.display())))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(role) = message_role(&value) else {
            continue;
        };
        let content = message_content(&value);
        if content.trim().is_empty() {
            continue;
        }
        messages.push(SessionMessage {
            role,
            content,
            timestamp: value.get("timestamp").and_then(parse_timestamp),
        });
    }

    Ok(messages)
}

fn message_role(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    let mut role = message.get("role")?.as_str()?.to_string();
    if role == "user" {
        if let Some(items) = message.get("content").and_then(Value::as_array) {
            if !items.is_empty()
                && items.iter().all(|item| {
                    item.get("type").and_then(Value::as_str) == Some("tool_result")
                })
            {
                role = "tool".to_string();
            }
        }
    }
    Some(role)
}

fn message_content(value: &Value) -> String {
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .map(extract_text)
        .unwrap_or_default()
}

fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(content) = map.get("content") {
                return extract_text(content);
            }
            if map.get("type").and_then(Value::as_str) == Some("tool_use") {
                let name = map.get("name").and_then(Value::as_str).unwrap_or("tool");
                return format!("[tool: {name}]");
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn parse_timestamp(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number < 10_000_000_000 {
            number * 1000
        } else {
            number
        });
    }
    value
        .as_str()
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|value| value.timestamp_millis())
}

fn session_metadata_contains(session: &SessionMeta, query: &str) -> bool {
    [
        Some(session.session_id.as_str()),
        session.title.as_deref(),
        session.summary.as_deref(),
        session.project_dir.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(query))
}

fn file_contains(path: &str, query: &str) -> AppResult<bool> {
    let root = claude_code_session_root();
    let source = validate_session_path_in_root(&root, Path::new(path))?;
    let file = File::open(&source)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", source.display())))?;
    for line in BufReader::new(file).lines() {
        if line
            .ok()
            .is_some_and(|value| value.to_lowercase().contains(query))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_session_path_in_root(root: &Path, source: &Path) -> AppResult<PathBuf> {
    let root = root
        .canonicalize()
        .map_err(|error| AppError::Path(format!("无法解析会话根目录 {}: {error}", root.display())))?;
    let source = source
        .canonicalize()
        .map_err(|error| AppError::Path(format!("无法解析会话文件 {}: {error}", source.display())))?;
    if !source.starts_with(&root)
        || source.extension().and_then(|value| value.to_str()) != Some("jsonl")
    {
        return Err(AppError::Path(format!(
            "会话文件不在允许的目录内: {}",
            source.display()
        )));
    }
    Ok(source)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn clamp_search_limit(limit: usize) -> usize {
    limit.clamp(1, SEARCH_RESULT_LIMIT)
}

fn resume_command(session_id: &str) -> Option<String> {
    if !session_id.is_empty()
        && session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Some(format!("claude --resume {session_id}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_session(path: &Path) {
        let mut file = File::create(path).unwrap();
        writeln!(
            file,
            r#"{{"sessionId":"session-1","cwd":"C:\\work","timestamp":"2026-03-01T12:00:00Z","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(file, "not-json").unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-03-01T12:00:01Z","message":{{"role":"assistant","content":[{{"type":"text","text":"world"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-03-01T12:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"done"}}]}}}}"#
        )
        .unwrap();
    }

    #[test]
    fn parses_metadata_and_tolerates_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-1.jsonl");
        write_session(&path);
        let session = parse_claude_code_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id, "session-1");
        assert_eq!(session.project_dir.as_deref(), Some("C:\\work"));
        assert_eq!(session.summary.as_deref(), Some("hello"));
    }

    #[test]
    fn loads_normalized_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-1.jsonl");
        write_session(&path);
        let messages = load_claude_code_messages(&path).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "world");
        assert_eq!(messages[2].role, "tool");
    }

    #[test]
    fn rejects_paths_outside_the_session_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("session.jsonl");
        File::create(&source).unwrap();
        let error = validate_session_path_in_root(root.path(), &source).unwrap_err();
        assert!(error.to_string().contains("不在允许的目录内"));
    }

    #[test]
    fn ignores_agent_session_files() {
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("agent-child.jsonl")).unwrap();
        File::create(dir.path().join("parent.jsonl")).unwrap();
        let mut files = Vec::new();
        collect_jsonl_files(dir.path(), &mut files).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "parent.jsonl");
    }

    #[test]
    fn empty_session_keeps_basic_file_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty-session.jsonl");
        File::create(&path).unwrap();
        let session = parse_claude_code_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id, "empty-session");
        assert!(session.summary.is_none());
        assert!(load_claude_code_messages(&path).unwrap().is_empty());
    }

    #[test]
    fn full_text_search_limit_is_bounded() {
        assert_eq!(clamp_search_limit(0), 1);
        assert_eq!(clamp_search_limit(20), 20);
        assert_eq!(clamp_search_limit(usize::MAX), 200);
    }

    #[test]
    fn resume_command_rejects_shell_metacharacters() {
        assert_eq!(
            resume_command("safe-session_1").as_deref(),
            Some("claude --resume safe-session_1")
        );
        assert!(resume_command("unsafe & whoami").is_none());
    }
}
