//! Anthropic Messages ↔ Gemini request/response mapping.

use serde_json::{json, Value};
use uuid::Uuid;

use super::args_fix::{correct_tool_args, param_keys_from_declarations, ToolParamKeys};
use super::models::{map_effort_to_suffix, map_model_id};
use crate::antigravity::model_catalog;
use crate::antigravity::thought_sig;
use crate::antigravity::usage_log::GeminiUsage;

pub struct GeminiRequestParts {
    pub model: String,
    pub request: Value,
    pub stream: bool,
    /// 本次请求的工具声明参数键名（工具名 → 已声明参数），响应侧用于
    /// 纠偏模型拼错的 args key（如 `-n` 被归一成 `n`）。
    pub tool_params: ToolParamKeys,
    /// Explicit effort/suffix/`thinking.disabled` to remember for the session.
    pub remember_effort: Option<&'static str>,
    /// 客户端开启了 thinking（enabled/adaptive 或显式 effort）→ 请求
    /// includeThoughts 并在响应侧透传 thinking 块。分类器等内部调用
    /// （thinking disabled、无 effort）必须为 false：它们解析不了 thinking
    /// 块，会误判模型不可用。
    pub thoughts_allowed: bool,
}

pub fn anthropic_to_gemini_request(
    body: &Value,
    sticky_effort: Option<&str>,
    session_key: Option<&str>,
) -> Result<GeminiRequestParts, String> {
    let requested_model = body.get("model").and_then(Value::as_str).unwrap_or("");
    let requested_explicit = model_catalog::explicit_level_suffix(requested_model);
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
    let mut remember_effort: Option<&'static str> = None;
    let lower_model = model.to_ascii_lowercase();
    // Gemini API 默认不返回 thought 文本，必须显式 includeThoughts:true
    // （对照 Antigravity-Manager claude/gemini mapper），否则客户端永远
    // 看不到思考过程。但仅当客户端自己开了 thinking 时才请求——分类器等
    // thinking=disabled 的内部调用收到 thinking 块会解析失败。
    let thoughts_allowed = matches!(thinking_kind.as_deref(), Some("enabled") | Some("adaptive"))
        || effort.is_some();
    let gemini_target = lower_model.starts_with("gemini-");
    if gemini_target {
        if let Some(level) = effort.as_deref().and_then(map_effort_to_suffix) {
            model = model_catalog::with_forced_level(&model, level);
            remember_effort = Some(level);
        } else if thinking_kind.as_deref() == Some("disabled") {
            // Claude Code Explore / Haiku / classifiers send thinking=disabled.
            // Only compose `-low` onto a *bare* Gemini id. An explicit suffix
            // (catalog default or CLAUDE_CODE_SUBAGENT_MODEL) must stay put.
            // Never sticky this onto the parent session — Code reuses the same
            // x-claude-session-id for the main agent, which would pin later
            // `gemini-3.6-flash` turns to `-low`.
            if requested_explicit.is_none() {
                model = model_catalog::with_forced_level(&model, "low");
            }
        } else if let Some(level) = requested_explicit {
            // Client already picked a suffixed id — pass through (map_model_id
            // kept it) and remember for bare follow-up turns in the session.
            remember_effort = Some(level);
        } else {
            // Bare Gemini, no effort: reuse session sticky, else default high
            // (Desktop side requests often omit effort after the user set high).
            let level = sticky_effort
                .and_then(map_effort_to_suffix)
                .unwrap_or("high");
            model = model_catalog::with_forced_level(&model, level);
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
        let parts = content_to_parts(
            message.get("content").unwrap_or(&Value::Null),
            &tool_names,
            gemini_target,
            session_key,
            Some(contents.len()),
        );
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
    if gemini_target && thoughts_allowed {
        generation["thinkingConfig"] = json!({ "includeThoughts": true });
    }
    if let Some(level) = claude_thinking_level {
        generation["thinkingConfig"]["thinkingLevel"] = json!(level);
        if thoughts_allowed {
            generation["thinkingConfig"]["includeThoughts"] = json!(true);
        }
    }
    if gemini_target {
        if let Some(budget) = crate::antigravity::thinking::resolve_thinking_budget(body, &model) {
            let max = generation.get("maxOutputTokens").and_then(Value::as_u64);
            generation["maxOutputTokens"] =
                json!(crate::antigravity::thinking::pad_max_tokens(max, budget));
            crate::antigravity::thinking::apply_thinking_budget(
                &mut generation,
                budget,
                thoughts_allowed,
            );
        }
    }
    if !generation.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        request["generationConfig"] = generation;
    }

    let mut tool_params = ToolParamKeys::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let has_search = tools.iter().any(is_google_search_tool);
        let declarations: Vec<Value> = tools
            .iter()
            .filter(|tool| !is_google_search_tool(tool))
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
        if has_search && !declarations.is_empty() {
            log::warn!(
                "Antigravity dropping Google Search server tool; functionDeclarations cannot mix with it"
            );
        }
        if !declarations.is_empty() {
            tool_params = param_keys_from_declarations(&declarations);
            request["tools"] = json!([{ "functionDeclarations": declarations }]);
            request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "AUTO" },
                "includeServerSideToolInvocations": true,
                "include_server_side_tool_invocations": true
            });
        }
    }

    Ok(GeminiRequestParts {
        model,
        request,
        stream,
        tool_params,
        remember_effort,
        thoughts_allowed,
    })
}

