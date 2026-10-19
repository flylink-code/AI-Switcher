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
        .unwrap_or(false);
    let normalized_anthropic: Option<Value> = match protocol {
        WireProtocol::Anthropic => None,
        WireProtocol::OpenAiChat => {
            let responses_body =
                crate::proxy::codex_chat::chat_completions_to_responses_body(incoming)?;
            Some(
                crate::proxy::codex_anthropic::responses_request_to_anthropic_messages(
                    &responses_body,
                    &backend_model,
                )
                .map_err(|e| e.to_string())?,
            )
        }
        WireProtocol::OpenAiResponses => Some(
            crate::proxy::codex_anthropic::responses_request_to_anthropic_messages(
                incoming,
                &backend_model,
            )
            .map_err(|e| e.to_string())?,
        ),
    };
    let effective_incoming = normalized_anthropic.as_ref().unwrap_or(incoming);
    let (history, current, tools) = anthropic_turns(effective_incoming, &backend_model)?;
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
    if let Some(extra) = extra_fields(effective_incoming, &backend_model) {
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
    let mut tools: Vec<Value> = incoming
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
            if messages.len() <= 1 {
                messages.insert(0, json!({"role": "user", "content": system}));
            } else {
                messages.insert(0, json!({"role": "user", "content": system}));
                messages.insert(
                    1,
                    json!({"role": "assistant", "content": "I will follow these instructions."}),
                );
            }
        }
    }
    let (mut history, mut current, _) = turns_from_messages(&messages, model)?;
    sanitize_tool_pairing_and_placeholders(&mut history, &mut current, &mut tools);
    Ok((history, current, tools))
}

fn turns_from_messages(
    messages: &[Value],
    model: &str,
) -> Result<(Vec<Value>, CurrentTurn, Vec<Value>), String> {
    if messages.is_empty() {
        return Err("请求没有消息".into());
    }
    let last = messages.len() - 1;
    let mut history = Vec::new();
    for message in &messages[..last] {
        if let Some(entry) = history_message(message, model) {
            history.push(entry);
        }
    }
    let current = current_from(messages.last().unwrap());
    Ok((history, current, Vec::new()))
}

fn history_message(message: &Value, model: &str) -> Option<Value> {
    let role = message
        .get("role")
        .and_then(|role| role.as_str())
        .unwrap_or("user");
    if role == "assistant" {
        let mut content = text_of(message.get("content")).unwrap_or_default();
        let tool_uses = anthropic_tool_uses(message.get("content"));
        if content.trim().is_empty() && tool_uses.is_empty() {
            return None;
        }
        if content.is_empty() && !tool_uses.is_empty() {
            content = " ".to_string();
        }
        let mut assistant = json!({ "content": content });
        if !tool_uses.is_empty() {
            assistant["toolUses"] = Value::Array(tool_uses);
        }
        return Some(json!({ "assistantResponseMessage": assistant }));
    }
    let mut content = message_text(message);
    let tool_results = anthropic_tool_results(message.get("content"));
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

fn current_from(message: &Value) -> CurrentTurn {
    let raw = message.get("content");
    let tool_results = anthropic_tool_results(raw);
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
        } else if kind == "document" {
            if let Some(doc_text) = crate::proxy::convert::extract_anthropic_document_text(block) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&doc_text);
            }
        } else if kind == "web_search_tool_result" {
            if let Some(ws_text) =
                crate::proxy::convert::extract_anthropic_web_search_result_text(block)
            {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&ws_text);
            }
        }
    }
    Some(text)
}

