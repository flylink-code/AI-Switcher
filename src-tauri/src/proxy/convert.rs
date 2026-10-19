//! Stateless Anthropic <-> OpenAI protocol conversion helpers.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn wants_stream(request: &Value) -> bool {
    request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
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
        .or_else(|| {
            session_hint
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    if let Some(key) = key {
        request["prompt_cache_key"] = Value::String(key.to_string());
    }
}

fn extract_effort_str(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Object(map) => map
            .get("type")
            .or_else(|| map.get("level"))
            .or_else(|| map.get("value"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        _ => None,
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
        if let Some(effort) = thinking.get("effort").and_then(extract_effort_str) {
            config.reasoning_effort = Some(effort);
            found = true;
        }
    }
    if let Some(effort) = request
        .pointer("/output_config/effort")
        .or_else(|| request.get("effort"))
        .and_then(extract_effort_str)
    {
        config.reasoning_effort = Some(effort);
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
        if let Some(system) = messages
            .iter_mut()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        {
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
        messages.insert(
            0,
            json!({ "role": "system", "content": PREFIX_THOUGHT_HINT }),
        );
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
    for message in request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
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
                if result.get("enable_thinking").is_some()
                    || cfg.mode.as_deref() == Some("enable_thinking")
                {
                    result["enable_thinking"] = json!(enable_thinking);
                }
            }
            if cfg.prefix_thought.unwrap_or(false) {
                apply_prefix_thought(&mut result);
            }
        }
    }

    // DeepSeek (and explicit enable_thinking=true chat models) reject object-form
    // tool_choice ({"type":"function","function":{"name":"..."}}) in thinking mode
    // with HTTP 400 ("Thinking mode does not support this tool_choice"), while
    // accepting "required".
    let thinking_explicitly_off = result.get("enable_thinking") == Some(&Value::Bool(false))
        || request
            .pointer("/thinking/type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case("disabled"));
    if result.get("tool_choice").is_some_and(Value::is_object)
        && !thinking_explicitly_off
        && (model.to_ascii_lowercase().contains("deepseek")
            || result.get("enable_thinking") == Some(&Value::Bool(true))
            || result.get("reasoning_effort").is_some())
    {
        result["tool_choice"] = Value::String("required".to_string());
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
        result["tools"] = Value::Array(
            tools
                .iter()
                .filter_map(anthropic_tool_to_responses)
                .collect(),
        );
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = anthropic_tool_choice_to_responses(choice);
    }

    if let Some(cfg) = merge_thinking_config(request, provider_thinking) {
        if !cfg.is_disabled() {
            if let Some(effort) = cfg.resolved_responses_reasoning_effort() {
                result["reasoning"] = json!({
                    "effort": effort,
                    "summary": "auto"
                });
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
    let Some(items) = include.as_array_mut() else {
        return;
    };
    let encrypted_content = Value::String("reasoning.encrypted_content".to_string());
    if !items.contains(&encrypted_content) {
        items.push(encrypted_content);
    }
}

/// Strip non-standard fields from Codex Responses request bodies to avoid upstream 400 errors,
/// and repair unpaired `function_call` / `function_call_output` items in `input`.
pub fn sanitize_codex_responses_body(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    object.remove("prompt_cache_retention");
    object.remove("prompt_cache_ttl");
    object.remove("prompt_cache_key");
    object.remove("wire_api");
    object.remove("auto_review");

    let has_prev_resp = object
        .get("previous_response_id")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    if has_prev_resp {
        return;
    }
    let input_empty = match object.get("input") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        Some(Value::Array(arr)) => arr.is_empty(),
        _ => false,
    };
    if input_empty {
        let instr = object
            .get("instructions")
            .or_else(|| object.get("system"))
            .and_then(|v| match v {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Array(items) => {
                    let joined = items
                        .iter()
                        .filter_map(|item| {
                            item.as_str().or_else(|| item.get("text").and_then(Value::as_str))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(joined.trim().to_string())
                }
                _ => None,
            })
            .filter(|s| !s.is_empty());
        if let Some(instr) = instr {
            object.insert(
                "input".to_string(),
                json!([{ "role": "user", "content": instr }]),
            );
        }
    }
    let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    let mut all_output_ids = std::collections::HashSet::new();
    for item in items.iter() {
        if item.get("type").and_then(Value::as_str) == Some("function_call_output") {
            if let Some(cid) = item
                .get("call_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                all_output_ids.insert(cid.to_string());
            }
        }
    }
    let mut seen_call_ids = std::collections::HashSet::new();
    let mut pending_missing_outputs: Vec<String> = Vec::new();
    let mut rebuilt = Vec::with_capacity(items.len() + 2);
    for item in items.drain(..) {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if item_type != "function_call" && !pending_missing_outputs.is_empty() {
            for cid in pending_missing_outputs.drain(..) {
                rebuilt.push(json!({
                    "type": "function_call_output",
                    "call_id": cid,
                    "output": "[Tool execution was cancelled or skipped before returning a result]"
                }));
            }
        }
        match item_type {
            "function_call" => {
                if let Some(cid) = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    seen_call_ids.insert(cid.to_string());
                    if !all_output_ids.contains(cid) {
                        pending_missing_outputs.push(cid.to_string());
                    }
                }
                rebuilt.push(item);
            }
            "function_call_output" => {
                if let Some(cid) = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    if !seen_call_ids.contains(cid) {
                        seen_call_ids.insert(cid.to_string());
                        rebuilt.push(json!({
                            "type": "function_call",
                            "call_id": cid,
                            "name": "tool",
                            "arguments": "{}"
                        }));
                    }
                }
                rebuilt.push(item);
            }
            _ => rebuilt.push(item),
        }
    }
    *items = rebuilt;
}

/// Ensure OpenAI Chat Completions `messages` invariants:
/// 1. Every historical `assistant` message with `tool_calls` (followed by another message or partial
///    `tool` results) is immediately followed by matching `role: "tool"` messages for each `tool_call.id`.
/// 2. Orphan `role: "tool"` messages without a preceding matching `assistant` `tool_calls` are
///    converted to `role: "user"` text messages so upstream OpenAI/DeepSeek/Claude gateways do not 400.
pub fn sanitize_openai_chat_messages(messages: &mut Vec<Value>) {
    let mut rebuilt: Vec<Value> = Vec::with_capacity(messages.len() + 2);
    let mut i = 0;
    while i < messages.len() {
        let role = messages[i]
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "assistant" {
            let expected_ids: Vec<String> = messages[i]
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|c| {
                            c.get("id")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            rebuilt.push(messages[i].clone());
            i += 1;
            if !expected_ids.is_empty() {
                let mut tool_by_id = std::collections::HashMap::new();
                while i < messages.len()
                    && messages[i].get("role").and_then(Value::as_str) == Some("tool")
                {
                    if let Some(tid) = messages[i]
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        tool_by_id
                            .entry(tid.to_string())
                            .or_insert_with(|| messages[i].clone());
                    }
                    i += 1;
                }
                if i < messages.len() || !tool_by_id.is_empty() {
                    for expected_id in expected_ids {
                        if let Some(msg) = tool_by_id.remove(&expected_id) {
                            rebuilt.push(msg);
                        } else {
                            rebuilt.push(json!({
                                "role": "tool",
                                "tool_call_id": expected_id,
                                "content": "[Tool execution was cancelled or skipped before returning a result]"
                            }));
                        }
                    }
                }
            }
        } else if role == "tool" {
            let content = messages[i]
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !content.is_empty() {
                rebuilt.push(json!({
                    "role": "user",
                    "content": content
                }));
            }
            i += 1;
        } else {
            rebuilt.push(messages[i].clone());
            i += 1;
        }
    }
    *messages = rebuilt;
}

/// Model id written back to the client. A non-empty client id always wins so
/// Claude Code keeps the catalog id it requested instead of an upstream slug.
pub(crate) fn client_facing_model<'a>(
    client_model: &'a str,
    upstream_model: Option<&'a str>,
) -> &'a str {
    let client = client_model.trim();
    if !client.is_empty() {
        return client;
    }
    upstream_model.unwrap_or("").trim()
}

pub fn openai_chat_to_anthropic(value: &Value, fallback_model: &str) -> Value {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let message = choice
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut content = Vec::new();
    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        content.push(json!({"type": "thinking", "thinking": reasoning}));
    }
    if let Some(text) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        let normalized = crate::antigravity::map::latex::replace_section_signs(text);
        content.push(json!({"type": "text", "text": normalized}));
    }
    for block in message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") | Some("output_text") => {
                if let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    let normalized = crate::antigravity::map::latex::replace_section_signs(text);
                    content.push(json!({"type": "text", "text": normalized}));
                }
            }
            // OpenAI refusal content has no dedicated Anthropic content block;
            // expose the provider's user-facing refusal as normal assistant text.
            Some("refusal") => {
                if let Some(text) = block
                    .get("refusal")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    let normalized = crate::antigravity::map::latex::replace_section_signs(text);
                    content.push(json!({"type": "text", "text": normalized}));
                }
            }
            // Reasoning/thinking fields are intentionally not replayed because
            // Anthropic requires a provider-specific signature for later turns.
            Some("reasoning") | Some("thinking") => {}
            _ => {}
        }
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = call.get("function").cloned().unwrap_or_else(|| json!({}));
        let input = function
            .get("arguments")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        content.push(json!({
            "type": "tool_use",
            "id": call.get("id").and_then(Value::as_str).unwrap_or("tool_call"),
            "name": function.get("name").and_then(Value::as_str).unwrap_or("tool"),
            "input": input,
        }));
    }
    let finish = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
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
        "model": client_facing_model(fallback_model, value.get("model").and_then(Value::as_str)),
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
    for item in value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            let mut reasoning_parts = Vec::new();
            for summary_block in item
                .get("summary")
                .or_else(|| item.get("content"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(t) = summary_block.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        reasoning_parts.push(t);
                    }
                }
            }
            if !reasoning_parts.is_empty() {
                content.push(json!({
                    "type": "thinking",
                    "thinking": reasoning_parts.join("\n"),
                }));
            }
            continue;
        }
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            let input = item
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            content.push(json!({
                "type": "tool_use",
                "id": item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),
                "name": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
                "input": input,
            }));
            continue;
        }
        for block in item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if matches!(
                block.get("type").and_then(Value::as_str),
                Some("output_text" | "text")
            ) {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    let normalized = crate::antigravity::map::latex::replace_section_signs(text);
                    content.push(json!({"type": "text", "text": normalized}));
                }
            } else if block.get("type").and_then(Value::as_str) == Some("refusal") {
                if let Some(text) = block
                    .get("refusal")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                {
                    let normalized = crate::antigravity::map::latex::replace_section_signs(text);
                    content.push(json!({"type": "text", "text": normalized}));
                }
            } else if let Some(text) = block.get("text").and_then(Value::as_str) {
                let normalized = crate::antigravity::map::latex::replace_section_signs(text);
                content.push(json!({"type": "text", "text": normalized}));
            }
        }
    }
    let has_tool = content
        .iter()
        .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
    let stop_reason = if has_tool {
        "tool_use"
    } else if value.get("status").and_then(Value::as_str) == Some("incomplete")
        && value
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
            == Some("max_output_tokens")
    {
        "max_tokens"
    } else {
        "end_turn"
    };
    json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type": "message",
        "role": "assistant",
        "model": client_facing_model(fallback_model, value.get("model").and_then(Value::as_str)),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    text_carry: BTreeMap<(usize, usize), String>,
    tool_blocks: BTreeMap<usize, usize>,
    /// Accumulated tool-call argument text already emitted as `input_json_delta`.
    tool_arguments_sent: BTreeMap<usize, String>,
    open_blocks: Vec<usize>,
    thinking_block: Option<usize>,
    thinking_has_delta: bool,
    /// True after a user-visible text delta. Midstream close adds an error
    /// text block only when this is still false.
    emitted_body: bool,
    /// Set when `error_event` closed the message. Callers stop the upstream.
    terminal_error: bool,
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
            text_carry: BTreeMap::new(),
            tool_blocks: BTreeMap::new(),
            tool_arguments_sent: BTreeMap::new(),
            open_blocks: Vec::new(),
            thinking_block: None,
            thinking_has_delta: false,
            emitted_body: false,
            terminal_error: false,
            input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            stop_reason: "end_turn".to_string(),
        }
    }

    /// Emit `message_start` and an initial `ping` immediately when an HTTP 200
    /// SSE stream begins so Claude Code / Claude Desktop never stalls with
    /// `"This is taking a little longer than usual"` while a reasoning model
    /// (`gpt-6.1-sol`, etc.) is thinking before its first output token.
    pub fn initial_events(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut output = String::new();
        self.ensure_started(&mut output);
        output.into_bytes()
    }

    pub fn push_event(&mut self, event: &Value) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        if self.protocol == OpenAiStreamProtocol::Chat {
            if let Some(message) = chat_upstream_error_message(event) {
                return self.error_event(&message);
            }
        }
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

    /// Close a broken stream as a normal assistant message.
    ///
    /// Claude Code stays on the current turn when it sees `event: error` or an
    /// empty `message_stop`. Open blocks are stopped, a text block carries the
    /// error when nothing visible was streamed, and `stop_reason` is `end_turn`.
    pub fn error_event(&mut self, message: &str) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        self.terminal_error = true;
        let mut output = String::new();
        self.ensure_started(&mut output);
        self.flush_all_text_carries(&mut output);
        self.thinking_block = None;
        for index in self.open_blocks.drain(..) {
            push_event(
                &mut output,
                "content_block_stop",
                json!({"type":"content_block_stop", "index":index}),
            );
        }
        if !self.emitted_body {
            push_error_text_block(&mut output, self.next_block_index, message);
            self.next_block_index += 1;
        }
        self.stop_reason = "end_turn".to_string();
        push_event(
            &mut output,
            "message_delta",
            json!({
                "type":"message_delta",
                "delta":{"stop_reason":"end_turn","stop_sequence":Value::Null},
                "usage":{
                    "input_tokens":self.input_tokens,
                    "cache_read_input_tokens":self.cache_read_input_tokens,
                    "cache_creation_input_tokens":self.cache_creation_input_tokens,
                    "output_tokens":self.output_tokens
                }
            }),
        );
        push_event(&mut output, "message_stop", json!({"type":"message_stop"}));
        self.completed = true;
        output.into_bytes()
    }

    pub fn took_terminal_error(&self) -> bool {
        self.terminal_error
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
        let Some(choice) = event
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
        else {
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
            self.thinking_has_delta = true;
            push_event(
                output,
                "content_block_delta",
                json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"thinking_delta", "thinking":reasoning}
                }),
            );
        }
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            self.emitted_body = true;
            self.close_thinking_block(output);
            self.emit_normalized_text_delta((0, 0), text, output);
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.close_thinking_block(output);
            self.flush_all_text_carries(output);
            let upstream_index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                call.get("id").and_then(Value::as_str),
                call.pointer("/function/name").and_then(Value::as_str),
                output,
            );
            if let Some(arguments) = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                push_event(
                    output,
                    "content_block_delta",
                    json!({
                        "type":"content_block_delta", "index":index,
                        "delta":{"type":"input_json_delta", "partial_json":arguments}
                    }),
                );
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.stop_reason = match reason {
                "tool_calls" | "function_call" => "tool_use",
                "length" => "max_tokens",
                _ => "end_turn",
            }
            .to_string();
        }
    }

    fn push_responses_event(&mut self, event: &Value, output: &mut String) {
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        if event_type == "error" || event_type == "response.failed" {
            let message = event
                .pointer("/error/message")
                .or_else(|| event.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("上游服务返回流式错误");
            output.push_str(&String::from_utf8_lossy(&self.error_event(message)));
            return;
        }
        if matches!(event_type, "response.created" | "response.in_progress") {
            self.ensure_started(output);
        } else if event_type == "response.output_item.added" {
            let item = event.get("item").unwrap_or(&Value::Null);
            let item_type = item.get("type").and_then(Value::as_str);
            if item_type == Some("reasoning") {
                self.ensure_thinking_block(output);
            } else if item_type == Some("function_call") {
                self.close_thinking_block(output);
                self.flush_all_text_carries(output);
                self.stop_reason = "tool_use".to_string();
                let upstream_index = event
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                self.ensure_tool_block(
                    upstream_index,
                    item.get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .or_else(|| item.get("id").and_then(Value::as_str).filter(|s| !s.is_empty())),
                    item.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()),
                    output,
                );
            }
        } else if event_type == "response.reasoning_summary_part.added" {
            self.ensure_thinking_block(output);
        } else if matches!(
            event_type,
            "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
                | "response.reasoning.delta"
        ) {
            if let Some(delta) = event
                .get("delta")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let index = self.ensure_thinking_block(output);
                self.thinking_has_delta = true;
                push_event(
                    output,
                    "content_block_delta",
                    json!({
                        "type":"content_block_delta", "index":index,
                        "delta":{"type":"thinking_delta", "thinking":delta}
                    }),
                );
            }
        } else if event_type == "response.content_part.added" {
            let part = event.get("part").unwrap_or(&Value::Null);
            if matches!(
                part.get("type").and_then(Value::as_str),
                Some("output_text" | "text")
            ) {
                self.close_thinking_block(output);
                let key = (
                    event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize,
                    event
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize,
                );
                self.ensure_text_block(key, output);
            }
        } else if event_type == "response.output_text.delta" {
            self.close_thinking_block(output);
            let key = (
                event
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
                event
                    .get("content_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
            );
            if let Some(delta) = event
                .get("delta")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                self.emitted_body = true;
                self.emit_normalized_text_delta(key, delta, output);
            } else {
                self.ensure_text_block(key, output);
            }
        } else if event_type == "response.function_call_arguments.delta" {
            self.close_thinking_block(output);
            self.flush_all_text_carries(output);
            let upstream_index = event
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                event.get("call_id").and_then(Value::as_str).filter(|s| !s.is_empty()),
                event.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()),
                output,
            );
            if let Some(delta) = event
                .get("delta")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                self.tool_arguments_sent
                    .entry(upstream_index)
                    .or_default()
                    .push_str(delta);
                push_event(
                    output,
                    "content_block_delta",
                    json!({
                        "type":"content_block_delta", "index":index,
                        "delta":{"type":"input_json_delta", "partial_json":delta}
                    }),
                );
            }
        } else if event_type == "response.function_call_arguments.done"
            || (event_type == "response.output_item.done"
                && event.pointer("/item/type").and_then(Value::as_str) == Some("function_call"))
        {
            let source = if event_type == "response.output_item.done" {
                event.get("item").unwrap_or(event)
            } else {
                event
            };
            self.stop_reason = "tool_use".to_string();
            let upstream_index = event
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let index = self.ensure_tool_block(
                upstream_index,
                source
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .or_else(|| source.get("id").and_then(Value::as_str).filter(|s| !s.is_empty())),
                source.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()),
                output,
            );
            let full = source.get("arguments").and_then(Value::as_str).unwrap_or("");
            let sent = self
                .tool_arguments_sent
                .get(&upstream_index)
                .map(String::as_str)
                .unwrap_or("");
            let suffix = if let Some(stripped) = full.strip_prefix(sent) {
                stripped
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
                push_event(
                    output,
                    "content_block_delta",
                    json!({
                        "type":"content_block_delta", "index":index,
                        "delta":{"type":"input_json_delta", "partial_json":suffix}
                    }),
                );
            }
        }
        if matches!(event_type, "response.completed" | "response.incomplete") {
            if event_type == "response.incomplete"
                || event.pointer("/response/status").and_then(Value::as_str) == Some("incomplete")
            {
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
        // Keep the id the client requested. Copying the upstream slug
        // (`gpt-5.6-sol`) makes Claude Code drop the catalog id and fall
        // back to `claude.auto` on the next resume.
        if self.fallback_model.trim().is_empty() {
            if let Some(model) = response.get("model").and_then(Value::as_str) {
                let model = model.trim();
                if !model.is_empty() {
                    self.model = model.to_string();
                }
            }
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
            self.output_tokens = usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(self.output_tokens);
        }
    }

    fn ensure_started(&mut self, output: &mut String) {
        if self.started {
            return;
        }
        push_event(
            output,
            "message_start",
            json!({"type":"message_start","message":{
                "id": self.message_id.as_str(), "type":"message", "role":"assistant", "content":[],
                "model": if self.model.is_empty() { &self.fallback_model } else { &self.model },
                "stop_reason":Value::Null, "stop_sequence":Value::Null,
                "usage":{
                    "input_tokens":self.input_tokens,
                    "cache_read_input_tokens":self.cache_read_input_tokens,
                    "cache_creation_input_tokens":self.cache_creation_input_tokens,
                    "output_tokens":0
                }
            }}),
        );
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
        self.thinking_has_delta = false;
        self.open_blocks.push(index);
        push_event(
            output,
            "content_block_start",
            json!({
                "type":"content_block_start", "index":index,
                "content_block":{"type":"thinking", "thinking":""}
            }),
        );
        index
    }

    fn close_thinking_block(&mut self, output: &mut String) {
        if let Some(t_idx) = self.thinking_block.take() {
            if !self.thinking_has_delta {
                push_event(
                    output,
                    "content_block_delta",
                    json!({
                        "type":"content_block_delta", "index":t_idx,
                        "delta":{"type":"thinking_delta", "thinking":" "}
                    }),
                );
                self.thinking_has_delta = true;
            }
            if let Some(pos) = self.open_blocks.iter().position(|&x| x == t_idx) {
                self.open_blocks.remove(pos);
            }
            push_event(
                output,
                "content_block_stop",
                json!({
                    "type":"content_block_stop", "index":t_idx
                }),
            );
        }
    }

    fn emit_normalized_text_delta(
        &mut self,
        key: (usize, usize),
        delta: &str,
        output: &mut String,
    ) {
        let index = self.ensure_text_block(key, output);
        let carry = self.text_carry.entry(key).or_default();
        carry.push_str(delta);
        let (ready, rest) = crate::antigravity::map::latex::split_safe_section_sign_prefix(carry);
        *carry = rest;
        if !ready.is_empty() {
            let normalized = crate::antigravity::map::latex::replace_section_signs(&ready);
            push_event(
                output,
                "content_block_delta",
                json!({
                    "type":"content_block_delta", "index":index,
                    "delta":{"type":"text_delta", "text":normalized}
                }),
            );
        }
    }

    fn flush_all_text_carries(&mut self, output: &mut String) {
        let keys: Vec<(usize, usize)> = self.text_carry.keys().copied().collect();
        for key in keys {
            if let Some(carry) = self.text_carry.remove(&key) {
                if !carry.is_empty() {
                    let index = self.ensure_text_block(key, output);
                    let normalized = crate::antigravity::map::latex::replace_section_signs(&carry);
                    push_event(
                        output,
                        "content_block_delta",
                        json!({
                            "type":"content_block_delta", "index":index,
                            "delta":{"type":"text_delta", "text":normalized}
                        }),
                    );
                }
            }
        }
    }

    fn ensure_text_block(&mut self, key: (usize, usize), output: &mut String) -> usize {
        if let Some(index) = self.text_blocks.get(&key) {
            return *index;
        }
        self.ensure_started(output);
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.text_blocks.insert(key, index);
        self.open_blocks.push(index);
        push_event(
            output,
            "content_block_start",
            json!({
                "type":"content_block_start", "index":index,
                "content_block":{"type":"text", "text":""}
            }),
        );
        index
    }

    fn ensure_tool_block(
        &mut self,
        upstream_index: usize,
        id: Option<&str>,
        name: Option<&str>,
        output: &mut String,
    ) -> usize {
        if let Some(index) = self.tool_blocks.get(&upstream_index) {
            return *index;
        }
        self.ensure_started(output);
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.tool_blocks.insert(upstream_index, index);
        self.open_blocks.push(index);
        push_event(
            output,
            "content_block_start",
            json!({
                "type":"content_block_start", "index":index,
                "content_block":{
                    "type":"tool_use", "id":id.unwrap_or("tool_call"),
                    "name":name.unwrap_or("tool"), "input":{}
                }
            }),
        );
        index
    }

    fn finish(&mut self, output: &mut String) {
        if self.completed {
            return;
        }
        self.ensure_started(output);
        self.flush_all_text_carries(output);
        self.close_thinking_block(output);
        for index in self.open_blocks.drain(..) {
            push_event(
                output,
                "content_block_stop",
                json!({"type":"content_block_stop", "index":index}),
            );
        }
        push_event(
            output,
            "message_delta",
            json!({
                "type":"message_delta", "delta":{"stop_reason":self.stop_reason.as_str(),"stop_sequence":Value::Null},
                "usage":{
                    "input_tokens":self.input_tokens,
                    "cache_read_input_tokens":self.cache_read_input_tokens,
                    "cache_creation_input_tokens":self.cache_creation_input_tokens,
                    "output_tokens":self.output_tokens
                }
            }),
        );
        push_event(output, "message_stop", json!({"type":"message_stop"}));
        self.completed = true;
    }
}

