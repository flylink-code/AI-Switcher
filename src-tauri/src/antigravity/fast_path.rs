//! Claude Code background-request short-circuit and Flash degrade.
//!
//! Inspired by Free Claude Code optimization handlers and Antigravity-Manager
//! background-task detection. Behavior is reimplemented independently.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::model_catalog;

fn settings_slot() -> &'static Mutex<FastPathSettings> {
    static SLOT: OnceLock<Mutex<FastPathSettings>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(FastPathSettings::default()))
}

pub fn current_settings() -> FastPathSettings {
    settings_slot()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn set_current_settings(settings: FastPathSettings) {
    if let Ok(mut guard) = settings_slot().lock() {
        *guard = settings;
    }
}

/// User-facing toggles. Quota / title / prefix default on; suggestion and
/// filepath mocks stay off to avoid false positives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FastPathSettings {
    pub quota_mock: bool,
    pub title_skip: bool,
    pub prefix_detect: bool,
    pub suggestion_skip: bool,
    pub filepath_mock: bool,
    pub flash_degrade: bool,
}

impl Default for FastPathSettings {
    fn default() -> Self {
        Self {
            quota_mock: true,
            title_skip: true,
            prefix_detect: true,
            suggestion_skip: false,
            filepath_mock: false,
            flash_degrade: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTask {
    Title,
    Summary,
    Compression,
    Suggestion,
    Probe,
}

#[derive(Debug, Clone)]
pub struct LocalReply {
    pub text: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub fn try_short_circuit(body: &Value, settings: &FastPathSettings) -> Option<LocalReply> {
    if settings.quota_mock && is_quota_check(body) {
        return Some(LocalReply {
            text: "Quota check passed.".into(),
            input_tokens: 10,
            output_tokens: 5,
        });
    }
    if settings.prefix_detect {
        if let Some(command) = prefix_command(body) {
            return Some(LocalReply {
                text: extract_command_prefix(&command),
                input_tokens: 100,
                output_tokens: 5,
            });
        }
    }
    if settings.title_skip && is_title_generation(body) {
        return Some(LocalReply {
            text: "Conversation".into(),
            input_tokens: 100,
            output_tokens: 5,
        });
    }
    if settings.suggestion_skip && is_suggestion_mode(body) {
        return Some(LocalReply {
            text: String::new(),
            input_tokens: 100,
            output_tokens: 1,
        });
    }
    if settings.filepath_mock {
        if let Some((command, output)) = filepath_extract(body) {
            return Some(LocalReply {
                text: extract_filepaths(&command, &output),
                input_tokens: 80,
                output_tokens: 8,
            });
        }
    }
    None
}

pub fn detect_background_task(body: &Value) -> Option<BackgroundTask> {
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty())
    {
        return None;
    }
    let last = last_user_text(body)?;
    if last.len() > 800 {
        return None;
    }
    let preview: String = last.chars().take(500).collect();
    let lower = preview.to_ascii_lowercase();
    if TITLE_KEYWORDS
        .iter()
        .any(|kw| preview.contains(kw) || lower.contains(&kw.to_ascii_lowercase()))
    {
        return Some(BackgroundTask::Title);
    }
    if SUMMARY_KEYWORDS.iter().any(|kw| preview.contains(*kw)) {
        if preview.contains("in under 50 characters") {
            return Some(BackgroundTask::Summary);
        }
        return Some(BackgroundTask::Compression);
    }
    if SUGGESTION_KEYWORDS
        .iter()
        .any(|kw| lower.contains(&kw.to_ascii_lowercase()))
    {
        return Some(BackgroundTask::Suggestion);
    }
    if PROBE_KEYWORDS.iter().any(|kw| lower.contains(*kw)) {
        return Some(BackgroundTask::Probe);
    }
    None
}

pub fn flash_degrade_model() -> String {
    model_catalog::preferred_gemini_flash_low()
}

pub fn anthropic_message_json(model: &str, reply: &LocalReply) -> Value {
    json!({
        "id": format!("msg_{}", Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{ "type": "text", "text": reply.text }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": reply.input_tokens,
            "output_tokens": reply.output_tokens,
        }
    })
}

pub fn anthropic_message_sse(model: &str, reply: &LocalReply) -> String {
    let id = format!("msg_{}", Uuid::new_v4().simple());
    let start = json!({
        "type": "message_start",
        "message": {
            "id": id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "usage": { "input_tokens": reply.input_tokens, "output_tokens": 0 }
        }
    });
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "text", "text": "" }
    });
    let delta = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": { "type": "text_delta", "text": reply.text }
    });
    let block_stop = json!({ "type": "content_block_stop", "index": 0 });
    let message_delta = json!({
        "type": "message_delta",
        "delta": { "stop_reason": "end_turn", "stop_sequence": null },
        "usage": { "output_tokens": reply.output_tokens }
    });
    let stop = json!({ "type": "message_stop" });
    [
        sse_event(&start),
        sse_event(&block_start),
        sse_event(&delta),
        sse_event(&block_stop),
        sse_event(&message_delta),
        sse_event(&stop),
    ]
    .join("")
}

