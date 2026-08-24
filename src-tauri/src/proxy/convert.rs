//! Stateless Anthropic <-> OpenAI protocol conversion helpers.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn wants_stream(request: &Value) -> bool {
    request.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

/// Moonshot / Kimi OpenAI Chat endpoints accept `prompt_cache_key`; most others 400 on it.
pub fn chat_prompt_cache_allowed_for_base_url(base_url: &str) -> bool {
    let url = base_url.to_ascii_lowercase();
    url.contains("moonshot") || url.contains("kimi")
}

/// Reinject `prompt_cache_key` for allowlisted Chat upstreams.
/// Prefer an explicit request value, then a stable session hint; never invent a random key.
pub fn reinject_chat_prompt_cache_key(
    request: &mut Value,
    explicit: Option<&str>,
    session_hint: Option<&str>,
    allow: bool,
) {
    if !allow {
        return;
    }
    if request
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }
    let key = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| session_hint.map(str::trim).filter(|value| !value.is_empty()));
    if let Some(key) = key {
        request["prompt_cache_key"] = Value::String(key.to_string());
    }
}

pub fn extract_anthropic_thinking(request: &Value) -> Option<crate::provider::ThinkingConfig> {
    let mut config = crate::provider::ThinkingConfig::default();
    let mut found = false;

    if let Some(thinking) = request.get("thinking") {
        if let Some(kind) = thinking.get("type").and_then(Value::as_str) {
            config.mode = Some(kind.to_string());
            found = true;
        }
        if let Some(budget) = thinking.get("budget_tokens").and_then(Value::as_u64) {
            config.budget_tokens = Some(budget as u32);
            found = true;
        }
        if let Some(effort) = thinking.get("effort").and_then(Value::as_str) {
            config.reasoning_effort = Some(effort.to_string());
            found = true;
        }
    }
    if let Some(effort) = request
        .pointer("/output_config/effort")
        .or_else(|| request.get("effort"))
        .and_then(Value::as_str)
    {
        config.reasoning_effort = Some(effort.to_string());
        found = true;
    }
    if found {
        Some(config)
    } else {
        None
    }
}

/// Overlay provider defaults onto request thinking. The request wins for
/// explicit fields; a client `thinking.type=enabled` without budget/effort
/// still picks up the provider-level budget so Claude Code traffic is covered.
fn merge_thinking_config(
    request: &Value,
    provider_thinking: Option<&crate::provider::ThinkingConfig>,
) -> Option<crate::provider::ThinkingConfig> {
    match (extract_anthropic_thinking(request), provider_thinking) {
        (None, provider) => provider.cloned(),
        (Some(request_cfg), None) => Some(request_cfg),
        (Some(mut request_cfg), Some(provider)) => {
            if request_cfg.is_disabled() {
                return Some(request_cfg);
            }
            if request_cfg.budget_tokens.is_none() {
                request_cfg.budget_tokens = provider.budget_tokens;
            }
            let request_effort_empty = request_cfg
                .reasoning_effort
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty();
            if request_effort_empty {
                request_cfg.reasoning_effort = provider.reasoning_effort.clone();
            }
            if request_cfg.prefix_thought.is_none() {
                request_cfg.prefix_thought = provider.prefix_thought;
            }
            Some(request_cfg)
        }
    }
}

const PREFIX_THOUGHT_HINT: &str =
    "Think step by step before answering. Put internal reasoning first, then the final answer.";

fn apply_prefix_thought(request: &mut Value) {
    if let Some(messages) = request.get_mut("messages").and_then(Value::as_array_mut) {
        if let Some(system) = messages.iter_mut().find(|message| {
            message.get("role").and_then(Value::as_str) == Some("system")
        }) {
            match system.get_mut("content") {
                Some(Value::String(text)) if !text.contains("Think step by step") => {
                    text.push_str("\n\n");
                    text.push_str(PREFIX_THOUGHT_HINT);
                }
                Some(Value::String(_)) => {}
                _ => {
                    system["content"] = json!(PREFIX_THOUGHT_HINT);
                }
            }
            return;
        }
        messages.insert(0, json!({ "role": "system", "content": PREFIX_THOUGHT_HINT }));
        return;
    }
    if let Some(Value::String(instructions)) = request.get_mut("instructions") {
        if !instructions.contains("Think step by step") {
            instructions.push_str("\n\n");
            instructions.push_str(PREFIX_THOUGHT_HINT);
        }
        return;
    }
    request["instructions"] = json!(PREFIX_THOUGHT_HINT);
}

pub fn anthropic_to_openai_chat(
    request: &Value,
    model: &str,
    stream: bool,
    provider_thinking: Option<&crate::provider::ThinkingConfig>,
) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = request.get("system") {
        let text = content_text(system);
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }
    for message in request.get("messages").and_then(Value::as_array).into_iter().flatten() {
        append_openai_message(message, &mut messages);
    }
    let mut result = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });
    if stream {
        result["stream_options"] = json!({"include_usage": true});
    }
    copy_if_present(request, &mut result, "temperature");
    copy_if_present(request, &mut result, "top_p");
    copy_if_present(request, &mut result, "enable_thinking");
    if let Some(max) = request.get("max_tokens") {
        result["max_tokens"] = max.clone();
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        result["tools"] = Value::Array(tools.iter().filter_map(anthropic_tool_to_openai).collect());
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = anthropic_tool_choice(choice);
    }

    if let Some(cfg) = merge_thinking_config(request, provider_thinking) {
        if cfg.is_disabled() {
            if result.get("enable_thinking").is_some() {
                result["enable_thinking"] = json!(false);
            }
        } else {
            if let Some(effort) = cfg.resolved_reasoning_effort() {
                result["reasoning_effort"] = json!(effort);
            }
            if let Some(enable_thinking) = cfg.resolved_enable_thinking() {
                if result.get("enable_thinking").is_some() || cfg.mode.as_deref() == Some("enable_thinking") {
                    result["enable_thinking"] = json!(enable_thinking);
                }
            }
            if cfg.prefix_thought.unwrap_or(false) {
                apply_prefix_thought(&mut result);
            }
        }
    }

    result
}

