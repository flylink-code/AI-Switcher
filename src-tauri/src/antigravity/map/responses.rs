//! OpenAI Responses API ↔ Gemini mapping (Codex Desktop / CLI).

use serde_json::{json, Value};
use uuid::Uuid;

use super::args_fix::ToolParamKeys;
use super::openai::{extract_assistant, openai_to_gemini_request, GeminiRequestParts};
use crate::antigravity::usage_log::GeminiUsage;

/// Convert an OpenAI Responses request into Gemini generateContent parts.
pub fn responses_to_gemini_request(
    body: &Value,
    session_key: Option<&str>,
) -> Result<GeminiRequestParts, String> {
    let chat = responses_to_chat_completions_body(body)?;
    openai_to_gemini_request(&chat, session_key)
}

/// Normalize Responses wire shape into Chat Completions so we reuse the
/// existing OpenAI→Gemini mapper (tools, effort, multimodal).
fn responses_to_chat_completions_body(body: &Value) -> Result<Value, String> {
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
    if let Some(temperature) = body.get("temperature") {
        chat["temperature"] = temperature.clone();
    }
    if let Some(top_p) = body.get("top_p") {
        chat["top_p"] = top_p.clone();
    }

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
                        let id = item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("call_tool")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string();
                        let arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                item.get("arguments")
                                    .cloned()
                                    .unwrap_or(json!({}))
                                    .to_string()
                            });
                        pending_tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": arguments }
                        }));
                    }
                    Some("function_call_output") => {
                        if pending_tool_calls.is_empty() {
                            let tool_call_id = item
                                .get("call_id")
                                .and_then(Value::as_str)
                                .unwrap_or("call_tool");
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [{
                                    "id": tool_call_id,
                                    "type": "function",
                                    "function": { "name": "tool", "arguments": "{}" }
                                }]
                            }));
                        } else {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": std::mem::take(&mut pending_tool_calls),
                            }));
                        }
                        let tool_call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("call_tool");
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": output_text(item.get("output").unwrap_or(&Value::Null)),
                        }));
                    }
                    _ if item.get("role").is_some() => {
                        if !pending_tool_calls.is_empty() {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": std::mem::take(&mut pending_tool_calls),
                            }));
                        }
                        let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                        let chat_role = match role {
                            "assistant" => "assistant",
                            "system" | "developer" => "system",
                            _ => "user",
                        };
                        let content = convert_responses_content(item.get("content").unwrap_or(&Value::Null));
                        if !content_is_empty(&content) {
                            messages.push(json!({ "role": chat_role, "content": content }));
                        }
                    }
                    _ => {
                        if let Some(text) = item.as_str().map(str::to_string).or_else(|| {
                            item.get("content")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .or_else(|| {
                                    item.get("text").and_then(Value::as_str).map(str::to_string)
                                })
                        }) {
                            if !text.trim().is_empty() {
                                if !pending_tool_calls.is_empty() {
                                    messages.push(json!({
                                        "role": "assistant",
                                        "content": null,
                                        "tool_calls": std::mem::take(&mut pending_tool_calls),
                                    }));
                                }
                                messages.push(json!({ "role": "user", "content": text }));
                            }
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
            rewrite_trailing_assistant_prefill(&mut messages);
            Ok(messages)
        }
        _ => Err("Responses input 必须是字符串或数组".into()),
    }
}

fn rewrite_trailing_assistant_prefill(messages: &mut Vec<Value>) {
    let Some(last) = messages.last() else {
        return;
    };
    if last.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    if last.get("tool_calls").is_some() {
        return;
    }
    let content = last.get("content").cloned().unwrap_or(Value::Null);
    if content_is_empty(&content) {
        return;
    }
    messages.pop();
    messages.push(json!({
        "role": "user",
        "content": content,
    }));
}

fn convert_responses_content(content: &Value) -> Value {
    match content {
        Value::String(text) => json!(text),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(json!({ "type": "text", "text": text }));
                            }
                        }
                    }
                    Some("input_image" | "image_url") => {
                        if let Some(url) = part
                            .get("image_url")
                            .and_then(|v| {
                                v.as_str()
                                    .map(str::to_string)
                                    .or_else(|| v.get("url").and_then(Value::as_str).map(str::to_string))
                            })
                            .or_else(|| part.pointer("/image_url/url").and_then(Value::as_str).map(str::to_string))
                        {
                            out.push(json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }));
                        }
                    }
                    _ => {
                        if let Some(text) = part.as_str() {
                            if !text.is_empty() {
                                out.push(json!({ "type": "text", "text": text }));
                            }
                        }
                    }
                }
            }
            Value::Array(out)
        }
        _ => Value::Null,
    }
}