fn sse_event(value: &Value) -> String {
    let event = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    format!("event: {event}\ndata: {value}\n\n")
}

fn is_quota_check(body: &Value) -> bool {
    if body.get("max_tokens").and_then(Value::as_u64) != Some(1) {
        return false;
    }
    let Some(text) = single_user_text(body) else {
        return false;
    };
    text.to_ascii_lowercase().contains("quota")
}

fn is_title_generation(body: &Value) -> bool {
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty())
    {
        return false;
    }
    let system = system_text(body).to_ascii_lowercase();
    if !system.contains("title") {
        return false;
    }
    system.contains("sentence-case title")
        || (system.contains("return json")
            && system.contains("field")
            && (system.contains("coding session") || system.contains("this session")))
}

fn is_suggestion_mode(body: &Value) -> bool {
    user_texts(body).any(|text| text.contains("[SUGGESTION MODE:"))
}

fn prefix_command(body: &Value) -> Option<String> {
    let text = single_user_text(body)?;
    if !text.contains("<policy_spec>") || !text.contains("Command:") {
        return None;
    }
    let start = text.rfind("Command:")? + "Command:".len();
    Some(text[start..].trim().to_string())
}

fn filepath_extract(body: &Value) -> Option<(String, String)> {
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty())
    {
        return None;
    }
    let text = single_user_text(body)?;
    if !text.contains("Command:") || !text.contains("Output:") {
        return None;
    }
    let system = system_text(body).to_ascii_lowercase();
    let user_hit = text.to_ascii_lowercase().contains("filepath");
    let system_hit = system.contains("extract any file paths")
        || system.contains("file paths that this command");
    if !user_hit && !system_hit {
        return None;
    }
    let cmd_start = text.find("Command:")? + "Command:".len();
    let output_at = text.find("Output:")?;
    if output_at < cmd_start {
        return None;
    }
    let command = text[cmd_start..output_at].trim().to_string();
    let mut output = text[output_at + "Output:".len()..].trim().to_string();
    if let Some(cut) = output.find("\n\n") {
        output.truncate(cut);
    }
    Some((command, output))
}

fn extract_command_prefix(command: &str) -> String {
    if command.contains('`') || command.contains("$(") {
        return "command_injection_detected".into();
    }
    let mut parts = shell_words(command);
    while parts
        .first()
        .is_some_and(|part| part.contains('=') && !part.starts_with('-'))
    {
        parts.remove(0);
    }
    let Some(first) = parts.first().cloned() else {
        return "none".into();
    };
    const TWO_WORD: &[&str] = &[
        "git", "npm", "docker", "kubectl", "cargo", "go", "pip", "yarn",
    ];
    if TWO_WORD.contains(&first.as_str()) {
        if let Some(second) = parts.get(1) {
            if !second.starts_with('-') {
                return format!("{first} {second}");
            }
        }
    }
    first
}