pub fn anthropic_to_openai_responses(
    request: &Value,
    model: &str,
    stream: bool,
    provider_thinking: Option<&crate::provider::ThinkingConfig>,
) -> Value {
    let mut result = json!({
        "model": model,
        "input": anthropic_messages_to_responses_input(request),
        "stream": stream,
    });
    if let Some(system) = request.get("system") {
        let instructions = content_text(system);
        if !instructions.is_empty() {
            result["instructions"] = Value::String(instructions);
        }
    }
    copy_if_present(request, &mut result, "temperature");
    copy_if_present(request, &mut result, "top_p");
    if let Some(max) = request.get("max_tokens") {
        result["max_output_tokens"] = max.clone();
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        result["tools"] = Value::Array(tools.iter().filter_map(anthropic_tool_to_responses).collect());
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = anthropic_tool_choice_to_responses(choice);
    }

    if let Some(cfg) = merge_thinking_config(request, provider_thinking) {
        if !cfg.is_disabled() {
            if let Some(effort) = cfg.resolved_reasoning_effort() {
                result["reasoning"] = json!({ "effort": effort });
            }
            if cfg.prefix_thought.unwrap_or(false) {
                apply_prefix_thought(&mut result);
            }
        }
    }

    result
}

/// Apply fields required by the ChatGPT Codex OAuth backend.
pub fn apply_codex_oauth_response_body(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.insert("store".to_string(), Value::Bool(false));
    let include = object
        .entry("include")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !include.is_array() {
        *include = Value::Array(Vec::new());
    }
    let items = include.as_array_mut().expect("include normalized to array");
    let encrypted_content = Value::String("reasoning.encrypted_content".to_string());
    if !items.contains(&encrypted_content) {
        items.push(encrypted_content);
    }
}

/// Strip non-standard fields from Codex Responses request bodies to avoid upstream 400 errors.
pub fn sanitize_codex_responses_body(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.remove("prompt_cache_retention");
    object.remove("prompt_cache_ttl");
    object.remove("prompt_cache_key");
    object.remove("wire_api");
    object.remove("auto_review");
}

pub fn openai_chat_to_anthropic(value: &Value, fallback_model: &str) -> Value {
    let choice = value.get("choices").and_then(Value::as_array).and_then(|items| items.first());
    let message = choice.and_then(|choice| choice.get("message")).cloned().unwrap_or_else(|| json!({}));
    let mut content = Vec::new();
    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        content.push(json!({"type": "thinking", "thinking": reasoning}));
    }
    if let Some(text) = message.get("content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
        content.push(json!({"type": "text", "text": text}));
    }
    for block in message.get("content").and_then(Value::as_array).into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") | Some("output_text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    content.push(json!({"type": "text", "text": text}));
                }
            }
            // OpenAI refusal content has no dedicated Anthropic content block;
            // expose the provider's user-facing refusal as normal assistant text.
            Some("refusal") => {
                if let Some(text) = block.get("refusal").or_else(|| block.get("text"))
                    .and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    content.push(json!({"type": "text", "text": text}));
                }
            }
            // Reasoning/thinking fields are intentionally not replayed because
            // Anthropic requires a provider-specific signature for later turns.
            Some("reasoning") | Some("thinking") => {}
            _ => {}
        }
    }
    for call in message.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        let function = call.get("function").cloned().unwrap_or_else(|| json!({}));
        let input = function.get("arguments").and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_else(|| json!({}));
        content.push(json!({
            "type": "tool_use",
            "id": call.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
            "name": function.get("name").and_then(Value::as_str).unwrap_or("tool"),
            "input": input,
        }));
    }
    let finish = choice.and_then(|choice| choice.get("finish_reason")).and_then(Value::as_str);
    let total_input = value
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = value
        .pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, total_input);
    let usage = json!({
        "input_tokens": total_input.saturating_sub(cached),
        "cache_read_input_tokens": cached,
        "cache_creation_input_tokens": 0,
        "output_tokens": value.pointer("/usage/completion_tokens").and_then(Value::as_i64).unwrap_or(0),
    });
    json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type": "message",
        "role": "assistant",
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "content": content,
        "stop_reason": match finish {
            Some("tool_calls") | Some("function_call") => "tool_use",
            Some("length") => "max_tokens",
            _ => "end_turn",
        },
        "stop_sequence": Value::Null,
        "usage": usage,
    })
}

/// Normalize OpenAI-compatible error payloads to Anthropic's error envelope.
/// The upstream body is intentionally not reflected: compatible gateways may
/// include request headers or other sensitive diagnostic data in that body.
pub fn openai_error_to_anthropic(status: u16) -> Value {
    let (kind, message) = match status {
        400 | 404 | 405 | 422 => ("invalid_request_error", "上游服务拒绝了请求"),
        401 | 403 => ("authentication_error", "上游服务拒绝了 API 凭据"),
        429 => ("rate_limit_error", "上游服务请求频率受限"),
        408 | 504 => ("api_error", "上游服务请求超时"),
        _ => ("api_error", "上游服务返回错误"),
    };
    json!({"type":"error", "error":{"type":kind, "message":message}})
}

pub fn openai_responses_to_anthropic(value: &Value, fallback_model: &str) -> Value {
    if let Some(error) = responses_failed_anthropic_error(value) {
        return error;
    }
    let mut content = Vec::new();
    for item in value.get("output").and_then(Value::as_array).into_iter().flatten() {
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            let input = item.get("arguments").and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_else(|| json!({}));
            content.push(json!({
                "type": "tool_use",
                "id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),
                "name": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
                "input": input,
            }));
            continue;
        }
        for block in item.get("content").and_then(Value::as_array).into_iter().flatten() {
            if matches!(block.get("type").and_then(Value::as_str), Some("output_text" | "text")) {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    content.push(json!({"type": "text", "text": text}));
                }
            } else if block.get("type").and_then(Value::as_str) == Some("refusal") {
                if let Some(text) = block.get("refusal").or_else(|| block.get("text")).and_then(Value::as_str) {
                    content.push(json!({"type": "text", "text": text}));
                }
            } else if let Some(text) = block.get("text").and_then(Value::as_str) {
                content.push(json!({"type": "text", "text": text}));
            }
        }
    }
    let has_tool = content.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
    let stop_reason = if has_tool {
        "tool_use"
    } else if value.get("status").and_then(Value::as_str) == Some("incomplete")
        && value.pointer("/incomplete_details/reason").and_then(Value::as_str) == Some("max_output_tokens") {
        "max_tokens"
    } else {
        "end_turn"
    };
    json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type": "message",
        "role": "assistant",
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": anthropic_usage_from_openai_responses(value)
    })
}