fn normalize_kiro_tool_schema(schema: Value) -> Value {
    let Value::Object(mut obj) = schema else {
        return json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": true
        });
    };
    obj.remove("$schema");
    let has_props = obj.contains_key("properties");
    for combinator in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(variants)) = obj.remove(combinator) {
            if !has_props && !obj.contains_key("properties") {
                for variant in variants {
                    if let Value::Object(m) = variant {
                        if m.get("type").and_then(Value::as_str) == Some("object") {
                            for key in ["properties", "required", "additionalProperties", "description"] {
                                if let Some(val) = m.get(key) {
                                    obj.entry(key.to_string()).or_insert_with(|| val.clone());
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    obj.insert("type".to_string(), Value::String("object".to_string()));
    if !obj.get("properties").is_some_and(Value::is_object) {
        obj.insert("properties".to_string(), json!({}));
    }
    Value::Object(obj)
}

fn sanitize_tool_pairing_and_placeholders(
    history: &mut Vec<Value>,
    current: &mut CurrentTurn,
    tools: &mut Vec<Value>,
) {
    let mut all_tool_use_ids = std::collections::HashSet::new();
    let mut paired_in_history = std::collections::HashSet::new();

    for entry in history.iter() {
        if let Some(uses) = entry
            .pointer("/assistantResponseMessage/toolUses")
            .and_then(Value::as_array)
        {
            for u in uses {
                if let Some(id) = u.get("toolUseId").and_then(Value::as_str) {
                    all_tool_use_ids.insert(id.to_string());
                }
            }
        }
        if let Some(results) = entry
            .pointer("/userInputMessage/userInputMessageContext/toolResults")
            .and_then(Value::as_array)
        {
            for r in results {
                if let Some(id) = r.get("toolUseId").and_then(Value::as_str) {
                    paired_in_history.insert(id.to_string());
                }
            }
        }
    }

    let mut unpaired: std::collections::HashSet<String> = all_tool_use_ids
        .difference(&paired_in_history)
        .cloned()
        .collect();

    current.tool_results.retain(|r| {
        let Some(id) = r.get("toolUseId").and_then(Value::as_str) else {
            return false;
        };
        unpaired.remove(id)
    });

    if !unpaired.is_empty() {
        for entry in history.iter_mut() {
            if let Some(assistant) = entry
                .get_mut("assistantResponseMessage")
                .and_then(Value::as_object_mut)
            {
                if let Some(uses) = assistant.get_mut("toolUses").and_then(Value::as_array_mut) {
                    uses.retain(|u| {
                        u.get("toolUseId")
                            .and_then(Value::as_str)
                            .is_some_and(|id| !unpaired.contains(id))
                    });
                    if uses.is_empty() {
                        assistant.remove("toolUses");
                    }
                }
            }
        }
    }

    let mut existing_names: std::collections::HashSet<String> = tools
        .iter()
        .filter_map(|t| {
            t.pointer("/toolSpecification/name")
                .and_then(Value::as_str)
                .map(|s| s.to_ascii_lowercase())
        })
        .collect();

    for entry in history.iter() {
        if let Some(uses) = entry
            .pointer("/assistantResponseMessage/toolUses")
            .and_then(Value::as_array)
        {
            for u in uses {
                if let Some(name) = u.get("name").and_then(Value::as_str) {
                    if existing_names.insert(name.to_ascii_lowercase()) {
                        tools.push(json!({
                            "toolSpecification": {
                                "name": name,
                                "description": "Tool used in conversation history",
                                "inputSchema": {
                                    "json": {
                                        "type": "object",
                                        "properties": {},
                                        "required": [],
                                        "additionalProperties": true
                                    }
                                }
                            }
                        }));
                    }
                }
            }
        }
    }
}

fn anthropic_tool(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(|item| item.as_str())?;
    let raw_desc = tool
        .get("description")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let desc = crate::proxy::codex_anthropic::enrich_edit_tool_description(name, raw_desc);
    let description = if desc.trim().is_empty() {
        name.to_string()
    } else {
        desc
    };
    let schema = normalize_kiro_tool_schema(
        tool.get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object"})),
    );
    Some(json!({
        "toolSpecification": {
            "name": name,
            "description": description,
            "inputSchema": { "json": schema }
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
                "input": block.get("input").filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({}))
            }))
        })
        .collect()
}

fn anthropic_tool_results(content: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = content.and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.get("type").and_then(|item| item.as_str()),
                Some("tool_result" | "tool_search_tool_result")
            )
        })
        .filter_map(|block| {
            let mut text = text_of(block.get("content")).unwrap_or_default();
            if text.is_empty()
                && block
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item.get("type").and_then(Value::as_str) == Some("image"))
                    })
            {
                text = "[Image attached]".to_string();
            }
            Some(json!({
                "toolUseId": block.get("tool_use_id").or_else(|| block.get("id")).and_then(|item| item.as_str())?,
                "content": [{"text": text}],
                "status": if block.get("is_error").and_then(|item| item.as_bool()).unwrap_or(false) { "error" } else { "success" }
            }))
        })
        .collect()
}

