//! Translate Anthropic Messages, OpenAI Chat, and OpenAI Responses into a Kiro body,
//! then turn AWS event-stream payloads back into those wire formats.

use serde_json::{json, Value};
use uuid::Uuid;

use super::account::{streaming_profile_arn, KiroAccount};
use super::event_stream::Frame;
use super::models::{clamped_thinking_budget, map_model, thinking_requested, DEFAULT_MODEL};

/// Kiro rejects `userInputMessage.content == ""` with REQUEST_BODY_INVALID.
/// Claude Code tool-result turns often have no text block.
const TOOL_TURN_PLACEHOLDER: &str = "tool result";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireProtocol {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
}

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    pub client_model: String,
    pub backend_model: String,
    pub body: Value,
    pub stream: bool,
}

pub fn prepare(
    protocol: WireProtocol,
    incoming: &Value,
    account: &KiroAccount,
) -> Result<PreparedRequest, String> {
    let client_model = incoming
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(DEFAULT_MODEL)
        .trim()
        .to_string();
    let backend_model = map_model(&client_model).ok_or_else(|| "模型名无效".to_string())?;
    let stream = incoming
        .get("stream")
        .and_then(|item| item.as_bool())
        .unwrap_or(protocol == WireProtocol::Anthropic);
    let (history, current, tools) = match protocol {
        WireProtocol::Anthropic => anthropic_turns(incoming, &backend_model)?,
        WireProtocol::OpenAiChat => openai_chat_turns(incoming, &backend_model)?,
        WireProtocol::OpenAiResponses => responses_turns(incoming, &backend_model)?,
    };
    let mut current_message = json!({
        "userInputMessage": {
            "content": current.content,
            "modelId": backend_model,
            "origin": "AI_EDITOR",
            "userInputMessageContext": {
                "envState": {
                    "operatingSystem": std::env::consts::OS,
                    "currentWorkingDirectory": "/"
                }
            }
        }
    });
    if !tools.is_empty() || !current.tool_results.is_empty() {
        let context = current_message["userInputMessage"]["userInputMessageContext"]
            .as_object_mut()
            .ok_or_else(|| "Kiro 请求上下文无效".to_string())?;
        if !tools.is_empty() {
            context.insert("tools".to_string(), Value::Array(tools));
        }
        if !current.tool_results.is_empty() {
            context.insert(
                "toolResults".to_string(),
                Value::Array(current.tool_results),
            );
        }
    }
    if !current.images.is_empty() {
        current_message["userInputMessage"]["images"] = Value::Array(current.images);
    }
    let mut body = json!({
        "conversationState": {
            "conversationId": Uuid::new_v4().to_string(),
            "agentTaskType": "vibe",
            "chatTriggerType": "MANUAL",
            "currentMessage": current_message,
            "history": history
        },
        "profileArn": streaming_profile_arn(account)
    });
    if let Some(extra) = extra_fields(incoming, &backend_model) {
        body["additionalModelRequestFields"] = extra;
    }
    Ok(PreparedRequest {
        client_model,
        backend_model,
        body,
        stream,
    })
}

struct CurrentTurn {
    content: String,
    tool_results: Vec<Value>,
    images: Vec<Value>,
}

fn anthropic_turns(
    incoming: &Value,
    model: &str,
) -> Result<(Vec<Value>, CurrentTurn, Vec<Value>), String> {
    let tools = incoming
        .get("tools")
        .and_then(|item| item.as_array())
        .map(|tools| tools.iter().filter_map(anthropic_tool).collect())
        .unwrap_or_default();
    let mut messages = incoming
        .get("messages")
        .and_then(|item| item.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(system) = text_of(incoming.get("system")) {
        if !system.is_empty() {
            messages.insert(0, json!({"role": "user", "content": system}));
        }
    }
    let (history, current, _) = turns_from_messages(&messages, model, true)?;
    Ok((history, current, tools))
}

fn openai_chat_turns(
    incoming: &Value,
    model: &str,
) -> Result<(Vec<Value>, CurrentTurn, Vec<Value>), String> {
    let tools = incoming
        .get("tools")
        .and_then(|item| item.as_array())
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.get("function").or(Some(tool)))
                .filter_map(openai_tool)
                .collect()
        })
        .unwrap_or_default();
    let messages = incoming
        .get("messages")
        .and_then(|item| item.as_array())
        .cloned()
        .unwrap_or_default();
    let (history, current, _) = turns_from_messages(&messages, model, false)?;
    Ok((history, current, tools))
}

