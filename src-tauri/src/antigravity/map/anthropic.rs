//! Anthropic Messages ↔ Gemini request/response mapping.

use serde_json::{json, Value};
use uuid::Uuid;

use super::models::map_model_id;

pub struct GeminiRequestParts {
    pub model: String,
    pub request: Value,
    pub stream: bool,
}

pub fn anthropic_to_gemini_request(body: &Value) -> Result<GeminiRequestParts, String> {
    let model = map_model_id(body.get("model").and_then(Value::as_str).unwrap_or(""));
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
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
        let parts = content_to_parts(message.get("content").unwrap_or(&Value::Null));
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
                    .cloned()
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
            request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "VALIDATED" }
            });
        }
    }

    Ok(GeminiRequestParts {
        model,
        request,
        stream,
    })
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

fn content_to_parts(content: &Value) -> Vec<Value> {
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
                        let name = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("tool");
                        let response = block.get("content").cloned().unwrap_or(json!(""));
                        parts.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": { "content": response },
                            }
                        }));
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

pub fn gemini_to_anthropic_sse_chunk(model: &str, gemini_chunk: &Value) -> Vec<Value> {
    let (text, tool_uses, stop_reason) = extract_assistant(gemini_chunk);
    let mut events = Vec::new();
    if !text.is_empty() {
        events.push(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }));
    }
    for (index, tool) in tool_uses.iter().enumerate() {
        events.push(json!({
            "type": "content_block_start",
            "index": index + 1,
            "content_block": tool,
        }));
    }
    if stop_reason != "null" && !stop_reason.is_empty() {
        let usage = usage_from_gemini(gemini_chunk);
        events.push(json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
            "usage": { "output_tokens": usage.get("output_tokens").cloned().unwrap_or(json!(0)) }
        }));
        events.push(json!({ "type": "message_stop" }));
        let _ = model;
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

fn extract_assistant(gemini: &Value) -> (String, Vec<Value>, String) {
    let mut text = String::new();
    let mut tools = Vec::new();
    let mut stop_reason = "end_turn".to_string();
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
                    stop_reason = "tool_use".into();
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            stop_reason = match reason {
                "MAX_TOKENS" => "max_tokens".into(),
                "STOP" | "FINISH_REASON_UNSPECIFIED" => {
                    if tools.is_empty() {
                        "end_turn".into()
                    } else {
                        "tool_use".into()
                    }
                }
                other => other.to_ascii_lowercase(),
            };
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
}