/// Whitelist-based recursive cleaner for Anthropic `input_schema` → Gemini
/// function `parameters`. Gemini's Schema proto rejects JSON Schema fields it
/// doesn't know (`$schema`, `additionalProperties`, `propertyNames`, `format`,
/// `default`, validation keywords, ...) with a 400, so we keep only the subset
/// Cloud Code accepts and normalize unions / type arrays.
pub(super) fn sanitize_schema(schema: &Value, depth: usize) -> Value {
    let defs = collect_schema_defs(schema);
    sanitize_schema_inner(schema, &defs, depth)
}

fn collect_schema_defs(schema: &Value) -> std::collections::HashMap<String, Value> {
    let mut defs = std::collections::HashMap::new();
    let Value::Object(map) = schema else {
        return defs;
    };
    for key in ["$defs", "definitions"] {
        if let Some(Value::Object(entries)) = map.get(key) {
            for (name, value) in entries {
                defs.insert(name.clone(), value.clone());
            }
        }
    }
    defs
}

fn resolve_schema_ref(
    schema: &Value,
    defs: &std::collections::HashMap<String, Value>,
) -> Value {
    let Some(pointer) = schema.get("$ref").and_then(Value::as_str) else {
        return schema.clone();
    };
    let name = pointer.rsplit('/').next().unwrap_or(pointer);
    let mut resolved = defs.get(name).cloned().unwrap_or_else(|| json!({}));
    if let (Value::Object(base), Value::Object(extra)) = (&mut resolved, schema.clone()) {
        for (key, value) in extra {
            if key != "$ref" {
                base.entry(key).or_insert(value);
            }
        }
    }
    resolved
}

fn sanitize_schema_inner(
    schema: &Value,
    defs: &std::collections::HashMap<String, Value>,
    depth: usize,
) -> Value {
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
    let schema = if schema.get("$ref").is_some() {
        resolve_schema_ref(schema, defs)
    } else {
        schema.clone()
    };
    let Value::Object(source) = schema else {
        return empty_object_schema();
    };
    if depth > MAX_DEPTH {
        return empty_object_schema();
    }

    let mut map = source;

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
                let branch = if branch.get("$ref").is_some() {
                    resolve_schema_ref(&branch, defs)
                } else {
                    branch
                };
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
                    props.insert(key, sanitize_schema_inner(&value, defs, depth + 1));
                }
            }
        }
    }

    match map.get("items") {
        Some(items) if items.is_object() => {
            let items = items.clone();
            map.insert(
                "items".into(),
                sanitize_schema_inner(&items, defs, depth + 1),
            );
        }
        Some(_) => {
            map.remove("items");
        }
        None => {}
    }

    map.retain(|key, _| ALLOWED_KEYS.contains(&key.as_str()));

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

