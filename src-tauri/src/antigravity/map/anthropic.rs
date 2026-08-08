//! Anthropic Messages ↔ Gemini request/response mapping.

use serde_json::{json, Value};
use uuid::Uuid;

use super::models::{map_effort_to_suffix, map_model_id};
use crate::antigravity::model_catalog;

pub struct GeminiRequestParts {
    pub model: String,
    pub request: Value,
    pub stream: bool,
}

pub fn anthropic_to_gemini_request(body: &Value) -> Result<GeminiRequestParts, String> {
    let requested_model = body.get("model").and_then(Value::as_str).unwrap_or("");
    let from_flash_alias = requested_model.eq_ignore_ascii_case(model_catalog::GEMINI_FLASH_ALIAS_ID);
    let mut model = map_model_id(requested_model);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    // Reasoning effort (Claude Desktop slider / Claude Code): GA wire format is
    // `output_config.effort`; beta-era clients send a top-level `effort`; some
    // put it under `thinking.effort`.
    let effort = extract_effort(body);
    let thinking_kind = body
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);

    // Claude targets get the effort as generationConfig.thinkingConfig
    // (Cloud Code converts back for Claude models); Gemini targets encode the
    // level in the model id suffix instead.
    let mut claude_thinking_level: Option<&'static str> = None;
    let lower_model = model.to_ascii_lowercase();
    if lower_model.starts_with("gemini-") {
        if let Some(level) = effort.as_deref().and_then(map_effort_to_suffix) {
            model = model_catalog::with_forced_level(&model, level);
        } else if thinking_kind.as_deref() == Some("disabled") {
            model = model_catalog::with_forced_level(&model, "low");
        } else if from_flash_alias
            || matches!(thinking_kind.as_deref(), Some("adaptive") | Some("enabled"))
        {
            // Desktop Flash alias / adaptive thinking without explicit effort:
            // default high (matches Claude Sonnet 5 API default), not low-first
            // bare-name catalog fallback.
            model = model_catalog::with_forced_level(&model, "high");
        }
    } else if lower_model.starts_with("claude-") {
        claude_thinking_level = match effort.as_deref().and_then(map_effort_to_suffix) {
            Some(level) => Some(level),
            // Adaptive/enabled thinking without an explicit effort: default high
            // (matches Antigravity-Manager's claude mapper).
            None if matches!(thinking_kind.as_deref(), Some("adaptive") | Some("enabled")) => {
                Some("high")
            }
            None => None,
        };
    }

    let mut contents = Vec::new();
    let mut system_parts = Vec::new();

    if let Some(system) = body.get("system") {
        push_system_parts(system, &mut system_parts);
    }

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // tool_use.id → tool name, so tool_result can reference the real function
    // name (Anthropic tool_result blocks only carry tool_use_id).
    let mut tool_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for message in &messages {
        if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let (Some(id), Some(name)) = (
                        block.get("id").and_then(Value::as_str),
                        block.get("name").and_then(Value::as_str),
                    ) {
                        tool_names.insert(id.to_string(), name.to_string());
                    }
                }
            }
        }
    }
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "system" {
            push_system_parts(message.get("content").unwrap_or(&Value::Null), &mut system_parts);
            continue;
        }
        let gemini_role = if role == "assistant" { "model" } else { "user" };
        let parts = content_to_parts(message.get("content").unwrap_or(&Value::Null), &tool_names);
        if parts.is_empty() {
            continue;
        }
        contents.push(json!({ "role": gemini_role, "parts": parts }));
    }

    if contents.is_empty() {
        return Err("messages 不能为空".into());
    }

    let mut request = json!({ "contents": contents });
    if !system_parts.is_empty() {
        request["systemInstruction"] = json!({
            "role": "user",
            "parts": system_parts,
        });
    }

    let mut generation = json!({});
    if let Some(max_tokens) = body.get("max_tokens").and_then(Value::as_u64) {
        generation["maxOutputTokens"] = json!(max_tokens);
    }
    if let Some(temperature) = body.get("temperature").and_then(Value::as_f64) {
        generation["temperature"] = json!(temperature);
    }
    if let Some(top_p) = body.get("top_p").and_then(Value::as_f64) {
        generation["topP"] = json!(top_p);
    }
    if let Some(level) = claude_thinking_level {
        generation["thinkingConfig"] = json!({ "thinkingLevel": level });
    }
    if !generation.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        request["generationConfig"] = generation;
    }

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let declarations: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name").and_then(Value::as_str)?;
                let description = tool.get("description").cloned().unwrap_or(json!(""));
                let parameters = tool
                    .get("input_schema")
                    .map(|schema| sanitize_schema(schema, 0))
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                Some(json!({
                    "name": name,
                    "description": description,
                    "parameters": parameters,
                }))
            })
            .collect();
        if !declarations.is_empty() {
            request["tools"] = json!([{ "functionDeclarations": declarations }]);
            // AUTO matches Cloud Code / Antigravity clients; VALIDATED rejects many
            // Desktop tool schemas and can fail streamGenerateContent.
            request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "AUTO" }
            });
        }
    }

    Ok(GeminiRequestParts {
        model,
        request,
        stream,
    })
}