fn parse_kiro_image_block(block: &Value) -> Option<Value> {
    if block.get("type").and_then(|item| item.as_str()) != Some("image") {
        return None;
    }
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
}

fn anthropic_images(content: Option<&Value>) -> Vec<Value> {
    let Some(blocks) = content.and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    let mut images = Vec::new();
    for block in blocks {
        if let Some(img) = parse_kiro_image_block(block) {
            images.push(img);
        } else if matches!(
            block.get("type").and_then(|item| item.as_str()),
            Some("tool_result" | "tool_search_tool_result")
        ) {
            if let Some(nested) = block.get("content").and_then(|item| item.as_array()) {
                for item in nested {
                    if let Some(img) = parse_kiro_image_block(item) {
                        images.push(img);
                    }
                }
            }
        }
    }
    images
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
    pub upstream_error: Option<String>,
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
        if frame.message_type == "error" || frame.message_type == "exception" {
            let detail = serde_json::from_slice::<Value>(&frame.payload)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .or_else(|| v.get("Message"))
                        .or_else(|| v.get("errorMessage"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| String::from_utf8_lossy(&frame.payload).trim().to_string());
            let label = frame
                .exception_type
                .as_deref()
                .or(frame.error_code.as_deref())
                .unwrap_or(&frame.event_type);
            if label != "ContentLengthExceededException" {
                self.upstream_error = Some(if detail.is_empty() {
                    label.to_string()
                } else {
                    format!("{label}: {detail}")
                });
            }
            return;
        }
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
                if total > 0 {
                    let output = estimate_tokens(&self.generated_text()).min(total);
                    self.output_tokens = output;
                    self.input_tokens = total.saturating_sub(output);
                    return;
                }
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
    if !reply.tools.is_empty() {
        let tool_calls: Vec<Value> = reply
            .tools
            .iter()
            .enumerate()
            .map(|(idx, tool)| {
                json!({
                    "index": idx,
                    "id": tool.id,
                    "type": "function",
                    "function": {"name": tool.name, "arguments": tool.input}
                })
            })
            .collect();
        let tool_chunk = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "model": client_model,
            "choices": [{"index": 0, "delta": {"tool_calls": tool_calls}, "finish_reason": null}]
        });
        out.push_str(&format!("data: {tool_chunk}\n\n"));
    }
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
    let mut output = Vec::new();
    if !reply.text.is_empty() || reply.tools.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": reply.text}]
        }));
    }
    for tool in &reply.tools {
        let arguments = if tool.input.trim().is_empty() {
            "{}".to_string()
        } else {
            tool.input.clone()
        };
        output.push(json!({
            "type": "function_call",
            "id": format!("fc_{}", tool.id),
            "call_id": tool.id,
            "name": tool.name,
            "arguments": arguments,
            "status": "completed"
        }));
    }
    json!({
        "id": format!("resp_{}", Uuid::new_v4().simple()),
        "object": "response",
        "model": client_model,
        "status": "completed",
        "output": output,
        "usage": {
            "input_tokens": reply.input_tokens,
            "output_tokens": reply.output_tokens,
            "total_tokens": reply.input_tokens.saturating_add(reply.output_tokens)
        }
    })
}

