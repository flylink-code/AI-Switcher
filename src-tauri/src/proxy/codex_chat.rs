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
    super::convert::sanitize_openai_chat_messages(&mut messages);
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

/// Convert an OpenAI Chat Completions request into a Responses JSON body.
pub fn chat_completions_to_responses_body(body: &Value) -> Result<Value, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "Chat Completions 请求缺少 messages 数组".to_string())?;

    let mut instructions_parts: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();
    let mut seen_conversation = false;

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("user");
        match role {
            "system" | "developer" => {
                let text = output_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.trim().is_empty() {
                    if !seen_conversation {
                        instructions_parts.push(text);
                    } else {
                        input.push(json!({
                            "type": "message",
                            "role": "user",
                            "content": [{ "type": "input_text", "text": format!("<system-reminder>\n{text}\n</system-reminder>") }]
                        }));
                    }
                }
            }
            "user" => {
                seen_conversation = true;
                let mut content_blocks: Vec<Value> = Vec::new();
                match msg.get("content") {
                    Some(Value::String(s)) if !s.is_empty() => {
                        content_blocks.push(json!({ "type": "input_text", "text": s }));
                    }
                    Some(Value::Array(items)) => {
                        for item in items {
                            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                            match item_type {
                                "text" | "input_text" => {
                                    if let Some(t) = item.get("text").and_then(Value::as_str) {
                                        if !t.is_empty() {
                                            content_blocks.push(json!({ "type": "input_text", "text": t }));
                                        }
                                    }
                                }
                                "image_url" | "input_image" => {
                                    let url = item
                                        .pointer("/image_url/url")
                                        .or_else(|| item.get("image_url"))
                                        .and_then(Value::as_str);
                                    if let Some(url) = url {
                                        content_blocks.push(json!({
                                            "type": "input_image",
                                            "image_url": url
                                        }));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                if !content_blocks.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content_blocks
                    }));
                }
            }
            "assistant" => {
                seen_conversation = true;
                if let Some(reasoning) = msg
                    .get("reasoning_content")
                    .or_else(|| msg.get("reasoning"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    input.push(json!({
                        "type": "reasoning",
                        "summary": [{ "type": "summary_text", "text": reasoning }]
                    }));
                }
                let text = output_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }]
                    }));
                }
                if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let call_id = call
                            .get("id")
                            .or_else(|| call.get("call_id"))
                            .and_then(Value::as_str)
                            .unwrap_or("call_tool");
                        let name = call
                            .pointer("/function/name")
                            .or_else(|| call.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("tool");
                        let arguments = match call
                            .pointer("/function/arguments")
                            .or_else(|| call.get("arguments"))
                        {
                            Some(Value::String(s)) => s.clone(),
                            Some(other) if !other.is_null() => other.to_string(),
                            _ => "{}".to_string(),
                        };
                        input.push(json!({
                            "type": "function_call",
                            "call_id": call_id,
                            "name": name,
                            "arguments": arguments
                        }));
                    }
                }
            }
            "tool" => {
                seen_conversation = true;
                let call_id = msg
                    .get("tool_call_id")
                    .or_else(|| msg.get("call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("call_tool");
                let out = output_text(msg.get("content").unwrap_or(&Value::Null));
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": out
                }));
            }
            _ => {}
        }
    }

    if input.is_empty() {
        return Err("Chat Completions 请求未包含可转换的对话消息".to_string());
    }

    let mut result = json!({
        "model": model,
        "input": input,
        "stream": stream,
    });
    if !instructions_parts.is_empty() {
        result["instructions"] = Value::String(instructions_parts.join("\n\n"));
    }
    if let Some(max) = body
        .get("max_completion_tokens")
        .or_else(|| body.get("max_tokens"))
        .or_else(|| body.get("max_output_tokens"))
    {
        result["max_output_tokens"] = max.clone();
    }
    copy_if_present(body, &mut result, "temperature");
    copy_if_present(body, &mut result, "top_p");
    if let Some(effort) = extract_responses_effort(body) {
        result["reasoning"] = json!({ "effort": effort });
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let mut converted_tools: Vec<Value> = Vec::new();
        for tool in tools {
            if let Some(func) = tool.get("function").and_then(Value::as_object) {
                if let Some(name) = func.get("name").and_then(Value::as_str) {
                    let mut entry = json!({
                        "type": "function",
                        "name": name,
                        "parameters": func.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}}))
                    });
                    if let Some(desc) = func.get("description").and_then(Value::as_str) {
                        entry["description"] = Value::String(desc.to_string());
                    }
                    converted_tools.push(entry);
                }
            } else if tool.get("name").and_then(Value::as_str).is_some() {
                converted_tools.push(tool.clone());
            }
        }
        if !converted_tools.is_empty() {
            result["tools"] = Value::Array(converted_tools);
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        if let Some(func_name) = choice.pointer("/function/name").and_then(Value::as_str) {
            result["tool_choice"] = json!({ "type": "function", "name": func_name });
        } else {
            result["tool_choice"] = choice.clone();
        }
    }
    Ok(result)
}

