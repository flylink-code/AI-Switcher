//! Pi 会话目录扫描与解析 (`~/.pi/agent/sessions/`)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::coding::pi::config::get_pi_dir;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiSessionItem {
    pub id: String,
    pub file_path: String,
    pub title: Option<String>,
    pub created_at: Option<u64>,
    pub updated_at: Option<u64>,
    pub token_count: Option<u64>,
    pub model: Option<String>,
    pub provider: Option<String>,
    /// Working directory from the session header (`cwd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

pub fn get_pi_sessions_dir() -> PathBuf {
    get_pi_dir().join("sessions")
}

/// 遍历并扫描 Pi 会话文件
pub fn scan_pi_sessions_sync() -> AppResult<Vec<PiSessionItem>> {
    let sessions_dir = get_pi_sessions_dir();
    if !sessions_dir.exists() || !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut list = Vec::new();
    let entries = fs::read_dir(&sessions_dir)
        .map_err(|e| AppError::Io(format!("读取 sessions 目录失败: {e}")))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "json" || ext == "jsonl" {
                    if let Some(item) = parse_session_file(&path) {
                        list.push(item);
                    }
                }
            }
        } else if path.is_dir() {
            // 支持一层子目录扫描
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub in sub_entries.flatten() {
                    let sub_path = sub.path();
                    if sub_path.is_file() {
                        if let Some(ext) = sub_path.extension() {
                            if ext == "json" || ext == "jsonl" {
                                if let Some(item) = parse_session_file(&sub_path) {
                                    list.push(item);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 按最后更新时间逆序排序
    list.sort_by_key(|item| std::cmp::Reverse(item.updated_at));
    Ok(list)
}

fn parse_session_file(path: &Path) -> Option<PiSessionItem> {
    let metadata = fs::metadata(path).ok()?;
    let updated_at = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let created_at = metadata
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let file_stem = path.file_stem()?.to_string_lossy().to_string();
    let file_path = path.to_string_lossy().to_string();

    let content = fs::read_to_string(path).ok()?;
    let mut title: Option<String> = None;
    let mut model: Option<String> = None;
    let mut provider: Option<String> = None;
    let mut token_count: Option<u64> = None;
    let mut cwd: Option<String> = None;

    if path.extension().is_some_and(|e| e == "json") {
        if let Ok(val) = serde_json::from_str::<Value>(&content) {
            if let Some(t) = val.get("title").and_then(Value::as_str) {
                title = Some(t.to_string());
            } else if let Some(prompt) = val.get("prompt").and_then(Value::as_str) {
                title = Some(prompt.chars().take(60).collect());
            }
            model = val
                .get("model")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            provider = val
                .get("provider")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            token_count = val
                .get("tokenCount")
                .and_then(Value::as_u64)
                .or_else(|| val.get("tokens").and_then(Value::as_u64));
            cwd = string_field(&val, "cwd").map(|value| normalize_pi_cwd(&value));
        }
    } else if path.extension().is_some_and(|e| e == "jsonl") {
        // jsonl: 首行通常为 session 元数据，次行为消息
        for line in content.lines().take(5) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                if title.is_none() {
                    if let Some(t) = val.get("title").and_then(Value::as_str) {
                        title = Some(t.to_string());
                    } else if let Some(text) = val.get("content").and_then(Value::as_str) {
                        title = Some(text.chars().take(60).collect());
                    } else if let Some(text) = val.get("text").and_then(Value::as_str) {
                        title = Some(text.chars().take(60).collect());
                    }
                }
                if model.is_none() {
                    model = val
                        .get("model")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                }
                if provider.is_none() {
                    provider = val
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                }
                if cwd.is_none() {
                    cwd = string_field(&val, "cwd").map(|value| normalize_pi_cwd(&value));
                }
            }
        }
    }

    if title.is_none() {
        title = Some(file_stem.clone());
    }

    Some(PiSessionItem {
        id: file_stem,
        file_path,
        title,
        created_at,
        updated_at,
        token_count,
        model,
        provider,
        cwd: cwd.filter(|value| !value.is_empty()),
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn normalize_pi_cwd(raw: &str) -> String {
    let cwd = raw.trim().trim_start_matches(r"\\?\").trim();
    #[cfg(windows)]
    {
        cwd.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        cwd.to_string()
    }
}

/// 读取单个会话文件完整内容文本（以供格式化预览/导出）
pub fn read_pi_session_file_content(file_path: &str) -> AppResult<String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(AppError::Config(format!("会话文件不存在: {file_path}")));
    }
    fs::read_to_string(path).map_err(|e| AppError::Io(format!("读取会话文件失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_header_cwd_becomes_the_session_directory() {
        let dir = std::env::temp_dir().join(format!("aisw-pi-cwd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"id\":\"abc\",\"cwd\":\"E:\\\\W-AI_WorkSpace\\\\demo\"}\n{\"type\":\"message\",\"content\":\"hi\"}\n",
        )
        .unwrap();
        let item = parse_session_file(&path).unwrap();
        assert_eq!(
            item.cwd.as_deref(),
            Some(r"E:\W-AI_WorkSpace\demo")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