pub fn responses_sse(client_model: &str, reply: &CollectedReply) -> String {
    let response = responses_json(client_model, reply);
    let resp_id = response
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_kiro")
        .to_string();
    let init_resp = json!({
        "id": resp_id,
        "object": "response",
        "model": client_model,
        "status": "in_progress",
        "output": []
    });
    let mut out = String::new();
    push_sse(
        &mut out,
        "response.created",
        &json!({"type":"response.created","response": init_resp}),
    );
    push_sse(
        &mut out,
        "response.in_progress",
        &json!({"type":"response.in_progress","response": init_resp}),
    );
    let mut out_idx: usize = 0;
    if !reply.text.is_empty() || reply.tools.is_empty() {
        push_sse(
            &mut out,
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta","delta": reply.text}),
        );
        push_sse(
            &mut out,
            "response.output_text.done",
            &json!({
                "type":"response.output_text.done",
                "output_index": out_idx,
                "content_index": 0,
                "text": reply.text
            }),
        );
        push_sse(
            &mut out,
            "response.output_item.done",
            &json!({
                "type":"response.output_item.done",
                "output_index": out_idx,
                "item": {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{"type": "output_text", "text": reply.text}]
                }
            }),
        );
        out_idx += 1;
    }
    for tool in &reply.tools {
        let arguments = if tool.input.trim().is_empty() {
            "{}".to_string()
        } else {
            tool.input.clone()
        };
        let item_id = format!("fc_{}", tool.id);
        push_sse(
            &mut out,
            "response.output_item.added",
            &json!({
                "type": "response.output_item.added",
                "output_index": out_idx,
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": tool.id,
                    "name": tool.name,
                    "arguments": "",
                    "status": "in_progress"
                }
            }),
        );
        push_sse(
            &mut out,
            "response.function_call_arguments.delta",
            &json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id,
                "output_index": out_idx,
                "delta": arguments
            }),
        );
        push_sse(
            &mut out,
            "response.function_call_arguments.done",
            &json!({
                "type": "response.function_call_arguments.done",
                "item_id": item_id,
                "output_index": out_idx,
                "arguments": arguments
            }),
        );
        push_sse(
            &mut out,
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": out_idx,
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": tool.id,
                    "name": tool.name,
                    "arguments": arguments,
                    "status": "completed"
                }
            }),
        );
        out_idx += 1;
    }
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

/// Incremental AWS EventStream -> SSE converter for real-time streaming responses.
pub struct KiroSseStreamer {
    protocol: WireProtocol,
    client_model: String,
    msg_id: String,
    started: bool,
    thinking_index: Option<usize>,
    text_index: Option<usize>,
    next_index: usize,
    pub collected: CollectedReply,
}

impl KiroSseStreamer {
    pub fn new(protocol: WireProtocol, client_model: &str) -> Self {
        let prefix = match protocol {
            WireProtocol::Anthropic => "msg",
            WireProtocol::OpenAiChat => "chatcmpl",
            WireProtocol::OpenAiResponses => "resp",
        };
        Self {
            protocol,
            client_model: client_model.to_string(),
            msg_id: format!("{prefix}_{}", Uuid::new_v4().simple()),
            started: false,
            thinking_index: None,
            text_index: None,
            next_index: 0,
            collected: CollectedReply::default(),
        }
    }