/// Abort an Anthropic Messages SSE that was forwarded without a converter.
///
/// `model` is the catalog id the client sent. The result is a normal assistant
/// message (`message_stop`, no `event: error`) so Claude Code can leave the turn.
pub fn anthropic_sse_abort(
    message: &str,
    model: &str,
    saw_message_start: bool,
    open_blocks: &[usize],
    next_block_index: usize,
    emitted_body: bool,
) -> Vec<u8> {
    let mut output = String::new();
    let model = if model.trim().is_empty() {
        "claude"
    } else {
        model.trim()
    };
    if !saw_message_start {
        push_event(
            &mut output,
            "message_start",
            json!({
                "type":"message_start",
                "message":{
                    "id":"msg_proxy",
                    "type":"message",
                    "role":"assistant",
                    "content":[],
                    "model": model,
                    "stop_reason":Value::Null,
                    "stop_sequence":Value::Null,
                    "usage":{"input_tokens":0,"output_tokens":0}
                }
            }),
        );
    }
    for index in open_blocks {
        push_event(
            &mut output,
            "content_block_stop",
            json!({"type":"content_block_stop", "index": index}),
        );
    }
    if !emitted_body {
        let index = open_blocks
            .iter()
            .copied()
            .max()
            .map(|open| open.saturating_add(1))
            .unwrap_or(0)
            .max(next_block_index);
        push_error_text_block(&mut output, index, message);
    }
    push_event(
        &mut output,
        "message_delta",
        json!({
            "type":"message_delta",
            "delta":{"stop_reason":"end_turn","stop_sequence":Value::Null},
            "usage":{"output_tokens":0}
        }),
    );
    push_event(&mut output, "message_stop", json!({"type":"message_stop"}));
    output.into_bytes()
}

