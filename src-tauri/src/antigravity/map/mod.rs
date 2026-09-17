//! Protocol mappers: Anthropic / OpenAI chat / Responses → Gemini → wire formats.

use serde_json::{json, Value};

pub mod anthropic;
pub mod args_fix;
pub mod history_media;
pub mod latex;
pub mod models;
pub mod openai;
pub mod responses;

pub use models::list_public_models;

const SYSTEM_REMINDER_START: &str = "<system-reminder>";
const SYSTEM_REMINDER_END: &str = "</system-reminder>";

fn text_parts(content: &Value) -> Vec<String> {
    match content {
        Value::String(text) if !text.trim().is_empty() => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string)
            })
            .collect(),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(|text| vec![text.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn system_reminder_text(content: &Value) -> Option<String> {
    let text = text_parts(content).join("\n");
    (!text.trim().is_empty())
        .then(|| format!("{SYSTEM_REMINDER_START}\n{text}\n{SYSTEM_REMINDER_END}"))
}

/// 按原位置把中途 system/developer 降级为 user part；只合并紧邻 user。
fn push_system_reminder(contents: &mut Vec<Value>, content: &Value) {
    let Some(text) = system_reminder_text(content) else {
        return;
    };
    let part = json!({ "text": text });
    if let Some(parts) = contents
        .last_mut()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.get_mut("parts"))
        .and_then(Value::as_array_mut)
    {
        parts.push(part);
    } else {
        contents.push(json!({ "role": "user", "parts": [part] }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_extracts_supported_text_without_rewriting_it() {
        let content = json!([
            { "type": "text", "text": "first" },
            "second",
            { "type": "image", "source": "ignored" },
            { "text": "  " }
        ]);
        assert_eq!(
            system_reminder_text(&content).as_deref(),
            Some("<system-reminder>\nfirst\nsecond\n</system-reminder>")
        );
    }

    #[test]
    fn reminder_appends_only_to_adjacent_user_turn() {
        let mut contents = vec![json!({ "role": "user", "parts": [{ "text": "question" }] })];
        push_system_reminder(&mut contents, &json!({ "text": "directive" }));
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["parts"].as_array().unwrap().len(), 2);

        contents.push(json!({ "role": "model", "parts": [{ "text": "answer" }] }));
        push_system_reminder(&mut contents, &json!("later"));
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[2]["role"], "user");
    }
}