/// Map a Responses `status=failed` envelope to an Anthropic error object.
pub fn responses_failed_anthropic_error(value: &Value) -> Option<Value> {
    if value.get("status").and_then(Value::as_str) != Some("failed") {
        return None;
    }
    let message = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error/code").and_then(Value::as_str))
        .unwrap_or("上游 Responses 请求失败");
    Some(json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": message,
        }
    }))
}

fn anthropic_usage_from_openai_responses(value: &Value) -> Value {
    let total_input = value
        .pointer("/usage/input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = value
        .pointer("/usage/input_tokens_details/cached_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, total_input);
    json!({
        "input_tokens": total_input.saturating_sub(cached),
        "cache_read_input_tokens": cached,
        "cache_creation_input_tokens": 0,
        "output_tokens": value.pointer("/usage/output_tokens").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// Upstream protocol used by [`OpenAiSseConverter`].
#[derive(Debug, Clone, Copy)]
pub enum OpenAiStreamProtocol {
    Chat,
    Responses,
}

/// Incrementally converts OpenAI SSE payloads into Anthropic Messages SSE
/// events. The converter deliberately emits deltas as they arrive instead of
/// buffering a completed OpenAI response.
pub struct OpenAiSseConverter {
    protocol: OpenAiStreamProtocol,
    fallback_model: String,
    message_id: String,
    model: String,
    started: bool,
    completed: bool,
    next_block_index: usize,
    text_blocks: BTreeMap<(usize, usize), usize>,
    tool_blocks: BTreeMap<usize, usize>,
    /// Accumulated tool-call argument text already emitted as `input_json_delta`.
    tool_arguments_sent: BTreeMap<usize, String>,
    open_blocks: Vec<usize>,
    thinking_block: Option<usize>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
    stop_reason: String,
}

impl OpenAiSseConverter {
    pub fn new(protocol: OpenAiStreamProtocol, fallback_model: &str) -> Self {
        Self {
            protocol,
            fallback_model: fallback_model.to_string(),
            message_id: "msg_proxy".to_string(),
            model: fallback_model.to_string(),
            started: false,
            completed: false,
            next_block_index: 0,
            text_blocks: BTreeMap::new(),
            tool_blocks: BTreeMap::new(),
            tool_arguments_sent: BTreeMap::new(),
            open_blocks: Vec::new(),
            thinking_block: None,
            input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            stop_reason: "end_turn".to_string(),
        }
    }

    pub fn push_event(&mut self, event: &Value) -> Vec<u8> {
        let mut output = String::new();
        self.observe_response(event);
        match self.protocol {
            OpenAiStreamProtocol::Chat => self.push_chat_event(event, &mut output),
            OpenAiStreamProtocol::Responses => self.push_responses_event(event, &mut output),
        }
        output.into_bytes()
    }

    /// Complete a stream when the upstream emits `[DONE]` or closes after a
    /// terminal Responses event.
    pub fn finish_stream(&mut self) -> Vec<u8> {
        let mut output = String::new();
        self.finish(&mut output);
        output.into_bytes()
    }

    /// Convert a transport or upstream event error to Anthropic SSE without
    /// exposing upstream headers, credentials, or raw response bodies.
    pub fn error_event(&mut self, message: &str) -> Vec<u8> {
        let mut output = String::new();
        push_event(&mut output, "error", json!({
            "type": "error",
            "error": {"type": "api_error", "message": message}
        }));
        self.completed = true;
        output.into_bytes()
    }

    pub fn usage(&self) -> Option<super::UsageCounts> {
        self.started.then_some(super::UsageCounts {
            input_tokens: self.input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens,
            output_tokens: self.output_tokens,
            envelope_id: None,
        })
    }

    fn push_chat_event(&mut self, event: &Value, output: &mut String) {
        let Some(choice) = event.get("choices").and_then(Value::as_array).and_then(|items| items.first()) else {
            return;
        };
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            let index = self.ensure_thinking_block(output);
            push_event(output, "content_block_delta", json!({
                "type":"content_block_delta", "index":index,
                "delta":{"type":"thinking_delta", "thinking":reasoning}
            }));
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
            if let Some(t_idx) = self.thinking_block.take() {
                if let Some(pos) = self.open_blocks.iter().position(|&x| x == t_idx) {
                    self.open_blocks.remove(pos);
                }
                push_event(output, "content_block_stop", json!({
                    "type":"content_block_stop", "index":t_idx
                }));
            }
            let index = self.ensure_text_block((0, 0), output);
            push_event(output, "content_block_delta", json!({
                "type":"content_block_delta", "index":index,
                "delta":{"type":"text_delta", "text":text}
            }));
        }
        for call in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
            if let Some(t_idx) = self.thinking_block.take() {
                if let Some(pos) = self.open_blocks.iter().position(|&x| x == t_idx) {
                    self.open_blocks.remove(pos);
                }
                push_event(output, "content_block_stop", json!({
                    "type":"content_block_stop", "index":t_idx
                }));
            }
            let upstream_index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                call.get("id").and_then(Value::as_str),
                call.pointer("/function/name").and_then(Value::as_str),
                output,
            );
            if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                push_event(output, "content_block_delta", json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"input_json_delta", "partial_json":arguments}
                }));
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.stop_reason = match reason {
                "tool_calls" | "function_call" => "tool_use",
                "length" => "max_tokens",
                _ => "end_turn",
            }.to_string();
        }
    }

    fn push_responses_event(&mut self, event: &Value, output: &mut String) {
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        if event_type == "error" || event_type == "response.failed" {
            let message = event.pointer("/error/message")
                .or_else(|| event.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("上游服务返回流式错误");
            output.push_str(&String::from_utf8_lossy(&self.error_event(message)));
            return;
        }
        if event_type == "response.output_item.added" {
            let item = event.get("item").unwrap_or(&Value::Null);
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                self.stop_reason = "tool_use".to_string();
                let upstream_index = event.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                self.ensure_tool_block(
                    upstream_index,
                    item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str),
                    item.get("name").and_then(Value::as_str),
                    output,
                );
            }
        } else if event_type == "response.content_part.added" {
            let part = event.get("part").unwrap_or(&Value::Null);
            if matches!(part.get("type").and_then(Value::as_str), Some("output_text" | "text")) {
                let key = (
                    event.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize,
                    event.get("content_index").and_then(Value::as_u64).unwrap_or(0) as usize,
                );
                self.ensure_text_block(key, output);
            }
        } else if event_type == "response.output_text.delta" {
            let key = (
                event.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize,
                event.get("content_index").and_then(Value::as_u64).unwrap_or(0) as usize,
            );
            let index = self.ensure_text_block(key, output);
            if let Some(delta) = event.get("delta").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                push_event(output, "content_block_delta", json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"text_delta", "text":delta}
                }));
            }
        } else if event_type == "response.function_call_arguments.delta" {
            let upstream_index = event.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                event.get("call_id").and_then(Value::as_str),
                event.get("name").and_then(Value::as_str),
                output,
            );
            if let Some(delta) = event.get("delta").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                self.tool_arguments_sent
                    .entry(upstream_index)
                    .or_default()
                    .push_str(delta);
                push_event(output, "content_block_delta", json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"input_json_delta", "partial_json":delta}
                }));
            }
        } else if event_type == "response.function_call_arguments.done" {
            let upstream_index = event.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                event.get("call_id").and_then(Value::as_str),
                event.get("name").and_then(Value::as_str),
                output,
            );
            let full = event
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("");
            let sent = self
                .tool_arguments_sent
                .get(&upstream_index)
                .map(String::as_str)
                .unwrap_or("");
            let suffix = if full.starts_with(sent) {
                &full[sent.len()..]
            } else if sent.is_empty() {
                full
            } else {
                // Dense streams may re-send a full snapshot that does not share a
                // common prefix with prior deltas; emit nothing to avoid duplicates.
                ""
            };
            if !suffix.is_empty() {
                self.tool_arguments_sent
                    .entry(upstream_index)
                    .or_default()
                    .push_str(suffix);
                push_event(output, "content_block_delta", json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"input_json_delta", "partial_json":suffix}
                }));
            }
        }
        if matches!(event_type, "response.completed" | "response.incomplete") {
            if event_type == "response.incomplete" || event.pointer("/response/status").and_then(Value::as_str) == Some("incomplete") {
                self.stop_reason = "max_tokens".to_string();
            }
            self.finish(output);
        }
    }

    fn observe_response(&mut self, event: &Value) {
        let response = event.get("response").unwrap_or(event);
        if let Some(id) = response.get("id").and_then(Value::as_str) {
            self.message_id = id.to_string();
        }
        if let Some(model) = response.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        let usage = response.get("usage").or_else(|| event.get("usage"));
        if let Some(usage) = usage {
            let total_input = usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(self.input_tokens + self.cache_read_input_tokens);
            let cached = usage
                .pointer("/input_tokens_details/cached_tokens")
                .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(self.cache_read_input_tokens)
                .clamp(0, total_input);
            self.input_tokens = total_input.saturating_sub(cached);
            self.cache_read_input_tokens = cached;
            self.cache_creation_input_tokens = usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(self.cache_creation_input_tokens);
            self.output_tokens = usage.get("output_tokens").or_else(|| usage.get("completion_tokens"))
                .and_then(Value::as_i64).unwrap_or(self.output_tokens);
        }
    }

    fn ensure_started(&mut self, output: &mut String) {
        if self.started { return; }
        push_event(output, "message_start", json!({"type":"message_start","message":{
            "id": self.message_id.as_str(), "type":"message", "role":"assistant", "content":[],
            "model": if self.model.is_empty() { &self.fallback_model } else { &self.model },
            "stop_reason":Value::Null, "stop_sequence":Value::Null,
            "usage":{
                "input_tokens":self.input_tokens,
                "cache_read_input_tokens":self.cache_read_input_tokens,
                "cache_creation_input_tokens":self.cache_creation_input_tokens,
                "output_tokens":0
            }
        }}));
        self.started = true;
    }

    fn ensure_thinking_block(&mut self, output: &mut String) -> usize {
        if let Some(index) = self.thinking_block {
            return index;
        }
        self.ensure_started(output);
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.thinking_block = Some(index);
        self.open_blocks.push(index);
        push_event(output, "content_block_start", json!({
            "type":"content_block_start", "index":index,
            "content_block":{"type":"thinking", "thinking":""}
        }));
        index
    }

    fn ensure_text_block(&mut self, key: (usize, usize), output: &mut String) -> usize {
        if let Some(index) = self.text_blocks.get(&key) { return *index; }
        self.ensure_started(output);
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.text_blocks.insert(key, index);
        self.open_blocks.push(index);
        push_event(output, "content_block_start", json!({
            "type":"content_block_start", "index":index,
            "content_block":{"type":"text", "text":""}
        }));
        index
    }

    fn ensure_tool_block(&mut self, upstream_index: usize, id: Option<&str>, name: Option<&str>, output: &mut String) -> usize {
        if let Some(index) = self.tool_blocks.get(&upstream_index) { return *index; }
        self.ensure_started(output);
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.tool_blocks.insert(upstream_index, index);
        self.open_blocks.push(index);
        push_event(output, "content_block_start", json!({
            "type":"content_block_start", "index":index,
            "content_block":{
                "type":"tool_use", "id":id.unwrap_or("tool_call"),
                "name":name.unwrap_or("tool"), "input":{}
            }
        }));
        index
    }

    fn finish(&mut self, output: &mut String) {
        if self.completed { return; }
        self.ensure_started(output);
        for index in self.open_blocks.drain(..) {
            push_event(output, "content_block_stop", json!({"type":"content_block_stop", "index":index}));
        }
        push_event(output, "message_delta", json!({
            "type":"message_delta", "delta":{"stop_reason":self.stop_reason.as_str(),"stop_sequence":Value::Null},
            "usage":{
                "input_tokens":self.input_tokens,
                "cache_read_input_tokens":self.cache_read_input_tokens,
                "cache_creation_input_tokens":self.cache_creation_input_tokens,
                "output_tokens":self.output_tokens
            }
        }));
        push_event(output, "message_stop", json!({"type":"message_stop"}));
        self.completed = true;
    }
}