fn push_error_text_block(output: &mut String, index: usize, message: &str) {
    push_event(
        output,
        "content_block_start",
        json!({
            "type":"content_block_start",
            "index": index,
            "content_block":{"type":"text", "text":""}
        }),
    );
    push_event(
        output,
        "content_block_delta",
        json!({
            "type":"content_block_delta",
            "index": index,
            "delta":{"type":"text_delta", "text": message}
        }),
    );
    push_event(
        output,
        "content_block_stop",
        json!({"type":"content_block_stop", "index": index}),
    );
}

fn chat_upstream_error_message(event: &Value) -> Option<String> {
    if event
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        return None;
    }
    let error = event.get("error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("上游服务返回流式错误");
    if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    }
}

fn anthropic_messages_to_responses_input(request: &Value) -> Vec<Value> {
    let mut output = Vec::new();
    for message in request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let is_assistant = role == "assistant";
        // Responses API: user/system content uses input_*; assistant replay uses
        // output_text. Using input_text on assistant turns is rejected by many
        // OpenAI-compatible gateways (opaque upstream 502 on Claude Code turn 2+).
        let text_type = if is_assistant {
            "output_text"
        } else {
            "input_text"
        };
        let mut message_content = Vec::new();
        let mut tool_items = Vec::new();
        match message.get("content") {
            Some(Value::String(text)) if !text.is_empty() => {
                message_content.push(json!({"type": text_type, "text": text}));
            }
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                            message_content.push(json!({"type": text_type, "text": text}));
                        }
                    }
                    Some("document") => {
                        if let Some(text) = extract_anthropic_document_text(block) {
                            message_content.push(json!({"type": text_type, "text": text}));
                        }
                    }
                    Some("web_search_tool_result") => {
                        if let Some(text) = extract_anthropic_web_search_result_text(block) {
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
                    Some("tool_result") | Some("tool_search_tool_result") => {
                        let raw_content = block.get("content").unwrap_or(&Value::Null);
                        let mut out_text = content_text(raw_content);
                        if !is_assistant {
                            for item in raw_content.as_array().into_iter().flatten() {
                                if item.get("type").and_then(Value::as_str) == Some("image") {
                                    if let Some(url) = image_url(item) {
                                        if out_text.is_empty() {
                                            out_text = "[Image attached]".to_string();
                                        }
                                        message_content.push(json!({"type": "input_image", "image_url": url}));
                                    }
                                }
                            }
                        }
                        tool_items.push(json!({
                            "type": "function_call_output",
                            "call_id": block.get("tool_use_id").or_else(|| block.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),
                            "output": out_text,
                        }));
                    }
                    _ => {}
                }
                }
            }
            _ => {}
        }
        if !is_assistant {
            output.extend(std::mem::take(&mut tool_items));
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
    push_event(
        &mut output,
        "message_start",
        json!({"type":"message_start","message":{
            "id": message.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
            "type":"message", "role":"assistant", "content":[],
            "model": message.get("model").and_then(Value::as_str).unwrap_or(""),
            "stop_reason":Value::Null, "stop_sequence":Value::Null,
            "usage":{"input_tokens":message.pointer("/usage/input_tokens").and_then(Value::as_i64).unwrap_or(0),"output_tokens":0}
        }}),
    );
    for (index, block) in message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        push_event(
            &mut output,
            "content_block_start",
            json!({"type":"content_block_start","index":index,"content_block":block}),
        );
        match block.get("type").and_then(Value::as_str) {
            Some("text") => push_event(
                &mut output,
                "content_block_delta",
                json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":block.get("text").and_then(Value::as_str).unwrap_or("")}}),
            ),
            Some("tool_use") => push_event(
                &mut output,
                "content_block_delta",
                json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string())}}),
            ),
            _ => {}
        }
        push_event(
            &mut output,
            "content_block_stop",
            json!({"type":"content_block_stop","index":index}),
        );
    }
    push_event(
        &mut output,
        "message_delta",
        json!({"type":"message_delta","delta":{"stop_reason":message.get("stop_reason").cloned().unwrap_or(Value::Null),"stop_sequence":Value::Null},"usage":{"output_tokens":message.pointer("/usage/output_tokens").and_then(Value::as_i64).unwrap_or(0)}}),
    );
    push_event(&mut output, "message_stop", json!({"type":"message_stop"}));
    output.into_bytes()
}