fn content_is_empty(content: &Value) -> bool {
    match content {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.as_str()
                    .map(str::to_string)
                    .or_else(|| part.get("text").and_then(Value::as_str).map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

fn responses_tool_to_chat(tool: &Value) -> Option<Value> {
    // Responses flat tools: { name, description, parameters }
    // Chat tools: { type: "function", function: { name, description, parameters } }
    if tool.get("type").and_then(Value::as_str) == Some("function") {
        return Some(tool.clone());
    }
    let name = tool
        .get("name")
        .or_else(|| tool.pointer("/function/name"))
        .and_then(Value::as_str)?;
    let description = tool
        .get("description")
        .or_else(|| tool.pointer("/function/description"))
        .cloned()
        .unwrap_or(json!(""));
    let parameters = tool
        .get("parameters")
        .or_else(|| tool.pointer("/function/parameters"))
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    Some(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    }))
}

/// Non-streaming Gemini → Responses JSON.
pub fn gemini_to_responses_response(
    model: &str,
    gemini: &Value,
    session_key: Option<&str>,
    tool_params: &ToolParamKeys,
) -> Value {
    let (text, tool_calls, finish_reason) = extract_assistant(gemini, session_key, tool_params);
    let mut output = Vec::new();
    if !text.is_empty() {
        output.push(json!({
            "type": "message",
            "id": format!("msg_{}", Uuid::new_v4().simple()),
            "role": "assistant",
            "content": [{ "type": "output_text", "text": text }],
            "status": "completed",
        }));
    }
    for call in tool_calls {
        output.push(json!({
            "type": "function_call",
            "id": call.get("id").cloned().unwrap_or(json!(format!("fc_{}", Uuid::new_v4().simple()))),
            "call_id": call.get("id").cloned().unwrap_or(json!(format!("call_{}", Uuid::new_v4().simple()))),
            "name": call.pointer("/function/name").cloned().unwrap_or(json!("tool")),
            "arguments": call.pointer("/function/arguments").cloned().unwrap_or(json!("{}")),
            "status": "completed",
        }));
    }
    let status = if finish_reason == "length" {
        "incomplete"
    } else {
        "completed"
    };
    let usage = GeminiUsage::parse(gemini).responses_usage();
    let mut body = json!({
        "id": format!("resp_{}", Uuid::new_v4().simple()),
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": status,
        "model": model,
        "output": output,
        "usage": usage,
    });
    if status == "incomplete" {
        body["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }
    body
}

/// Stateful Gemini chunk → Responses SSE event bytes.
pub struct ResponsesStreamEncoder {
    model: String,
    response_id: String,
    message_id: String,
    started: bool,
    text_started: bool,
    completed: bool,
    output_index: usize,
    status: String,
    usage: GeminiUsage,
    full_text: String,
    sequence: u64,
    /// Completed function_call items for the final `response.completed.output`.
    function_outputs: Vec<Value>,
    /// 本次请求的工具声明参数键名，用于纠偏 args key。
    tool_params: ToolParamKeys,
    session_key: Option<String>,
}

impl ResponsesStreamEncoder {
    pub fn new(model: &str, tool_params: ToolParamKeys, session_key: Option<String>) -> Self {
        let response_id = format!("resp_{}", Uuid::new_v4().simple());
        Self {
            model: model.to_string(),
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            response_id,
            started: false,
            text_started: false,
            completed: false,
            output_index: 0,
            status: "completed".into(),
            usage: GeminiUsage::default(),
            full_text: String::new(),
            sequence: 0,
            function_outputs: Vec::new(),
            tool_params,
            session_key,
        }
    }

    fn next_seq(&mut self) -> u64 {
        let seq = self.sequence;
        self.sequence = self.sequence.saturating_add(1);
        seq
    }

    pub fn encode_gemini_chunk(&mut self, gemini: &Value) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut out = String::new();
        self.ensure_started(&mut out);
        self.note_usage(gemini);
        let (text, tool_calls, finish_reason) =
            extract_assistant(gemini, self.session_key.as_deref(), &self.tool_params);
        if finish_reason == "length" {
            self.status = "incomplete".into();
        }
        if !text.is_empty() {
            self.emit_text_start(&mut out);
            self.full_text.push_str(&text);
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "sequence_number": seq,
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "delta": text,
                }),
            );
        }
        for call in tool_calls {
            let output_index = if self.text_started {
                self.output_index.max(1)
            } else {
                self.output_index
            };
            self.output_index = output_index + 1;
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("call_tool")
                .to_string();
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": seq,
                    "output_index": output_index,
                    "item": {
                        "type": "function_call",
                        "id": call_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": "",
                        "status": "in_progress",
                    }
                }),
            );
            if arguments != "{}" {
                let seq = self.next_seq();
                push_event(
                    &mut out,
                    "response.function_call_arguments.delta",
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "sequence_number": seq,
                        "output_index": output_index,
                        "delta": arguments,
                    }),
                );
            }
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "sequence_number": seq,
                    "output_index": output_index,
                    "arguments": arguments,
                }),
            );
            let item = json!({
                "type": "function_call",
                "id": call_id,
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
                "status": "completed",
            });
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "sequence_number": seq,
                    "output_index": output_index,
                    "item": item,
                }),
            );
            self.function_outputs.push(json!({
                "type": "function_call",
                "id": call_id,
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
                "status": "completed",
            }));
        }
        out.into_bytes()
    }

    pub fn finish(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut out = String::new();
        self.ensure_started(&mut out);

        let mut final_output = Vec::new();
        if self.text_started || !self.full_text.is_empty() {
            if !self.text_started {
                self.emit_text_start(&mut out);
            }
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "sequence_number": seq,
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "text": self.full_text,
                }),
            );
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "sequence_number": seq,
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": self.full_text },
                }),
            );
            let message_item = json!({
                "type": "message",
                "id": self.message_id,
                "role": "assistant",
                "status": "completed",
                "content": [{ "type": "output_text", "text": self.full_text }],
            });
            let seq = self.next_seq();
            push_event(
                &mut out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "sequence_number": seq,
                    "output_index": 0,
                    "item": message_item,
                }),
            );
            final_output.push(json!({
                "type": "message",
                "id": self.message_id,
                "role": "assistant",
                "status": "completed",
                "content": [{ "type": "output_text", "text": self.full_text }],
            }));
        }
        final_output.extend(self.function_outputs.iter().cloned());

        let event_type = if self.status == "incomplete" {
            "response.incomplete"
        } else {
            "response.completed"
        };
        let mut response = json!({
            "id": self.response_id,
            "object": "response",
            "created_at": chrono::Utc::now().timestamp(),
            "status": self.status,
            "model": self.model,
            // Codex (and the Responses SDK) re-reads the final envelope for UI text.
            "output": final_output,
            "usage": self.usage.responses_usage(),
        });
        if self.status == "incomplete" {
            response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
        }
        let seq = self.next_seq();
        push_event(
            &mut out,
            event_type,
            json!({
                "type": event_type,
                "sequence_number": seq,
                "response": response,
            }),
        );
        self.completed = true;
        out.into_bytes()
    }

    fn ensure_started(&mut self, out: &mut String) {
        if self.started {
            return;
        }
        let response = json!({
            "id": self.response_id,
            "object": "response",
            "created_at": chrono::Utc::now().timestamp(),
            "status": "in_progress",
            "model": self.model,
            "output": [],
        });
        let seq = self.next_seq();
        push_event(
            out,
            "response.created",
            json!({ "type": "response.created", "sequence_number": seq, "response": response }),
        );
        let seq = self.next_seq();
        push_event(
            out,
            "response.in_progress",
            json!({ "type": "response.in_progress", "sequence_number": seq, "response": response }),
        );
        self.started = true;
    }

    fn emit_text_start(&mut self, out: &mut String) {
        if self.text_started {
            return;
        }
        let seq = self.next_seq();
        push_event(
            out,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": seq,
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": self.message_id,
                    "role": "assistant",
                    "status": "in_progress",
                    "content": [],
                }
            }),
        );
        let seq = self.next_seq();
        push_event(
            out,
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "sequence_number": seq,
                "item_id": self.message_id,
                "output_index": 0,
                "content_index": 0,
                "part": { "type": "output_text", "text": "" },
            }),
        );
        self.text_started = true;
        self.output_index = 1;
    }

    fn note_usage(&mut self, gemini: &Value) {
        self.usage.merge_max(GeminiUsage::parse(gemini));
    }
}