pub fn chat_response_to_responses(body: &Value, fallback_model: &str) -> Value {
    let message = body
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or(json!({}));
    let mut output = Vec::new();
    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        output.push(json!({
            "type": "reasoning",
            "id": format!("rs_{}", Uuid::new_v4().simple()),
            "summary": [{ "type": "summary_text", "text": reasoning }],
        }));
    }
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
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .unwrap_or("tool_call");
            output.push(json!({
                "type": "function_call",
                "id": format!("fc_{call_id}"),
                "call_id": call_id,
                "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or("tool"),
                "arguments": call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}"),
                "status": "completed",
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
        let input_tokens = usage
            .get("prompt_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let output_tokens = usage
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        result["usage"] = json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": usage
                .get("total_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(input_tokens.saturating_add(output_tokens)),
        });
    } else {
        result["usage"] = json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
        });
    }
    result
}

pub fn is_unsupported_content_type_error(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("unsupported content type") || text.contains("unsupported_content_type")
}

struct PendingChatToolCall {
    output_index: usize,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}

/// Incrementally convert Chat Completions SSE into Responses SSE.
pub struct ChatSseToResponsesConverter {
    model: String,
    response_id: String,
    started: bool,
    completed: bool,
    next_output_index: usize,
    reasoning_started: bool,
    reasoning_output_index: usize,
    reasoning_item_id: String,
    reasoning: String,
    text_started: bool,
    text_output_index: usize,
    text_item_id: String,
    text: String,
    tools: Vec<PendingChatToolCall>,
    tool_indices: std::collections::BTreeMap<usize, usize>,
    last_usage: Option<Value>,
}

