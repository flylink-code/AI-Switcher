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
        return Err(AppError::Config(
            "Responses 请求未包含可转换的消息".to_string(),
        ));
    }

    let max_tokens = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(16384);

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
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(responses_tool_to_anthropic)
            .collect();
        if !converted.is_empty() {
            result["tools"] = Value::Array(converted);
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_anthropic(choice);
    }

    let effort = body
        .pointer("/reasoning/effort")
        .or_else(|| body.get("reasoning_effort"))
        .and_then(Value::as_str);
    if let Some(effort) = effort {
        let normalized_effort = match effort.trim().to_ascii_lowercase().as_str() {
            "minimal" | "low" => "low",
            "medium" => "medium",
            "high" | "xhigh" | "max" => "high",
            _ => "medium",
        };
        let budget = match normalized_effort {
            "low" => 2048,
            "medium" => 8192,
            "high" => 16384,
            _ => 8192,
        };
        result["output_config"] = json!({
            "effort": normalized_effort
        });
        result["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget
        });
        if max_tokens <= budget + 4096 {
            result["max_tokens"] = json!(budget + 16384);
        }
    }

    ensure_deepseek_assistant_thinking(&mut result, model, None);

    Ok(result)
}

/// DeepSeek Anthropic-compatible endpoints (`deepseek-v4-*`, `deepseek-reasoner`, etc.)
/// run in thinking mode by default and reject multi-turn tool calls with HTTP 400
/// (`"The content[].thinking in the thinking mode must be passed back to the API."`)
/// whenever a historical `assistant` message contains `tool_use` without a `thinking` block.
pub(crate) fn ensure_deepseek_assistant_thinking(
    body: &mut Value,
    model: &str,
    base_url: Option<&str>,
) {
    let is_deepseek = model.trim().to_ascii_lowercase().contains("deepseek")
        || base_url.is_some_and(|u| u.to_ascii_lowercase().contains("deepseek"));
    if !is_deepseek {
        return;
    }
    let thinking_disabled = body
        .pointer("/thinking/type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("disabled"));
    if thinking_disabled {
        return;
    }
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(blocks) = msg.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let has_tool_use = blocks
            .iter()
            .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"));
        let has_thinking = blocks.iter().any(|b| {
            matches!(
                b.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            )
        });
        if has_tool_use && !has_thinking {
            blocks.insert(0, json!({"type": "thinking", "thinking": "..."}));
        }
    }
}

/// Map a non-streaming Anthropic Messages response to Responses-like JSON.
pub fn anthropic_response_to_responses(body: &Value) -> Value {
    if body.get("type").and_then(Value::as_str) == Some("error") {
        return anthropic_error_to_responses(body);
    }

    let mut output = Vec::new();
    let mut text_parts = Vec::new();
    for block in body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                if let Some(thinking) = block
                    .get("thinking")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    output.push(json!({
                        "type": "reasoning",
                        "summary": [{"type": "summary_text", "text": thinking}],
                    }));
                }
            }
            Some("text") => {
                if let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
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
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .unwrap_or("tool_call");
                output.push(json!({
                    "type": "function_call",
                    "id": format!("fc_{call_id}"),
                    "call_id": call_id,
                    "name": block.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({})))
                        .unwrap_or_else(|_| "{}".to_string()),
                    "status": "completed",
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
    } else {
        result["usage"] = json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
        });
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
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_input = input_tokens.saturating_add(cache_read);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut result = json!({
        "input_tokens": total_input,
        "output_tokens": output_tokens,
        "total_tokens": total_input.saturating_add(output_tokens),
    });
    if cache_read > 0 {
        result["input_tokens_details"] = json!({"cached_tokens": cache_read});
    }
    result
}

fn responses_input_to_messages(input: &Value) -> AppResult<Vec<Value>> {
    match input {
        Value::String(text) if !text.is_empty() => {
            Ok(vec![json!({"role": "user", "content": text})])
        }
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
                            .unwrap_or_else(|| {
                                item.get("arguments").cloned().unwrap_or_else(|| json!({}))
                            });
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
                        let anthropic_role = if role == "assistant" {
                            "assistant"
                        } else {
                            "user"
                        };
                        let content =
                            convert_responses_content(item.get("content").unwrap_or(&Value::Null))?;
                        if !content.is_empty() {
                            messages.push(json!({"role": anthropic_role, "content": content}));
                        }
                    }
                    _ => {}
                }
            }
            flush_tool_uses(&mut messages, &mut pending_tool_uses);
            flush_tool_results(&mut messages, &mut pending_tool_results);
            sanitize_anthropic_messages(&mut messages);
            Ok(messages)
        }
        _ => Err(AppError::Config(
            "Responses input 必须是字符串或数组".to_string(),
        )),
    }
}

