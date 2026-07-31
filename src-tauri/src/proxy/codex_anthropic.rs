//! Codex Responses ↔ Anthropic Messages protocol bridge.

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub fn anthropic_version_header() -> &'static str {
    ANTHROPIC_VERSION
}

/// Map an OpenAI Responses request body to Anthropic `/v1/messages` JSON.
pub fn responses_request_to_anthropic_messages(body: &Value, model: &str) -> AppResult<Value> {
    let input = body
        .get("input")
        .ok_or_else(|| AppError::Config("Responses 请求缺少 input 字段".to_string()))?;
    let messages = responses_input_to_messages(input)?;
    if messages.is_empty() {
        return Err(AppError::Config("Responses 请求未包含可转换的消息".to_string()));
    }

    let max_tokens = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(4096);

    let mut result = json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
    });

    if let Some(instructions) = body.get("instructions").or_else(|| body.get("system")) {
        let system = instructions_text(instructions);
        if !system.is_empty() {
            result["system"] = Value::String(system);
        }
    }

    if let Some(stream) = body.get("stream").and_then(Value::as_bool) {
        result["stream"] = Value::Bool(stream);
    }
    copy_if_present(body, &mut result, "temperature");
    copy_if_present(body, &mut result, "top_p");
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools.iter().filter_map(responses_tool_to_anthropic).collect();
        if !converted.is_empty() {
            result["tools"] = Value::Array(converted);
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_anthropic(choice);
    }
    Ok(result)
}

/// Map a non-streaming Anthropic Messages response to Responses-like JSON.
pub fn anthropic_response_to_responses(body: &Value) -> Value {
    if body.get("type").and_then(Value::as_str) == Some("error") {
        return anthropic_error_to_responses(body);
    }

    let mut output = Vec::new();
    let mut text_parts = Vec::new();
    for block in body.get("content").and_then(Value::as_array).into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str).filter(|text| !text.is_empty())
                {
                    text_parts.push(json!({"type": "output_text", "text": text}));
                }
            }
            Some("tool_use") => {
                if !text_parts.is_empty() {
                    output.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": text_parts,
                    }));
                    text_parts = Vec::new();
                }
                output.push(json!({
                    "type": "function_call",
                    "call_id": block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
                    "name": block.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({})))
                        .unwrap_or_else(|_| "{}".to_string()),
                }));
            }
            _ => {}
        }
    }
    if !text_parts.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": text_parts,
        }));
    }

    let stop_reason = body.get("stop_reason").and_then(Value::as_str);
    let status = if stop_reason == Some("max_tokens") {
        "incomplete"
    } else {
        "completed"
    };
    let mut result = json!({
        "id": body.get("id").and_then(Value::as_str).unwrap_or("resp_proxy"),
        "object": "response",
        "status": status,
        "model": body.get("model").and_then(Value::as_str).unwrap_or(""),
        "output": output,
    });
    if status == "incomplete" {
        result["incomplete_details"] = json!({"reason": "max_output_tokens"});
    }
    if let Some(usage) = body.get("usage") {
        result["usage"] = anthropic_usage_to_responses(usage);
    }
    result
}

fn anthropic_error_to_responses(body: &Value) -> Value {
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("上游 Anthropic 请求失败");
    json!({
        "id": "resp_error",
        "object": "response",
        "status": "failed",
        "error": {
            "code": body.pointer("/error/type").and_then(Value::as_str).unwrap_or("api_error"),
            "message": message,
        }
    })
}

fn anthropic_usage_to_responses(usage: &Value) -> Value {
    let input_tokens = usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_input = input_tokens.saturating_add(cache_read);
    let mut result = json!({
        "input_tokens": total_input,
        "output_tokens": usage.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
    });
    if cache_read > 0 {
        result["input_tokens_details"] = json!({"cached_tokens": cache_read});
    }
    result
}

