//! Codex `/v1/responses/compact` runtime facade.
//!
//! Native Responses upstreams keep the compact path. Chat / Anthropic targets get a
//! lossy summarization fallback that returns `object: "response.compaction"`.
//! Explicit streaming compact requests are rejected locally.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::provider::ProtocolType;

use super::codex_anthropic::responses_request_to_anthropic_messages;

pub fn is_responses_compact_route(path: &str) -> bool {
    let trimmed = path.trim_end_matches('/');
    trimmed.ends_with("/responses/compact")
}

pub fn reject_streaming_compact(body: &Value) -> AppResult<()> {
    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        return Err(AppError::Config(
            "Codex /responses/compact 不支持显式 stream=true".to_string(),
        ));
    }
    Ok(())
}

/// Convert a Responses compact request into an upstream Chat Completions body.
pub fn compact_request_to_openai_chat(body: &Value, model: &str) -> AppResult<Value> {
    reject_streaming_compact(body)?;
    let mut chat = responses_compact_to_chat_like(body, model)?;
    chat["stream"] = Value::Bool(false);
    Ok(chat)
}

/// Convert a Responses compact request into Anthropic `/v1/messages`.
pub fn compact_request_to_anthropic(body: &Value, model: &str) -> AppResult<Value> {
    reject_streaming_compact(body)?;
    let mut messages_body = body.clone();
    messages_body["stream"] = Value::Bool(false);
    if messages_body.get("max_output_tokens").is_none()
        && messages_body.get("max_tokens").is_none()
    {
        messages_body["max_output_tokens"] = json!(2048);
    }
    let mut anthropic = responses_request_to_anthropic_messages(&messages_body, model)?;
    anthropic["stream"] = Value::Bool(false);
    Ok(anthropic)
}

/// Wrap a Chat Completions JSON response as `response.compaction`.
pub fn chat_response_to_responses_compact(body: &Value, fallback_model: &str) -> Value {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("resp_{}", Uuid::new_v4().simple()));
    let created = body
        .get("created")
        .and_then(Value::as_i64)
        .unwrap_or_else(unix_now);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_model);
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(content_to_text)
        .unwrap_or_default();
    compaction_envelope(id, created, model, &text, body.get("usage").cloned())
}

/// Wrap an Anthropic Messages JSON response as `response.compaction`.
pub fn anthropic_response_to_responses_compact(body: &Value, fallback_model: &str) -> Value {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("resp_{}", Uuid::new_v4().simple()));
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_model);
    let text = body
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(Value::as_str) == Some("text") {
                        block.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    compaction_envelope(id, unix_now(), model, &text, body.get("usage").cloned())
}

pub fn compact_upstream_path(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::OpenAiResponses => "/v1/responses/compact",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => "/v1/chat/completions",
        ProtocolType::Anthropic => "/v1/messages",
    }
}

fn responses_compact_to_chat_like(body: &Value, model: &str) -> AppResult<Value> {
    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
        if !instructions.trim().is_empty() {
            messages.push(json!({"role": "system", "content": instructions}));
        }
    }
    match body.get("input") {
        Some(Value::Array(items)) => {
            for item in items {
                append_compact_input_item(item, &mut messages);
            }
        }
        Some(Value::String(text)) if !text.trim().is_empty() => {
            messages.push(json!({"role": "user", "content": text}));
        }
        Some(Value::Object(_)) => {
            append_compact_input_item(body.get("input").unwrap(), &mut messages);
        }
        _ => {}
    }
    if messages.is_empty() {
        return Err(AppError::Config(
            "compact 请求缺少可转换的 input".to_string(),
        ));
    }
    let mut result = json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });
    if let Some(max) = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
    {
        result["max_tokens"] = max.clone();
    }
    Ok(result)
}

fn append_compact_input_item(item: &Value, messages: &mut Vec<Value>) {
    if let Some(role) = item.get("role").and_then(Value::as_str) {
        let content = item
            .get("content")
            .map(|content| content_to_plain(content))
            .unwrap_or_default();
        if !content.trim().is_empty() {
            let mapped = match role {
                "assistant" => "assistant",
                "system" | "developer" => "system",
                _ => "user",
            };
            messages.push(json!({"role": mapped, "content": content}));
        }
        return;
    }
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            messages.push(json!({"role": "user", "content": text}));
        }
    }
}

fn content_to_plain(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn content_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str).or_else(|| part.as_str()))
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

fn compaction_envelope(
    id: String,
    created_at: i64,
    model: &str,
    text: &str,
    usage: Option<Value>,
) -> Value {
    let mut body = json!({
        "id": id,
        "object": "response.compaction",
        "created_at": created_at,
        "status": "completed",
        "model": model,
        "output": [{
            "id": format!("msg_{}", Uuid::new_v4().simple()),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text,
            }]
        }]
    });
    if let Some(usage) = usage {
        body["usage"] = usage;
    }
    body
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_compact_routes() {
        assert!(is_responses_compact_route("/v1/responses/compact"));
        assert!(is_responses_compact_route("/responses/compact/"));
        assert!(!is_responses_compact_route("/v1/responses"));
    }

    #[test]
    fn rejects_streaming_compact() {
        assert!(reject_streaming_compact(&json!({"stream": true})).is_err());
        assert!(reject_streaming_compact(&json!({"stream": false})).is_ok());
    }

    #[test]
    fn chat_fallback_builds_messages_and_compaction_object() {
        let request = json!({
            "model": "kimi-k3",
            "instructions": "keep it short",
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "summarize this context"}]
            }]
        });
        let chat = compact_request_to_openai_chat(&request, "kimi-k3").unwrap();
        assert_eq!(chat["stream"], false);
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(chat["messages"][1]["role"], "user");

        let compact = chat_response_to_responses_compact(
            &json!({
                "id": "chatcmpl_1",
                "object": "chat.completion",
                "created": 1,
                "model": "kimi-k3",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "done"},
                    "finish_reason": "stop"
                }]
            }),
            "kimi-k3",
        );
        assert_eq!(compact["object"], "response.compaction");
        assert_eq!(compact["status"], "completed");
        assert_eq!(compact["output"][0]["type"], "message");
        assert_eq!(compact["output"][0]["content"][0]["text"], "done");
    }

    #[test]
    fn anthropic_fallback_builds_messages() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "summarize"}]
            }]
        });
        let anthropic = compact_request_to_anthropic(&request, "claude-sonnet-5").unwrap();
        assert_eq!(anthropic["stream"], false);
        assert_eq!(anthropic["messages"][0]["role"], "user");
    }
}