fn anthropic_messages_to_responses_input(request: &Value) -> Vec<Value> {
    let mut output = Vec::new();
    for message in request.get("messages").and_then(Value::as_array).into_iter().flatten() {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        let is_assistant = role == "assistant";
        // Responses API: user/system content uses input_*; assistant replay uses
        // output_text. Using input_text on assistant turns is rejected by many
        // OpenAI-compatible gateways (opaque upstream 502 on Claude Code turn 2+).
        let text_type = if is_assistant { "output_text" } else { "input_text" };
        let mut message_content = Vec::new();
        let mut tool_items = Vec::new();
        match message.get("content") {
            Some(Value::String(text)) if !text.is_empty() => {
                message_content.push(json!({"type": text_type, "text": text}));
            }
            Some(Value::Array(blocks)) => for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                            message_content.push(json!({"type": text_type, "text": text}));
                        }
                    }
                    Some("image") => {
                        if !is_assistant {
                            if let Some(url) = image_url(block) {
                                message_content.push(json!({"type": "input_image", "image_url": url}));
                            }
                        }
                    }
                    // Anthropic thinking signatures have no portable Responses
                    // equivalent, so they are intentionally not replayed upstream.
                    Some("thinking") => {}
                    Some("tool_use") => tool_items.push(json!({
                        "type": "function_call",
                        "call_id": block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
                        "name": block.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        "arguments": serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string()),
                    })),
                    Some("tool_result") | Some("tool_search_tool_result") => tool_items.push(json!({
                        "type": "function_call_output",
                        "call_id": block.get("tool_use_id").or_else(|| block.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),
                        "output": content_text(block.get("content").unwrap_or(&Value::Null)),
                    })),
                    _ => {}
                }
            },
            _ => {}
        }
        if !message_content.is_empty() {
            output.push(json!({
                "type": "message",
                "role": if is_assistant { "assistant" } else { "user" },
                "content": message_content,
            }));
        }
        output.extend(tool_items);
    }
    output
}