fn message_blocks_vec(content: &Value) -> Vec<Value> {
    match content {
        Value::Array(arr) => arr.clone(),
        Value::String(s) if !s.is_empty() => vec![json!({"type": "text", "text": s})],
        _ => Vec::new(),
    }
}

/// Ensure Anthropic Messages invariants:
/// 1. Consecutive messages with the same role are merged.
/// 2. Every `tool_use` in an `assistant` message has a matching `tool_result`
///    at the start of the immediately following `user` message.
/// 3. Orphan `tool_result` blocks without a preceding `tool_use` are removed.
pub(crate) fn sanitize_anthropic_messages(messages: &mut Vec<Value>) {
    let mut merged: Vec<Value> = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        let role = msg
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let blocks = message_blocks_vec(msg.get("content").unwrap_or(&Value::Null));
        if blocks.is_empty() {
            continue;
        }
        if let Some(last) = merged
            .last_mut()
            .filter(|m| m.get("role").and_then(Value::as_str) == Some(role.as_str()))
        {
            let mut combined = message_blocks_vec(last.get("content").unwrap_or(&Value::Null));
            combined.extend(blocks);
            last["content"] = Value::Array(combined);
        } else {
            merged.push(json!({ "role": role, "content": blocks }));
        }
    }

    let mut i = 0;
    while i < merged.len() {
        let role = merged[i]
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        if role == "assistant" {
            let blocks = message_blocks_vec(merged[i].get("content").unwrap_or(&Value::Null));
            let mut thinking_blocks = Vec::new();
            let mut text_blocks = Vec::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_use_ids = Vec::new();
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("thinking" | "redacted_thinking") => thinking_blocks.push(b),
                    Some("tool_use") => {
                        if let Some(id) = b
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                        {
                            if !tool_use_ids.iter().any(|existing: &String| existing == id) {
                                tool_use_ids.push(id.to_string());
                                tool_use_blocks.push(b);
                            }
                        }
                    }
                    _ => text_blocks.push(b),
                }
            }
            let mut ordered = thinking_blocks;
            ordered.extend(text_blocks);
            ordered.extend(tool_use_blocks);
            merged[i]["content"] = Value::Array(ordered);

            if !tool_use_ids.is_empty() {
                let next_is_user = merged
                    .get(i + 1)
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str)
                    == Some("user");
                if !next_is_user {
                    merged.insert(i + 1, json!({ "role": "user", "content": [] }));
                }
                let next_blocks =
                    message_blocks_vec(merged[i + 1].get("content").unwrap_or(&Value::Null));
                let mut existing_results = std::collections::HashMap::new();
                let mut non_result_blocks = Vec::new();
                for b in next_blocks {
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                            existing_results.entry(id.to_string()).or_insert(b);
                        }
                    } else {
                        non_result_blocks.push(b);
                    }
                }
                let mut final_next = Vec::with_capacity(tool_use_ids.len() + non_result_blocks.len());
                for id in &tool_use_ids {
                    if let Some(res) = existing_results.remove(id) {
                        final_next.push(res);
                    } else {
                        final_next.push(json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": "[Tool execution was cancelled or skipped before returning a result]"
                        }));
                    }
                }
                final_next.extend(non_result_blocks);
                merged[i + 1]["content"] = Value::Array(final_next);
            }
        } else {
            let prev_has_tool_use = i > 0
                && merged[i - 1].get("role").and_then(Value::as_str) == Some("assistant")
                && merged[i - 1]
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|arr| {
                        arr.iter()
                            .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                    });
            if !prev_has_tool_use {
                let blocks = message_blocks_vec(merged[i].get("content").unwrap_or(&Value::Null));
                let filtered: Vec<Value> = blocks
                    .into_iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) != Some("tool_result"))
                    .collect();
                if filtered.is_empty() {
                    merged.remove(i);
                    continue;
                }
                merged[i]["content"] = Value::Array(filtered);
            }
        }
        i += 1;
    }

    *messages = merged;
}

