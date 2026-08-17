//! Codex Responses ↔ OpenAI Chat Completions bridge for Chat-only relays.

use serde_json::{json, Value};
use uuid::Uuid;

use super::codex_anthropic::push_responses_event;

/// Convert a Codex Responses request into Chat Completions JSON.
pub fn responses_to_chat_completions_body(body: &Value) -> Result<Value, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut messages = Vec::new();

    if let Some(instructions) = body.get("instructions").or_else(|| body.get("system")) {
        let text = instructions_text(instructions);
        if !text.is_empty() {
            messages.push(json!({ "role": "system", "content": text }));
        }
    }

    let input = body
        .get("input")
        .ok_or_else(|| "Responses 请求缺少 input 字段".to_string())?;
    messages.extend(responses_input_to_chat_messages(input)?);
    if messages.is_empty() {
        return Err("Responses 请求未包含可转换的消息".into());
    }

    let mut chat = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(max) = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
    {
        chat["max_tokens"] = max.clone();
    }
    copy_if_present(body, &mut chat, "temperature");
    copy_if_present(body, &mut chat, "top_p");
    if let Some(effort) = extract_responses_effort(body) {
        chat["reasoning_effort"] = json!(effort);
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools.iter().filter_map(responses_tool_to_chat).collect();
        if !converted.is_empty() {
            chat["tools"] = Value::Array(converted);
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        chat["tool_choice"] = choice.clone();
    }
    Ok(chat)
}

pub fn chat_response_to_responses(body: &Value, fallback_model: &str) -> Value {
    let message = body
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or(json!({}));
    let mut output = Vec::new();
    let mut text_parts = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            text_parts.push(json!({ "type": "output_text", "text": text }));
        }
        Some(Value::Array(blocks)) => {
            for block in blocks {
                if let Some(text) = block
                    .get("text")
                    .or_else(|| block.get("content"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    text_parts.push(json!({ "type": "output_text", "text": text }));
                }
            }
        }
        _ => {}
    }
    if !text_parts.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": text_parts,
        }));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            output.push(json!({
                "type": "function_call",
                "call_id": call.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
                "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or("tool"),
                "arguments": call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}"),
            }));
        }
    }
    let finish = body
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let status = if finish == "length" {
        "incomplete"
    } else {
        "completed"
    };
    let mut result = json!({
        "id": body.get("id").and_then(Value::as_str).unwrap_or("resp_proxy"),
        "object": "response",
        "status": status,
        "model": body.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "output": output,
    });
    if status == "incomplete" {
        result["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }
    if let Some(usage) = body.get("usage") {
        result["usage"] = json!({
            "input_tokens": usage.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0),
            "output_tokens": usage.get("completion_tokens").and_then(Value::as_i64).unwrap_or(0),
            "total_tokens": usage.get("total_tokens").and_then(Value::as_i64).unwrap_or(0),
        });
    }
    result
}

pub fn is_unsupported_content_type_error(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("unsupported content type") || text.contains("unsupported_content_type")
}

/// Incrementally convert Chat Completions SSE into Responses SSE.
pub struct ChatSseToResponsesConverter {
    model: String,
    response_id: String,
    started: bool,
    completed: bool,
    text_started: bool,
}

impl ChatSseToResponsesConverter {
    pub fn new(fallback_model: &str) -> Self {
        Self {
            model: fallback_model.to_string(),
            response_id: format!("resp_{}", Uuid::new_v4().simple()),
            started: false,
            completed: false,
            text_started: false,
        }
    }

    pub fn push_chat_chunk(&mut self, chunk: &Value) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut out = String::new();
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(id) = chunk.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                self.response_id = id.to_string();
            }
        }
        self.ensure_started(&mut out);
        let delta = chunk.pointer("/choices/0/delta").unwrap_or(&Value::Null);
        if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            self.emit_text_start(&mut out);
            push_responses_event(
                &mut out,
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": text,
                }),
            );
        }
        if let Some(reason) = chunk
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty() && *reason != "null")
        {
            self.finish(&mut out, reason, chunk.get("usage"));
        }
        out.into_bytes()
    }

    pub fn finish_done(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut out = String::new();
        self.ensure_started(&mut out);
        self.finish(&mut out, "stop", None);
        out.into_bytes()
    }

    fn ensure_started(&mut self, out: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        push_responses_event(
            out,
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "status": "in_progress",
                    "model": self.model,
                    "output": [],
                }
            }),
        );
    }

    fn emit_text_start(&mut self, out: &mut String) {
        if self.text_started {
            return;
        }
        self.text_started = true;
        push_responses_event(
            out,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": format!("msg_{}", Uuid::new_v4().simple()),
                    "role": "assistant",
                    "content": [],
                    "status": "in_progress",
                }
            }),
        );
        push_responses_event(
            out,
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "output_index": 0,
                "content_index": 0,
                "part": { "type": "output_text", "text": "" },
            }),
        );
    }

    fn finish(&mut self, out: &mut String, reason: &str, usage: Option<&Value>) {
        if self.completed {
            return;
        }
        self.completed = true;
        let status = if reason == "length" {
            "incomplete"
        } else {
            "completed"
        };
        let mut response = json!({
            "id": self.response_id,
            "object": "response",
            "status": status,
            "model": self.model.clone(),
            "output": [],
        });
        if let Some(usage) = usage {
            response["usage"] = json!({
                "input_tokens": usage.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0),
                "output_tokens": usage.get("completion_tokens").and_then(Value::as_i64).unwrap_or(0),
            });
        }
        push_responses_event(
            out,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": response,
            }),
        );
    }
}