/// Convert a completed Anthropic message into an Anthropic SSE sequence. This
/// remains a fallback for providers that return JSON despite a streaming request;
/// normal OpenAI Chat/Responses traffic uses [`OpenAiSseConverter`] instead.
pub fn anthropic_message_to_sse(message: &Value) -> Vec<u8> {
    let mut output = String::new();
    push_event(&mut output, "message_start", json!({"type":"message_start","message":{
        "id": message.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type":"message", "role":"assistant", "content":[],
        "model": message.get("model").and_then(Value::as_str).unwrap_or(""),
        "stop_reason":Value::Null, "stop_sequence":Value::Null,
        "usage":{"input_tokens":message.pointer("/usage/input_tokens").and_then(Value::as_i64).unwrap_or(0),"output_tokens":0}
    }}));
    for (index, block) in message.get("content").and_then(Value::as_array).into_iter().flatten().enumerate() {
        push_event(&mut output, "content_block_start", json!({"type":"content_block_start","index":index,"content_block":block}));
        match block.get("type").and_then(Value::as_str) {
            Some("text") => push_event(&mut output, "content_block_delta", json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":block.get("text").and_then(Value::as_str).unwrap_or("")}})),
            Some("tool_use") => push_event(&mut output, "content_block_delta", json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string())}})),
            _ => {}
        }
        push_event(&mut output, "content_block_stop", json!({"type":"content_block_stop","index":index}));
    }
    push_event(&mut output, "message_delta", json!({"type":"message_delta","delta":{"stop_reason":message.get("stop_reason").cloned().unwrap_or(Value::Null),"stop_sequence":Value::Null},"usage":{"output_tokens":message.pointer("/usage/output_tokens").and_then(Value::as_i64).unwrap_or(0)}}));
    push_event(&mut output, "message_stop", json!({"type":"message_stop"}));
    output.into_bytes()
}

fn append_openai_message(message: &Value, output: &mut Vec<Value>) {
    let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
    let mut text_blocks = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) => text_blocks.push(json!({"type":"text","text":text})),
        Some(Value::Array(blocks)) => for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => text_blocks.push(json!({"type":"text","text":block.get("text").and_then(Value::as_str).unwrap_or("")})),
                // Anthropic thinking requires a signature that cannot be
                // synthesized for OpenAI-compatible upstreams.
                Some("thinking") => {},
                Some("image") => if let Some(url) = image_url(block) { text_blocks.push(json!({"type":"image_url","image_url":{"url":url}})); },
                Some("tool_use") => tool_calls.push(json!({"id":block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),"type":"function","function":{"name":block.get("name").and_then(Value::as_str).unwrap_or("tool"),"arguments":serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string())}})),
                Some("tool_result") | Some("tool_search_tool_result") => tool_results.push(json!({"role":"tool","tool_call_id":block.get("tool_use_id").or_else(|| block.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),"content":content_text(block.get("content").unwrap_or(&Value::Null))})),
                _ => {}
            }
        },
        _ => {}
    }
    if !text_blocks.is_empty() || !tool_calls.is_empty() {
        let mut result = Map::new();
        result.insert("role".to_string(), Value::String(if role == "assistant" { "assistant" } else { "user" }.to_string()));
        if text_blocks.len() == 1 && text_blocks[0].get("type").and_then(Value::as_str) == Some("text") {
            result.insert("content".to_string(), text_blocks[0].get("text").cloned().unwrap_or(Value::String(String::new())));
        } else { result.insert("content".to_string(), Value::Array(text_blocks)); }
        if !tool_calls.is_empty() { result.insert("tool_calls".to_string(), Value::Array(tool_calls)); }
        output.push(Value::Object(result));
    }
    output.extend(tool_results);
}

pub fn is_claude_server_tool_type(tool_type: &str) -> bool {
    let lower = tool_type.trim().to_ascii_lowercase();
    let prefixes = [
        "advisor_",
        "agent_toolset_",
        "bash_",
        "code_execution_",
        "computer_",
        "memory_",
        "text_editor_",
        "tool_search_tool_",
        "web_fetch_",
        "web_search_",
    ];
    prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

fn extract_tool_name<'a>(tool: &'a Value) -> Option<&'a str> {
    if let Some(name) = tool.get("name").and_then(Value::as_str) {
        if !name.trim().is_empty() {
            return Some(name);
        }
    }
    if let Some(tool_type) = tool.get("type").and_then(Value::as_str) {
        if is_claude_server_tool_type(tool_type) {
            return Some(tool_type);
        }
    }
    None
}