fn append_openai_message(message: &Value, output: &mut Vec<Value>) {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let mut text_blocks = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) => text_blocks.push(json!({"type":"text","text":text})),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                Some("text") => text_blocks.push(json!({"type":"text","text":block.get("text").and_then(Value::as_str).unwrap_or("")})),
                Some("document") => if let Some(text) = extract_anthropic_document_text(block) { text_blocks.push(json!({"type":"text","text":text})); },
                Some("web_search_tool_result") => if let Some(text) = extract_anthropic_web_search_result_text(block) { text_blocks.push(json!({"type":"text","text":text})); },
                // Anthropic thinking requires a signature that cannot be
                // synthesized for OpenAI-compatible upstreams.
                Some("thinking") => {},
                Some("image") => if let Some(url) = image_url(block) { text_blocks.push(json!({"type":"image_url","image_url":{"url":url}})); },
                Some("tool_use") => tool_calls.push(json!({"id":block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),"type":"function","function":{"name":block.get("name").and_then(Value::as_str).unwrap_or("tool"),"arguments":serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string())}})),
                Some("tool_result") | Some("tool_search_tool_result") => {
                    let raw_content = block.get("content").unwrap_or(&Value::Null);
                    let mut out_text = content_text(raw_content);
                    for item in raw_content.as_array().into_iter().flatten() {
                        if item.get("type").and_then(Value::as_str) == Some("image") {
                            if let Some(url) = image_url(item) {
                                if out_text.is_empty() {
                                    out_text = "[Image attached]".to_string();
                                }
                                text_blocks.push(json!({"type":"image_url","image_url":{"url":url}}));
                            }
                        }
                    }
                    tool_results.push(json!({"role":"tool","tool_call_id":block.get("tool_use_id").or_else(|| block.get("id")).and_then(Value::as_str).unwrap_or("tool_call"),"content":out_text}));
                }
                _ => {}
            }
            }
        }
        _ => {}
    }
    if role != "assistant" {
        output.append(&mut tool_results);
    }
    if !text_blocks.is_empty() || !tool_calls.is_empty() {
        let mut result = Map::new();
        result.insert(
            "role".to_string(),
            Value::String(
                if role == "assistant" {
                    "assistant"
                } else {
                    "user"
                }
                .to_string(),
            ),
        );
        if text_blocks.len() == 1
            && text_blocks[0].get("type").and_then(Value::as_str) == Some("text")
        {
            result.insert(
                "content".to_string(),
                text_blocks[0]
                    .get("text")
                    .cloned()
                    .unwrap_or(Value::String(String::new())),
            );
        } else {
            result.insert("content".to_string(), Value::Array(text_blocks));
        }
        if !tool_calls.is_empty() {
            result.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        output.push(Value::Object(result));
    }
    output.extend(tool_results);
}