fn extract_responses_effort(body: &Value) -> Option<String> {
    body.get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            body.get("reasoning")
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn instructions_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn responses_input_to_chat_messages(input: &Value) -> Result<Vec<Value>, String> {
    match input {
        Value::String(text) if !text.is_empty() => {
            Ok(vec![json!({ "role": "user", "content": text })])
        }
        Value::Array(items) => {
            let mut messages = Vec::new();
            let mut pending_tool_calls = Vec::new();
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        pending_tool_calls.push(json!({
                            "id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("call_tool"),
                            "type": "function",
                            "function": {
                                "name": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
                                "arguments": item.get("arguments").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| "{}".into()),
                            }
                        }));
                    }
                    Some("function_call_output") => {
                        if !pending_tool_calls.is_empty() {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": std::mem::take(&mut pending_tool_calls),
                            }));
                        }
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": item.get("call_id").and_then(Value::as_str).unwrap_or("call_tool"),
                            "content": output_text(item.get("output").unwrap_or(&Value::Null)),
                        }));
                    }
                    Some("reasoning") | Some("computer_call") | Some("computer_call_output") => {}
                    _ if item.get("role").is_some() => {
                        if !pending_tool_calls.is_empty() {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": std::mem::take(&mut pending_tool_calls),
                            }));
                        }
                        let role = match item.get("role").and_then(Value::as_str).unwrap_or("user") {
                            "assistant" => "assistant",
                            "system" | "developer" => "system",
                            _ => "user",
                        };
                        let content = convert_responses_content(item.get("content").unwrap_or(&Value::Null));
                        if !content_is_empty(&content) {
                            messages.push(json!({ "role": role, "content": content }));
                        }
                    }
                    _ => {
                        let text = output_text(item);
                        if !text.is_empty() {
                            messages.push(json!({ "role": "user", "content": text }));
                        }
                    }
                }
            }
            if !pending_tool_calls.is_empty() {
                messages.push(json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": pending_tool_calls,
                }));
            }
            Ok(messages)
        }
        _ => Err("Responses input 必须是字符串或数组".into()),
    }
}

fn convert_responses_content(content: &Value) -> Value {
    match content {
        Value::String(text) => json!(text),
        Value::Array(blocks) => {
            let parts: Vec<Value> = blocks
                .iter()
                .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("input_text") | Some("output_text") | Some("text") => block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| json!({ "type": "text", "text": text })),
                    Some("input_image") => block
                        .get("image_url")
                        .or_else(|| block.pointer("/image_url/url"))
                        .and_then(Value::as_str)
                        .map(|url| json!({ "type": "image_url", "image_url": { "url": url } })),
                    _ => block.as_str().map(|text| json!({ "type": "text", "text": text })),
                })
                .collect();
            if parts.len() == 1 {
                if let Some(text) = parts[0].get("text") {
                    return text.clone();
                }
            }
            Value::Array(parts)
        }
        _ => Value::String(String::new()),
    }
}

fn content_is_empty(content: &Value) -> bool {
    match content {
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(output_text).collect::<Vec<_>>().join(""),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn responses_tool_to_chat(tool: &Value) -> Option<Value> {
    let name = tool
        .get("name")
        .or_else(|| tool.pointer("/function/name"))
        .and_then(Value::as_str)?;
    let parameters = tool
        .get("parameters")
        .or_else(|| tool.pointer("/function/parameters"))
        .cloned()
        .unwrap_or(json!({ "type": "object", "properties": {} }));
    Some(json!({
        "type": "function",
        "function": { "name": name, "parameters": parameters }
    }))
}

fn copy_if_present(src: &Value, dest: &mut Value, key: &str) {
    if let Some(value) = src.get(key) {
        dest[key] = value.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_array_input_becomes_chat_messages() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "stream": true,
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        assert_eq!(chat["model"], "gpt-5.6-luna");
        assert_eq!(chat["stream"], true);
        assert_eq!(chat["messages"][0]["role"], "user");
        assert_eq!(chat["messages"][0]["content"], "hello");
    }

    #[test]
    fn unsupported_content_type_detects_relay_error() {
        assert!(is_unsupported_content_type_error(
            br#"{"error":{"message":"Unsupported content type"}}"#
        ));
        assert!(!is_unsupported_content_type_error(br#"{"error":{"message":"rate limit"}}"#));
    }
}