fn responses_turns(
    incoming: &Value,
    model: &str,
) -> Result<(Vec<Value>, CurrentTurn, Vec<Value>), String> {
    if let Some(input) = incoming.get("input").and_then(|item| item.as_str()) {
        return Ok((
            Vec::new(),
            CurrentTurn {
                content: input.to_string(),
                tool_results: Vec::new(),
                images: Vec::new(),
            },
            Vec::new(),
        ));
    }
    let mut messages = Vec::new();
    if let Some(items) = incoming.get("input").and_then(|item| item.as_array()) {
        for item in items {
            let role = item
                .get("role")
                .and_then(|role| role.as_str())
                .unwrap_or("user");
            let content = text_of(item.get("content")).unwrap_or_default();
            messages.push(json!({"role": role, "content": content}));
        }
    }
    if messages.is_empty() {
        messages.push(json!({"role": "user", "content": ""}));
    }
    turns_from_messages(&messages, model, false)
}

fn turns_from_messages(
    messages: &[Value],
    model: &str,
    anthropic: bool,
) -> Result<(Vec<Value>, CurrentTurn, Vec<Value>), String> {
    if messages.is_empty() {
        return Err("请求没有消息".into());
    }
    let last = messages.len() - 1;
    let mut history = Vec::new();
    for message in &messages[..last] {
        if let Some(entry) = history_message(message, model, anthropic) {
            history.push(entry);
        }
    }
    let current = current_from(messages.last().unwrap(), anthropic);
    Ok((history, current, Vec::new()))
}

fn history_message(message: &Value, model: &str, anthropic: bool) -> Option<Value> {
    let role = message
        .get("role")
        .and_then(|role| role.as_str())
        .unwrap_or("user");
    if role == "assistant" {
        let content = text_of(message.get("content")).unwrap_or_default();
        let tool_uses = if anthropic {
            anthropic_tool_uses(message.get("content"))
        } else {
            openai_tool_uses(message)
        };
        if content.trim().is_empty() && tool_uses.is_empty() {
            return None;
        }
        let mut assistant = json!({ "content": content });
        if !tool_uses.is_empty() {
            assistant["toolUses"] = Value::Array(tool_uses);
        }
        return Some(json!({ "assistantResponseMessage": assistant }));
    }
    let mut content = message_text(message);
    let tool_results = if anthropic {
        anthropic_tool_results(message.get("content"))
    } else {
        Vec::new()
    };
    if content.trim().is_empty() {
        if tool_results.is_empty() {
            return None;
        }
        content = TOOL_TURN_PLACEHOLDER.to_string();
    }
    let mut user = json!({
        "userInputMessage": {
            "content": content,
            "modelId": model,
            "origin": "AI_EDITOR"
        }
    });
    if !tool_results.is_empty() {
        user["userInputMessage"]["userInputMessageContext"]["toolResults"] =
            Value::Array(tool_results);
    }
    Some(user)
}

fn current_from(message: &Value, anthropic: bool) -> CurrentTurn {
    let raw = message.get("content");
    let tool_results = if anthropic {
        anthropic_tool_results(raw)
    } else {
        Vec::new()
    };
    let mut content = message_text(message);
    if content.trim().is_empty() && !tool_results.is_empty() {
        content = TOOL_TURN_PLACEHOLDER.to_string();
    }
    CurrentTurn {
        content,
        tool_results,
        images: anthropic_images(raw),
    }
}

fn message_text(message: &Value) -> String {
    text_of(message.get("content")).unwrap_or_else(|| {
        message
            .get("content")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string()
    })
}