fn is_google_search_tool(tool: &Value) -> bool {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            tool.get("google_search")
                .and_then(|_| Some("google_search"))
        })
        .unwrap_or("");
    let lower = name.to_ascii_lowercase();
    lower == "google_search" || lower == "googlesearch" || tool.get("googleSearch").is_some()
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
    gemini_target: bool,
    session_key: Option<&str>,
    message_index: Option<usize>,
) -> Vec<Value> {
    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                super::history_media::parts_from_text(text)
            }
        }
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                match block_type {
                    "thinking" | "redacted_thinking" => {
                        if let (Some(session), Some(sig)) = (
                            session_key,
                            block.get("signature").and_then(Value::as_str),
                        ) {
                            thought_sig::cache_session_signature(session, sig);
                            if let Some(index) = message_index {
                                thought_sig::cache_session_index_signature(session, index, sig);
                            }
                        }
                    }
                    "text" => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                parts.extend(super::history_media::parts_from_text(text));
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
                        let mut part = json!({ "functionCall": fc });
                        if gemini_target {
                            let signature = thought_sig::resolve_function_call_signature(
                                id,
                                session_key,
                                message_index,
                            );
                            let signature = json!(signature);
                            part["thoughtSignature"] = signature.clone();
                            part["thought_signature"] = signature;
                        }
                        parts.push(part);
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

pub fn gemini_to_anthropic_response(
    model: &str,
    gemini: &Value,
    session_key: Option<&str>,
    thoughts_allowed: bool,
    tool_params: &ToolParamKeys,
) -> Value {
    let assistant = extract_assistant(gemini, session_key, thoughts_allowed, tool_params);
    let mut content = Vec::new();
    // thinking blocks 必须在 text/tool_use 之前（Anthropic 协议要求）。
    if !assistant.thought_text.is_empty() {
        let mut block = json!({ "type": "thinking", "thinking": assistant.thought_text });
        if let Some(sig) = &assistant.thought_signature {
            block["signature"] = json!(sig);
        }
        content.push(block);
    }
    if !assistant.text.is_empty() {
        content.push(json!({
            "type": "text",
            "text": super::latex::unwrap_gemini_latex(&assistant.text)
        }));
    }
    content.extend(assistant.tools.iter().cloned());
    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "" }));
    }
    let usage = GeminiUsage::parse(gemini).anthropic_usage();
    let stop_reason = assistant.stop_reason.unwrap_or_else(|| {
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

/// Anthropic SSE 输出的跨 chunk 状态：内容块按到达顺序开/关
/// （thinking → text → tool_use），thinking 块必须先于其它块。
#[derive(Default)]
pub struct AnthropicStreamState {
    started: bool,
    closed: bool,
    open_block: Option<(BlockKind, usize)>,
    next_index: usize,
    usage: GeminiUsage,
    text_carry: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Thinking,
    Text,
}

impl AnthropicStreamState {
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 上游流结束（或无 finishReason 被截断）时的收尾事件：关掉未闭合的块，
    /// 补 message_delta + message_stop。幂等（closed 后返回空）。
    pub fn finish_events(&mut self, model: &str) -> Vec<Value> {
        if self.closed {
            return Vec::new();
        }
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(anthropic_sse_message_start(model, self.usage));
        }
        self.flush_text_carry(&mut events, true);
        if let Some((_, index)) = self.open_block.take() {
            events.push(json!({ "type": "content_block_stop", "index": index }));
        } else {
            // 什么内容都没来过：补一个空 text 块，保持协议完整。
            events.push(json!({
                "type": "content_block_start",
                "index": self.next_index,
                "content_block": { "type": "text", "text": "" }
            }));
            events.push(json!({ "type": "content_block_stop", "index": self.next_index }));
        }
        events.push(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": self.usage.anthropic_usage()
        }));
        events.push(json!({ "type": "message_stop" }));
        self.closed = true;
        events
    }

    fn close_open_block(&mut self, events: &mut Vec<Value>) {
        self.flush_text_carry(events, true);
        if let Some((_, index)) = self.open_block.take() {
            events.push(json!({ "type": "content_block_stop", "index": index }));
        }
    }

    fn push_visible_text(&mut self, chunk: &str, events: &mut Vec<Value>) {
        if chunk.is_empty() {
            return;
        }
        let _ = self.open_block_if_needed(BlockKind::Text, events);
        self.text_carry.push_str(chunk);
        self.flush_text_carry(events, false);
    }

    fn flush_text_carry(&mut self, events: &mut Vec<Value>, force: bool) {
        if self.text_carry.is_empty() {
            return;
        }
        let Some((BlockKind::Text, index)) = self.open_block else {
            if force {
                self.text_carry.clear();
            }
            return;
        };
        let emit = if force {
            let out = super::latex::unwrap_gemini_latex(&self.text_carry);
            self.text_carry.clear();
            out
        } else {
            let (safe, rest) = super::latex::split_safe_latex_prefix(&self.text_carry);
            self.text_carry = rest;
            super::latex::unwrap_gemini_latex(&safe)
        };
        if emit.is_empty() {
            return;
        }
        events.push(json!({
            "type": "content_block_delta",
            "index": index,
            "delta": { "type": "text_delta", "text": emit }
        }));
    }

    fn open_block_if_needed(&mut self, kind: BlockKind, events: &mut Vec<Value>) -> usize {
        if let Some((current, index)) = self.open_block {
            if current == kind {
                return index;
            }
        }
        self.close_open_block(events);
        let index = self.next_index;
        self.next_index += 1;
        let content_block = match kind {
            BlockKind::Thinking => json!({ "type": "thinking", "thinking": "" }),
            BlockKind::Text => json!({ "type": "text", "text": "" }),
        };
        events.push(json!({
            "type": "content_block_start",
            "index": index,
            "content_block": content_block
        }));
        self.open_block = Some((kind, index));
        index
    }
}