fn push_event(out: &mut String, event: &str, value: Value) {
    out.push_str("event: ");
    out.push_str(event);
    out.push_str("\ndata: ");
    out.push_str(&value.to_string());
    out.push_str("\n\n");
}

/// Minimal `/v1/responses/compact` stub — returns a compaction envelope so Codex
/// does not 404 / hang. Lossily concatenates input text; no upstream call.
pub fn responses_compact_stub(body: &Value) -> Value {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("antigravity");
    let text = body
        .get("input")
        .map(summarize_input)
        .unwrap_or_default();
    let clipped: String = text.chars().take(4000).collect();
    json!({
        "id": format!("resp_{}", Uuid::new_v4().simple()),
        "object": "response.compaction",
        "created_at": chrono::Utc::now().timestamp(),
        "status": "completed",
        "model": model,
        "output": [{
            "id": format!("msg_{}", Uuid::new_v4().simple()),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": clipped }],
        }]
    })
}

fn summarize_input(input: &Value) -> String {
    match input {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.as_str() {
                    return Some(text.to_string());
                }
                let content = item.get("content")?;
                match content {
                    Value::String(text) => Some(text.clone()),
                    Value::Array(parts) => Some(
                        parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join(""),
                    ),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_input_maps_to_gemini() {
        let body = json!({
            "model": "gemini-3.6-flash-high",
            "instructions": "be brief",
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }
            ],
            "reasoning": { "effort": "high" },
            "stream": false,
        });
        let parts = responses_to_gemini_request(&body, None).unwrap();
        assert!(parts.model.contains("flash"));
        assert!(parts.model.contains("high") || parts.model.ends_with("-high") || parts.model.contains("flash"));
        assert!(parts.request.get("systemInstruction").is_some());
        assert!(!parts.stream);
    }

    #[test]
    fn function_call_roundtrip_shapes() {
        let body = json!({
            "model": "gemini-3.6-flash-low",
            "input": [
                { "role": "user", "content": "hi" },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"a.txt\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file body"
                }
            ],
            "tools": [{
                "name": "read_file",
                "description": "read",
                "parameters": { "type": "object", "properties": { "path": { "type": "string" } } }
            }]
        });
        let parts = responses_to_gemini_request(&body, None).unwrap();
        let tools = parts.request.get("tools").unwrap();
        assert!(tools[0]["functionDeclarations"][0]["name"] == "read_file");
        let contents = parts.request["contents"].as_array().unwrap();
        assert!(contents.iter().any(|c| {
            c["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|p| p.get("functionResponse").is_some())
        }));
    }

    #[test]
    fn stream_encoder_emits_created_and_completed() {
        let mut enc = ResponsesStreamEncoder::new("gemini-3.6-flash-high", ToolParamKeys::new(), None);
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "hello" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "promptTokenCount": 3, "candidatesTokenCount": 1 }
        });
        let mid = String::from_utf8(enc.encode_gemini_chunk(&chunk)).unwrap();
        assert!(mid.contains("response.created"));
        assert!(mid.contains("response.output_text.delta"));
        assert!(mid.contains("hello"));
        let end = String::from_utf8(enc.finish()).unwrap();
        assert!(end.contains("response.output_text.done"));
        assert!(end.contains("response.completed"));
        assert!(
            end.contains("\"total_tokens\":4") || end.contains("\"total_tokens\": 4"),
            "Codex requires total_tokens on ResponseCompleted: {end}"
        );
        // Final envelope must carry the assistant text — Codex UI re-reads it.
        assert!(
            end.contains("\"text\":\"hello\"") || end.contains("\"text\": \"hello\""),
            "completed.output must include text: {end}"
        );
    }

    #[test]
    fn stream_encoder_counts_thoughts_in_output_and_reasoning_details() {
        let mut enc = ResponsesStreamEncoder::new("gemini-3.7-flash-high", ToolParamKeys::new(), None);
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "ok" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 1500,
                "candidatesTokenCount": 20,
                "thoughtsTokenCount": 80
            }
        });
        let _ = enc.encode_gemini_chunk(&chunk);
        let end = String::from_utf8(enc.finish()).unwrap();
        assert!(
            end.contains("\"output_tokens\":20") || end.contains("\"output_tokens\": 20"),
            "classic candidatesTokenCount already includes thoughts: {end}"
        );
        assert!(
            end.contains("\"reasoning_tokens\":80") || end.contains("\"reasoning_tokens\": 80"),
            "reasoning_tokens should carry thoughtsTokenCount: {end}"
        );
        assert!(
            end.contains("\"total_tokens\":1520") || end.contains("\"total_tokens\": 1520"),
            "total should be prompt + candidates: {end}"
        );
    }

    #[test]
    fn compact_stub_returns_compaction_object() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "input": [{ "role": "user", "content": "long context here" }]
        });
        let compact = responses_compact_stub(&body);
        assert_eq!(compact["object"], "response.compaction");
        assert_eq!(compact["status"], "completed");
    }

    #[test]
    fn historical_function_calls_get_thought_signature() {
        let body = json!({
            "model": "gemini-3.6-flash-low",
            "input": [
                { "role": "user", "content": "hi" },
                {
                    "type": "function_call",
                    "call_id": "call_resp_sig",
                    "name": "read_file",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_resp_sig",
                    "output": "ok"
                }
            ]
        });
        let parts = responses_to_gemini_request(&body, None).unwrap();
        let model_parts = parts.request["contents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|content| content["role"] == "model")
            .unwrap();
        assert_eq!(
            model_parts["parts"][0]["thoughtSignature"],
            json!(crate::antigravity::thought_sig::SKIP_VALIDATOR_SENTINEL)
        );
    }
}