fn text_of(content: Option<&Value>) -> Option<String> {
    let content = content?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let blocks = content.as_array()?;
    let mut text = String::new();
    for block in blocks {
        let kind = block
            .get("type")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        if kind == "text" || kind.is_empty() {
            if let Some(piece) = block.get("text").and_then(|item| item.as_str()) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(piece);
            }
        }
    }
    Some(text)
}

fn anthropic_tool(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(|item| item.as_str())?;
    Some(json!({
        "toolSpecification": {
            "name": name,
            "description": tool.get("description").and_then(|item| item.as_str()).unwrap_or(""),
            "inputSchema": { "json": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object"})) }
        }
    }))
}

fn openai_tool(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(|item| item.as_str())?;
    Some(json!({
        "toolSpecification": {
            "name": name,
            "description": tool.get("description").and_then(|item| item.as_str()).unwrap_or(""),
            "inputSchema": { "json": tool.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"})) }
        }
    }))
}

fn anthropic_tool_uses(content: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = content.and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|item| item.as_str()) == Some("tool_use"))
        .filter_map(|block| {
            Some(json!({
                "toolUseId": block.get("id").and_then(|item| item.as_str())?,
                "name": block.get("name").and_then(|item| item.as_str())?,
                "input": block.get("input").cloned().unwrap_or_else(|| json!({}))
            }))
        })
        .collect()
}

fn openai_tool_uses(message: &Value) -> Vec<Value> {
    message
        .get("tool_calls")
        .and_then(|item| item.as_array())
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let function = call.get("function")?;
                    let arguments = function
                        .get("arguments")
                        .and_then(|item| item.as_str())
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .unwrap_or_else(|| json!({}));
                    Some(json!({
                        "toolUseId": call.get("id").and_then(|item| item.as_str()).unwrap_or("call"),
                        "name": function.get("name").and_then(|item| item.as_str())?,
                        "input": arguments
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn anthropic_tool_results(content: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = content.and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|item| item.as_str()) == Some("tool_result"))
        .filter_map(|block| {
            let text = text_of(block.get("content")).unwrap_or_default();
            Some(json!({
                "toolUseId": block.get("tool_use_id").and_then(|item| item.as_str())?,
                "content": [{"text": text}],
                "status": if block.get("is_error").and_then(|item| item.as_bool()).unwrap_or(false) { "error" } else { "success" }
            }))
        })
        .collect()
}

fn anthropic_images(content: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = content.and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|item| item.as_str()) == Some("image"))
        .filter_map(|block| {
            let source = block.get("source")?;
            let data = source
                .get("data")
                .or_else(|| source.get("bytes"))
                .and_then(|item| item.as_str())?;
            let format = source
                .get("media_type")
                .and_then(|item| item.as_str())
                .unwrap_or("image/png")
                .rsplit('/')
                .next()
                .unwrap_or("png");
            Some(json!({"format": format, "source": {"bytes": data}}))
        })
        .collect()
}

/// Models confirmed to accept `additionalModelRequestFields`.
/// Haiku 4.5, Sonnet 4.5/4.8/5.5 and unknown ids return
/// `additionalModelRequestFields is not supported for this model`.
fn model_supports_kiro_reasoning(model_id: &str) -> bool {
    matches!(
        model_id.to_ascii_lowercase().as_str(),
        "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
            | "claude-opus-4.6"
            | "claude-opus-4.7"
            | "claude-opus-4.8"
            | "claude-sonnet-4.6"
            | "claude-sonnet-5"
            | "claude-fable-5"
            | "claude-mythos-5"
    )
}

fn model_uses_gpt_reasoning(model_id: &str) -> bool {
    matches!(
        model_id.to_ascii_lowercase().as_str(),
        "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna"
    )
}

fn kiro_reasoning_requested(incoming: &Value, model_id: &str) -> bool {
    let thinking_type = incoming
        .pointer("/thinking/type")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if thinking_type.eq_ignore_ascii_case("disabled") {
        return false;
    }
    // Opus 4.6 only accepts the field on adaptive thinking.
    if model_id.eq_ignore_ascii_case("claude-opus-4.6") {
        return thinking_type.eq_ignore_ascii_case("adaptive");
    }
    thinking_requested(incoming)
        || incoming
            .pointer("/output_config/effort")
            .and_then(|item| item.as_str())
            .is_some_and(|effort| !effort.trim().is_empty())
}