fn anthropic_tool_to_openai(tool: &Value) -> Option<Value> {
    let name = extract_tool_name(tool)?;
    Some(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
            "parameters": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}}))
        }
    }))
}

fn anthropic_tool_to_responses(tool: &Value) -> Option<Value> {
    let name = extract_tool_name(tool)?;
    Some(json!({
        "type": "function",
        "name": name,
        "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
        "parameters": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
    }))
}

fn anthropic_tool_choice(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("tool") => json!({"type":"function","function":{"name":value.get("name").and_then(Value::as_str).unwrap_or("")}}),
        Some("any") => Value::String("required".to_string()),
        _ => Value::String("auto".to_string()),
    }
}

fn anthropic_tool_choice_to_responses(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("tool") => json!({"type": "function", "name": value.get("name").and_then(Value::as_str).unwrap_or("")}),
        Some("any") => Value::String("required".to_string()),
        Some("none") => Value::String("none".to_string()),
        _ => Value::String("auto".to_string()),
    }
}

fn image_url(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    if source.get("type").and_then(Value::as_str) == Some("base64") {
        Some(format!("data:{};base64,{}", source.get("media_type").and_then(Value::as_str).unwrap_or("image/png"), source.get("data").and_then(Value::as_str).unwrap_or("")))
    } else { source.get("url").and_then(Value::as_str).map(str::to_string) }
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().filter_map(|item| item.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        Value::Object(_) => value.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn copy_if_present(from: &Value, to: &mut Value, key: &str) {
    if let Some(value) = from.get(key) { to[key] = value.clone(); }
}

fn push_event(output: &mut String, event: &str, data: Value) {
    output.push_str("event: "); output.push_str(event); output.push('\n');
    output.push_str("data: "); output.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string())); output.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_oauth_body_disables_storage_and_requests_encrypted_reasoning() {
        let mut value = json!({"model":"gpt-test","include":["file_search_call.results"]});
        apply_codex_oauth_response_body(&mut value);
        assert_eq!(value["store"], false);
        assert!(value["include"]
            .as_array()
            .unwrap()
            .contains(&json!("reasoning.encrypted_content")));
        assert!(value["include"]
            .as_array()
            .unwrap()
            .contains(&json!("file_search_call.results")));
    }

    #[test]
    fn chat_conversion_preserves_tools_images_and_model() {
        let request = json!({
            "model": "ignored", "max_tokens": 32, "stream": true,
            "messages": [{"role":"user","content":[
                {"type":"text","text":"look"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}
            ]}],
            "tools":[{"name":"lookup","description":"Find data","input_schema":{"type":"object"}}]
        });
        let output = anthropic_to_openai_chat(&request, "gpt-test", true, None);
        assert_eq!(output["model"], "gpt-test");
        assert_eq!(output["stream"], true);
        assert_eq!(output["stream_options"]["include_usage"], true);
        assert_eq!(output["tools"][0]["function"]["name"], "lookup");
        assert_eq!(output["messages"][0]["content"][1]["image_url"]["url"], "data:image/png;base64,abc");
    }

    #[test]
    fn chat_response_and_sse_preserve_tool_call() {
        let upstream = json!({
            "id":"chatcmpl_1", "model":"gpt-test",
            "choices":[{"finish_reason":"tool_calls","message":{"content":null,"tool_calls":[{
                "id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"x\"}"}
            }]}}],
            "usage":{"prompt_tokens":3,"completion_tokens":2}
        });
        let message = openai_chat_to_anthropic(&upstream, "fallback");
        assert_eq!(message["content"][0]["type"], "tool_use");
        assert_eq!(message["content"][0]["input"]["q"], "x");
        let sse = String::from_utf8(anthropic_message_to_sse(&message)).unwrap();
        assert!(sse.contains("event: message_start"));
        assert!(sse.contains("\"type\":\"input_json_delta\""));
        assert!(sse.contains("event: message_stop"));
    }

    #[test]
    fn chat_response_handles_array_content_refusal_and_usage() {
        let upstream = json!({
            "id":"chatcmpl_2", "model":"gpt-test",
            "choices":[{"finish_reason":"length","message":{"content":[
                {"type":"text","text":"partial"},
                {"type":"refusal","refusal":"cannot continue"},
                {"type":"reasoning","text":"not replayed"}
            ]}}],
            "usage":{"prompt_tokens":7,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":2}}
        });
        let message = openai_chat_to_anthropic(&upstream, "fallback");
        assert_eq!(message["content"][0]["text"], "partial");
        assert_eq!(message["content"][1]["text"], "cannot continue");
        assert_eq!(message["content"].as_array().unwrap().len(), 2);
        assert_eq!(message["stop_reason"], "max_tokens");
        assert_eq!(message["usage"]["input_tokens"], 5);
        assert_eq!(message["usage"]["cache_read_input_tokens"], 2);
    }

    #[test]
    fn upstream_errors_are_sanitized() {
        let error = openai_error_to_anthropic(401);
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"]["type"], "authentication_error");
        assert!(error.to_string().contains("凭据"));
    }

    #[test]
    fn responses_failed_envelope_maps_to_anthropic_error() {
        let upstream = json!({
            "id":"resp_failed","status":"failed","error":{"code":"server_error","message":"model overloaded"}
        });
        let message = openai_responses_to_anthropic(&upstream, "fallback");
        assert_eq!(message["type"], "error");
        assert_eq!(message["error"]["type"], "api_error");
        assert!(message["error"]["message"].as_str().unwrap().contains("overloaded"));
        assert!(message.get("stop_reason").is_none());
    }

    #[test]
    fn responses_conversion_preserves_function_call() {
        let upstream = json!({"id":"resp_1","model":"gpt-test","output":[
            {"type":"message","content":[{"type":"output_text","text":"done"}]},
            {"type":"function_call","call_id":"call_2","name":"save","arguments":"{\"ok\":true}"}
        ],"usage":{"input_tokens":4,"input_tokens_details":{"cached_tokens":1},"output_tokens":5}});
        let message = openai_responses_to_anthropic(&upstream, "fallback");
        assert_eq!(message["content"][0]["text"], "done");
        assert_eq!(message["content"][1]["name"], "save");
        assert_eq!(message["usage"]["input_tokens"], 3);
        assert_eq!(message["usage"]["cache_read_input_tokens"], 1);
        assert_eq!(message["usage"]["output_tokens"], 5);
    }

    #[test]
    fn responses_request_uses_native_tools_images_and_tool_outputs() {
        let request = json!({
            "system": "Be concise.", "max_tokens": 64,
            "messages": [
                {"role":"user","content":[
                    {"type":"text","text":"Inspect this"},
                    {"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}
                ]},
                {"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"lookup","input":{"q":"x"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"found"}]}
            ],
            "tools":[{"name":"lookup","description":"Find","input_schema":{"type":"object","properties":{"q":{"type":"string"}}}}],
            "tool_choice":{"type":"tool","name":"lookup"}
        });
        let output = anthropic_to_openai_responses(&request, "gpt-test", false, None);
        assert_eq!(output["instructions"], "Be concise.");
        assert_eq!(output["input"][0]["type"], "message");
        assert_eq!(output["input"][0]["role"], "user");
        assert_eq!(output["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(output["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(output["input"][1]["type"], "function_call");
        assert_eq!(output["input"][2]["type"], "function_call_output");
        assert_eq!(output["tools"][0]["name"], "lookup");
        assert!(output["tools"][0].get("function").is_none());
        assert_eq!(output["tool_choice"]["name"], "lookup");
    }

    #[test]
    fn responses_replays_assistant_text_as_output_text() {
        let request = json!({
            "messages": [
                {"role":"user","content":"hello"},
                {"role":"assistant","content":[{"type":"text","text":"hi there"}]},
                {"role":"user","content":"继续"}
            ]
        });
        let output = anthropic_to_openai_responses(&request, "gpt-test", true, None);
        let input = output["input"].as_array().expect("input array");
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[1]["content"][0]["text"], "hi there");
        assert_eq!(input[2]["role"], "user");
        assert_eq!(input[2]["content"][0]["type"], "input_text");
    }

    #[test]
    fn responses_system_is_only_in_instructions_and_tool_results_keep_order() {
        let request = json!({
            "system":[{"type":"text","text":"System rule"}],
            "messages":[
                {"role":"assistant","content":[
                    {"type":"tool_use","id":"call_a","name":"first","input":{}},
                    {"type":"tool_use","id":"call_b","name":"second","input":{}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"call_a","content":"a"},
                    {"type":"tool_result","tool_use_id":"call_b","content":"b"}
                ]}
            ]
        });
        let output = anthropic_to_openai_responses(&request, "gpt-test", false, None);
        assert_eq!(output["instructions"], "System rule");
        assert!(output["input"].as_array().unwrap().iter().all(|item| {
            item.get("role").and_then(Value::as_str) != Some("system")
        }));
        assert_eq!(output["input"][0]["call_id"], "call_a");
        assert_eq!(output["input"][1]["call_id"], "call_b");
        assert_eq!(output["input"][2]["call_id"], "call_a");
        assert_eq!(output["input"][3]["call_id"], "call_b");
    }

    #[test]
    fn responses_incomplete_maps_to_max_tokens() {
        let upstream = json!({
            "id":"resp_2", "status":"incomplete",
            "incomplete_details":{"reason":"max_output_tokens"},
            "output":[{"type":"message","content":[{"type":"output_text","text":"partial"}]}]
        });
        assert_eq!(openai_responses_to_anthropic(&upstream, "fallback")["stop_reason"], "max_tokens");
    }

    #[test]
    fn chat_sse_emits_text_delta_before_done() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "fallback");
        let first = converter.push_event(&json!({
            "id":"chatcmpl_stream", "model":"gpt-test",
            "choices":[{"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}]
        }));
        let done = converter.push_event(&json!({
            "choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":2,"completion_tokens":1}
        }));
        let stop = converter.finish_stream();
        let first = String::from_utf8(first).unwrap();
        let done = String::from_utf8(done).unwrap();
        let stop = String::from_utf8(stop).unwrap();
        assert!(first.contains("event: message_start"));
        assert!(first.contains("\"text\":\"Hel\""));
        assert!(done.contains("\"text\":\"lo\""));
        assert!(stop.contains("event: message_stop"));
    }

    #[test]
    fn responses_sse_arguments_done_flushes_unsent_suffix_only() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "fallback");
        let _ = converter.push_event(&json!({
            "type":"response.output_item.added", "output_index":0,
            "item":{"type":"function_call","call_id":"call_1","name":"lookup"}
        }));
        let delta = String::from_utf8(converter.push_event(&json!({
            "type":"response.function_call_arguments.delta", "output_index":0,
            "call_id":"call_1", "delta":"{\"q\":"
        }))).unwrap();
        let done = String::from_utf8(converter.push_event(&json!({
            "type":"response.function_call_arguments.done", "output_index":0,
            "call_id":"call_1", "arguments":"{\"q\":\"x\"}"
        }))).unwrap();
        assert!(delta.contains("input_json_delta"));
        assert!(delta.contains("{\\\"q\\\":") || delta.contains("{\"q\":"));
        assert!(done.contains("input_json_delta"));
        assert!(done.contains("\\\"x\\\"}") || done.contains("\"x\"}"));
        assert!(!done.contains("{\\\"q\\\":\\\"x\\\"}") && !done.contains("{\"q\":\"x\"}"));
    }

    #[test]
    fn responses_sse_arguments_done_skips_full_retransmit() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "fallback");
        let _ = converter.push_event(&json!({
            "type":"response.output_item.added", "output_index":0,
            "item":{"type":"function_call","call_id":"call_1","name":"lookup"}
        }));
        let _ = converter.push_event(&json!({
            "type":"response.function_call_arguments.delta", "output_index":0,
            "call_id":"call_1", "delta":"{\"q\":\"x\"}"
        }));
        let done = String::from_utf8(converter.push_event(&json!({
            "type":"response.function_call_arguments.done", "output_index":0,
            "call_id":"call_1", "arguments":"{\"q\":\"x\"}"
        }))).unwrap();
        assert!(!done.contains("input_json_delta"));
    }

    #[test]
    fn responses_sse_emits_incremental_tool_arguments() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "fallback");
        let start = converter.push_event(&json!({
            "type":"response.output_item.added", "output_index":0,
            "item":{"type":"function_call","call_id":"call_1","name":"lookup"}
        }));
        let delta = converter.push_event(&json!({
            "type":"response.function_call_arguments.delta", "output_index":0,
            "call_id":"call_1", "name":"lookup", "delta":"{\"q\":\"x\"}"
        }));
        let finish = converter.push_event(&json!({
            "type":"response.completed", "response":{"id":"resp_1","model":"gpt-test","status":"completed",
            "usage":{"input_tokens":3,"input_tokens_details":{"cached_tokens":1},"output_tokens":2}}
        }));
        let start = String::from_utf8(start).unwrap();
        let delta = String::from_utf8(delta).unwrap();
        let finish = String::from_utf8(finish).unwrap();
        assert!(start.contains("event: content_block_start"));
        assert!(delta.contains("\"type\":\"input_json_delta\""));
        assert!(finish.contains("\"stop_reason\":\"tool_use\""));
        assert!(finish.contains("\"cache_read_input_tokens\":1"));
        assert!(finish.contains("event: message_stop"));
    }

    #[test]
    fn sse_block_start_does_not_duplicate_completed_content() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "fallback");
        let event = String::from_utf8(converter.push_event(&json!({
            "choices":[{"delta":{"content":"hello"},"finish_reason":null}]
        }))).unwrap();
        let start = event.lines().find(|line| line.starts_with("data: ") && line.contains("content_block_start")).unwrap();
        assert!(start.contains("\"text\":\"\""));
        assert!(!start.contains("hello"));
        assert!(event.contains("\"text\":\"hello\""));
    }

    #[test]
    fn stream_errors_are_anthropic_error_events() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "fallback");
        let output = String::from_utf8(converter.error_event("上游流式响应中断")).unwrap();
        assert!(output.contains("event: error"));
        assert!(output.contains("\"type\":\"api_error\""));
        assert!(!output.contains("Bearer"));
    }

    #[test]
    fn chat_prompt_cache_key_strips_by_default_without_allowlist() {
        let mut body = json!({"model": "m", "messages": []});
        reinject_chat_prompt_cache_key(&mut body, Some("explicit"), Some("session"), false);
        assert!(body.get("prompt_cache_key").is_none());
        assert!(!chat_prompt_cache_allowed_for_base_url("https://api.openai.com/v1"));
    }

    #[test]
    fn chat_prompt_cache_key_reinjects_explicit_for_allowlisted_provider() {
        assert!(chat_prompt_cache_allowed_for_base_url("https://api.moonshot.cn/v1"));
        assert!(chat_prompt_cache_allowed_for_base_url("https://api.kimi.com/coding/v1"));
        let mut body = json!({"model": "kimi-k3", "messages": []});
        reinject_chat_prompt_cache_key(&mut body, Some("explicit-key"), Some("session"), true);
        assert_eq!(body["prompt_cache_key"], "explicit-key");
    }

    #[test]
    fn chat_prompt_cache_key_reinjects_session_when_allowlisted_and_no_explicit() {
        let mut body = json!({"model": "kimi-k3", "messages": []});
        reinject_chat_prompt_cache_key(&mut body, None, Some("sess-42"), true);
        assert_eq!(body["prompt_cache_key"], "sess-42");
        reinject_chat_prompt_cache_key(&mut body, Some("ignored"), Some("other"), true);
        assert_eq!(body["prompt_cache_key"], "sess-42");
    }

    #[test]
    fn anthropic_thinking_translates_to_openai_chat_and_responses() {
        let request = json!({
            "model": "claude-3-7-sonnet",
            "messages": [{"role": "user", "content": "solve math"}],
            "thinking": {"type": "enabled", "budget_tokens": 16000}
        });
        let chat = anthropic_to_openai_chat(&request, "o3-mini", false, None);
        assert_eq!(chat["reasoning_effort"], "medium");
        assert!(chat.get("thinking_budget").is_none());

        let responses = anthropic_to_openai_responses(&request, "o3-mini", false, None);
        assert_eq!(responses["reasoning"]["effort"], "medium");

        let provider = crate::provider::ThinkingConfig {
            mode: Some("budget".into()),
            budget_tokens: Some(16000),
            reasoning_effort: None,
            prefix_thought: None,
        };
        let enabled_only = json!({
            "model": "claude-3-7-sonnet",
            "messages": [{"role": "user", "content": "solve math"}],
            "thinking": {"type": "enabled"}
        });
        let merged = anthropic_to_openai_chat(&enabled_only, "o3-mini", false, Some(&provider));
        assert_eq!(merged["reasoning_effort"], "medium");
        assert!(merged.get("thinking_budget").is_none());
    }

    #[test]
    fn prefix_thought_injects_system_hint() {
        let request = json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "solve math"}],
            "thinking": {"type": "enabled"}
        });
        let provider = crate::provider::ThinkingConfig {
            mode: Some("enabled".into()),
            budget_tokens: None,
            reasoning_effort: Some("medium".into()),
            prefix_thought: Some(true),
        };
        let chat = anthropic_to_openai_chat(&request, "deepseek-chat", false, Some(&provider));
        let system = chat["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("Think step by step"));
    }

    #[test]
    fn deepseek_reasoning_content_streams_as_anthropic_thinking_events() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "fallback");
        let thought_chunk = converter.push_event(&json!({
            "id": "chatcmpl_r1",
            "model": "deepseek-r1",
            "choices": [{"delta": {"reasoning_content": "let me think"}, "finish_reason": null}]
        }));
        let thought_str = String::from_utf8(thought_chunk).unwrap();
        assert!(thought_str.contains("event: content_block_start"));
        assert!(thought_str.contains("\"type\":\"thinking\""));
        assert!(thought_str.contains("event: content_block_delta"));
        assert!(thought_str.contains("\"type\":\"thinking_delta\""));
        assert!(thought_str.contains("\"thinking\":\"let me think\""));

        let text_chunk = converter.push_event(&json!({
            "choices": [{"delta": {"content": "the answer is 42"}, "finish_reason": "stop"}]
        }));
        let text_str = String::from_utf8(text_chunk).unwrap();
        assert!(text_str.contains("event: content_block_stop"));
        assert!(text_str.contains("event: content_block_start"));
        assert!(text_str.contains("\"type\":\"text\""));
        assert!(text_str.contains("\"text\":\"the answer is 42\""));
    }
}