fn responses_input_to_messages(input: &Value) -> AppResult<Vec<Value>> {
    match input {
        Value::String(text) if !text.is_empty() => Ok(vec![json!({"role": "user", "content": text})]),
        Value::Array(items) => {
            let mut messages = Vec::new();
            let mut pending_tool_uses = Vec::new();
            let mut pending_tool_results = Vec::new();

            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        flush_tool_results(&mut messages, &mut pending_tool_results);
                        let parsed_args = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .and_then(|raw| serde_json::from_str(raw).ok())
                            .unwrap_or_else(|| item.get("arguments").cloned().unwrap_or_else(|| json!({})));
                        pending_tool_uses.push(json!({
                            "type": "tool_use",
                            "id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),
                            "name": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
                            "input": parsed_args,
                        }));
                    }
                    Some("function_call_output") => {
                        flush_tool_uses(&mut messages, &mut pending_tool_uses);
                        pending_tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": item.get("call_id").and_then(Value::as_str).unwrap_or("tool_call"),
                            "content": output_text(item.get("output").unwrap_or(&Value::Null)),
                        }));
                    }
                    _ if item.get("role").is_some() => {
                        flush_tool_uses(&mut messages, &mut pending_tool_uses);
                        flush_tool_results(&mut messages, &mut pending_tool_results);
                        let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                        let anthropic_role = if role == "assistant" { "assistant" } else { "user" };
                        let content = convert_responses_content(item.get("content").unwrap_or(&Value::Null))?;
                        if !content.is_empty() {
                            messages.push(json!({"role": anthropic_role, "content": content}));
                        }
                    }
                    _ => {}
                }
            }
            flush_tool_uses(&mut messages, &mut pending_tool_uses);
            flush_tool_results(&mut messages, &mut pending_tool_results);
            Ok(messages)
        }
        _ => Err(AppError::Config(
            "Responses input 必须是字符串或数组".to_string(),
        )),
    }
}

fn flush_tool_uses(messages: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    messages.push(json!({"role": "assistant", "content": std::mem::take(pending)}));
}

fn flush_tool_results(messages: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    messages.push(json!({"role": "user", "content": std::mem::take(pending)}));
}

fn convert_responses_content(content: &Value) -> AppResult<Vec<Value>> {
    match content {
        Value::String(text) if !text.is_empty() => Ok(vec![json!({"type": "text", "text": text})]),
        Value::Array(parts) => {
            let mut blocks = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str).filter(|text| !text.is_empty())
                        {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                    }
                    Some("input_image") => {
                        if let Some(image) = responses_image_to_anthropic(part) {
                            blocks.push(image);
                        }
                    }
                    Some("image_url") => {
                        if let Some(url) = part.pointer("/image_url/url").and_then(Value::as_str) {
                            blocks.push(json!({
                                "type": "image",
                                "source": {"type": "url", "url": url}
                            }));
                        }
                    }
                    _ => {}
                }
            }
            Ok(blocks)
        }
        _ => Ok(Vec::new()),
    }
}

fn responses_image_to_anthropic(part: &Value) -> Option<Value> {
    let url = part
        .get("image_url")
        .and_then(Value::as_str)
        .or_else(|| part.pointer("/image_url/url").and_then(Value::as_str))?;
    if let Some(rest) = url.strip_prefix("data:") {
        let (media_type, data) = rest.split_once(";base64,")?;
        return Some(json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": data}
        }));
    }
    Some(json!({
        "type": "image",
        "source": {"type": "url", "url": url}
    }))
}

fn responses_tool_to_anthropic(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(Value::as_str)?;
    let schema = tool
        .get("parameters")
        .or_else(|| tool.pointer("/function/parameters"))
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    Some(json!({
        "name": name,
        "description": tool.get("description")
            .or_else(|| tool.pointer("/function/description"))
            .and_then(Value::as_str)
            .unwrap_or(""),
        "input_schema": schema,
    }))
}