fn extra_fields(incoming: &Value, backend_model: &str) -> Option<Value> {
    if !model_supports_kiro_reasoning(backend_model)
        || !kiro_reasoning_requested(incoming, backend_model)
    {
        return None;
    }
    let effort = if incoming
        .pointer("/thinking/type")
        .and_then(|item| item.as_str())
        == Some("adaptive")
        || backend_model.contains("opus-4.6")
    {
        "high"
    } else {
        match clamped_thinking_budget(incoming).unwrap_or(20_000) {
            0..=4_000 => "low",
            4_001..=10_000 => "medium",
            _ => "high",
        }
    };
    if model_uses_gpt_reasoning(backend_model) {
        Some(json!({"reasoning": {"effort": effort}}))
    } else {
        Some(json!({"output_config": {"effort": effort}}))
    }
}

#[derive(Debug, Default)]
pub struct CollectedReply {
    pub text: String,
    pub thinking: String,
    pub thinking_signature: String,
    pub tools: Vec<ToolCall>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// `contextUsageEvent.contextUsagePercentage` (0–100). Exact `tokenUsage` wins.
    context_percent: Option<f64>,
    usage_measured: bool,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: String,
}

impl CollectedReply {
    pub fn push_frame(&mut self, frame: &Frame) {
        let Ok(payload) = serde_json::from_slice::<Value>(&frame.payload) else {
            return;
        };
        match frame.event_type.as_str() {
            "assistantResponseEvent" => {
                if let Some(content) = payload.get("content").and_then(|item| item.as_str()) {
                    self.text.push_str(content);
                }
            }
            "reasoningContentEvent" => {
                if let Some(text) = payload.get("text").and_then(|item| item.as_str()) {
                    self.thinking.push_str(text);
                }
                if let Some(signature) = payload.get("signature").and_then(|item| item.as_str()) {
                    self.thinking_signature = signature.to_string();
                }
            }
            "toolUseEvent" => {
                let id = payload
                    .get("toolUseId")
                    .and_then(|item| item.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let name = payload
                    .get("name")
                    .and_then(|item| item.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = match payload.get("input") {
                    Some(Value::String(text)) => text.clone(),
                    Some(other) if !other.is_null() => other.to_string(),
                    _ => String::new(),
                };
                if let Some(existing) = self.tools.iter_mut().find(|tool| tool.id == id) {
                    existing.input.push_str(&input);
                    if !name.is_empty() {
                        existing.name = name;
                    }
                } else {
                    self.tools.push(ToolCall { id, name, input });
                }
            }
            "metadataEvent" => {
                if let Some(usage) = payload
                    .get("tokenUsage")
                    .or_else(|| payload.pointer("/metadataEvent/tokenUsage"))
                    .or_else(|| payload.pointer("/messageMetadataEvent/tokenUsage"))
                {
                    self.apply_token_usage(usage);
                }
            }
            "contextUsageEvent" => {
                if let Some(percent) = json_f64(payload.get("contextUsagePercentage")) {
                    self.context_percent = Some(percent);
                }
            }
            _ => {
                if let Some(usage) = payload.get("tokenUsage") {
                    self.apply_token_usage(usage);
                } else if let Some(percent) = json_f64(payload.get("contextUsagePercentage")) {
                    self.context_percent = Some(percent);
                }
            }
        }
    }

    /// Fill tokens the client and the smart gateway can store.
    ///
    /// Kiro often omits `metadataEvent`. `contextUsagePercentage` is
    /// `totalTokens / contextWindow * 100` for the whole turn, so the reply
    /// text is split out as output and the rest is input. With neither signal,
    /// a short local estimate is used so a finished reply is not logged as
    /// "upstream did not provide usage".
    pub fn finalize_usage(&mut self, context_window: u64, request_chars: usize) {
        if self.usage_measured
            && (self.input_tokens > 0
                || self.output_tokens > 0
                || self.cache_read > 0
                || self.cache_write > 0)
        {
            return;
        }
        if let Some(percent) = self.context_percent.filter(|value| *value > 0.0) {
            if context_window > 0 {
                let total = ((percent / 100.0) * context_window as f64)
                    .round()
                    .max(0.0) as i64;
                let output = estimate_tokens(&self.generated_text()).min(total);
                self.output_tokens = output;
                self.input_tokens = total.saturating_sub(output);
                return;
            }
        }
        let output = estimate_tokens(&self.generated_text());
        let input = estimate_chars(request_chars);
        if input > 0 || output > 0 {
            self.input_tokens = input;
            self.output_tokens = output;
        }
    }

    fn apply_token_usage(&mut self, usage: &Value) {
        let mut saw = false;
        if let Some(value) = json_i64_field(usage, &["uncachedInputTokens", "inputTokens"]) {
            self.input_tokens = value;
            saw = true;
        }
        if let Some(value) = json_i64_field(usage, &["outputTokens"]) {
            self.output_tokens = value;
            saw = true;
        }
        if let Some(value) = json_i64_field(usage, &["cacheReadInputTokens"]) {
            self.cache_read = value;
            saw = true;
        }
        if let Some(value) = json_i64_field(usage, &["cacheWriteInputTokens"]) {
            self.cache_write = value;
            saw = true;
        }
        if self.input_tokens == 0 && self.cache_read == 0 && self.cache_write == 0 {
            if let Some(total) = json_i64_field(usage, &["totalTokens"]) {
                if total > self.output_tokens {
                    self.input_tokens = total
                        .saturating_sub(self.output_tokens)
                        .saturating_sub(self.cache_read)
                        .saturating_sub(self.cache_write);
                }
                saw = true;
            }
        }
        if let Some(percent) = json_f64(usage.get("contextUsagePercentage")) {
            self.context_percent = Some(percent);
        }
        if saw {
            self.usage_measured = true;
        }
    }

    fn generated_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&self.text);
        text.push_str(&self.thinking);
        for tool in &self.tools {
            text.push_str(&tool.name);
            text.push_str(&tool.input);
        }
        text
    }
}