/// Extract readable text from an Anthropic `{"type": "document"}` block
/// (`source.type == "text" | "content" | "base64"` with text-like media_type).
pub fn extract_anthropic_document_text(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    let source_type = source.get("type").and_then(Value::as_str).unwrap_or("");
    let raw_text = match source_type {
        "text" => source
            .get("data")
            .or_else(|| source.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
        "content" => {
            let text = content_text(source.get("content").unwrap_or(&Value::Null));
            (!text.is_empty()).then_some(text)
        }
        "base64" => {
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_text_mime = media_type.starts_with("text/")
                || matches!(
                    media_type.as_str(),
                    "application/json"
                        | "application/xml"
                        | "application/javascript"
                        | "application/x-yaml"
                        | "application/csv"
                );
            if is_text_mime {
                use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
                use base64::Engine;
                let data = source.get("data").and_then(Value::as_str).unwrap_or("").trim();
                STANDARD
                    .decode(data)
                    .or_else(|_| STANDARD_NO_PAD.decode(data))
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            } else {
                None
            }
        }
        _ => None,
    }?;
    if raw_text.trim().is_empty() {
        return None;
    }
    if let Some(title) = block
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(format!("[Document: {title}]\n{raw_text}"))
    } else {
        Some(raw_text)
    }
}