/// Whitelist-based recursive cleaner for Anthropic `input_schema` → Gemini
/// function `parameters`. Gemini's Schema proto rejects JSON Schema fields it
/// doesn't know (`$schema`, `additionalProperties`, `propertyNames`, `format`,
/// `default`, validation keywords, ...) with a 400, so we keep only the subset
/// Cloud Code accepts and normalize unions / type arrays.
pub(super) fn sanitize_schema(schema: &Value, depth: usize) -> Value {
    const MAX_DEPTH: usize = 10;
    const ALLOWED_KEYS: [&str; 8] = [
        "type",
        "description",
        "properties",
        "required",
        "items",
        "enum",
        "title",
        "nullable",
    ];
    let Value::Object(source) = schema else {
        // Gemini requires every schema node to be an object; degrade bare
        // booleans / nulls (legal in JSON Schema) to a generic object.
        return empty_object_schema();
    };
    if depth > MAX_DEPTH {
        return empty_object_schema();
    }

    let mut map = source.clone();

    // Resolve union keywords: allOf merges every branch; anyOf/oneOf pick the
    // richest non-null branch. Branches are cleaned when merged below.
    if let Some(Value::Array(branches)) = map.remove("allOf") {
        for branch in branches {
            if let Value::Object(branch) = branch {
                for (k, v) in branch {
                    map.entry(k).or_insert(v);
                }
            }
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(branches)) = map.remove(key) {
            let mut best: Option<serde_json::Map<String, Value>> = None;
            for branch in branches {
                if let Value::Object(branch) = branch {
                    if branch.get("type").and_then(Value::as_str) == Some("null") {
                        continue;
                    }
                    if best.as_ref().map_or(true, |current| branch.len() > current.len()) {
                        best = Some(branch);
                    }
                }
            }
            if let Some(branch) = best {
                for (k, v) in branch {
                    map.entry(k).or_insert(v);
                }
            }
        }
    }

    // Normalize `type`: ["string", "null"] → "string".
    match map.get("type") {
        Some(Value::Array(options)) => {
            let chosen = options
                .iter()
                .filter_map(Value::as_str)
                .find(|t| *t != "null")
                .unwrap_or("string")
                .to_string();
            map.insert("type".into(), json!(chosen));
        }
        Some(Value::String(_)) => {}
        Some(_) => {
            map.remove("type");
        }
        None => {}
    }

    if let Some(Value::Object(props)) = map.get_mut("properties") {
        let keys: Vec<String> = props.keys().cloned().collect();
        for key in keys {
            if let Some(value) = props.remove(&key) {
                if value.is_object() {
                    props.insert(key, sanitize_schema(&value, depth + 1));
                }
            }
        }
    }

    match map.get("items") {
        Some(items) if items.is_object() => {
            let items = items.clone();
            map.insert("items".into(), sanitize_schema(&items, depth + 1));
        }
        Some(_) => {
            map.remove("items");
        }
        None => {}
    }

    // Only keep fields Gemini understands (also drops $schema, propertyNames,
    // additionalProperties, format, default, min*/max*, pattern, ...).
    map.retain(|key, _| ALLOWED_KEYS.contains(&key.as_str()));

    // `required` must reference surviving properties.
    let kept_required: Option<Vec<Value>> = match (map.get("required"), map.get("properties")) {
        (Some(Value::Array(required)), Some(Value::Object(props))) => Some(
            required
                .iter()
                .filter(|name| {
                    name.as_str()
                        .map(|s| props.contains_key(s))
                        .unwrap_or(false)
                })
                .cloned()
                .collect(),
        ),
        _ => None,
    };
    match kept_required {
        Some(kept) if kept.is_empty() => {
            map.remove("required");
        }
        Some(kept) => {
            map.insert("required".into(), Value::Array(kept));
        }
        None if map.contains_key("required") && !map.contains_key("properties") => {
            map.remove("required");
        }
        None => {}
    }

    if map.get("type").and_then(Value::as_str) == Some("object") && !map.contains_key("properties")
    {
        map.insert("properties".into(), json!({}));
    }
    if map.get("description").map(|d| !d.is_string()).unwrap_or(false) {
        map.remove("description");
    }

    Value::Object(map)
}