/// Convert one Gemini stream chunk into Anthropic SSE events.
///
/// Only emits `message_delta` / `message_stop` when the chunk carries a real
/// `finishReason`. Intermediate tokens must not close the stream — doing so
/// breaks Claude Desktop (premature stop → client abort → apparent 502).
///
/// thought parts 透传为 thinking blocks（Desktop 会折叠展示思考过程），
/// thoughtSignature 以 signature_delta 收尾 thinking 块。
pub fn gemini_to_anthropic_sse_chunk(
    state: &mut AnthropicStreamState,
    model: &str,
    gemini_chunk: &Value,
    session_key: Option<&str>,
    thoughts_allowed: bool,
    tool_params: &ToolParamKeys,
) -> Vec<Value> {
    let mut events = Vec::new();
    if state.closed {
        return events;
    }
    state.usage.merge_max(GeminiUsage::parse(gemini_chunk));
    if !state.started {
        state.started = true;
        events.push(anthropic_sse_message_start(model, state.usage));
    }
    let assistant = extract_assistant(gemini_chunk, session_key, thoughts_allowed, tool_params);

    if !assistant.thought_text.is_empty() {
        let index = state.open_block_if_needed(BlockKind::Thinking, &mut events);
        events.push(json!({
            "type": "content_block_delta",
            "index": index,
            "delta": { "type": "thinking_delta", "thinking": assistant.thought_text }
        }));
    }
    if let Some(sig) = assistant.thought_signature {
        // 签名到达即收尾 thinking 块（Gemini 在最后一个 thought part 上给签名）。
        if let Some((BlockKind::Thinking, index)) = state.open_block {
            events.push(json!({
                "type": "content_block_delta",
                "index": index,
                "delta": { "type": "signature_delta", "signature": sig }
            }));
            state.close_open_block(&mut events);
        }
    }
    if !assistant.text.is_empty() {
        state.push_visible_text(&assistant.text, &mut events);
    }
    if !assistant.tools.is_empty() {
        // Anthropic requires closing the previous block before tool_use blocks.
        state.close_open_block(&mut events);
    }
    for tool in &assistant.tools {
        let index = state.next_index;
        state.next_index += 1;
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
    if let Some(stop_reason) = assistant.stop_reason {
        state.close_open_block(&mut events);
        events.push(json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
            "usage": state.usage.anthropic_usage()
        }));
        events.push(json!({ "type": "message_stop" }));
        state.closed = true;
    }
    events
}

pub fn anthropic_sse_message_start(model: &str, usage: GeminiUsage) -> Value {
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
            "usage": usage.anthropic_usage()
        }
    })
}

struct AssistantContent {
    thought_text: String,
    thought_signature: Option<String>,
    text: String,
    tools: Vec<Value>,
    stop_reason: Option<String>,
}