fn json_i64_field(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| json_i64(value.get(*key)))
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .map(|number| number.round() as i64)
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_u64().map(|number| number as f64))
        .filter(|number| number.is_finite())
}

fn estimate_chars(chars: usize) -> i64 {
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4) as i64
    }
}

fn estimate_tokens(text: &str) -> i64 {
    estimate_chars(text.chars().count())
}

pub fn anthropic_message(client_model: &str, reply: &CollectedReply) -> Value {
    let mut content = Vec::new();
    if !reply.thinking.is_empty() {
        content.push(json!({
            "type": "thinking",
            "thinking": reply.thinking,
            "signature": reply.thinking_signature
        }));
    }
    if !reply.text.is_empty() {
        content.push(json!({"type": "text", "text": reply.text}));
    }
    for tool in &reply.tools {
        let input = serde_json::from_str::<Value>(&tool.input)
            .unwrap_or_else(|_| json!({"raw": tool.input}));
        content.push(json!({
            "type": "tool_use",
            "id": tool.id,
            "name": tool.name,
            "input": input
        }));
    }
    json!({
        "id": format!("msg_{}", Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": client_model,
        "stop_reason": if reply.tools.is_empty() { "end_turn" } else { "tool_use" },
        "stop_sequence": null,
        "content": content,
        "usage": {
            "input_tokens": reply.input_tokens,
            "output_tokens": reply.output_tokens,
            "cache_read_input_tokens": reply.cache_read,
            "cache_creation_input_tokens": reply.cache_write
        }
    })
}

pub fn anthropic_sse(client_model: &str, reply: &CollectedReply) -> String {
    let message = anthropic_message(client_model, reply);
    let id = message
        .get("id")
        .and_then(|item| item.as_str())
        .unwrap_or("msg");
    let mut out = String::new();
    push_sse(
        &mut out,
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": client_model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": reply.input_tokens, "output_tokens": 0}
            }
        }),
    );
    let mut index = 0;
    if !reply.thinking.is_empty() {
        push_sse(
            &mut out,
            "content_block_start",
            &json!({"type":"content_block_start","index": index, "content_block": {"type":"thinking","thinking":""}}),
        );
        push_sse(
            &mut out,
            "content_block_delta",
            &json!({"type":"content_block_delta","index": index, "delta": {"type":"thinking_delta","thinking": reply.thinking}}),
        );
        push_sse(
            &mut out,
            "content_block_stop",
            &json!({"type":"content_block_stop","index": index}),
        );
        index += 1;
    }
    if !reply.text.is_empty() {
        push_sse(
            &mut out,
            "content_block_start",
            &json!({"type":"content_block_start","index": index, "content_block": {"type":"text","text":""}}),
        );
        push_sse(
            &mut out,
            "content_block_delta",
            &json!({"type":"content_block_delta","index": index, "delta": {"type":"text_delta","text": reply.text}}),
        );
        push_sse(
            &mut out,
            "content_block_stop",
            &json!({"type":"content_block_stop","index": index}),
        );
        index += 1;
    }
    for tool in &reply.tools {
        push_sse(
            &mut out,
            "content_block_start",
            &json!({"type":"content_block_start","index": index, "content_block": {"type":"tool_use","id": tool.id, "name": tool.name, "input": {}}}),
        );
        push_sse(
            &mut out,
            "content_block_delta",
            &json!({"type":"content_block_delta","index": index, "delta": {"type":"input_json_delta","partial_json": tool.input}}),
        );
        push_sse(
            &mut out,
            "content_block_stop",
            &json!({"type":"content_block_stop","index": index}),
        );
        index += 1;
    }
    let stop = if reply.tools.is_empty() {
        "end_turn"
    } else {
        "tool_use"
    };
    push_sse(
        &mut out,
        "message_delta",
        &json!({"type":"message_delta","delta":{"stop_reason": stop, "stop_sequence": null}, "usage": {
            "input_tokens": reply.input_tokens,
            "output_tokens": reply.output_tokens,
            "cache_read_input_tokens": reply.cache_read,
            "cache_creation_input_tokens": reply.cache_write
        }}),
    );
    push_sse(&mut out, "message_stop", &json!({"type":"message_stop"}));
    let _ = index;
    out
}