/// Extract readable text from an Anthropic `{"type": "web_search_tool_result"}` block.
pub fn extract_anthropic_web_search_result_text(block: &Value) -> Option<String> {
    let content = block.get("content")?;
    if let Some(text) = content.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(format!("[Web Search Results]\n{text}"));
    }
    let items = content.as_array()?;
    let mut lines = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("web_search_result") => {
                let title = item.get("title").and_then(Value::as_str).unwrap_or("").trim();
                let url = item.get("url").and_then(Value::as_str).unwrap_or("").trim();
                let snippet = item
                    .get("snippet")
                    .or_else(|| item.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !title.is_empty() || !url.is_empty() {
                    if snippet.is_empty() {
                        lines.push(format!("- {title}: {url}"));
                    } else {
                        lines.push(format!("- {title} ({url}): {snippet}"));
                    }
                }
            }
            Some("text") => {
                if let Some(t) = item.get("text").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
                    lines.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(format!("[Web Search Results]\n{}", lines.join("\n")))
    }
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

fn extract_tool_name(tool: &Value) -> Option<&str> {
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
        Some("tool") => {
            json!({"type":"function","function":{"name":value.get("name").and_then(Value::as_str).unwrap_or("")}})
        }
        Some("any") => Value::String("required".to_string()),
        Some("none") => Value::String("none".to_string()),
        _ => Value::String("auto".to_string()),
    }
}

fn anthropic_tool_choice_to_responses(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("tool") => {
            json!({"type": "function", "name": value.get("name").and_then(Value::as_str).unwrap_or("")})
        }
        Some("any") => Value::String("required".to_string()),
        Some("none") => Value::String("none".to_string()),
        _ => Value::String("auto".to_string()),
    }
}

fn image_url(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    if source.get("type").and_then(Value::as_str) == Some("base64") {
        Some(format!(
            "data:{};base64,{}",
            source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png"),
            source.get("data").and_then(Value::as_str).unwrap_or("")
        ))
    } else {
        source
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    Some(text.to_string())
                } else if item.get("type").and_then(Value::as_str) == Some("document") {
                    extract_anthropic_document_text(item)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => {
            if value.get("type").and_then(Value::as_str) == Some("document") {
                extract_anthropic_document_text(value).unwrap_or_default()
            } else {
                value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            }
        }
        _ => String::new(),
    }
}

fn copy_if_present(from: &Value, to: &mut Value, key: &str) {
    if let Some(value) = from.get(key) {
        to[key] = value.clone();
    }
}