fn extract_filepaths(command: &str, _output: &str) -> String {
    let parts = shell_words(command);
    let Some(base) = parts.first() else {
        return "<filepaths>\n</filepaths>".into();
    };
    let base = base
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(base)
        .to_ascii_lowercase();
    const LISTING: &[&str] = &[
        "ls", "dir", "find", "tree", "pwd", "cd", "mkdir", "rmdir", "rm",
    ];
    const READING: &[&str] = &["cat", "head", "tail", "less", "more", "bat", "type"];
    if LISTING.contains(&base.as_str()) {
        return "<filepaths>\n</filepaths>".into();
    }
    if READING.contains(&base.as_str()) {
        let paths: Vec<&str> = parts
            .iter()
            .skip(1)
            .filter(|part| !part.starts_with('-'))
            .map(String::as_str)
            .collect();
        if paths.is_empty() {
            return "<filepaths>\n</filepaths>".into();
        }
        return format!("<filepaths>\n{}\n</filepaths>", paths.join("\n"));
    }
    "<filepaths>\n</filepaths>".into()
}

fn shell_words(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|part| part.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn single_user_text(body: &Value) -> Option<String> {
    let mut found: Option<String> = None;
    for message in body.get("messages").and_then(Value::as_array)? {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" {
            continue;
        }
        if role != "user" || found.is_some() {
            return None;
        }
        found = Some(content_text(message.get("content").unwrap_or(&Value::Null)));
    }
    found.filter(|text| !text.is_empty())
}

fn last_user_text(body: &Value) -> Option<String> {
    body.get("messages")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(|message| content_text(message.get("content").unwrap_or(&Value::Null)))
        .find(|text| {
            let trimmed = text.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("Warmup")
                && !trimmed.contains("<system-reminder>")
        })
}

fn user_texts(body: &Value) -> impl Iterator<Item = String> + '_ {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(|message| content_text(message.get("content").unwrap_or(&Value::Null)))
}

fn system_text(body: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(system) = body.get("system") {
        let text = content_text(system);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) == Some("system") {
                let text = content_text(message.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    parts.push(text);
                }
            }
        }
    }
    parts.join("\n")
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                block.as_str().map(str::to_string).or_else(|| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

const TITLE_KEYWORDS: &[&str] = &[
    "write a 5-10 word title",
    "Please write a 5-10 word title",
    "Respond with the title",
    "Generate a title for",
    "Create a brief title",
    "title for the conversation",
    "conversation title",
    "sentence-case title",
];

const SUMMARY_KEYWORDS: &[&str] = &[
    "Summarize this coding conversation",
    "Summarize the conversation",
    "Concise summary",
    "in under 50 characters",
    "compress the context",
    "Provide a concise summary",
    "condense the previous messages",
];

const SUGGESTION_KEYWORDS: &[&str] = &[
    "prompt suggestion generator",
    "suggest next prompts",
    "[SUGGESTION MODE:",
];

const PROBE_KEYWORDS: &[&str] = &[
    "check current directory",
    "list available tools",
    "verify environment",
    "test connection",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_probe_is_short_circuited() {
        let body = json!({
            "model": "claude-haiku-4-5",
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "quota" }]
        });
        let reply = try_short_circuit(&body, &FastPathSettings::default()).unwrap();
        assert!(reply.text.contains("Quota"));
    }

    #[test]
    fn title_generation_skips_upstream() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "system": "Write a sentence-case title for this coding session and return json with a title field.",
            "messages": [{ "role": "user", "content": "hello world" }]
        });
        let reply = try_short_circuit(&body, &FastPathSettings::default()).unwrap();
        assert_eq!(reply.text, "Conversation");
    }

    #[test]
    fn prefix_detection_extracts_git_commit() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": "<policy_spec>\nallow\n</policy_spec>\nCommand: git commit -m hi"
            }]
        });
        let reply = try_short_circuit(&body, &FastPathSettings::default()).unwrap();
        assert_eq!(reply.text, "git commit");
    }

    #[test]
    fn real_chat_is_not_short_circuited() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 4096,
            "messages": [{ "role": "user", "content": "refactor this module" }]
        });
        assert!(try_short_circuit(&body, &FastPathSettings::default()).is_none());
        assert!(detect_background_task(&body).is_none());
    }
}