fn flush_tool_uses(messages: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut().filter(|last| {
        last.get("role").and_then(Value::as_str) == Some("assistant")
            && last
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| {
                    blocks
                        .iter()
                        .all(|b| b.get("type").and_then(Value::as_str) != Some("tool_use"))
                })
    }) {
        if let Some(blocks) = last.get_mut("content").and_then(Value::as_array_mut) {
            blocks.extend(std::mem::take(pending));
            return;
        }
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
                        if let Some(text) = part
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
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

const EDIT_UNIQUE_CONTEXT_HINT: &str = "\nIMPORTANT: Include enough surrounding context lines (such as the enclosing function header or unique adjacent statements) so `old_str` / `oldString` is 100% unique in the file and never matches multiple locations.";

const VIEW_TOOL_READ_HINT: &str = "\nIMPORTANT: Never call this tool on binary or archive files (e.g. `.asar`, `.asar.bak`, `.exe`, `.dll`, `.db`, `.sqlite`, `.zip`). On Windows, `path` MUST use backslashes (`\\`) and never `file://` URIs or forward slashes (`/`). If `view_range: [1, N]` returns `[File content truncated at 20KB...]` without an explicit `Use view_range=[M, ...]` line hint, the file has fewer than `N` lines with very long/minified lines — do NOT guess higher `view_range` start lines like `[N+1, ...]` (which will fail with out-of-bounds); use a shell or search tool instead. Never retry `view` on the same file after a read failure.";

pub(crate) fn enrich_edit_tool_description(name: &str, raw_description: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    let is_edit_tool = matches!(
        lower.as_str(),
        "edit"
            | "edit_file"
            | "str_replace"
            | "replace_string_in_file"
            | "multi_replace_string_in_file"
    );
    if is_edit_tool {
        if raw_description.contains("100% unique") {
            return raw_description.to_string();
        }
        return if raw_description.trim().is_empty() {
            EDIT_UNIQUE_CONTEXT_HINT.trim_start().to_string()
        } else {
            format!("{raw_description}{EDIT_UNIQUE_CONTEXT_HINT}")
        };
    }

    let is_view_tool = matches!(lower.as_str(), "view" | "read_file" | "read");
    if is_view_tool {
        if raw_description.contains("Never call this tool on binary or archive files") {
            return raw_description.to_string();
        }
        return if raw_description.trim().is_empty() {
            VIEW_TOOL_READ_HINT.trim_start().to_string()
        } else {
            format!("{raw_description}{VIEW_TOOL_READ_HINT}")
        };
    }

    raw_description.to_string()
}

fn responses_tool_to_anthropic(tool: &Value) -> Option<Value> {
    let name = tool.get("name").and_then(Value::as_str)?;
    let schema = tool
        .get("parameters")
        .or_else(|| tool.pointer("/function/parameters"))
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    let raw_desc = tool
        .get("description")
        .or_else(|| tool.pointer("/function/description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(json!({
        "name": name,
        "description": enrich_edit_tool_description(name, raw_desc),
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
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("function") => {
            json!({
                "type": "tool",
                "name": map
                    .get("name")
                    .or_else(|| map.get("function").and_then(|value| value.get("name")))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            })
        }
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

/// Codex 0.162 rejects `response.completed` unless `usage.total_tokens` is present.
pub(crate) fn ensure_response_usage_fields(value: &mut Value) {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    if kind == "response.completed" || kind == "response.incomplete" {
        if let Some(response) = value.get_mut("response") {
            fill_responses_usage(response);
        }
        return;
    }
    if value.get("object").and_then(Value::as_str) == Some("response") {
        fill_responses_usage(value);
    }
}

fn fill_responses_usage(response: &mut Value) {
    let Some(object) = response.as_object_mut() else {
        return;
    };
    let usage = object.entry("usage").or_insert_with(|| {
        json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
        })
    });
    let Some(usage) = usage.as_object_mut() else {
        return;
    };
    let input = json_i64(usage.get("input_tokens"))
        .or_else(|| json_i64(usage.get("prompt_tokens")))
        .unwrap_or(0);
    let output = json_i64(usage.get("output_tokens"))
        .or_else(|| json_i64(usage.get("completion_tokens")))
        .unwrap_or(0);
    usage.entry("input_tokens").or_insert(json!(input));
    usage.entry("output_tokens").or_insert(json!(output));
    usage
        .entry("total_tokens")
        .or_insert(json!(input.saturating_add(output)));
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).or_else(|| {
        value
            .and_then(Value::as_u64)
            .and_then(|number| i64::try_from(number).ok())
    })
}

/// Rewrite one SSE frame so a terminal Responses event carries `total_tokens`.
/// Unchanged frames are returned as-is. The frame delimiter is not included.
pub(crate) fn ensure_responses_sse_usage(block: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(block) else {
        return block.to_vec();
    };
    let Some((event, mut value)) = parse_sse_event(text) else {
        return block.to_vec();
    };
    if event != "response.completed" && event != "response.incomplete" {
        return block.to_vec();
    }
    let before = value.clone();
    ensure_response_usage_fields(&mut value);
    if value == before {
        return block.to_vec();
    }
    let mut out = String::new();
    out.push_str("event: ");
    out.push_str(&event);
    out.push('\n');
    out.push_str("data: ");
    out.push_str(&value.to_string());
    out.into_bytes()
}

fn parse_sse_event(frame: &str) -> Option<(String, Value)> {
    let mut event_type = None;
    let mut data_lines = Vec::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.trim_start();
            if !value.is_empty() {
                data_lines.push(value);
            }
        }
    }
    let data = data_lines.join("\n");
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let parsed = serde_json::from_str::<Value>(&data).ok()?;
    let event_type = event_type.map(str::to_string).or_else(|| {
        parsed
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
    })?;
    Some((event_type, parsed))
}