impl ChatSseToResponsesConverter {
    pub fn new(fallback_model: &str) -> Self {
        Self {
            model: fallback_model.to_string(),
            response_id: format!("resp_{}", Uuid::new_v4().simple()),
            started: false,
            completed: false,
            next_output_index: 0,
            reasoning_started: false,
            reasoning_output_index: 0,
            reasoning_item_id: format!("rs_{}", Uuid::new_v4().simple()),
            reasoning: String::new(),
            text_started: false,
            text_output_index: 0,
            text_item_id: format!("msg_{}", Uuid::new_v4().simple()),
            text: String::new(),
            tools: Vec::new(),
            tool_indices: std::collections::BTreeMap::new(),
            last_usage: None,
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
        if !self.started {
            if let Some(id) = chunk.get("id").and_then(Value::as_str) {
                if !id.is_empty() {
                    self.response_id = id.to_string();
                }
            }
        }
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            self.last_usage = Some(usage.clone());
        }
        self.ensure_started(&mut out);
        let delta = chunk.pointer("/choices/0/delta").unwrap_or(&Value::Null);
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
        {
            self.reasoning.push_str(reasoning);
            self.emit_reasoning_start(&mut out);
            push_responses_event(
                &mut out,
                "response.reasoning_summary_text.delta",
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": self.reasoning_item_id,
                    "output_index": self.reasoning_output_index,
                    "summary_index": 0,
                    "delta": reasoning,
                }),
            );
        }
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
        {
            self.text.push_str(text);
            self.emit_text_start(&mut out);
            push_responses_event(
                &mut out,
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "item_id": self.text_item_id,
                    "output_index": self.text_output_index,
                    "content_index": 0,
                    "delta": text,
                }),
            );
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let upstream_idx = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let call_id_opt = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let name_opt = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let tool_pos = if let Some(&pos) = self.tool_indices.get(&upstream_idx) {
                    if let Some(cid) = call_id_opt {
                        self.tools[pos].call_id = cid.to_string();
                    }
                    if let Some(name) = name_opt {
                        self.tools[pos].name = name.to_string();
                    }
                    pos
                } else {
                    let output_index = self.next_output_index;
                    self.next_output_index += 1;
                    let call_id = call_id_opt
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
                    let item_id = format!("fc_{call_id}");
                    let name = name_opt.unwrap_or("tool").to_string();
                    let pos = self.tools.len();
                    self.tool_indices.insert(upstream_idx, pos);
                    self.tools.push(PendingChatToolCall {
                        output_index,
                        item_id: item_id.clone(),
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    });
                    push_responses_event(
                        &mut out,
                        "response.output_item.added",
                        json!({
                            "type": "response.output_item.added",
                            "output_index": output_index,
                            "item": {
                                "id": item_id,
                                "type": "function_call",
                                "call_id": call_id,
                                "name": name,
                                "arguments": "",
                                "status": "in_progress",
                            }
                        }),
                    );
                    pos
                };
                if let Some(args_delta) = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    let output_index = self.tools[tool_pos].output_index;
                    let item_id = self.tools[tool_pos].item_id.clone();
                    let call_id = self.tools[tool_pos].call_id.clone();
                    self.tools[tool_pos].arguments.push_str(args_delta);
                    push_responses_event(
                        &mut out,
                        "response.function_call_arguments.delta",
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "item_id": item_id,
                            "output_index": output_index,
                            "call_id": call_id,
                            "delta": args_delta,
                        }),
                    );
                }
            }
        }
        if let Some(reason) = chunk
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty() && *reason != "null")
        {
            let usage = chunk.get("usage").cloned().or_else(|| self.last_usage.clone());
            self.finish(&mut out, reason, usage.as_ref());
        }
        out.into_bytes()
    }

    pub fn finish_done(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut out = String::new();
        self.ensure_started(&mut out);
        let usage = self.last_usage.clone();
        self.finish(&mut out, "stop", usage.as_ref());
        out.into_bytes()
    }

    pub fn start_handshake(&mut self) -> Vec<u8> {
        let mut out = String::new();
        self.ensure_started(&mut out);
        self.emit_reasoning_start(&mut out);
        out.into_bytes()
    }

    fn ensure_started(&mut self, out: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        let response = json!({
            "id": self.response_id,
            "object": "response",
            "status": "in_progress",
            "model": self.model,
            "output": [],
        });
        push_responses_event(
            out,
            "response.created",
            json!({
                "type": "response.created",
                "response": response,
            }),
        );
        push_responses_event(
            out,
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "response": response,
            }),
        );
    }

    fn emit_reasoning_start(&mut self, out: &mut String) {
        if self.reasoning_started {
            return;
        }
        self.reasoning_started = true;
        self.reasoning_output_index = self.next_output_index;
        self.next_output_index += 1;
        push_responses_event(
            out,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": self.reasoning_output_index,
                "item": {
                    "id": self.reasoning_item_id,
                    "type": "reasoning",
                    "summary": [],
                }
            }),
        );
        push_responses_event(
            out,
            "response.reasoning_summary_part.added",
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": self.reasoning_item_id,
                "output_index": self.reasoning_output_index,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": "" },
            }),
        );
    }

    fn emit_text_start(&mut self, out: &mut String) {
        if self.text_started {
            return;
        }
        self.text_started = true;
        self.text_output_index = self.next_output_index;
        self.next_output_index += 1;
        push_responses_event(
            out,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": self.text_output_index,
                "item": {
                    "type": "message",
                    "id": self.text_item_id,
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
                "item_id": self.text_item_id,
                "output_index": self.text_output_index,
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
        let reasoning = std::mem::take(&mut self.reasoning);
        let text = std::mem::take(&mut self.text);
        let mut indexed_output: Vec<(usize, Value)> = Vec::new();
        if self.reasoning_started || !reasoning.is_empty() {
            let item = json!({
                "id": self.reasoning_item_id.clone(),
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": reasoning.clone()}],
            });
            push_responses_event(
                out,
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": self.reasoning_item_id,
                    "output_index": self.reasoning_output_index,
                    "summary_index": 0,
                    "text": reasoning,
                }),
            );
            push_responses_event(
                out,
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": self.reasoning_item_id,
                    "output_index": self.reasoning_output_index,
                    "summary_index": 0,
                    "part": {"type": "summary_text", "text": reasoning},
                }),
            );
            push_responses_event(
                out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": self.reasoning_output_index,
                    "item": item,
                }),
            );
            indexed_output.push((self.reasoning_output_index, item));
        }
        if self.text_started || !text.is_empty() {
            let item = json!({
                "type": "message",
                "id": self.text_item_id.clone(),
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": text.clone()}],
            });
            push_responses_event(
                out,
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": self.text_item_id,
                    "output_index": self.text_output_index,
                    "content_index": 0,
                    "text": text.clone(),
                }),
            );
            push_responses_event(
                out,
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "item_id": self.text_item_id,
                    "output_index": self.text_output_index,
                    "content_index": 0,
                    "part": {"type": "output_text", "text": text.clone()},
                }),
            );
            push_responses_event(
                out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": self.text_output_index,
                    "item": item,
                }),
            );
            indexed_output.push((self.text_output_index, item));
        }
        for tool in &self.tools {
            let item = json!({
                "id": tool.item_id,
                "type": "function_call",
                "call_id": tool.call_id,
                "name": tool.name,
                "arguments": tool.arguments,
                "status": "completed",
            });
            push_responses_event(
                out,
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": tool.item_id,
                    "output_index": tool.output_index,
                    "call_id": tool.call_id,
                    "name": tool.name,
                    "arguments": tool.arguments,
                }),
            );
            push_responses_event(
                out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": tool.output_index,
                    "item": item,
                }),
            );
            indexed_output.push((tool.output_index, item));
        }
        indexed_output.sort_by_key(|(idx, _)| *idx);
        let output: Vec<Value> = indexed_output.into_iter().map(|(_, item)| item).collect();
        let mut response = json!({
            "id": self.response_id.clone(),
            "object": "response",
            "status": status,
            "model": self.model.clone(),
            "output": output,
        });
        let (input_tokens, output_tokens, total_tokens) = if let Some(usage) = usage {
            let input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total_tokens = usage
                .get("total_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(input_tokens.saturating_add(output_tokens));
            (input_tokens, output_tokens, total_tokens)
        } else {
            (0, 0, 0)
        };
        response["usage"] = json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens,
        });
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
                        let arguments = match item.get("arguments") {
                            Some(Value::String(s)) => s.clone(),
                            Some(other) if !other.is_null() => other.to_string(),
                            _ => "{}".into(),
                        };
                        pending_tool_calls.push(json!({
                            "id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("call_tool"),
                            "type": "function",
                            "function": {
                                "name": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
                                "arguments": arguments,
                            }
                        }));
                    }
                    Some("function_call_output") => {
                        flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("call_tool"),
                            "content": output_text(item.get("output").unwrap_or(&Value::Null)),
                        }));
                    }
                    Some("reasoning") | Some("computer_call") | Some("computer_call_output") => {}
                    _ if item.get("role").is_some() => {
                        let role = match item.get("role").and_then(Value::as_str).unwrap_or("user")
                        {
                            "assistant" => "assistant",
                            "system" | "developer" => "system",
                            _ => "user",
                        };
                        let content =
                            convert_responses_content(item.get("content").unwrap_or(&Value::Null));
                        if role == "assistant" {
                            append_assistant_message(
                                &mut messages,
                                &mut pending_tool_calls,
                                content,
                            );
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
                && last
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
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
                    _ => block
                        .as_str()
                        .map(|text| json!({ "type": "text", "text": text })),
                })
                .collect();
            if parts.len() == 1 {
                if let Some(text) = parts[0].get("text") {
                    return text.clone();
                }
            }
            Value::Array(parts)
        }
        Value::Object(map) => map
            .get("text")
            .filter(|text| text.is_string())
            .cloned()
            .unwrap_or(Value::Null),
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
            .or_else(|| map.get("output"))
            .or_else(|| map.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
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
    let raw_desc = tool
        .get("description")
        .or_else(|| tool.pointer("/function/description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let description = super::codex_anthropic::enrich_edit_tool_description(name, raw_desc);
    let mut function = json!({ "name": name, "parameters": parameters });
    if !description.is_empty() {
        function["description"] = Value::String(description);
    }
    Some(json!({
        "type": "function",
        "function": function
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
        assert!(!is_unsupported_content_type_error(
            br#"{"error":{"message":"rate limit"}}"#
        ));
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
        let mut calls = vec![
            json!({ "id": "call_2", "type": "function", "function": { "name": "second", "arguments": "{}" } }),
        ];
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
            ]))
            .unwrap();
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
