//! OpenAI Chat Completions ↔ Gemini request/response mapping.

use serde_json::{json, Value};
use uuid::Uuid;

use super::args_fix::{correct_tool_args, param_keys_from_declarations, ToolParamKeys};
use super::models::{map_effort_to_suffix, map_model_id};
use crate::antigravity::model_catalog;

pub struct GeminiRequestParts {
    pub model: String,
    pub request: Value,
    pub stream: bool,
    /// 本次请求的工具声明参数键名，响应侧用于纠偏模型拼错的 args key。
    pub tool_params: ToolParamKeys,
}

pub fn openai_to_gemini_request(body: &Value) -> Result<GeminiRequestParts, String> {
    let mut model = map_model_id(body.get("model").and_then(Value::as_str).unwrap_or(""));
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    // OpenAI / Codex `reasoning_effort` ("minimal"|"low"|"medium"|"high"):
    // Gemini targets encode it in the model id suffix; Claude targets get it
    // as generationConfig.thinkingConfig.thinkingLevel.
    let effort = body
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .and_then(|effort| {
            if effort == "minimal" {
                Some("low".to_string())
            } else {
                Some(effort)
            }
        })
        .and_then(|effort| map_effort_to_suffix(&effort));
    let mut claude_thinking_level: Option<&'static str> = None;
    let lower_model = model.to_ascii_lowercase();
    if let Some(level) = effort {
        if lower_model.starts_with("gemini-") {
            model = model_catalog::with_forced_level(&model, level);
        } else if lower_model.starts_with("claude-") {
            claude_thinking_level = Some(level);
        }
    }

    let mut contents = Vec::new();
    let mut system_parts = Vec::new();

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // tool_call_id → function name, so tool messages can reference the real
    // function name (tool messages only carry tool_call_id).
    let mut tool_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for message in &messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                if let (Some(id), Some(name)) = (
                    call.get("id").and_then(Value::as_str),
                    call.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str),
                ) {
                    tool_names.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match role {
            "system" | "developer" => {
                push_text_content(message.get("content").unwrap_or(&Value::Null), &mut system_parts);
            }
            "assistant" => {
                let mut parts = content_to_parts(message.get("content").unwrap_or(&Value::Null));
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let name = call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("tool");
                        let args_raw = call
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let args: Value =
                            serde_json::from_str(args_raw).unwrap_or_else(|_| json!({}));
                        let mut fc = json!({ "name": name, "args": args });
                        if let Some(id) = call.get("id") {
                            fc["id"] = id.clone();
                        }
                        parts.push(json!({ "functionCall": fc }));
                    }
                }
                if !parts.is_empty() {
                    contents.push(json!({ "role": "model", "parts": parts }));
                }
            }
            "tool" => {
                let tool_call_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let name = message
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| tool_names.get(tool_call_id).cloned())
                    .unwrap_or_else(|| {
                        if tool_call_id.is_empty() {
                            "tool".to_string()
                        } else {
                            tool_call_id.to_string()
                        }
                    });
                let result = super::anthropic::tool_result_text(
                    message.get("content").unwrap_or(&Value::Null),
                );
                let mut function_response =
                    json!({ "name": name, "response": { "result": result } });
                // Cloud Code converts back to Anthropic format for Claude
                // models; without `id` it emits tool_result without
                // tool_use_id and upstream 400s ("Field required").
                if !tool_call_id.is_empty() {
                    function_response["id"] = json!(tool_call_id);
                }
                contents.push(json!({
                    "role": "user",
                    "parts": [{ "functionResponse": function_response }]
                }));
            }
            _ => {
                let parts = content_to_parts(message.get("content").unwrap_or(&Value::Null));
                if !parts.is_empty() {
                    contents.push(json!({ "role": "user", "parts": parts }));
                }
            }
        }
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
    if let Some(max_tokens) = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .and_then(Value::as_u64)
    {
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

    let mut tool_params = ToolParamKeys::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let declarations: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                let name = function.get("name").and_then(Value::as_str)?;
                Some(json!({
                    "name": name,
                    "description": function.get("description").cloned().unwrap_or(json!("")),
                    "parameters": function
                        .get("parameters")
                        .map(|schema| super::anthropic::sanitize_schema(schema, 0))
                        .unwrap_or(json!({
                            "type": "object",
                            "properties": {}
                        })),
                }))
            })
            .collect();
        if !declarations.is_empty() {
            tool_params = param_keys_from_declarations(&declarations);
            request["tools"] = json!([{ "functionDeclarations": declarations }]);
            // AUTO matches Cloud Code / Antigravity clients; VALIDATED rejects
            // many real-world tool schemas and can fail streamGenerateContent.
            request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "AUTO" }
            });
        }
    }

    Ok(GeminiRequestParts {
        model,
        request,
        stream,
        tool_params,
    })
}

fn push_text_content(content: &Value, out: &mut Vec<Value>) {
    match content {
        Value::String(text) if !text.is_empty() => out.push(json!({ "text": text })),
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
        _ => {}
    }
}

fn content_to_parts(content: &Value) -> Vec<Value> {
    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "text": text })]
            }
        }
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                parts.push(json!({ "text": text }));
                            }
                        }
                    }
                    "image_url" => {
                        if let Some(url) = item
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(Value::as_str)
                        {
                            if let Some((mime, data)) = parse_data_url(url) {
                                parts.push(json!({
                                    "inlineData": { "mimeType": mime, "data": data }
                                }));
                            }
                        }
                    }
                    _ => {
                        if let Some(text) = item.as_str() {
                            if !text.is_empty() {
                                parts.push(json!({ "text": text }));
                            }
                        }
                    }
                }
            }
            parts
        }
        _ => Vec::new(),
    }
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("image/png").to_string();
    Some((mime, data.to_string()))
}