fn empty_object_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// Reasoning effort from whichever Anthropic wire shape the client used:
/// GA `output_config.effort` → beta top-level `effort` → `thinking.effort`.
/// Accepts string values and a few nested object shapes Desktop may send.
fn extract_effort(body: &Value) -> Option<String> {
    body.get("output_config")
        .and_then(|config| config.get("effort"))
        .and_then(effort_value_to_string)
        .or_else(|| body.get("effort").and_then(effort_value_to_string))
        .or_else(|| {
            body.get("thinking")
                .and_then(|thinking| thinking.get("effort"))
                .and_then(effort_value_to_string)
        })
}

fn effort_value_to_string(value: &Value) -> Option<String> {
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

/// Compact diagnostic for usage logs: effort / thinking / mapped model only.
pub fn effort_mapping_diagnostic(body: &Value, mapped_model: &str) -> String {
    let effort = extract_effort(body).unwrap_or_else(|| "none".into());
    let thinking = body
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("none");
    format!("effort={effort} thinking={thinking} mapped={mapped_model}")
}

fn push_system_parts(system: &Value, out: &mut Vec<Value>) {
    match system {
        Value::String(text) if !text.is_empty() => {
            out.push(json!({ "text": text }));
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = item.as_str() {
                    if !text.is_empty() {
                        out.push(json!({ "text": text }));
                    }
                } else if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            out.push(json!({ "text": text }));
                        }
                    }
                }
            }
        }
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        out.push(json!({ "text": text }));
                    }
                }
            }
        }
        _ => {}
    }
}

pub(super) fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                if let Some(text) = block.as_str() {
                    Some(text.to_string())
                } else if block.get("type").and_then(Value::as_str) == Some("text") {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn content_to_parts(
    content: &Value,
    tool_names: &std::collections::HashMap<String, String>,
) -> Vec<Value> {
    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "text": text })]
            }
        }
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                parts.push(json!({ "text": text }));
                            }
                        }
                    }
                    "tool_use" => {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                        let id = block.get("id").and_then(Value::as_str);
                        let args = block.get("input").cloned().unwrap_or(json!({}));
                        let mut fc = json!({ "name": name, "args": args });
                        if let Some(id) = id {
                            fc["id"] = json!(id);
                        }
                        parts.push(json!({ "functionCall": fc }));
                    }
                    "tool_result" => {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let name = tool_names
                            .get(tool_use_id)
                            .cloned()
                            .unwrap_or_else(|| {
                                if tool_use_id.is_empty() {
                                    "tool".to_string()
                                } else {
                                    tool_use_id.to_string()
                                }
                            });
                        let result = tool_result_text(block.get("content").unwrap_or(&Value::Null));
                        let mut function_response =
                            json!({ "name": name, "response": { "result": result } });
                        // Cloud Code converts back to Anthropic format for Claude
                        // models; without `id` it emits tool_result without
                        // tool_use_id and upstream 400s ("Field required").
                        if !tool_use_id.is_empty() {
                            function_response["id"] = json!(tool_use_id);
                        }
                        parts.push(json!({ "functionResponse": function_response }));
                    }
                    "image" => {
                        if let Some(source) = block.get("source") {
                            let data = source.get("data").and_then(Value::as_str).unwrap_or("");
                            let mime = source
                                .get("media_type")
                                .and_then(Value::as_str)
                                .unwrap_or("image/png");
                            if !data.is_empty() {
                                parts.push(json!({
                                    "inlineData": { "mimeType": mime, "data": data }
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
            parts
        }
        _ => Vec::new(),
    }
}

pub fn gemini_to_anthropic_response(model: &str, gemini: &Value) -> Value {
    let (text, tool_uses, stop_reason) = extract_assistant(gemini);
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({ "type": "text", "text": text }));
    }
    content.extend(tool_uses);
    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "" }));
    }
    let usage = usage_from_gemini(gemini);
    let stop_reason = stop_reason.unwrap_or_else(|| {
        if content.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        {
            "tool_use".into()
        } else {
            "end_turn".into()
        }
    });
    json!({
        "id": format!("msg_{}", Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage,
    })
}