fn push_event(output: &mut String, event: &str, data: Value) {
    output.push_str("event: ");
    output.push_str(event);
    output.push('\n');
    output.push_str("data: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string()));
    output.push_str("\n\n");
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
        assert_eq!(
            output["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
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
        assert!(message["error"]["message"]
            .as_str()
            .unwrap()
            .contains("overloaded"));
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
        assert!(output["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| { item.get("role").and_then(Value::as_str) != Some("system") }));
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
        assert_eq!(
            openai_responses_to_anthropic(&upstream, "fallback")["stop_reason"],
            "max_tokens"
        );
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
        })))
        .unwrap();
        let done = String::from_utf8(converter.push_event(&json!({
            "type":"response.function_call_arguments.done", "output_index":0,
            "call_id":"call_1", "arguments":"{\"q\":\"x\"}"
        })))
        .unwrap();
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
        })))
        .unwrap();
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
        })))
        .unwrap();
        let start = event
            .lines()
            .find(|line| line.starts_with("data: ") && line.contains("content_block_start"))
            .unwrap();
        assert!(start.contains("\"text\":\"\""));
        assert!(!start.contains("hello"));
        assert!(event.contains("\"text\":\"hello\""));
    }

    #[test]
    fn stream_errors_are_anthropic_error_events() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "gpt-5.6-sol");
        let output = String::from_utf8(converter.error_event("上游流式响应中断")).unwrap();
        assert!(converter.took_terminal_error());
        assert!(output.contains("event: message_stop"));
        assert!(output.contains("上游流式响应中断"));
        assert!(output.contains("gpt-5.6-sol"));
        assert!(output.contains("\"stop_reason\":\"end_turn\""));
        assert!(!output.contains("event: error"));
        assert!(!output.contains("Bearer"));
        let text = output.find("上游流式响应中断").unwrap();
        let stop = output.find("event: message_stop").unwrap();
        assert!(text < stop);
    }

    #[test]
    fn chat_error_frame_finishes_with_catalog_model_text() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "gpt-5.6-sol");
        let output = String::from_utf8(converter.push_event(&json!({
            "error": {"message": "model not found", "type": "invalid_request_error"}
        })))
        .unwrap();
        assert!(converter.took_terminal_error());
        assert!(output.contains("gpt-5.6-sol"));
        assert!(output.contains("model not found"));
        assert!(output.contains("event: message_stop"));
        assert!(!output.contains("event: error"));
        let text = output.find("model not found").unwrap();
        let stop = output.find("event: message_stop").unwrap();
        assert!(text < stop);
    }

    #[test]
    fn error_event_closes_open_thinking_block_before_message_stop() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "fallback");
        let _ = converter.push_event(&json!({
            "choices":[{"delta":{"reasoning_content":"let me think"},"finish_reason":null}]
        }));
        let output = String::from_utf8(converter.error_event("上游流式响应中断")).unwrap();
        assert!(output.contains("event: content_block_stop"));
        assert!(output.contains("event: message_delta"));
        assert!(output.contains("event: message_stop"));
        assert!(output.contains("上游流式响应中断"));
        assert!(!output.contains("event: error"));
        let block_stop = output.find("event: content_block_stop").unwrap();
        let text = output.find("上游流式响应中断").unwrap();
        let stop = output.find("event: message_stop").unwrap();
        assert!(block_stop < text);
        assert!(text < stop);
    }

    #[test]
    fn error_event_then_finish_stream_does_not_repeat_message_stop() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Chat, "fallback");
        let first = String::from_utf8(converter.error_event("上游流式响应中断")).unwrap();
        let second = String::from_utf8(converter.finish_stream()).unwrap();
        assert_eq!(first.matches("event: message_stop").count(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn anthropic_sse_abort_injects_message_start_when_missing() {
        let sse = String::from_utf8(anthropic_sse_abort(
            "上游流式响应中断",
            "gpt-5.6-sol",
            false,
            &[],
            0,
            false,
        ))
        .unwrap();
        assert!(sse.contains("event: message_start"));
        assert!(sse.contains("\"model\":\"gpt-5.6-sol\""));
        assert!(!sse.contains("\"model\":\"proxy\""));
        assert!(sse.contains("event: message_delta"));
        assert!(sse.contains("event: message_stop"));
        assert!(!sse.contains("event: error"));
        assert!(sse.contains("上游流式响应中断"));
        let text = sse.find("上游流式响应中断").unwrap();
        let stop = sse.find("event: message_stop").unwrap();
        assert!(text < stop);
    }

    #[test]
    fn anthropic_sse_abort_skips_message_start_when_already_seen() {
        let sse = String::from_utf8(anthropic_sse_abort(
            "流式响应空闲超时",
            "gpt-5.6-sol",
            true,
            &[0],
            1,
            false,
        ))
        .unwrap();
        assert!(!sse.contains("event: message_start"));
        assert!(sse.contains("\"index\":0"));
        assert!(sse.contains("event: message_stop"));
        assert!(!sse.contains("event: error"));
        assert!(sse.contains("流式响应空闲超时"));
        let closed = sse.find("\"index\":0").unwrap();
        let text = sse.find("流式响应空闲超时").unwrap();
        let stop = sse.find("event: message_stop").unwrap();
        assert!(closed < text);
        assert!(text < stop);
    }

    #[test]
    fn chat_prompt_cache_key_strips_by_default_without_allowlist() {
        let mut body = json!({"model": "m", "messages": []});
        reinject_chat_prompt_cache_key(&mut body, Some("explicit"), Some("session"), false);
        assert!(body.get("prompt_cache_key").is_none());
        assert!(!chat_prompt_cache_allowed_for_base_url(
            "https://api.openai.com/v1"
        ));
    }

    #[test]
    fn chat_prompt_cache_key_reinjects_explicit_for_allowlisted_provider() {
        assert!(chat_prompt_cache_allowed_for_base_url(
            "https://api.moonshot.cn/v1"
        ));
        assert!(chat_prompt_cache_allowed_for_base_url(
            "https://api.kimi.com/coding/v1"
        ));
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
    fn client_catalog_id_wins_over_upstream_slug() {
        let client = "claude.sub2api.gpt-5.6-sol";
        let chat = json!({
            "id": "chatcmpl_sol",
            "model": "gpt-5.6-sol",
            "choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]
        });
        assert_eq!(openai_chat_to_anthropic(&chat, client)["model"], client);

        let responses = json!({
            "id": "resp_sol",
            "model": "gpt-5.6-sol",
            "status": "completed",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}]
        });
        assert_eq!(
            openai_responses_to_anthropic(&responses, client)["model"],
            client
        );

        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, client);
        let start = String::from_utf8(converter.push_event(&json!({
            "type": "response.created",
            "response": {"id": "resp_sol", "model": "gpt-5.6-sol", "status": "in_progress"}
        })))
        .unwrap();
        let more = String::from_utf8(converter.push_event(&json!({
            "type": "response.output_text.delta",
            "delta": "ok"
        })))
        .unwrap();
        let combined = format!("{start}{more}");
        assert!(combined.contains("\"model\":\"claude.sub2api.gpt-5.6-sol\""));
        assert!(!combined.contains("\"model\":\"gpt-5.6-sol\""));
    }

    #[test]
    fn empty_client_model_still_uses_upstream_slug() {
        assert_eq!(client_facing_model("", Some("gpt-5.6-sol")), "gpt-5.6-sol");
        let message = openai_chat_to_anthropic(
            &json!({
                "id": "chatcmpl_1",
                "model": "gpt-test",
                "choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]
            }),
            "",
        );
        assert_eq!(message["model"], "gpt-test");
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

    #[test]
    fn responses_reasoning_item_immediately_opens_thinking_block_and_normalizes_section_signs() {
        let mut converter = OpenAiSseConverter::new(OpenAiStreamProtocol::Responses, "gpt-6.1-sol");
        let init = String::from_utf8(converter.initial_events()).unwrap();
        assert!(init.contains("event: message_start"));

        let reasoning_added = String::from_utf8(converter.push_event(&json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": { "id": "rs_1", "type": "reasoning" }
        })))
        .unwrap();
        assert!(reasoning_added.contains("event: content_block_start"));
        assert!(reasoning_added.contains("\"type\":\"thinking\""));

        // Split section sign across chunks: "S1 §" + "1.4" + "、S2 第5项"
        let chunk1 = String::from_utf8(converter.push_event(&json!({
            "type": "response.output_text.delta",
            "output_index": 1,
            "content_index": 0,
            "delta": "S1 §"
        })))
        .unwrap();
        // Thinking block without explicit summary delta should get a single space thinking_delta before closing.
        assert!(chunk1.contains("\"type\":\"thinking_delta\""));
        assert!(chunk1.contains("event: content_block_stop"));

        let chunk2 = String::from_utf8(converter.push_event(&json!({
            "type": "response.output_text.delta",
            "output_index": 1,
            "content_index": 0,
            "delta": "1.4"
        })))
        .unwrap();
        assert!(!chunk2.contains("第1节"));

        let chunk3 = String::from_utf8(converter.push_event(&json!({
            "type": "response.output_text.delta",
            "output_index": 1,
            "content_index": 0,
            "delta": "、S2 第5项"
        })))
        .unwrap();
        assert!(chunk3.contains("第1.4节、S2 第5项"));
    }

    #[test]
    fn deepseek_thinking_downgrades_named_tool_choice_to_required_and_orders_tool_before_user_text() {
        let request = json!({
            "model": "claude.deepseek-v4-flash",
            "thinking": { "type": "enabled", "budget_tokens": 4096 },
            "tools": [{
                "name": "calc",
                "description": "add numbers",
                "input_schema": { "type": "object", "properties": { "a": { "type": "integer" } } }
            }],
            "tool_choice": { "type": "tool", "name": "calc" },
            "messages": [
                { "role": "user", "content": "compute" },
                {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "call_1",
                        "name": "calc",
                        "input": { "a": 1 }
                    }]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "And now double it" },
                        { "type": "tool_result", "tool_use_id": "call_1", "content": "result=1" }
                    ]
                }
            ]
        });
        let chat = anthropic_to_openai_chat(&request, "deepseek-v4-flash", false, None);
        assert_eq!(chat["tool_choice"], json!("required"));
        let msgs = chat["messages"].as_array().unwrap();
        let tool_pos = msgs.iter().position(|m| m["role"] == "tool").unwrap();
        let trailing_user_pos = msgs
            .iter()
            .rposition(|m| m["role"] == "user")
            .unwrap();
        assert!(
            tool_pos < trailing_user_pos,
            "tool role message must immediately follow assistant tool_calls before trailing user text"
        );

        let none_req = json!({
            "model": "gpt-5.6-luna",
            "tool_choice": { "type": "none" },
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let none_chat = anthropic_to_openai_chat(&none_req, "gpt-5.6-luna", false, None);
        assert_eq!(none_chat["tool_choice"], json!("none"));
    }

    #[test]
    fn sanitize_responses_and_chat_repair_interrupted_and_orphan_tools() {
        let mut resp = json!({
            "model": "gpt-5.6-luna",
            "input": [
                { "type": "function_call_output", "call_id": "orphan_1", "output": "old" },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "run" }] },
                { "type": "function_call", "call_id": "interrupted_1", "name": "bash", "arguments": "{}" },
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "cancel" }] }
            ]
        });
        sanitize_codex_responses_body(&mut resp);
        let input = resp["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "orphan_1");
        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["call_id"], "orphan_1");
        assert_eq!(input[3]["type"], "function_call");
        assert_eq!(input[3]["call_id"], "interrupted_1");
        assert_eq!(input[4]["type"], "function_call_output");
        assert_eq!(input[4]["call_id"], "interrupted_1");

        let mut chat_msgs = vec![
            json!({ "role": "tool", "tool_call_id": "orphan_c1", "content": "old_out" }),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{ "id": "int_c1", "type": "function", "function": { "name": "bash", "arguments": "{}" } }]
            }),
            json!({ "role": "user", "content": "cancel" }),
        ];
        sanitize_openai_chat_messages(&mut chat_msgs);
        assert_eq!(chat_msgs[0]["role"], "user");
        assert_eq!(chat_msgs[1]["role"], "assistant");
        assert_eq!(chat_msgs[2]["role"], "tool");
        assert_eq!(chat_msgs[2]["tool_call_id"], "int_c1");
        assert_eq!(chat_msgs[3]["role"], "user");
    }

    #[test]
    fn extract_anthropic_document_text_handles_text_content_and_base64() {
        let doc_text = extract_anthropic_document_text(&json!({
            "type": "document",
            "title": "readme.txt",
            "source": {
                "type": "text",
                "media_type": "text/plain",
                "data": "Project code is ALPHA_42."
            }
        }))
        .unwrap();
        assert!(doc_text.contains("[Document: readme.txt]"));
        assert!(doc_text.contains("ALPHA_42"));

        let b64_doc = extract_anthropic_document_text(&json!({
            "type": "document",
            "source": {
                "type": "base64",
                "media_type": "text/markdown",
                "data": "SGVsbG8gV29ybGQ="
            }
        }))
        .unwrap();
        assert_eq!(b64_doc, "Hello World");
    }
}