fn responses_tool_choice_to_anthropic(value: &Value) -> Value {
    match value {
        Value::String(text) => match text.as_str() {
            "required" => json!({"type": "any"}),
            "none" => json!({"type": "none"}),
            _ => json!({"type": "auto"}),
        },
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("function") => json!({
            "type": "tool",
            "name": map
                .get("name")
                .or_else(|| map.get("function").and_then(|value| value.get("name")))
                .and_then(Value::as_str)
                .unwrap_or(""),
        }),
        _ => json!({"type": "auto"}),
    }
}

fn instructions_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => value.to_string(),
    }
}

fn copy_if_present(from: &Value, to: &mut Value, key: &str) {
    if let Some(value) = from.get(key) {
        to[key] = value.clone();
    }
}

fn push_responses_event(output: &mut String, event_type: &str, data: Value) {
    output.push_str("event: ");
    output.push_str(event_type);
    output.push('\n');
    output.push_str("data: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string()));
    output.push_str("\n\n");
}

/// Incrementally convert Anthropic Messages SSE into Responses SSE.
pub struct AnthropicSseToResponsesConverter {
    fallback_model: String,
    response_id: String,
    model: String,
    started: bool,
    completed: bool,
    output_index: usize,
    content_index: usize,
    text_item_started: bool,
    tool_output_indices: std::collections::BTreeMap<usize, usize>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
    status: String,
}

impl AnthropicSseToResponsesConverter {
    pub fn new(fallback_model: &str) -> Self {
        Self {
            fallback_model: fallback_model.to_string(),
            response_id: "resp_proxy".to_string(),
            model: fallback_model.to_string(),
            started: false,
            completed: false,
            output_index: 0,
            content_index: 0,
            text_item_started: false,
            tool_output_indices: std::collections::BTreeMap::new(),
            input_tokens: 0,
            cache_read_input_tokens: 0,
            output_tokens: 0,
            status: "completed".to_string(),
        }
    }

    pub fn push_event(&mut self, event_type: &str, data: &Value) -> Vec<u8> {
        let mut output = String::new();
        match event_type {
            "message_start" => {
                if let Some(message) = data.get("message") {
                    if let Some(id) = message.get("id").and_then(Value::as_str) {
                        self.response_id = id.to_string();
                    }
                    if let Some(model) = message.get("model").and_then(Value::as_str) {
                        self.model = model.to_string();
                    }
                    if let Some(usage) = message.get("usage") {
                        self.observe_usage(usage);
                    }
                }
                self.ensure_started(&mut output);
            }
            "content_block_start" => {
                self.ensure_started(&mut output);
                let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let block = data.get("content_block").unwrap_or(&Value::Null);
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if !self.text_item_started {
                            self.emit_text_item_added(&mut output);
                        }
                    }
                    Some("tool_use") => {
                        let output_index = self.output_index;
                        self.tool_output_indices.insert(index, output_index);
                        self.output_index += 1;
                        push_responses_event(
                            &mut output,
                            "response.output_item.added",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": output_index,
                                "item": {
                                    "type": "function_call",
                                    "call_id": block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
                                    "name": block.get("name").and_then(Value::as_str).unwrap_or("tool"),
                                }
                            }),
                        );
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                self.ensure_started(&mut output);
                let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = data.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if !self.text_item_started {
                            self.emit_text_item_added(&mut output);
                        }
                        if let Some(text) = delta.get("text").and_then(Value::as_str).filter(|text| !text.is_empty())
                        {
                            push_responses_event(
                                &mut output,
                                "response.output_text.delta",
                                json!({
                                    "type": "response.output_text.delta",
                                    "output_index": self.text_output_index(),
                                    "content_index": self.content_index,
                                    "delta": text,
                                }),
                            );
                        }
                    }
                    Some("input_json_delta") => {
                        let output_index = self
                            .tool_output_indices
                            .get(&index)
                            .copied()
                            .unwrap_or(self.output_index.saturating_sub(1));
                        if let Some(partial) = delta.get("partial_json").and_then(Value::as_str).filter(|value| !value.is_empty())
                        {
                            push_responses_event(
                                &mut output,
                                "response.function_call_arguments.delta",
                                json!({
                                    "type": "response.function_call_arguments.delta",
                                    "output_index": output_index,
                                    "delta": partial,
                                }),
                            );
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(usage) = data.get("usage") {
                    self.observe_usage(usage);
                }
                if let Some(reason) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.status = match reason {
                        "max_tokens" => "incomplete".to_string(),
                        "tool_use" => "completed".to_string(),
                        _ => "completed".to_string(),
                    };
                }
            }
            "message_stop" => {
                let finished = self.finish_stream();
                output.push_str(&String::from_utf8_lossy(&finished));
            }
            "error" => {
                let message = data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("上游 Anthropic 流式响应错误");
                push_responses_event(
                    &mut output,
                    "response.failed",
                    json!({
                        "type": "response.failed",
                        "error": {"code": "api_error", "message": message},
                    }),
                );
                self.completed = true;
            }
            _ => {}
        }
        output.into_bytes()
    }

    pub fn finish_stream(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut output = String::new();
        self.ensure_started(&mut output);
        let event_type = if self.status == "incomplete" {
            "response.incomplete"
        } else {
            "response.completed"
        };
        let total_input = self.input_tokens.saturating_add(self.cache_read_input_tokens);
        let mut response = json!({
            "id": self.response_id,
            "object": "response",
            "status": self.status,
            "model": if self.model.is_empty() { &self.fallback_model } else { &self.model },
            "output": [],
            "usage": {
                "input_tokens": total_input,
                "output_tokens": self.output_tokens,
            }
        });
        if self.cache_read_input_tokens > 0 {
            response["usage"]["input_tokens_details"] =
                json!({"cached_tokens": self.cache_read_input_tokens});
        }
        if self.status == "incomplete" {
            response["incomplete_details"] = json!({"reason": "max_output_tokens"});
        }
        push_responses_event(
            &mut output,
            event_type,
            json!({
                "type": event_type,
                "response": response,
            }),
        );
        self.completed = true;
        output.into_bytes()
    }

    pub fn error_event(&mut self, message: &str) -> Vec<u8> {
        let mut output = String::new();
        push_responses_event(
            &mut output,
            "response.failed",
            json!({
                "type": "response.failed",
                "error": {"code": "api_error", "message": message},
            }),
        );
        self.completed = true;
        output.into_bytes()
    }

    fn text_output_index(&self) -> usize {
        if self.text_item_started {
            0
        } else {
            self.output_index
        }
    }

    fn ensure_started(&mut self, output: &mut String) {
        if self.started {
            return;
        }
        let model = if self.model.is_empty() {
            self.fallback_model.as_str()
        } else {
            self.model.as_str()
        };
        let response = json!({
            "id": self.response_id,
            "object": "response",
            "status": "in_progress",
            "model": model,
        });
        push_responses_event(
            output,
            "response.created",
            json!({"type": "response.created", "response": response}),
        );
        push_responses_event(
            output,
            "response.in_progress",
            json!({"type": "response.in_progress", "response": response}),
        );
        self.started = true;
    }

    fn emit_text_item_added(&mut self, output: &mut String) {
        if self.text_item_started {
            return;
        }
        push_responses_event(
            output,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": self.output_index,
                "item": {"type": "message", "role": "assistant", "content": []},
            }),
        );
        push_responses_event(
            output,
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "output_index": self.output_index,
                "content_index": self.content_index,
                "part": {"type": "output_text", "text": ""},
            }),
        );
        self.text_item_started = true;
        if self.tool_output_indices.is_empty() {
            self.output_index = 1;
        }
    }

    fn observe_usage(&mut self, usage: &Value) {
        let input = usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
        let cache = usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        self.input_tokens = input;
        self.cache_read_input_tokens = cache;
        self.output_tokens = usage
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(self.output_tokens);
    }
}