/// Convert one Gemini stream chunk into Anthropic SSE events.
///
/// Only emits `message_delta` / `message_stop` when the chunk carries a real
/// `finishReason`. Intermediate tokens must not close the stream — doing so
/// breaks Claude Desktop (premature stop → client abort → apparent 502).
pub fn gemini_to_anthropic_sse_chunk(model: &str, gemini_chunk: &Value) -> Vec<Value> {
    let _ = model;
    let (text, tool_uses, stop_reason) = extract_assistant(gemini_chunk);
    let mut events = Vec::new();
    if !text.is_empty() {
        events.push(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }));
    }
    if !tool_uses.is_empty() {
        // Anthropic requires closing the text block before tool_use blocks.
        events.push(json!({
            "type": "content_block_stop",
            "index": 0
        }));
    }
    for (offset, tool) in tool_uses.iter().enumerate() {
        let index = offset + 1;
        let id = tool.get("id").cloned().unwrap_or(json!(format!("toolu_{index}")));
        let name = tool.get("name").cloned().unwrap_or(json!("tool"));
        let input = tool.get("input").cloned().unwrap_or(json!({}));
        events.push(json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": {}
            }
        }));
        if let Ok(serialized) = serde_json::to_string(&input) {
            if serialized != "{}" {
                events.push(json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "input_json_delta", "partial_json": serialized }
                }));
            }
        }
        events.push(json!({
            "type": "content_block_stop",
            "index": index
        }));
    }
    if let Some(stop_reason) = stop_reason {
        let usage = usage_from_gemini(gemini_chunk);
        if tool_uses.is_empty() {
            events.push(json!({
                "type": "content_block_stop",
                "index": 0
            }));
        }
        events.push(json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
            "usage": { "output_tokens": usage.get("output_tokens").cloned().unwrap_or(json!(0)) }
        }));
        events.push(json!({ "type": "message_stop" }));
    }
    events
}

pub fn anthropic_sse_message_start(model: &str) -> Value {
    json!({
        "type": "message_start",
        "message": {
            "id": format!("msg_{}", Uuid::new_v4().simple()),
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": { "input_tokens": 0, "output_tokens": 0 }
        }
    })
}

pub fn anthropic_sse_content_block_start() -> Value {
    json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": { "type": "text", "text": "" }
    })
}

fn extract_assistant(gemini: &Value) -> (String, Vec<Value>, Option<String>) {
    let mut text = String::new();
    let mut tools = Vec::new();
    let mut stop_reason = None;
    let candidates = gemini
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(candidate) = candidates.first() {
        if let Some(parts) = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                // Skip model "thought" parts — not Anthropic wire content.
                if part.get("thought").and_then(Value::as_bool).unwrap_or(false) {
                    continue;
                }
                if let Some(chunk) = part.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
                if let Some(fc) = part.get("functionCall") {
                    let id = fc
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("toolu_{}", Uuid::new_v4().simple()));
                    tools.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": fc.get("name").cloned().unwrap_or(json!("tool")),
                        "input": fc.get("args").cloned().unwrap_or(json!({})),
                    }));
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            if !reason.is_empty() && reason != "FINISH_REASON_UNSPECIFIED" {
                stop_reason = Some(match reason {
                    "MAX_TOKENS" => "max_tokens".into(),
                    "STOP" => {
                        if tools.is_empty() {
                            "end_turn".into()
                        } else {
                            "tool_use".into()
                        }
                    }
                    other => other.to_ascii_lowercase(),
                });
            }
        }
    }
    (text, tools, stop_reason)
}

