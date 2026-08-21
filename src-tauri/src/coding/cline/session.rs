//! Cline CLI/SDK session scan (`~/.cline/data/sessions` + `sessions.db`).

use std::fs;
use std::path::Path;

use chrono::DateTime;
use rusqlite::OpenFlags;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::cline::{cline_sessions_db_candidates, cline_sessions_dir};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClineSessionItem {
    pub id: String,
    pub file_path: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub project_dir: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
}

pub fn scan_cline_sessions() -> AppResult<Vec<ClineSessionItem>> {
    let mut items = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for db_path in cline_sessions_db_candidates() {
        if !db_path.is_file() {
            continue;
        }
        for item in scan_sessions_db(&db_path).unwrap_or_default() {
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    }
    for item in scan_session_json_files(&cline_sessions_dir()) {
        if seen.insert(item.id.clone()) {
            items.push(item);
        }
    }
    items.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    Ok(items)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClineMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Option<i64>,
}

pub fn load_cline_messages(path: &Path) -> AppResult<Vec<ClineMessage>> {
    if !path.exists() {
        return Err(AppError::Config(format!(
            "Cline 会话文件不存在: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&content).unwrap_or(Value::Null);
    let mut messages = extract_messages(&value);
    if messages.is_empty() && !content.trim().is_empty() {
        messages.push(ClineMessage {
            role: "system".to_string(),
            content,
            timestamp: None,
        });
    }
    Ok(messages)
}

fn scan_sessions_db(path: &Path) -> AppResult<Vec<ClineSessionItem>> {
    let conn = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| AppError::Database(format!("打开 Cline sessions.db 失败: {error}")))?;
    let mut stmt = conn
        .prepare(
            "SELECT session_id, prompt, model, cwd, workspace_root, started_at, ended_at,
                    updated_at, messages_path, metadata_json
             FROM sessions
             ORDER BY started_at DESC",
        )
        .map_err(|error| AppError::Database(format!("读取 Cline sessions 表失败: {error}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    let mut items = Vec::new();
    for row in rows.flatten() {
        let (
            session_id,
            prompt,
            model,
            cwd,
            workspace_root,
            started_at,
            ended_at,
            updated_at,
            messages_path,
            metadata_json,
        ) = row;
        if session_id.trim().is_empty() {
            continue;
        }
        let title = title_from_metadata(metadata_json.as_deref())
            .or_else(|| prompt.as_ref().and_then(|value| first_line(value)));
        let file_path = resolve_messages_path(&session_id, messages_path.as_deref());
        items.push(ClineSessionItem {
            id: session_id,
            file_path,
            title,
            model,
            project_dir: workspace_root.or(cwd),
            created_at: started_at.as_deref().and_then(parse_time_millis),
            last_active_at: updated_at
                .as_deref()
                .or(ended_at.as_deref())
                .and_then(parse_time_millis),
        });
    }
    Ok(items)
}

fn scan_session_json_files(root: &Path) -> Vec<ClineSessionItem> {
    let mut items = Vec::new();
    collect_json_files(root, &mut items, 0);
    items
}

fn collect_json_files(dir: &Path, items: &mut Vec<ClineSessionItem>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_json_files(&path, items, depth + 1);
            continue;
        }
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if !name.ends_with(".json") || name.contains("compaction") || name == "sessions.db" {
            continue;
        }
        if let Some(item) = parse_session_json_file(&path) {
            items.push(item);
        }
    }
}

fn parse_session_json_file(path: &Path) -> Option<ClineSessionItem> {
    let raw = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    if value.get("messages").is_none()
        && value.get("session_id").is_none()
        && !value.is_array()
    {
        return None;
    }
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })?;
    let title = value
        .pointer("/metadata/title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("prompt")
                .and_then(Value::as_str)
                .and_then(first_line)
        });
    let created_at = value
        .get("started_at")
        .and_then(Value::as_str)
        .and_then(parse_time_millis)
        .or_else(|| {
            path.metadata()
                .ok()
                .and_then(|meta| meta.created().ok().or_else(|| meta.modified().ok()))
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
        });
    Some(ClineSessionItem {
        id: session_id,
        file_path: path.to_string_lossy().into_owned(),
        title,
        model: value.get("model").and_then(Value::as_str).map(str::to_string),
        project_dir: value
            .get("workspace_root")
            .or_else(|| value.get("cwd"))
            .and_then(Value::as_str)
            .map(str::to_string),
        created_at,
        last_active_at: value
            .get("updated_at")
            .or_else(|| value.get("ended_at"))
            .and_then(Value::as_str)
            .and_then(parse_time_millis)
            .or(created_at),
    })
}

fn resolve_messages_path(session_id: &str, messages_path: Option<&str>) -> String {
    if let Some(path) = messages_path.map(str::trim).filter(|value| !value.is_empty()) {
        return path.to_string();
    }
    cline_sessions_dir()
        .join(format!("{session_id}.json"))
        .to_string_lossy()
        .into_owned()
}

fn title_from_metadata(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let value: Value = serde_json::from_str(raw).ok()?;
    value.get("title").and_then(Value::as_str).map(str::to_string)
}

fn first_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(80).collect())
}

fn parse_time_millis(value: &str) -> Option<i64> {
    if let Ok(millis) = value.parse::<i64>() {
        return Some(if millis < 1_000_000_000_000 {
            millis * 1000
        } else {
            millis
        });
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn extract_messages(value: &Value) -> Vec<ClineMessage> {
    let items = if let Some(arr) = value.get("messages").and_then(Value::as_array) {
        arr.as_slice()
    } else if let Some(arr) = value.as_array() {
        arr.as_slice()
    } else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    for item in items {
        let role = item
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let content = message_text(item.get("content")).or_else(|| {
            item.get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let Some(content) = content.filter(|text| !text.trim().is_empty()) else {
            continue;
        };
        let timestamp = item
            .get("ts")
            .or_else(|| item.get("timestamp"))
            .and_then(Value::as_str)
            .and_then(parse_time_millis)
            .or_else(|| item.get("ts").and_then(Value::as_i64));
        messages.push(ClineMessage {
            role,
            content,
            timestamp,
        });
    }
    messages
}

fn message_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    if let Some(arr) = content.as_array() {
        let joined = arr
            .iter()
            .filter_map(|block| {
                block.get("text").and_then(Value::as_str).or_else(|| {
                    if block.get("type").and_then(Value::as_str) == Some("text") {
                        block.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>()
            .join("\n");
        if joined.trim().is_empty() {
            return None;
        }
        return Some(joined);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_text_and_array_content() {
        let payload = json!({
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}
            ]
        });
        let messages = extract_messages(&payload);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "hi");
    }

    #[test]
    fn scans_sqlite_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sessions.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                prompt TEXT,
                model TEXT,
                cwd TEXT,
                workspace_root TEXT,
                started_at TEXT,
                ended_at TEXT,
                updated_at TEXT,
                messages_path TEXT,
                metadata_json TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (session_id, prompt, model, cwd, workspace_root, started_at, updated_at, messages_path, metadata_json)
             VALUES ('s1', 'Fix the bug', 'gpt-5.4', '/repo', '/repo', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', ?, ?)",
            rusqlite::params![
                dir.path().join("s1.json").to_string_lossy(),
                json!({"title": "Bugfix"}).to_string()
            ],
        )
        .unwrap();
        let items = scan_sessions_db(&db_path).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "s1");
        assert_eq!(items[0].title.as_deref(), Some("Bugfix"));
        assert_eq!(items[0].model.as_deref(), Some("gpt-5.4"));
    }
}