pub fn openai_chat_json(client_model: &str, reply: &CollectedReply) -> Value {
    let mut message = json!({"role": "assistant", "content": reply.text});
    if !reply.tools.is_empty() {
        message["tool_calls"] = Value::Array(
            reply
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "id": tool.id,
                        "type": "function",
                        "function": {"name": tool.name, "arguments": tool.input}
                    })
                })
                .collect(),
        );
    }
    json!({
        "id": format!("chatcmpl_{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "model": client_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": if reply.tools.is_empty() { "stop" } else { "tool_calls" }
        }],
        "usage": {
            "prompt_tokens": reply.input_tokens,
            "completion_tokens": reply.output_tokens,
            "total_tokens": reply.input_tokens + reply.output_tokens
        }
    })
}

pub fn openai_chat_sse(client_model: &str, reply: &CollectedReply) -> String {
    let id = format!("chatcmpl_{}", Uuid::new_v4().simple());
    let mut out = String::new();
    let chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "model": client_model,
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": reply.text}, "finish_reason": null}]
    });
    out.push_str(&format!("data: {chunk}\n\n"));
    let done = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "model": client_model,
        "choices": [{"index": 0, "delta": {}, "finish_reason": if reply.tools.is_empty() { "stop" } else { "tool_calls" }}],
        "usage": {
            "prompt_tokens": reply.input_tokens,
            "completion_tokens": reply.output_tokens,
            "total_tokens": reply.input_tokens.saturating_add(reply.output_tokens)
        }
    });
    out.push_str(&format!("data: {done}\n\n"));
    out.push_str("data: [DONE]\n\n");
    out
}

pub fn responses_json(client_model: &str, reply: &CollectedReply) -> Value {
    json!({
        "id": format!("resp_{}", Uuid::new_v4().simple()),
        "object": "response",
        "model": client_model,
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": reply.text}]
        }],
        "usage": {
            "input_tokens": reply.input_tokens,
            "output_tokens": reply.output_tokens,
            "total_tokens": reply.input_tokens.saturating_add(reply.output_tokens)
        }
    })
}