fn usage_from_gemini(gemini: &Value) -> Value {
    let meta = gemini.get("usageMetadata").cloned().unwrap_or(json!({}));
    json!({
        "input_tokens": meta.get("promptTokenCount").and_then(Value::as_u64).unwrap_or(0),
        "output_tokens": meta.get("candidatesTokenCount").and_then(Value::as_u64).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_messages() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert_eq!(parts.model, "claude-sonnet-4-6");
        assert_eq!(parts.request["contents"][0]["role"], "user");
    }

    #[test]
    fn sse_chunk_without_finish_reason_does_not_stop_message() {
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "Hello" }] }
            }]
        });
        let events = gemini_to_anthropic_sse_chunk("gemini-3.6-flash-high", &chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "content_block_delta");
        assert!(events.iter().all(|event| event["type"] != "message_stop"));
    }

    #[test]
    fn sse_chunk_with_stop_emits_message_stop_once() {
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "Hi" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "candidatesTokenCount": 1 }
        });
        let events = gemini_to_anthropic_sse_chunk("gemini-3.6-flash-high", &chunk);
        let stops = events
            .iter()
            .filter(|event| event["type"] == "message_stop")
            .count();
        assert_eq!(stops, 1);
        assert!(events.iter().any(|event| event["type"] == "content_block_stop"));
    }

    #[test]
    fn tool_result_keeps_tool_use_id_and_real_name() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "read Cargo.toml" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "Cargo.toml" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": [
                        { "type": "text", "text": "[package]" }
                    ]}
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        let response = &parts.request["contents"][2]["parts"][0]["functionResponse"];
        assert_eq!(response["id"], json!("toolu_1"));
        assert_eq!(response["name"], json!("read_file"));
        assert_eq!(response["response"]["result"], json!("[package]"));
        let call = &parts.request["contents"][1]["parts"][0]["functionCall"];
        assert_eq!(call["id"], json!("toolu_1"));
    }

    #[test]
    fn tool_parameters_are_sanitized_for_gemini() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "name": "read_file",
                "description": "Read a file",
                "input_schema": {
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "additionalProperties": false,
                    "propertyNames": { "pattern": "^[a-z_]+$" },
                    "properties": {
                        "path": { "type": "string", "format": "uri", "default": "." },
                        "mode": { "type": ["string", "null"], "enum": ["a", "b"] },
                        "options": {
                            "anyOf": [{ "type": "null" }, { "type": "object", "properties": { "verbose": { "type": "boolean", "minimum": 1 } } }]
                        }
                    },
                    "required": ["path", "missing_prop"]
                }
            }]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        let params = &parts.request["tools"][0]["functionDeclarations"][0]["parameters"];
        assert!(params.get("$schema").is_none());
        assert!(params.get("additionalProperties").is_none());
        assert!(params.get("propertyNames").is_none());
        assert_eq!(params["properties"]["path"].get("format"), None);
        assert_eq!(params["properties"]["path"].get("default"), None);
        assert_eq!(params["properties"]["mode"]["type"], json!("string"));
        assert_eq!(params["properties"]["options"]["type"], json!("object"));
        assert!(params["properties"]["options"]["properties"]["verbose"]
            .get("minimum")
            .is_none());
        assert_eq!(params["required"], json!(["path"]));
    }

    #[test]
    fn effort_drives_gemini_suffix_for_desktop_alias() {
        let body = json!({
            "model": "claude-sonnet-5",
            "max_tokens": 128,
            "output_config": { "effort": "high" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert!(parts.model.contains("flash"));
        assert!(
            parts.model.ends_with("-high") || parts.model.contains("-high"),
            "expected high suffix, got {}",
            parts.model
        );
        // Anthropic-only fields never leak into the Gemini request body.
        assert!(parts.request.get("output_config").is_none());
        assert!(parts.request.get("thinking").is_none());
        assert!(parts.request.get("effort").is_none());
    }

    #[test]
    fn alias_without_effort_defaults_to_high() {
        let body = json!({
            "model": "claude-sonnet-5",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert!(
            parts.model.ends_with("-high") || parts.model.contains("-high"),
            "alias without effort should default high, got {}",
            parts.model
        );
        let diag = effort_mapping_diagnostic(&body, &parts.model);
        assert!(diag.contains("effort=none"));
        assert!(diag.contains("mapped="));
    }

    #[test]
    fn nested_effort_object_is_parsed() {
        let body = json!({
            "model": "claude-sonnet-5",
            "max_tokens": 128,
            "output_config": { "effort": { "type": "low" } },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert!(
            parts.model.ends_with("-low") || parts.model.contains("-low"),
            "expected low suffix from nested effort object, got {}",
            parts.model
        );
    }

    #[test]
    fn thinking_disabled_forces_low_for_gemini() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "thinking": { "type": "disabled" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert!(parts.request.get("thinking").is_none());
    }

    #[test]
    fn effort_becomes_thinking_level_for_claude() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "output_config": { "effort": "medium" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert_eq!(parts.model, "claude-sonnet-4-6");
        assert_eq!(
            parts.request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            json!("medium")
        );
    }

    #[test]
    fn adaptive_thinking_defaults_to_high_for_claude() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "thinking": { "type": "adaptive" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body).unwrap();
        assert_eq!(
            parts.request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            json!("high")
        );
    }
}