pub fn parse_anthropic_sse_frame(frame: &str) -> Option<(&str, Value)> {
    let mut event_type = None;
    let mut data = None;
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data:") {
            data = Some(value.trim_start());
        }
    }
    let event_type = event_type?;
    let data = data?;
    if data == "[DONE]" {
        return None;
    }
    let parsed = serde_json::from_str::<Value>(data).ok()?;
    Some((event_type, parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSES_REQUEST: &str = r#"{
        "model": "gpt-5",
        "instructions": "Be concise.",
        "max_output_tokens": 128,
        "stream": false,
        "input": [
            {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
            {"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"q\":\"x\"}"},
            {"type": "function_call_output", "call_id": "call_1", "output": "found"}
        ],
        "tools": [{"type": "function", "name": "lookup", "description": "Find", "parameters": {"type": "object"}}],
        "tool_choice": {"type": "function", "name": "lookup"}
    }"#;

    const ANTHROPIC_RESPONSE: &str = r#"{
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet",
        "content": [
            {"type": "text", "text": "done"},
            {"type": "tool_use", "id": "call_2", "name": "save", "input": {"ok": true}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 4, "cache_read_input_tokens": 1, "output_tokens": 5}
    }"#;

    #[test]
    fn responses_request_round_trip_to_anthropic_messages() {
        let body: Value = serde_json::from_str(RESPONSES_REQUEST).unwrap();
        let anthropic = responses_request_to_anthropic_messages(&body, "claude-sonnet").unwrap();
        assert_eq!(anthropic["model"], "claude-sonnet");
        assert_eq!(anthropic["system"], "Be concise.");
        assert_eq!(anthropic["max_tokens"], 128);
        assert_eq!(anthropic["messages"][0]["content"][0]["text"], "Hello");
        assert_eq!(anthropic["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(anthropic["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(anthropic["tools"][0]["name"], "lookup");
        assert_eq!(anthropic["tool_choice"]["name"], "lookup");
    }

    #[test]
    fn anthropic_response_round_trip_to_responses() {
        let body: Value = serde_json::from_str(ANTHROPIC_RESPONSE).unwrap();
        let responses = anthropic_response_to_responses(&body);
        assert_eq!(responses["status"], "completed");
        assert_eq!(responses["output"][0]["content"][0]["text"], "done");
        assert_eq!(responses["output"][1]["name"], "save");
        assert_eq!(responses["usage"]["input_tokens"], 5);
        assert_eq!(responses["usage"]["input_tokens_details"]["cached_tokens"], 1);
        assert_eq!(responses["usage"]["output_tokens"], 5);
    }

    #[test]
    fn anthropic_max_tokens_maps_to_incomplete_responses() {
        let body = json!({
            "id": "msg_2",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet",
            "content": [{"type": "text", "text": "partial"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 2, "output_tokens": 1}
        });
        let responses = anthropic_response_to_responses(&body);
        assert_eq!(responses["status"], "incomplete");
        assert_eq!(responses["incomplete_details"]["reason"], "max_output_tokens");
    }

    #[test]
    fn anthropic_sse_text_stream_emits_responses_deltas() {
        let mut converter = AnthropicSseToResponsesConverter::new("claude-sonnet");
        let start = String::from_utf8(
            converter.push_event(
                "message_start",
                &json!({"type":"message_start","message":{"id":"msg_stream","model":"claude-sonnet","usage":{"input_tokens":1,"output_tokens":0}}}),
            ),
        )
        .unwrap();
        let delta = String::from_utf8(
            converter.push_event(
                "content_block_delta",
                &json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}),
            ),
        )
        .unwrap();
        let stop = String::from_utf8(converter.push_event("message_stop", &json!({"type":"message_stop"}))).unwrap();
        assert!(start.contains("response.created"));
        assert!(delta.contains("response.output_text.delta"));
        assert!(delta.contains("\"delta\":\"Hi\""));
        assert!(stop.contains("response.completed"));
    }

    #[test]
    fn string_input_maps_to_single_user_message() {
        let body = json!({"input": "ping", "max_output_tokens": 32});
        let anthropic = responses_request_to_anthropic_messages(&body, "claude-sonnet").unwrap();
        assert_eq!(anthropic["messages"][0], json!({"role": "user", "content": "ping"}));
    }
}