pub fn responses_sse(client_model: &str, reply: &CollectedReply) -> String {
    let response = responses_json(client_model, reply);
    let mut out = String::new();
    push_sse(
        &mut out,
        "response.output_text.delta",
        &json!({"type":"response.output_text.delta","delta": reply.text}),
    );
    push_sse(
        &mut out,
        "response.completed",
        &json!({"type":"response.completed","response": response}),
    );
    out
}

fn push_sse(out: &mut String, event: &str, data: &Value) {
    out.push_str("event: ");
    out.push_str(event);
    out.push_str("\ndata: ");
    out.push_str(&data.to_string());
    out.push_str("\n\n");
}

pub fn should_log_usage(path: &str) -> bool {
    !path.trim_end_matches('/').ends_with("/count_tokens")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiro::account::{account_from_import, BUILDER_ID_PROFILE_ARN};
    use serde_json::json;

    fn account() -> KiroAccount {
        account_from_import(&json!({
            "refreshToken": "rt",
            "authMethod": "idc",
            "provider": "BuilderId",
            "clientId": "cid",
            "clientSecret": "sec"
        }))
        .unwrap()
    }

    #[test]
    fn builder_id_request_injects_placeholder_arn_and_keeps_client_model() {
        let incoming = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 32,
            "messages": [{"role":"user","content":"hello"}]
        });
        let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
        assert_eq!(prepared.client_model, "claude-3-5-sonnet-20241022");
        assert_eq!(prepared.backend_model, "claude-sonnet-3.5");
        assert_eq!(prepared.body["profileArn"], BUILDER_ID_PROFILE_ARN);
        assert_eq!(
            prepared.body["conversationState"]["currentMessage"]["userInputMessage"]["modelId"],
            "claude-sonnet-3.5"
        );
    }

    #[test]
    fn thinking_effort_uses_snake_case_output_config() {
        let incoming = json!({
            "model": "claude-sonnet-4.6",
            "thinking": {"type": "enabled", "budget_tokens": 90000},
            "messages": [{"role":"user","content":"think"}]
        });
        let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
        assert_eq!(
            prepared.body["additionalModelRequestFields"]["output_config"]["effort"],
            "high"
        );
        assert!(prepared.body["additionalModelRequestFields"]
            .get("outputConfig")
            .is_none());
    }

    #[test]
    fn tool_result_only_turn_keeps_nonempty_content() {
        let incoming = json!({
            "model": "claude-sonnet-5.5",
            "messages": [
                {"role":"user","content":"list files"},
                {"role":"assistant","content":[
                    {"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"toolu_1","content":"a.txt"}
                ]}
            ]
        });
        let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
        let current = &prepared.body["conversationState"]["currentMessage"]["userInputMessage"];
        assert_eq!(current["content"], "tool result");
        assert_eq!(
            current["userInputMessageContext"]["toolResults"][0]["toolUseId"],
            "toolu_1"
        );
        assert_eq!(
            current["userInputMessageContext"]["toolResults"][0]["content"][0]["text"],
            "a.txt"
        );
        let history = prepared.body["conversationState"]["history"]
            .as_array()
            .unwrap();
        assert_eq!(history[0]["userInputMessage"]["content"], "list files");
        assert_eq!(
            history[1]["assistantResponseMessage"]["toolUses"][0]["name"],
            "Bash"
        );
    }

    #[test]
    fn unsupported_models_omit_additional_model_request_fields() {
        for model in [
            "claude-haiku-4.5",
            "claude-sonnet-4.5",
            "claude-sonnet-5.5",
            "glm-5",
        ] {
            let incoming = json!({
                "model": model,
                "thinking": {"type": "enabled", "budget_tokens": 8000},
                "messages": [{"role":"user","content":"think"}]
            });
            let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
            assert!(
                prepared.body.get("additionalModelRequestFields").is_none(),
                "{model} must not send additionalModelRequestFields"
            );
        }
    }

    #[test]
    fn opus_4_6_effort_requires_adaptive_thinking() {
        let enabled = json!({
            "model": "claude-opus-4.6",
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role":"user","content":"think"}]
        });
        let prepared = prepare(WireProtocol::Anthropic, &enabled, &account()).unwrap();
        assert!(prepared.body.get("additionalModelRequestFields").is_none());

        let adaptive = json!({
            "model": "claude-opus-4.6",
            "thinking": {"type": "adaptive", "budget_tokens": 8000},
            "messages": [{"role":"user","content":"think"}]
        });
        let prepared = prepare(WireProtocol::Anthropic, &adaptive, &account()).unwrap();
        assert_eq!(
            prepared.body["additionalModelRequestFields"]["output_config"]["effort"],
            "high"
        );
    }

    #[test]
    fn response_model_is_the_client_catalog_id() {
        let reply = CollectedReply {
            text: "ok".into(),
            ..CollectedReply::default()
        };
        let message = anthropic_message("claude-sonnet-4.6-thinking", &reply);
        assert_eq!(message["model"], "claude-sonnet-4.6-thinking");
    }

    #[test]
    fn count_tokens_is_not_usage() {
        assert!(!should_log_usage("/v1/messages/count_tokens"));
        assert!(should_log_usage("/v1/messages"));
    }

    fn reply_from_payload(event_type: &str, payload: &Value) -> CollectedReply {
        let bytes = crate::kiro::event_stream::encode_frame(
            event_type,
            payload.to_string().as_bytes(),
        );
        let (frame, _) = crate::kiro::event_stream::parse_frame(&bytes)
            .unwrap()
            .unwrap();
        let mut reply = CollectedReply::default();
        reply.push_frame(&frame);
        reply
    }

    #[test]
    fn metadata_token_usage_accepts_floats_and_total_only() {
        let mut reply = reply_from_payload(
            "metadataEvent",
            &json!({"tokenUsage": {
                "uncachedInputTokens": 12.2,
                "outputTokens": 7,
                "cacheReadInputTokens": 3,
                "cacheWriteInputTokens": 1
            }}),
        );
        reply.finalize_usage(200_000, 100);
        assert_eq!(reply.input_tokens, 12);
        assert_eq!(reply.output_tokens, 7);
        assert_eq!(reply.cache_read, 3);
        assert_eq!(reply.cache_write, 1);

        let mut totals = reply_from_payload(
            "metadataEvent",
            &json!({"tokenUsage": {"totalTokens": 100, "outputTokens": 25}}),
        );
        totals.finalize_usage(200_000, 100);
        assert_eq!(totals.input_tokens, 75);
        assert_eq!(totals.output_tokens, 25);
    }

    #[test]
    fn context_usage_percentage_fills_tokens_when_metadata_is_absent() {
        let bytes = crate::kiro::event_stream::encode_frame(
            "assistantResponseEvent",
            br#"{"content":"hello!!!"}"#,
        );
        let usage = crate::kiro::event_stream::encode_frame(
            "contextUsageEvent",
            br#"{"contextUsagePercentage":2.5}"#,
        );
        let mut decoder = crate::kiro::event_stream::FrameDecoder::new();
        let mut combined = bytes;
        combined.extend(usage);
        let frames = decoder.push(&combined).unwrap();
        let mut reply = CollectedReply::default();
        for frame in &frames {
            reply.push_frame(frame);
        }
        reply.finalize_usage(200_000, 40);
        assert_eq!(reply.output_tokens, 2);
        assert_eq!(reply.input_tokens, 4998);
        let sse = anthropic_sse("claude-haiku-4.5", &reply);
        assert!(sse.contains("\"input_tokens\":4998"));
        assert!(sse.contains("\"output_tokens\":2"));
    }

    #[test]
    fn local_estimate_is_used_when_kiro_sends_no_usage_event() {
        let mut reply = reply_from_payload(
            "assistantResponseEvent",
            &json!({"content": "abcd"}),
        );
        reply.finalize_usage(0, 8);
        assert_eq!(reply.input_tokens, 2);
        assert_eq!(reply.output_tokens, 1);
    }
}
