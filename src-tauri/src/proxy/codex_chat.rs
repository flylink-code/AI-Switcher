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
    copy_if_present(body, &mut chat, "enable_thinking");
    if let Some(effort) = extract_responses_effort(body) {
        if effort == "none" {
            if chat.get("enable_thinking").is_some() {
                chat["enable_thinking"] = json!(false);
            }
        } else {
            chat["reasoning_effort"] = json!(effort);
            if chat.get("enable_thinking").is_some() {
                chat["enable_thinking"] = json!(true);
            }
        }
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
                        flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": item.get("call_id").and_then(Value::as_str).unwrap_or("call_tool"),
                            "content": output_text(item.get("output").unwrap_or(&Value::Null)),
                        }));
                    }
                    Some("reasoning") | Some("computer_call") | Some("computer_call_output") => {}
                    _ if item.get("role").is_some() => {
                        let role = match item.get("role").and_then(Value::as_str).unwrap_or("user") {
                            "assistant" => "assistant",
                            "system" | "developer" => "system",
                            _ => "user",
                        };
                        let content = convert_responses_content(item.get("content").unwrap_or(&Value::Null));
                        if role == "assistant" {
                            append_assistant_message(&mut messages, &mut pending_tool_calls, content);
                        } else {
                            flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                            if !content_is_empty(&content) {
                                messages.push(json!({ "role": role, "content": content }));
                            }
                        }
                    }
                    _ => {
                        let text = output_text(item);
                        if !text.is_empty() {
                            flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                            messages.push(json!({ "role": "user", "content": text }));
                        }
                    }
                }
            }
            flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
            Ok(messages)
        }
        _ => Err("Responses input 必须是字符串或数组".into()),
    }
}

/// 仅将待处理调用合并到紧邻且尚无调用的 assistant，不合并独立文本轮次。
pub(crate) fn append_assistant_message(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    content: Value,
) {
    if content_is_empty(&content) && pending_tool_calls.is_empty() {
        return;
    }
    if !pending_tool_calls.is_empty() {
        if let Some(last) = messages.last_mut().filter(|last| {
            last.get("role").and_then(Value::as_str) == Some("assistant")
                && last.get("tool_calls").and_then(Value::as_array).is_none_or(Vec::is_empty)
        }) {
            merge_assistant_content(last, content);
            last["tool_calls"] = Value::Array(std::mem::take(pending_tool_calls));
            return;
        }
    }
    let mut message = json!({
        "role": "assistant",
        "content": if content_is_empty(&content) { Value::Null } else { content },
    });
    if !pending_tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(std::mem::take(pending_tool_calls));
    }
    messages.push(message);
}

pub(crate) fn flush_pending_tool_calls(messages: &mut Vec<Value>, calls: &mut Vec<Value>) {
    if !calls.is_empty() {
        append_assistant_message(messages, calls, Value::Null);
    }
}

fn merge_assistant_content(last: &mut Value, content: Value) {
    if content_is_empty(&content) {
        return;
    }
    let previous = last.get("content").cloned().unwrap_or(Value::Null);
    last["content"] = if content_is_empty(&previous) {
        content
    } else {
        match (previous, content) {
            (Value::String(a), Value::String(b)) => Value::String(format!("{a}\n{b}")),
            (a, b) => {
                let mut parts = match a {
                    Value::Array(parts) => parts,
                    other => vec![json!({ "type": "text", "text": other })],
                };
                match b {
                    Value::Array(next) => parts.extend(next),
                    other => parts.push(json!({ "type": "text", "text": other })),
                }
                Value::Array(parts)
            }
        }
    };
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
        Value::Object(map) => map.get("text").filter(|text| text.is_string()).cloned().unwrap_or(Value::Null),
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
    #[test]
    fn responses_assistant_text_and_function_call_merged_in_same_turn() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "stream": false,
            "input": [
                { "role": "user", "content": "hi" },
                { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "I will check the file." }] },
                { "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "hello file" }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        let msgs = chat["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");

        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "I will check the file.");
        let tool_calls = msgs[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "call_1");
        assert_eq!(tool_calls[0]["function"]["name"], "read_file");
        assert_eq!(
            tool_calls[0]["function"]["arguments"],
            "{\"path\":\"a.txt\"}"
        );

        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
        assert_eq!(msgs[2]["content"], "hello file");
    }

    #[test]
    fn responses_function_call_before_assistant_text_merged_in_same_turn() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "role": "user", "content": "hi" },
                { "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{}" },
                { "role": "assistant", "content": "I am reading the file." },
                { "type": "function_call_output", "call_id": "call_1", "output": "content" }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        let msgs = chat["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "I am reading the file.");
        assert_eq!(msgs[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(msgs[2]["role"], "tool");
    }

    #[test]
    fn responses_does_not_merge_across_role_boundaries() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "role": "user", "content": "question 1" },
                { "role": "assistant", "content": "answer 1" },
                { "role": "user", "content": "question 2" },
                { "type": "function_call", "call_id": "call_2", "name": "search", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_2", "output": "results" }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        let msgs = chat["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "question 1");

        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "answer 1");
        assert!(msgs[1].get("tool_calls").is_none());

        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "question 2");

        assert_eq!(msgs[3]["role"], "assistant");
        assert_eq!(msgs[3]["content"], Value::Null);
        assert_eq!(msgs[3]["tool_calls"][0]["id"], "call_2");

        assert_eq!(msgs[4]["role"], "tool");
        assert_eq!(msgs[4]["tool_call_id"], "call_2");
    }

    #[test]
    fn responses_adjacent_assistant_messages_remain_distinct_turns() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "role": "user", "content": "start" },
                { "role": "assistant", "content": "first turn" },
                { "role": "assistant", "content": "second turn" }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["content"], "first turn");
        assert_eq!(messages[2]["content"], "second turn");
    }

    #[test]
    fn responses_second_tool_batch_does_not_extend_completed_assistant() {
        let mut messages = vec![json!({
            "role": "assistant", "content": "completed batch",
            "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "first", "arguments": "{}" } }]
        })];
        let mut calls = vec![json!({ "id": "call_2", "type": "function", "function": { "name": "second", "arguments": "{}" } })];
        flush_pending_tool_calls(&mut messages, &mut calls);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_2");
        assert!(calls.is_empty());
    }

    #[test]
    fn responses_system_and_tool_output_boundaries_stay_separate() {
        for role in ["system", "developer", "user"] {
            let messages = responses_input_to_chat_messages(&json!([
                { "role": "assistant", "content": "before boundary" },
                { "role": role, "content": { "text": "boundary" } },
                { "type": "function_call", "call_id": "next", "name": "read", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "next", "output": "result" },
                { "role": "assistant", "content": "after output" }
            ])).unwrap();
            assert_eq!(messages.len(), 5);
            assert!(messages[0].get("tool_calls").is_none());
            assert_eq!(messages[1]["content"], "boundary");
            assert_eq!(messages[2]["tool_calls"][0]["id"], "next");
            assert_eq!(messages[3]["role"], "tool");
            assert!(messages[4].get("tool_calls").is_none());
        }
    }

    #[test]
    fn responses_trailing_tool_calls_merged_with_assistant_text() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "role": "user", "content": "start" },
                { "role": "assistant", "content": "starting tools" },
                { "type": "function_call", "call_id": "call_trail", "name": "init", "arguments": "{}" }
            ]
        });
        let chat = responses_to_chat_completions_body(&body).unwrap();
        let msgs = chat["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "starting tools");
        assert_eq!(msgs[1]["tool_calls"][0]["id"], "call_trail");
    }

}