fn extract_assistant(
    gemini: &Value,
    session_key: Option<&str>,
    thoughts_allowed: bool,
    tool_params: &ToolParamKeys,
) -> AssistantContent {
    let mut content = AssistantContent {
        thought_text: String::new(),
        thought_signature: None,
        text: String::new(),
        tools: Vec::new(),
        stop_reason: None,
    };
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
                // 捕获 Gemini 3 thought_signature：thinking part 上的记入会话
                // 缓存，functionCall part 上的同时按 tool_use_id 精确缓存，
                // 供下一轮请求回放历史时回注。
                let signature = part
                    .get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(Value::as_str);
                // thought parts：签名照常进缓存（供下轮回注），但思考文本
                // 只在客户端开了 thinking 时才透传为 thinking 块——分类器等
                // thinking=disabled 的调用解析不了 thinking 块。
                if part.get("thought").and_then(Value::as_bool).unwrap_or(false) {
                    if let Some(sig) = signature {
                        if let Some(session) = session_key {
                            thought_sig::cache_session_signature(session, sig);
                        }
                        if thoughts_allowed {
                            content.thought_signature = Some(sig.to_string());
                        }
                    }
                    if thoughts_allowed {
                        if let Some(chunk) = part.get("text").and_then(Value::as_str) {
                            content.thought_text.push_str(chunk);
                        }
                    }
                    continue;
                }
                if let Some(chunk) = part.get("text").and_then(Value::as_str) {
                    content.text.push_str(chunk);
                }
                if let Some(fc) = part.get("functionCall") {
                    let id = fc
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("toolu_{}", Uuid::new_v4().simple()));
                    if let Some(sig) = signature {
                        thought_sig::cache_tool_signature(&id, sig);
                        if let Some(session) = session_key {
                            thought_sig::cache_session_signature(session, sig);
                            thought_sig::cache_session_index_signature(
                                session,
                                content.tools.len(),
                                sig,
                            );
                        }
                    }
                    let name = fc.get("name").cloned().unwrap_or(json!("tool"));
                    let args = super::latex::unwrap_latex_in_tool_args(correct_tool_args(
                        name.as_str().unwrap_or("tool"),
                        fc.get("args").cloned().unwrap_or(json!({})),
                        tool_params,
                    ));
                    content.tools.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": args,
                    }));
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            if !reason.is_empty() && reason != "FINISH_REASON_UNSPECIFIED" {
                content.stop_reason = Some(match reason {
                    "MAX_TOKENS" => "max_tokens".into(),
                    "STOP" => {
                        if content.tools.is_empty() {
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
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：缓存是全局共享的，并行测试必须用互不相同的 id/session，勿 clear_all。

    fn no_params() -> ToolParamKeys {
        ToolParamKeys::new()
    }

    #[test]
    fn gemini_history_tool_use_gets_sentinel_signature_without_cache() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "run ls" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_sentinel_case", "name": "Bash", "input": { "command": "ls" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_sentinel_case", "content": "ok" }
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body, None, Some("sess-sentinel-case")).unwrap();
        let part = &parts.request["contents"][1]["parts"][0];
        assert_eq!(
            part["thoughtSignature"],
            json!(thought_sig::SKIP_VALIDATOR_SENTINEL)
        );
        assert_eq!(part["thoughtSignature"], part["thought_signature"]);
    }

    #[test]
    fn captured_response_signature_is_reinjected_by_tool_id() {
        // 响应侧：functionCall part 携带 thoughtSignature → 按 tool id 缓存。
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [
                    { "thought": true, "text": "thinking...", "thoughtSignature": "session-sig-long" },
                    { "functionCall": { "id": "toolu_capture_9", "name": "Bash", "args": {} },
                      "thoughtSignature": "tool-sig-9" }
                ] }
            }]
        });
        let mut state = AnthropicStreamState::default();
        let events = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.6-flash-high",
            &chunk,
            Some("sess-capture-9"),
            true,
            &no_params(),
        );
        assert!(events.iter().any(|e| e["type"] == "content_block_start"));
        assert_eq!(
            thought_sig::get_tool_signature("toolu_capture_9").as_deref(),
            Some("tool-sig-9")
        );
        assert_eq!(
            thought_sig::get_session_signature("sess-capture-9").as_deref(),
            Some("session-sig-long")
        );

        // 请求侧：客户端回放同一 tool_use id → 回注真实签名而非哨兵。
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "run ls" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_capture_9", "name": "Bash", "input": {} }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_capture_9", "content": "ok" }
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body, None, Some("sess-capture-9")).unwrap();
        let part = &parts.request["contents"][1]["parts"][0];
        assert_eq!(part["thoughtSignature"], json!("tool-sig-9"));
    }

    #[test]
    fn session_signature_is_fallback_for_unknown_tool_id() {
        thought_sig::cache_session_signature("sess-fallback-case", "session-fallback-sig");
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_unknown_fb", "name": "Bash", "input": {} }
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body, None, Some("sess-fallback-case")).unwrap();
        let part = &parts.request["contents"][1]["parts"][0];
        assert_eq!(part["thoughtSignature"], json!("session-fallback-sig"));
    }

    #[test]
    fn claude_target_history_tool_use_has_no_signature_injected() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_claude_case", "name": "Bash", "input": {} }
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body, None, Some("sess-claude-case")).unwrap();
        let part = &parts.request["contents"][1]["parts"][0];
        assert!(part.get("thoughtSignature").is_none());
        assert!(part.get("thought_signature").is_none());
    }

    #[test]
    fn gemini_target_requests_include_thoughts() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "thinking": { "type": "adaptive" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(parts.thoughts_allowed);
        assert_eq!(
            parts.request["generationConfig"]["thinkingConfig"]["includeThoughts"],
            json!(true)
        );
        // Gemini 目标的 level 仍只走模型 id 后缀，不写 thinkingLevel。
        assert!(parts.request["generationConfig"]["thinkingConfig"]
            .get("thinkingLevel")
            .is_none());
    }

    #[test]
    fn classifier_style_request_disables_thoughts() {
        // Desktop 分类器特征：thinking=disabled、无 effort → 不请求 includeThoughts，
        // 响应侧的 thought parts 也直接丢弃（客户端解析不了 thinking 块）。
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 64,
            "thinking": { "type": "disabled" },
            "messages": [{"role": "user", "content": "classify"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(!parts.thoughts_allowed);
        assert!(parts.request["generationConfig"].get("thinkingConfig").is_none()
            || parts.request["generationConfig"]["thinkingConfig"]
                .get("includeThoughts")
                .is_none());

        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "thought": true, "text": "内部推理", "thoughtSignature": "sig-cls-1" },
                    { "text": "allow" }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_anthropic_response(
            "gemini-3.6-flash-low",
            &gemini,
            Some("sess-cls-1"),
            false,
            &no_params(),
        );
        let content = response["content"].as_array().unwrap();
        // 无 thinking 块，纯文本；签名仍进会话缓存。
        assert!(content.iter().all(|b| b["type"] != "thinking"));
        assert_eq!(content[0]["text"], "allow");
        assert_eq!(
            thought_sig::get_session_signature("sess-cls-1").as_deref(),
            Some("sig-cls-1")
        );

        // 流式同理：thinking=disabled 时不产出 thinking_delta。
        let mut state = AnthropicStreamState::default();
        let chunk = json!({
            "candidates": [{ "content": { "parts": [
                { "thought": true, "text": "内部推理" }
            ] } }]
        });
        let events = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.6-flash-low",
            &chunk,
            None,
            false,
            &no_params(),
        );
        assert!(events.iter().all(|e| e["delta"]["type"] != "thinking_delta"));
    }

    #[test]
    fn tool_use_input_args_are_corrected_against_declarations() {
        // 请求侧声明了带连字符的 `-n`；上游把 args key 归一成 `n` → 响应侧改回。
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [{ "role": "user", "content": "grep it" }],
            "tools": [{
                "name": "Grep",
                "description": "search",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "-n": { "type": "boolean" }
                    }
                }
            }]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "functionCall": { "id": "toolu_fix_1", "name": "Grep",
                        "args": { "pattern": "antigravity", "n": true } } }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_anthropic_response(
            "gemini-3.6-flash",
            &gemini,
            None,
            false,
            &parts.tool_params,
        );
        let tool = &response["content"][0];
        assert_eq!(tool["type"], "tool_use");
        assert_eq!(tool["input"]["-n"], json!(true));
        assert!(tool["input"].get("n").is_none());
        assert_eq!(tool["input"]["pattern"], json!("antigravity"));
    }

    #[test]
    fn write_tool_contents_unwrap_katex_but_keep_edit_old_string() {
        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "functionCall": { "id": "toolu_write_1", "name": "Write",
                        "args": { "path": "spec.md", "contents": "重量 $\\le 50\\text{g}$" } } },
                    { "functionCall": { "id": "toolu_edit_1", "name": "Edit",
                        "args": {
                            "old_string": "重量 $\\le 50\\text{g}$",
                            "new_string": "重量 $\\le 50\\text{g}$"
                        } } }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_anthropic_response(
            "gemini-3.6-flash",
            &gemini,
            None,
            false,
            &no_params(),
        );
        assert_eq!(response["content"][0]["input"]["contents"], "重量 ≤ 50g");
        assert_eq!(
            response["content"][1]["input"]["old_string"],
            "重量 $\\le 50\\text{g}$"
        );
        assert_eq!(response["content"][1]["input"]["new_string"], "重量 ≤ 50g");
    }

    #[test]
    fn converts_basic_messages() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
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
        let mut state = AnthropicStreamState::default();
        let events = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk, None, true, &no_params());
        assert!(events.iter().any(|event| event["type"] == "message_start"));
        assert!(events
            .iter()
            .any(|event| event["delta"]["type"] == "text_delta"));
        assert!(events.iter().all(|event| event["type"] != "message_stop"));
        assert!(!state.is_closed());
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
        let mut state = AnthropicStreamState::default();
        let events = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk, None, true, &no_params());
        let stops = events
            .iter()
            .filter(|event| event["type"] == "message_stop")
            .count();
        assert_eq!(stops, 1);
        assert!(events.iter().any(|event| event["type"] == "content_block_stop"));
        assert!(state.is_closed());
        // closed 之后不再产出事件；finish_events 幂等。
        assert!(gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk, None, true, &no_params())
            .is_empty());
        assert!(state.finish_events("gemini-3.6-flash-high").is_empty());
    }

    #[test]
    fn sse_reports_prompt_and_thought_tokens_on_stop() {
        let mut state = AnthropicStreamState::default();
        let first = json!({
            "candidates": [{ "content": { "parts": [{ "text": "Hello" }] } }]
        });
        let start_events = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.7-flash-high",
            &first,
            None,
            true,
            &no_params(),
        );
        let start = start_events
            .iter()
            .find(|event| event["type"] == "message_start")
            .expect("message_start");
        assert_eq!(start["message"]["usage"]["input_tokens"], 0);
        assert_eq!(start["message"]["usage"]["output_tokens"], 0);

        let stop = json!({
            "candidates": [{
                "content": { "parts": [{ "text": " world" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 1500,
                "candidatesTokenCount": 20,
                "thoughtsTokenCount": 80
            }
        });
        let stop_events = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.7-flash-high",
            &stop,
            None,
            true,
            &no_params(),
        );
        let delta = stop_events
            .iter()
            .find(|event| event["type"] == "message_delta")
            .expect("message_delta");
        assert_eq!(delta["usage"]["input_tokens"], 1500);
        assert_eq!(delta["usage"]["output_tokens"], 20);
        assert!(state.is_closed());
    }

    #[test]
    fn non_stream_response_includes_thinking_block_with_signature() {
        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "thought": true, "text": "先看一下文件", "thoughtSignature": "sig-nonstream-1" },
                    { "text": "好的" }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_anthropic_response(
            "gemini-3.6-flash-high",
            &gemini,
            Some("sess-nonstream"),
            true,
            &no_params(),
        );
        let content = response["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[0]["thinking"], "先看一下文件");
        assert_eq!(content[0]["signature"], "sig-nonstream-1");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "好的");
    }

    #[test]
    fn non_stream_unwraps_visible_gemini_latex_but_not_thinking() {
        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "thought": true, "text": "算 $t_{WB}$" },
                    { "text": "加入 $10\\ \\mu\\text{s}$ 的 $t_{WB}$" }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_anthropic_response(
            "gemini-3.6-flash-high",
            &gemini,
            None,
            true,
            &no_params(),
        );
        let content = response["content"].as_array().unwrap();
        assert_eq!(content[0]["thinking"], "算 $t_{WB}$");
        assert_eq!(content[1]["text"], "加入 10 μs 的 t_WB");
    }

    #[test]
    fn sse_unwraps_gemini_latex_split_across_chunks() {
        let mut state = AnthropicStreamState::default();
        let chunk1 = json!({
            "candidates": [{ "content": { "parts": [{ "text": "延时 $10\\ \\mu" }] } }]
        });
        let e1 = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.6-flash-high",
            &chunk1,
            None,
            true,
            &no_params(),
        );
        let first: String = e1
            .iter()
            .filter(|event| event["delta"]["type"] == "text_delta")
            .filter_map(|event| event["delta"]["text"].as_str())
            .collect();
        assert_eq!(first, "延时 ");

        let chunk2 = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "\\text{s}$" }] },
                "finishReason": "STOP"
            }]
        });
        let e2 = gemini_to_anthropic_sse_chunk(
            &mut state,
            "gemini-3.6-flash-high",
            &chunk2,
            None,
            true,
            &no_params(),
        );
        let second: String = e2
            .iter()
            .filter(|event| event["delta"]["type"] == "text_delta")
            .filter_map(|event| event["delta"]["text"].as_str())
            .collect();
        assert_eq!(second, "10 μs");
    }

    #[test]
    fn streaming_thought_parts_become_thinking_block_then_text_block() {
        let mut state = AnthropicStreamState::default();
        // chunk 1：思考文本
        let chunk1 = json!({
            "candidates": [{ "content": { "parts": [
                { "thought": true, "text": "打算先读配置" }
            ] } }]
        });
        let e1 = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk1, None, true, &no_params());
        let start = e1
            .iter()
            .find(|e| e["type"] == "content_block_start")
            .unwrap();
        assert_eq!(start["index"], 0);
        assert_eq!(start["content_block"]["type"], "thinking");
        assert!(e1.iter().any(|e| e["delta"]["type"] == "thinking_delta"));

        // chunk 2：签名到达 → signature_delta + 关闭 thinking 块
        let chunk2 = json!({
            "candidates": [{ "content": { "parts": [
                { "thought": true, "thoughtSignature": "sig-stream-2" }
            ] } }]
        });
        let e2 = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk2, None, true, &no_params());
        assert!(e2.iter().any(|e| e["delta"]["type"] == "signature_delta"
            && e["delta"]["signature"] == "sig-stream-2"));
        assert!(e2
            .iter()
            .any(|e| e["type"] == "content_block_stop" && e["index"] == 0));

        // chunk 3：正文 → 新的 text 块（index 1），不重复开 thinking
        let chunk3 = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "结论" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "candidatesTokenCount": 2 }
        });
        let e3 = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk3, None, true, &no_params());
        let text_start = e3
            .iter()
            .find(|e| e["type"] == "content_block_start")
            .unwrap();
        assert_eq!(text_start["index"], 1);
        assert_eq!(text_start["content_block"]["type"], "text");
        assert!(e3
            .iter()
            .any(|e| e["type"] == "content_block_stop" && e["index"] == 1));
        assert_eq!(
            e3.iter().filter(|e| e["type"] == "message_stop").count(),
            1
        );
    }

    #[test]
    fn finish_events_closes_dangling_thinking_block() {
        let mut state = AnthropicStreamState::default();
        let chunk = json!({
            "candidates": [{ "content": { "parts": [
                { "thought": true, "text": "半截思考" }
            ] } }]
        });
        let _ = gemini_to_anthropic_sse_chunk(&mut state, "gemini-3.6-flash-high", &chunk, None, true, &no_params());
        let events = state.finish_events("gemini-3.6-flash-high");
        assert!(events
            .iter()
            .any(|e| e["type"] == "content_block_stop" && e["index"] == 0));
        assert_eq!(
            events.iter().filter(|e| e["type"] == "message_stop").count(),
            1
        );
        assert!(state.is_closed());
    }

    #[test]
    fn client_thinking_block_signature_feeds_session_cache() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "thinking", "thinking": "之前的思考", "signature": "client-echo-sig-1" },
                    { "type": "tool_use", "id": "toolu_echo_case", "name": "Bash", "input": {} }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_echo_case", "content": "ok" }
                ]}
            ]
        });
        let parts = anthropic_to_gemini_request(&body, None, Some("sess-echo-case")).unwrap();
        // thinking 块不产生 Gemini part，但签名进了会话缓存并回注到 functionCall。
        let model_msg = &parts.request["contents"][1];
        assert_eq!(model_msg["parts"].as_array().unwrap().len(), 1);
        assert_eq!(
            thought_sig::get_session_signature("sess-echo-case").as_deref(),
            Some("client-echo-sig-1")
        );
        assert_eq!(
            model_msg["parts"][0]["thoughtSignature"],
            json!("client-echo-sig-1")
        );
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
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
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
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
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
    fn effort_drives_gemini_suffix() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "output_config": { "effort": "high" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert!(
            parts.model.ends_with("-high") || parts.model.contains("-high"),
            "expected high suffix, got {}",
            parts.model
        );
        assert_eq!(parts.remember_effort, Some("high"));
        // Anthropic-only fields never leak into the Gemini request body.
        assert!(parts.request.get("output_config").is_none());
        assert!(parts.request.get("thinking").is_none());
        assert!(parts.request.get("effort").is_none());
    }

    #[test]
    fn bare_gemini_without_effort_defaults_to_high() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert!(
            parts.model.ends_with("-high") || parts.model.contains("-high"),
            "bare gemini without effort should default high, got {}",
            parts.model
        );
        assert_eq!(parts.remember_effort, None);
        let diag = effort_mapping_diagnostic(&body, &parts.model);
        assert!(diag.contains("effort=none"));
        assert!(diag.contains("mapped="));
    }

    #[test]
    fn sticky_effort_reused_when_request_omits_effort() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "side"}]
        });
        let parts = anthropic_to_gemini_request(&body, Some("high"), None).unwrap();
        assert!(
            parts.model.ends_with("-high") || parts.model.contains("-high"),
            "sticky high should apply, got {}",
            parts.model
        );
        assert_eq!(parts.remember_effort, None);
    }

    #[test]
    fn explicit_suffix_beats_sticky_effort() {
        let body = json!({
            "model": "gemini-3.6-flash-low",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, Some("high"), None).unwrap();
        assert!(
            parts.model.ends_with("-low"),
            "explicit -low should win over sticky high, got {}",
            parts.model
        );
        assert_eq!(parts.remember_effort, Some("low"));
    }

    #[test]
    fn nested_effort_object_is_parsed() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "output_config": { "effort": { "type": "low" } },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert_eq!(parts.remember_effort, Some("low"));
    }

    #[test]
    fn thinking_disabled_forces_low_for_gemini() {
        let body = json!({
            "model": "gemini-3.6-flash",
            "max_tokens": 128,
            "thinking": { "type": "disabled" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert!(parts.model.starts_with("gemini-"));
        assert_eq!(
            parts.remember_effort, None,
            "classifier/subagent must not sticky -low onto the parent session"
        );
        assert!(parts.request.get("thinking").is_none());
    }

    #[test]
    fn thinking_disabled_keeps_explicit_suffix_and_does_not_sticky() {
        let body = json!({
            "model": "gemini-3.6-flash-high",
            "max_tokens": 128,
            "thinking": { "type": "disabled" },
            "messages": [{"role": "user", "content": "explore"}]
        });
        let parts = anthropic_to_gemini_request(&body, Some("low"), None).unwrap();
        assert_eq!(parts.model, "gemini-3.6-flash-high");
        assert_eq!(parts.remember_effort, None);
    }

    #[test]
    fn effort_becomes_thinking_level_for_claude() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "output_config": { "effort": "medium" },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
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
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert_eq!(
            parts.request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            json!("high")
        );
    }

    #[test]
    fn gemini_budget_tokens_become_thinking_budget_and_pad_max_tokens() {
        let body = json!({
            "model": "gemini-3.7-flash",
            "max_tokens": 100,
            "thinking": { "type": "enabled", "budget_tokens": 8192 },
            "messages": [{"role": "user", "content": "hi"}]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        assert_eq!(
            parts.request["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            json!(8192)
        );
        assert_eq!(parts.request["generationConfig"]["maxOutputTokens"], json!(8193));
    }

    #[test]
    fn ref_defs_flatten_into_tool_parameters() {
        let body = json!({
            "model": "gemini-3.7-flash-high",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "name": "edit",
                "description": "edit a file",
                "input_schema": {
                    "type": "object",
                    "$defs": {
                        "Path": { "type": "string", "description": "file path" }
                    },
                    "properties": {
                        "path": { "$ref": "#/$defs/Path" }
                    },
                    "required": ["path"]
                }
            }]
        });
        let parts = anthropic_to_gemini_request(&body, None, None).unwrap();
        let params = &parts.request["tools"][0]["functionDeclarations"][0]["parameters"];
        assert_eq!(params["properties"]["path"]["type"], json!("string"));
        assert!(params.get("$ref").is_none());
        assert_eq!(
            parts.request["toolConfig"]["includeServerSideToolInvocations"],
            json!(true)
        );
    }
}