    fn ensure_started(&mut self, out: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        match self.protocol {
            WireProtocol::Anthropic => {
                push_sse(
                    out,
                    "message_start",
                    &json!({
                        "type": "message_start",
                        "message": {
                            "id": self.msg_id,
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": self.client_model,
                            "stop_reason": null,
                            "stop_sequence": null,
                            "usage": {"input_tokens": self.collected.input_tokens, "output_tokens": 0}
                        }
                    }),
                );
            }
            WireProtocol::OpenAiChat => {
                let chunk = json!({
                    "id": self.msg_id,
                    "object": "chat.completion.chunk",
                    "model": self.client_model,
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]
                });
                out.push_str(&format!("data: {chunk}\n\n"));
            }
            WireProtocol::OpenAiResponses => {
                let init_resp = json!({
                    "id": self.msg_id,
                    "object": "response",
                    "model": self.client_model,
                    "status": "in_progress",
                    "output": []
                });
                push_sse(
                    out,
                    "response.created",
                    &json!({"type": "response.created", "response": init_resp}),
                );
                push_sse(
                    out,
                    "response.in_progress",
                    &json!({"type": "response.in_progress", "response": init_resp}),
                );
            }
        }
    }

    pub fn push_frame(&mut self, frame: &Frame) -> String {
        self.collected.push_frame(frame);
        if frame.message_type != "event" {
            return String::new();
        }
        let Ok(payload) = serde_json::from_slice::<Value>(&frame.payload) else {
            return String::new();
        };
        let mut out = String::new();
        self.ensure_started(&mut out);
        match frame.event_type.as_str() {
            "reasoningContentEvent" => {
                if let Some(text) = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    match self.protocol {
                        WireProtocol::Anthropic => {
                            let idx = match self.thinking_index {
                                Some(idx) => idx,
                                None => {
                                    if let Some(txt_idx) = self.text_index.take() {
                                        push_sse(
                                            &mut out,
                                            "content_block_stop",
                                            &json!({"type":"content_block_stop","index": txt_idx}),
                                        );
                                    }
                                    let idx = self.next_index;
                                    self.next_index += 1;
                                    self.thinking_index = Some(idx);
                                    push_sse(
                                        &mut out,
                                        "content_block_start",
                                        &json!({"type":"content_block_start","index": idx, "content_block": {"type":"thinking","thinking":""}}),
                                    );
                                    idx
                                }
                            };
                            push_sse(
                                &mut out,
                                "content_block_delta",
                                &json!({"type":"content_block_delta","index": idx, "delta": {"type":"thinking_delta","thinking": text}}),
                            );
                        }
                        WireProtocol::OpenAiChat => {
                            let chunk = json!({
                                "id": self.msg_id,
                                "object": "chat.completion.chunk",
                                "model": self.client_model,
                                "choices": [{"index": 0, "delta": {"reasoning_content": text}, "finish_reason": null}]
                            });
                            out.push_str(&format!("data: {chunk}\n\n"));
                        }
                        WireProtocol::OpenAiResponses => {
                            push_sse(
                                &mut out,
                                "response.reasoning_summary_text.delta",
                                &json!({"type":"response.reasoning_summary_text.delta","delta": text}),
                            );
                        }
                    }
                }
            }
            "assistantResponseEvent" => {
                if let Some(content) = payload
                    .get("content")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    match self.protocol {
                        WireProtocol::Anthropic => {
                            if let Some(t_idx) = self.thinking_index.take() {
                                push_sse(
                                    &mut out,
                                    "content_block_stop",
                                    &json!({"type":"content_block_stop","index": t_idx}),
                                );
                            }
                            let idx = match self.text_index {
                                Some(idx) => idx,
                                None => {
                                    let idx = self.next_index;
                                    self.next_index += 1;
                                    self.text_index = Some(idx);
                                    push_sse(
                                        &mut out,
                                        "content_block_start",
                                        &json!({"type":"content_block_start","index": idx, "content_block": {"type":"text","text":""}}),
                                    );
                                    idx
                                }
                            };
                            push_sse(
                                &mut out,
                                "content_block_delta",
                                &json!({"type":"content_block_delta","index": idx, "delta": {"type":"text_delta","text": content}}),
                            );
                        }
                        WireProtocol::OpenAiChat => {
                            let chunk = json!({
                                "id": self.msg_id,
                                "object": "chat.completion.chunk",
                                "model": self.client_model,
                                "choices": [{"index": 0, "delta": {"content": content}, "finish_reason": null}]
                            });
                            out.push_str(&format!("data: {chunk}\n\n"));
                        }
                        WireProtocol::OpenAiResponses => {
                            push_sse(
                                &mut out,
                                "response.output_text.delta",
                                &json!({"type":"response.output_text.delta","delta": content}),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    pub fn finish(&mut self, context_window: u64, request_chars: usize) -> String {
        self.collected.finalize_usage(context_window, request_chars);
        let mut out = String::new();
        self.ensure_started(&mut out);
        match self.protocol {
            WireProtocol::Anthropic => {
                if let Some(t_idx) = self.thinking_index.take() {
                    push_sse(
                        &mut out,
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index": t_idx}),
                    );
                }
                if let Some(txt_idx) = self.text_index.take() {
                    push_sse(
                        &mut out,
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index": txt_idx}),
                    );
                }
                for tool in &self.collected.tools {
                    let idx = self.next_index;
                    self.next_index += 1;
                    push_sse(
                        &mut out,
                        "content_block_start",
                        &json!({"type":"content_block_start","index": idx, "content_block": {"type":"tool_use","id": tool.id, "name": tool.name, "input": {}}}),
                    );
                    push_sse(
                        &mut out,
                        "content_block_delta",
                        &json!({"type":"content_block_delta","index": idx, "delta": {"type":"input_json_delta","partial_json": tool.input}}),
                    );
                    push_sse(
                        &mut out,
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index": idx}),
                    );
                }
                let stop = if self.collected.tools.is_empty() {
                    "end_turn"
                } else {
                    "tool_use"
                };
                push_sse(
                    &mut out,
                    "message_delta",
                    &json!({"type":"message_delta","delta":{"stop_reason": stop, "stop_sequence": null}, "usage": {
                        "input_tokens": self.collected.input_tokens,
                        "output_tokens": self.collected.output_tokens,
                        "cache_read_input_tokens": self.collected.cache_read,
                        "cache_creation_input_tokens": self.collected.cache_write
                    }}),
                );
                push_sse(&mut out, "message_stop", &json!({"type":"message_stop"}));
            }
            WireProtocol::OpenAiChat => {
                if !self.collected.tools.is_empty() {
                    let tool_calls: Vec<Value> = self
                        .collected
                        .tools
                        .iter()
                        .enumerate()
                        .map(|(idx, tool)| {
                            json!({
                                "index": idx,
                                "id": tool.id,
                                "type": "function",
                                "function": {"name": tool.name, "arguments": tool.input}
                            })
                        })
                        .collect();
                    let chunk = json!({
                        "id": self.msg_id,
                        "object": "chat.completion.chunk",
                        "model": self.client_model,
                        "choices": [{"index": 0, "delta": {"tool_calls": tool_calls}, "finish_reason": null}]
                    });
                    out.push_str(&format!("data: {chunk}\n\n"));
                }
                let done = json!({
                    "id": self.msg_id,
                    "object": "chat.completion.chunk",
                    "model": self.client_model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": if self.collected.tools.is_empty() { "stop" } else { "tool_calls" }}],
                    "usage": {
                        "prompt_tokens": self.collected.input_tokens,
                        "completion_tokens": self.collected.output_tokens,
                        "total_tokens": self.collected.input_tokens.saturating_add(self.collected.output_tokens)
                    }
                });
                out.push_str(&format!("data: {done}\n\n"));
                out.push_str("data: [DONE]\n\n");
            }
            WireProtocol::OpenAiResponses => {
                let mut out_idx: usize = 0;
                if !self.collected.text.is_empty() || self.collected.tools.is_empty() {
                    let msg_item = json!({
                        "id": format!("msg_{}", Uuid::new_v4().simple()),
                        "type": "message",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": self.collected.text}]
                    });
                    push_sse(
                        &mut out,
                        "response.output_text.done",
                        &json!({
                            "type": "response.output_text.done",
                            "output_index": out_idx,
                            "content_index": 0,
                            "text": self.collected.text
                        }),
                    );
                    push_sse(
                        &mut out,
                        "response.output_item.done",
                        &json!({
                            "type": "response.output_item.done",
                            "output_index": out_idx,
                            "item": msg_item
                        }),
                    );
                    out_idx += 1;
                }
                for tool in &self.collected.tools {
                    let arguments = if tool.input.trim().is_empty() {
                        "{}".to_string()
                    } else {
                        tool.input.clone()
                    };
                    let item_id = format!("fc_{}", tool.id);
                    push_sse(
                        &mut out,
                        "response.output_item.added",
                        &json!({
                            "type": "response.output_item.added",
                            "output_index": out_idx,
                            "item": {
                                "type": "function_call",
                                "id": item_id,
                                "call_id": tool.id,
                                "name": tool.name,
                                "arguments": "",
                                "status": "in_progress"
                            }
                        }),
                    );
                    push_sse(
                        &mut out,
                        "response.function_call_arguments.delta",
                        &json!({
                            "type": "response.function_call_arguments.delta",
                            "item_id": item_id,
                            "output_index": out_idx,
                            "delta": arguments
                        }),
                    );
                    push_sse(
                        &mut out,
                        "response.function_call_arguments.done",
                        &json!({
                            "type": "response.function_call_arguments.done",
                            "item_id": item_id,
                            "output_index": out_idx,
                            "arguments": arguments
                        }),
                    );
                    push_sse(
                        &mut out,
                        "response.output_item.done",
                        &json!({
                            "type": "response.output_item.done",
                            "output_index": out_idx,
                            "item": {
                                "type": "function_call",
                                "id": item_id,
                                "call_id": tool.id,
                                "name": tool.name,
                                "arguments": arguments,
                                "status": "completed"
                            }
                        }),
                    );
                    out_idx += 1;
                }
                let mut response = responses_json(&self.client_model, &self.collected);
                response["id"] = Value::String(self.msg_id.clone());
                push_sse(
                    &mut out,
                    "response.completed",
                    &json!({"type":"response.completed","response": response}),
                );
            }
        }
        out
    }
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

    #[test]
    fn system_prompt_and_orphaned_tools_are_sanitized_for_kiro() {
        let incoming = json!({
            "model": "claude-sonnet-5.5",
            "system": "Be concise",
            "tools": [
                {
                    "name": "edit",
                    "description": "Edit a file",
                    "input_schema": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "properties": { "path": { "type": "string" } }
                    }
                }
            ],
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "orphan_1", "name": "old_tool", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "dangling_res", "content": "ignored"},
                    {"type": "text", "text": "next"}
                ]}
            ]
        });
        let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
        let history = prepared.body["conversationState"]["history"]
            .as_array()
            .unwrap();
        // System prompt should be paired with an assistant acknowledgement so two user messages aren't adjacent
        assert_eq!(history[0]["userInputMessage"]["content"], "Be concise");
        assert_eq!(
            history[1]["assistantResponseMessage"]["content"],
            "I will follow these instructions."
        );
        // Orphaned tool_result (`dangling_res`) should be stripped from current message
        assert!(
            prepared.body["conversationState"]["currentMessage"]["userInputMessage"]
                ["userInputMessageContext"]
                .get("toolResults")
                .is_none()
        );
        // `edit` tool description should be enriched with uniqueness hint and `$schema` stripped
        let tool_spec = &prepared.body["conversationState"]["currentMessage"]["userInputMessage"]
            ["userInputMessageContext"]["tools"][0]["toolSpecification"];
        assert!(tool_spec["description"]
            .as_str()
            .unwrap()
            .contains("100% unique"));
        assert!(tool_spec["inputSchema"]["json"].get("$schema").is_none());
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

    #[test]
    fn anthropic_request_defaults_stream_to_false_and_responses_json_maps_tools() {
        let incoming = json!({
            "model": "claude-haiku-4.5",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let prepared = prepare(WireProtocol::Anthropic, &incoming, &account()).unwrap();
        assert!(!prepared.stream);

        let reply = CollectedReply {
            text: String::new(),
            tools: vec![ToolCall {
                id: "call_1".into(),
                name: "query_metric".into(),
                input: r#"{"metric_name":"cpu"}"#.into(),
            }],
            ..CollectedReply::default()
        };
        let resp = responses_json("claude-haiku-4.5", &reply);
        let output = resp["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "function_call");
        assert_eq!(output[0]["call_id"], "call_1");
        assert_eq!(output[0]["name"], "query_metric");
    }

    #[test]
    fn openai_chat_and_responses_preserve_tools_system_and_tool_results_for_kiro() {
        let chat_req = json!({
            "model": "claude-sonnet-4.6",
            "reasoning_effort": "high",
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } }
                    }
                }
            }],
            "messages": [
                {"role": "system", "content": "Always answer in Chinese"},
                {"role": "user", "content": "read cargo.toml"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_read_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"Cargo.toml\"}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_read_1",
                    "content": "[package]\nname = \"ai-switcher\""
                }
            ]
        });
        let prepared = prepare(WireProtocol::OpenAiChat, &chat_req, &account()).unwrap();
        let history = prepared.body["conversationState"]["history"]
            .as_array()
            .unwrap();
        assert_eq!(
            history[0]["userInputMessage"]["content"],
            "Always answer in Chinese"
        );
        assert_eq!(
            history[1]["assistantResponseMessage"]["content"],
            "I will follow these instructions."
        );
        assert_eq!(
            history[3]["assistantResponseMessage"]["toolUses"][0]["toolUseId"],
            "call_read_1"
        );
        let cur_ctx = &prepared.body["conversationState"]["currentMessage"]["userInputMessage"]
            ["userInputMessageContext"];
        assert_eq!(cur_ctx["toolResults"][0]["toolUseId"], "call_read_1");
        assert_eq!(
            cur_ctx["tools"][0]["toolSpecification"]["name"],
            "read_file"
        );
        assert_eq!(
            prepared.body["additionalModelRequestFields"]["output_config"]["effort"],
            "high"
        );

        let resp_req = json!({
            "model": "claude-sonnet-4.6",
            "instructions": "Follow system rules",
            "reasoning": { "effort": "medium" },
            "tools": [{
                "type": "function",
                "name": "exec_cmd",
                "description": "Execute shell",
                "parameters": {
                    "type": "object",
                    "properties": { "cmd": { "type": "string" } }
                }
            }],
            "input": [
                {"role": "user", "content": "run ls"},
                {
                    "type": "function_call",
                    "call_id": "call_exec_1",
                    "name": "exec_cmd",
                    "arguments": "{\"cmd\":\"ls\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_exec_1",
                    "output": "README.md"
                }
            ]
        });
        let prepared_resp = prepare(WireProtocol::OpenAiResponses, &resp_req, &account()).unwrap();
        let cur_resp_ctx = &prepared_resp.body["conversationState"]["currentMessage"]
            ["userInputMessage"]["userInputMessageContext"];
        assert_eq!(cur_resp_ctx["toolResults"][0]["toolUseId"], "call_exec_1");
        assert_eq!(
            cur_resp_ctx["tools"][0]["toolSpecification"]["name"],
            "exec_cmd"
        );
        assert_eq!(
            prepared_resp.body["additionalModelRequestFields"]["output_config"]["effort"],
            "medium"
        );
    }

    #[test]
    fn kiro_sse_streamer_emits_full_openai_responses_lifecycle_and_tool_calls() {
        let mut streamer = KiroSseStreamer::new(WireProtocol::OpenAiResponses, "claude-haiku-4.5");
        let frame = Frame {
            message_type: "event".to_string(),
            event_type: "assistantResponseEvent".to_string(),
            error_code: None,
            exception_type: None,
            payload: serde_json::to_vec(&json!({ "content": "Checking status..." })).unwrap(),
        };
        let chunk = streamer.push_frame(&frame);
        assert!(chunk.contains("event: response.created"));
        assert!(chunk.contains("event: response.in_progress"));
        assert!(chunk.contains("event: response.output_text.delta"));

        streamer.collected.tools.push(ToolCall {
            id: "call_kiro_1".to_string(),
            name: "run_probe".to_string(),
            input: "{\"target\":\"gateway\"}".to_string(),
        });
        let tail = streamer.finish(200_000, 120);
        assert!(tail.contains("event: response.output_text.done"));
        assert!(tail.contains("event: response.output_item.added"));
        assert!(tail.contains("event: response.function_call_arguments.delta"));
        assert!(tail.contains("event: response.function_call_arguments.done"));
        assert!(tail.contains("event: response.output_item.done"));
        assert!(tail.contains("event: response.completed"));
        assert!(tail.contains("\"call_id\":\"call_kiro_1\""));

        let chat_sse = openai_chat_sse("claude-haiku-4.5", &streamer.collected);
        assert!(chat_sse.contains("\"tool_calls\""));
        assert!(chat_sse.contains("\"finish_reason\":\"tool_calls\""));
    }

    #[test]
    fn finalize_usage_falls_back_to_char_estimate_when_tiny_context_percent_rounds_to_zero() {
        let mut reply = CollectedReply {
            text: "OK".to_string(),
            context_percent: Some(0.0001),
            ..CollectedReply::default()
        };
        reply.finalize_usage(200_000, 80);
        assert!(reply.input_tokens > 0);
        assert!(reply.output_tokens > 0);
    }
}