pub fn gemini_to_openai_response(model: &str, gemini: &Value, tool_params: &ToolParamKeys) -> Value {
    let (text, tool_calls, finish_reason) = extract_assistant(gemini, tool_params);
    let mut message = json!({
        "role": "assistant",
        "content": text,
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
        message["content"] = Value::Null;
    }
    let usage = usage_from_gemini(gemini);
    json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason,
        }],
        "usage": usage,
    })
}

pub fn gemini_to_openai_sse_chunk(
    model: &str,
    gemini_chunk: &Value,
    tool_params: &ToolParamKeys,
) -> Value {
    let (text, tool_calls, finish_reason) = extract_assistant(gemini_chunk, tool_params);
    let mut delta = json!({ "role": "assistant" });
    if !text.is_empty() {
        delta["content"] = json!(text);
    }
    if !tool_calls.is_empty() {
        delta["tool_calls"] = json!(tool_calls);
    }
    json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4().simple()),
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": if finish_reason == "null" { Value::Null } else { json!(finish_reason) },
        }]
    })
}

fn extract_assistant(gemini: &Value, tool_params: &ToolParamKeys) -> (String, Vec<Value>, String) {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut finish_reason = "stop".to_string();
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
            for (index, part) in parts.iter().enumerate() {
                if let Some(chunk) = part.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
                if let Some(fc) = part.get("functionCall") {
                    let id = fc
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
                    let name = fc.get("name").cloned().unwrap_or(json!("tool"));
                    let args = correct_tool_args(
                        name.as_str().unwrap_or("tool"),
                        fc.get("args").cloned().unwrap_or(json!({})),
                        tool_params,
                    );
                    tool_calls.push(json!({
                        "id": id,
                        "index": index,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": args.to_string(),
                        }
                    }));
                    finish_reason = "tool_calls".into();
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            finish_reason = match reason {
                "MAX_TOKENS" => "length".into(),
                "STOP" => {
                    if tool_calls.is_empty() {
                        "stop".into()
                    } else {
                        "tool_calls".into()
                    }
                }
                other => other.to_ascii_lowercase(),
            };
        }
    }
    (text, tool_calls, finish_reason)
}

fn usage_from_gemini(gemini: &Value) -> Value {
    let meta = gemini.get("usageMetadata").cloned().unwrap_or(json!({}));
    let prompt = meta.get("promptTokenCount").and_then(Value::as_u64).unwrap_or(0);
    let completion = meta
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_openai_chat() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "hi"}
            ]
        });
        let parts = openai_to_gemini_request(&body).unwrap();
        assert_eq!(parts.model, "claude-sonnet-4-6");
        assert!(parts.request.get("systemInstruction").is_some());
    }

    #[test]
    fn tool_message_keeps_call_id_and_real_name() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": null, "tool_calls": [
                    { "id": "call_1", "type": "function", "function": { "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" } }
                ]},
                { "role": "tool", "tool_call_id": "call_1", "content": "file body" }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read_file",
                    "parameters": { "$schema": "http://json-schema.org/draft-07/schema#", "type": "object", "properties": { "path": { "type": "string" } } }
                }
            }]
        });
        let parts = openai_to_gemini_request(&body).unwrap();
        let response = &parts.request["contents"][2]["parts"][0]["functionResponse"];
        assert_eq!(response["id"], json!("call_1"));
        assert_eq!(response["name"], json!("read_file"));
        assert_eq!(response["response"]["result"], json!("file body"));
        let params = &parts.request["tools"][0]["functionDeclarations"][0]["parameters"];
        assert!(params.get("$schema").is_none());
        assert_eq!(
            parts.request["toolConfig"]["functionCallingConfig"]["mode"],
            json!("AUTO")
        );
    }

    #[test]
    fn reasoning_effort_maps_gemini_suffix_and_claude_thinking_level() {
        let gemini = openai_to_gemini_request(&json!({
            "model": "gemini-3.6-flash",
            "reasoning_effort": "high",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert!(gemini.model.starts_with("gemini-"));
        assert!(gemini.request.get("reasoning_effort").is_none());

        let claude = openai_to_gemini_request(&json!({
            "model": "claude-sonnet-4-6",
            "reasoning_effort": "low",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert_eq!(
            claude.request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            json!("low")
        );
    }

    #[test]
    fn tool_call_arguments_are_corrected_against_declarations() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "grep it"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "Grep",
                    "parameters": { "type": "object", "properties": {
                        "pattern": { "type": "string" },
                        "-n": { "type": "boolean" }
                    } }
                }
            }]
        });
        let parts = openai_to_gemini_request(&body).unwrap();
        let gemini = json!({
            "candidates": [{
                "content": { "parts": [
                    { "functionCall": { "id": "call_fix_1", "name": "Grep",
                        "args": { "pattern": "x", "n": true } } }
                ] },
                "finishReason": "STOP"
            }]
        });
        let response = gemini_to_openai_response("gpt-4o", &gemini, &parts.tool_params);
        let args: Value = serde_json::from_str(
            response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(args["-n"], json!(true));
        assert!(args.get("n").is_none());
    }
}