pub(crate) fn push_responses_event(output: &mut String, event_type: &str, data: Value) {
    output.push_str("event: ");
    output.push_str(event_type);
    output.push('\n');
    output.push_str("data: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string()));
    output.push_str("\n\n");
}

struct PendingTool {
    output_index: usize,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
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
    reasoning_item_started: bool,
    reasoning_item_closed: bool,
    reasoning_output_index: usize,
    reasoning_item_id: String,
    reasoning: String,
    text_item_started: bool,
    text_output_index: usize,
    text_item_id: String,
    text: String,
    tools: Vec<PendingTool>,
    tool_output_indices: std::collections::BTreeMap<usize, usize>,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    output_tokens: i64,
    status: String,
}

impl AnthropicSseToResponsesConverter {
    pub fn new(fallback_model: &str) -> Self {
        let uid = uuid::Uuid::new_v4().simple().to_string();
        Self {
            fallback_model: fallback_model.to_string(),
            response_id: format!("resp_{uid}"),
            model: fallback_model.to_string(),
            started: false,
            completed: false,
            output_index: 0,
            content_index: 0,
            reasoning_item_started: false,
            reasoning_item_closed: false,
            reasoning_output_index: 0,
            reasoning_item_id: format!("rs_{uid}"),
            reasoning: String::new(),
            text_item_started: false,
            text_output_index: 0,
            text_item_id: format!("msg_{uid}"),
            text: String::new(),
            tools: Vec::new(),
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
                    if !self.started {
                        if let Some(id) = message.get("id").and_then(Value::as_str) {
                            self.response_id = id.to_string();
                            self.reasoning_item_id = format!("rs_{id}");
                            self.text_item_id = id.to_string();
                        }
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
                    Some("thinking" | "redacted_thinking") => {
                        if !self.reasoning_item_started {
                            self.emit_reasoning_item_added(&mut output);
                        }
                        if let Some(thinking) = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            self.reasoning.push_str(thinking);
                            push_responses_event(
                                &mut output,
                                "response.reasoning_summary_text.delta",
                                json!({
                                    "type": "response.reasoning_summary_text.delta",
                                    "item_id": self.reasoning_item_id,
                                    "output_index": self.reasoning_output_index,
                                    "summary_index": 0,
                                    "delta": thinking,
                                }),
                            );
                        }
                    }
                    Some("text") => {
                        self.close_reasoning_item_if_open(&mut output);
                        if !self.text_item_started {
                            self.emit_text_item_added(&mut output);
                        }
                    }
                    Some("tool_use") => {
                        self.close_reasoning_item_if_open(&mut output);
                        let output_index = self.output_index;
                        let call_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .unwrap_or("tool_call")
                            .to_string();
                        let item_id = format!("fc_{call_id}");
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|n| !n.is_empty())
                            .unwrap_or("tool")
                            .to_string();
                        let initial_args = block
                            .get("input")
                            .filter(|v| v.as_object().is_some_and(|o| !o.is_empty()))
                            .and_then(|v| serde_json::to_string(v).ok())
                            .unwrap_or_default();
                        self.tool_output_indices.insert(index, output_index);
                        self.tools.push(PendingTool {
                            output_index,
                            item_id: item_id.clone(),
                            call_id: call_id.clone(),
                            name: name.clone(),
                            arguments: initial_args.clone(),
                        });
                        self.output_index += 1;
                        push_responses_event(
                            &mut output,
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
                        if !initial_args.is_empty() {
                            push_responses_event(
                                &mut output,
                                "response.function_call_arguments.delta",
                                json!({
                                    "type": "response.function_call_arguments.delta",
                                    "item_id": item_id,
                                    "output_index": output_index,
                                    "call_id": call_id,
                                    "delta": initial_args,
                                }),
                            );
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                self.ensure_started(&mut output);
                let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = data.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("thinking_delta") => {
                        if !self.reasoning_item_started {
                            self.emit_reasoning_item_added(&mut output);
                        }
                        if let Some(thinking) = delta
                            .get("thinking")
                            .or_else(|| delta.get("text"))
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            self.reasoning.push_str(thinking);
                            push_responses_event(
                                &mut output,
                                "response.reasoning_summary_text.delta",
                                json!({
                                    "type": "response.reasoning_summary_text.delta",
                                    "item_id": self.reasoning_item_id,
                                    "output_index": self.reasoning_output_index,
                                    "summary_index": 0,
                                    "delta": thinking,
                                }),
                            );
                        }
                    }
                    Some("text_delta") => {
                        self.close_reasoning_item_if_open(&mut output);
                        if !self.text_item_started {
                            self.emit_text_item_added(&mut output);
                        }
                        if let Some(text) = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            self.text.push_str(text);
                            push_responses_event(
                                &mut output,
                                "response.output_text.delta",
                                json!({
                                    "type": "response.output_text.delta",
                                    "item_id": self.text_item_id,
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
                        if let Some(partial) = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .filter(|value| !value.is_empty())
                        {
                            let mut item_id = String::new();
                            let mut call_id = String::new();
                            if let Some(tool) = self
                                .tools
                                .iter_mut()
                                .find(|tool| tool.output_index == output_index)
                            {
                                tool.arguments.push_str(partial);
                                item_id = tool.item_id.clone();
                                call_id = tool.call_id.clone();
                            }
                            push_responses_event(
                                &mut output,
                                "response.function_call_arguments.delta",
                                json!({
                                    "type": "response.function_call_arguments.delta",
                                    "item_id": item_id,
                                    "output_index": output_index,
                                    "call_id": call_id,
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
        if !self.reasoning.trim().is_empty()
            && !self.text_item_started
            && self.text.trim().is_empty()
            && self.tools.is_empty()
        {
            // Gemini occasionally stops right after emitting `thought` blocks without a
            // separate `text` or `functionCall` part. Surface the reasoning text as the
            // visible assistant message so VS Code Copilot does not silently end the turn
            // with an empty bubble after "Finished thinking".
            self.close_reasoning_item_if_open(&mut output);
            self.emit_text_item_added(&mut output);
            let fallback_text = self.reasoning.clone();
            self.text = fallback_text.clone();
            push_responses_event(
                &mut output,
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "item_id": self.text_item_id,
                    "output_index": self.text_output_index,
                    "content_index": self.content_index,
                    "delta": fallback_text,
                }),
            );
        }
        let event_type = if self.status == "incomplete" {
            "response.incomplete"
        } else {
            "response.completed"
        };
        let total_input = self
            .input_tokens
            .saturating_add(self.cache_read_input_tokens);
        let mut final_output = Vec::new();
        if self.reasoning_item_started || !self.reasoning.is_empty() {
            final_output.push((
                self.reasoning_output_index,
                json!({
                    "id": self.reasoning_item_id.clone(),
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": self.reasoning.clone()}],
                }),
            ));
        }
        if self.text_item_started || !self.text.is_empty() {
            final_output.push((
                self.text_output_index,
                json!({
                    "type": "message",
                    "id": self.text_item_id.clone(),
                    "role": "assistant",
                    "status": "completed",
                    "content": [{"type": "output_text", "text": self.text.clone()}],
                }),
            ));
        }
        for tool in &self.tools {
            final_output.push((
                tool.output_index,
                json!({
                    "id": tool.item_id,
                    "type": "function_call",
                    "call_id": tool.call_id,
                    "name": tool.name,
                    "arguments": tool.arguments,
                    "status": "completed",
                }),
            ));
        }
        final_output.sort_by_key(|(index, _)| *index);
        for (index, item) in &final_output {
            if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                if self.reasoning_item_closed {
                    continue;
                }
                self.reasoning_item_closed = true;
                let text = item
                    .pointer("/summary/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                push_responses_event(
                    &mut output,
                    "response.reasoning_summary_text.done",
                    json!({
                        "type": "response.reasoning_summary_text.done",
                        "item_id": self.reasoning_item_id,
                        "output_index": index,
                        "summary_index": 0,
                        "text": text,
                    }),
                );
                push_responses_event(
                    &mut output,
                    "response.reasoning_summary_part.done",
                    json!({
                        "type": "response.reasoning_summary_part.done",
                        "item_id": self.reasoning_item_id,
                        "output_index": index,
                        "summary_index": 0,
                        "part": {"type": "summary_text", "text": text},
                    }),
                );
            } else if item.get("type").and_then(Value::as_str) == Some("message") {
                let text = item
                    .pointer("/content/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                push_responses_event(
                    &mut output,
                    "response.output_text.done",
                    json!({
                        "type": "response.output_text.done",
                        "item_id": self.text_item_id,
                        "output_index": index,
                        "content_index": 0,
                        "text": text,
                    }),
                );
                push_responses_event(
                    &mut output,
                    "response.content_part.done",
                    json!({
                        "type": "response.content_part.done",
                        "item_id": self.text_item_id,
                        "output_index": index,
                        "content_index": 0,
                        "part": {"type": "output_text", "text": text},
                    }),
                );
            } else if item.get("type").and_then(Value::as_str) == Some("function_call") {
                push_responses_event(
                    &mut output,
                    "response.function_call_arguments.done",
                    json!({
                        "type": "response.function_call_arguments.done",
                        "item_id": item.get("id").and_then(Value::as_str).unwrap_or(""),
                        "output_index": index,
                        "call_id": item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                        "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
                        "arguments": item.get("arguments").and_then(Value::as_str).unwrap_or(""),
                    }),
                );
            }
            push_responses_event(
                &mut output,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item,
                }),
            );
        }
        let final_output: Vec<Value> = final_output.into_iter().map(|(_, item)| item).collect();
        let mut response = json!({
            "id": self.response_id.clone(),
            "object": "response",
            "status": self.status.clone(),
            "model": if self.model.is_empty() { &self.fallback_model } else { &self.model },
            "output": final_output,
            "usage": {
                "input_tokens": total_input,
                "output_tokens": self.output_tokens,
                "total_tokens": total_input.saturating_add(self.output_tokens),
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
            self.text_output_index
        } else {
            self.output_index
        }
    }

    pub fn start_handshake(&mut self) -> Vec<u8> {
        let mut output = String::new();
        self.ensure_started(&mut output);
        self.emit_reasoning_item_added(&mut output);
        output.into_bytes()
    }

    pub fn keepalive_event(&mut self) -> Vec<u8> {
        if self.completed {
            return Vec::new();
        }
        let mut output = String::new();
        self.ensure_started(&mut output);
        if !self.reasoning_item_started {
            self.emit_reasoning_item_added(&mut output);
        }
        if !self.reasoning_item_closed {
            push_responses_event(
                &mut output,
                "response.reasoning_summary_text.delta",
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": self.reasoning_item_id,
                    "output_index": self.reasoning_output_index,
                    "summary_index": 0,
                    "delta": "",
                }),
            );
        } else {
            output.push_str(": keepalive\n\n");
        }
        output.into_bytes()
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
            "output": [],
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

    fn emit_reasoning_item_added(&mut self, output: &mut String) {
        if self.reasoning_item_started {
            return;
        }
        self.reasoning_output_index = self.output_index;
        self.output_index += 1;
        self.reasoning_item_started = true;
        push_responses_event(
            output,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": self.reasoning_output_index,
                "item": {
                    "id": self.reasoning_item_id,
                    "type": "reasoning",
                    "summary": [],
                },
            }),
        );
        push_responses_event(
            output,
            "response.reasoning_summary_part.added",
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": self.reasoning_item_id,
                "output_index": self.reasoning_output_index,
                "summary_index": 0,
                "part": {"type": "summary_text", "text": ""},
            }),
        );
    }

    fn close_reasoning_item_if_open(&mut self, output: &mut String) {
        if !self.reasoning_item_started || self.reasoning_item_closed {
            return;
        }
        self.reasoning_item_closed = true;
        let index = self.reasoning_output_index;
        let text = self.reasoning.clone();
        let item = json!({
            "id": self.reasoning_item_id.clone(),
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": text.clone()}],
        });
        push_responses_event(
            output,
            "response.reasoning_summary_text.done",
            json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": self.reasoning_item_id,
                "output_index": index,
                "summary_index": 0,
                "text": text,
            }),
        );
        push_responses_event(
            output,
            "response.reasoning_summary_part.done",
            json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": self.reasoning_item_id,
                "output_index": index,
                "summary_index": 0,
                "part": {"type": "summary_text", "text": text},
            }),
        );
        push_responses_event(
            output,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": index,
                "item": item,
            }),
        );
    }

    fn emit_text_item_added(&mut self, output: &mut String) {
        if self.text_item_started {
            return;
        }
        self.text_output_index = self.output_index;
        push_responses_event(
            output,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": self.output_index,
                "item": {
                    "id": self.text_item_id,
                    "type": "message",
                    "role": "assistant",
                    "status": "in_progress",
                    "content": []
                },
            }),
        );
        push_responses_event(
            output,
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": self.text_item_id,
                "output_index": self.output_index,
                "content_index": self.content_index,
                "part": {"type": "output_text", "text": ""},
            }),
        );
        self.text_item_started = true;
        self.output_index += 1;
    }

    fn observe_usage(&mut self, usage: &Value) {
        if let Some(input) = usage.get("input_tokens").and_then(Value::as_i64) {
            self.input_tokens = input;
        }
        if let Some(cache) = usage.get("cache_read_input_tokens").and_then(Value::as_i64) {
            self.cache_read_input_tokens = cache;
        }
        if let Some(output) = usage.get("output_tokens").and_then(Value::as_i64) {
            self.output_tokens = output;
        }
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
        assert_eq!(
            anthropic["messages"][2]["content"][0]["type"],
            "tool_result"
        );
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
        assert_eq!(
            responses["usage"]["input_tokens_details"]["cached_tokens"],
            1
        );
        assert_eq!(responses["usage"]["output_tokens"], 5);
        assert_eq!(responses["usage"]["total_tokens"], 10);
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
        assert_eq!(
            responses["incomplete_details"]["reason"],
            "max_output_tokens"
        );
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
        let stop = String::from_utf8(
            converter.push_event("message_stop", &json!({"type":"message_stop"})),
        )
        .unwrap();
        assert!(start.contains("response.created"));
        assert!(delta.contains("response.output_text.delta"));
        assert!(delta.contains("\"delta\":\"Hi\""));
        assert!(stop.contains("response.output_item.done"));
        assert!(stop.contains("response.completed"));
        assert!(stop.contains("\"total_tokens\":1") || stop.contains("\"total_tokens\": 1"));
        assert!(stop.contains("\"text\":\"Hi\""));
    }

    #[test]
    fn responses_completed_without_total_tokens_is_filled() {
        let block = b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":4}}}";
        let rewritten = String::from_utf8(ensure_responses_sse_usage(block)).unwrap();
        assert!(rewritten.contains("\"total_tokens\":7"));
        let unchanged = ensure_responses_sse_usage(b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}");
        assert_eq!(
            unchanged,
            b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}"
        );
    }

    #[test]
    fn string_input_maps_to_single_user_message() {
        let body = json!({"input": "ping", "max_output_tokens": 32});
        let anthropic = responses_request_to_anthropic_messages(&body, "claude-sonnet").unwrap();
        assert_eq!(
            anthropic["messages"][0],
            json!({"role": "user", "content": "ping"})
        );
    }

    #[test]
    fn responses_reasoning_effort_maps_to_anthropic_thinking() {
        let body = json!({
            "input": "solve",
            "reasoning": {"effort": "high"},
            "max_output_tokens": 1000
        });
        let anthropic =
            responses_request_to_anthropic_messages(&body, "claude-3-7-sonnet").unwrap();
        assert_eq!(anthropic["thinking"]["type"], "enabled");
        assert_eq!(anthropic["thinking"]["budget_tokens"], 16384);
        assert!(anthropic["max_tokens"].as_u64().unwrap() > 16384);
    }

    #[test]
    fn sanitizes_parallel_tool_use_with_interleaved_user_and_missing_output() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "input": [
                {"role": "user", "content": "check two files"},
                {"type": "function_call", "call_id": "toolu_vrtx_01DAnNhwShoffBR3tWJ9K5Qg", "name": "view", "arguments": "{\"path\":\"a.rs\"}"},
                {"type": "function_call", "call_id": "toolu_vrtx_01XjaCnBdS5FLpHeLHD4xBCy", "name": "view", "arguments": "{\"path\":\"b.rs\"}"},
                {"role": "developer", "content": "system reminder injected before outputs"},
                {"type": "function_call_output", "call_id": "toolu_vrtx_01DAnNhwShoffBR3tWJ9K5Qg", "output": "file A content"}
            ]
        });
        let anthropic =
            responses_request_to_anthropic_messages(&body, "claude-sonnet-4-5").unwrap();
        let msgs = anthropic["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "assistant");
        let assistant_blocks = msgs[1]["content"].as_array().unwrap();
        assert_eq!(assistant_blocks.len(), 2);
        assert_eq!(assistant_blocks[0]["type"], "tool_use");
        assert_eq!(
            assistant_blocks[0]["id"],
            "toolu_vrtx_01DAnNhwShoffBR3tWJ9K5Qg"
        );
        assert_eq!(assistant_blocks[1]["type"], "tool_use");
        assert_eq!(
            assistant_blocks[1]["id"],
            "toolu_vrtx_01XjaCnBdS5FLpHeLHD4xBCy"
        );

        assert_eq!(msgs[2]["role"], "user");
        let user_blocks = msgs[2]["content"].as_array().unwrap();
        assert_eq!(user_blocks[0]["type"], "tool_result");
        assert_eq!(
            user_blocks[0]["tool_use_id"],
            "toolu_vrtx_01DAnNhwShoffBR3tWJ9K5Qg"
        );
        assert_eq!(user_blocks[1]["type"], "tool_result");
        assert_eq!(
            user_blocks[1]["tool_use_id"],
            "toolu_vrtx_01XjaCnBdS5FLpHeLHD4xBCy"
        );
        assert_eq!(user_blocks[2]["type"], "text");
    }

    #[test]
    fn start_handshake_emits_reasoning_item_and_preserves_consistent_ids() {
        let mut converter = AnthropicSseToResponsesConverter::new("claude-sonnet-4-5");
        let handshake = String::from_utf8(converter.start_handshake()).unwrap();
        assert!(handshake.contains("event: response.created"));
        assert!(handshake.contains("event: response.in_progress"));
        assert!(handshake.contains("\"type\":\"reasoning\""));

        let _ = converter.push_event(
            "message_start",
            &json!({"type":"message_start","message":{"id":"msg_upstream_other","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":0}}}),
        );
        let delta = String::from_utf8(
            converter.push_event(
                "content_block_delta",
                &json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}),
            ),
        )
        .unwrap();
        // Reasoning item closes as soon as text starts, and output_text.delta includes item_id.
        assert!(delta.contains("event: response.reasoning_summary_text.done"));
        assert!(delta.contains("event: response.output_text.delta"));
        assert!(delta.contains("\"item_id\":\"msg_"));
    }

    #[test]
    fn thinking_only_stream_surfaces_reasoning_as_visible_message() {
        let mut converter = AnthropicSseToResponsesConverter::new("gemini-4-argon-medium");
        let _ = converter.start_handshake();
        let ka = String::from_utf8(converter.keepalive_event()).unwrap();
        assert!(ka.contains("event: response.reasoning_summary_text.delta"));
        assert!(ka.contains("\"delta\":\"\""));

        let _ = converter.push_event(
            "content_block_delta",
            &json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Analysis complete."}}),
        );
        let done = String::from_utf8(
            converter.push_event("message_stop", &json!({"type":"message_stop"})),
        )
        .unwrap();
        assert!(done.contains("event: response.output_text.delta"));
        assert!(done.contains("Analysis complete."));
        assert!(done.contains("\"type\":\"message\""));
    }

    #[test]
    fn deepseek_tool_use_turns_receive_thinking_block() {
        let body: Value = serde_json::from_str(RESPONSES_REQUEST).unwrap();
        let anthropic =
            responses_request_to_anthropic_messages(&body, "deepseek-v4-flash").unwrap();
        let assistant_blocks = anthropic["messages"][1]["content"].as_array().unwrap();
        assert_eq!(assistant_blocks[0]["type"], "thinking");
        assert_eq!(assistant_blocks[0]["thinking"], "...");
        assert_eq!(assistant_blocks[1]["type"], "tool_use");

        // Non-DeepSeek models must NOT receive synthetic unsigned thinking blocks.
        let claude = responses_request_to_anthropic_messages(&body, "claude-sonnet-4-6").unwrap();
        let claude_blocks = claude["messages"][1]["content"].as_array().unwrap();
        assert_eq!(claude_blocks[0]["type"], "tool_use");
    }
}
