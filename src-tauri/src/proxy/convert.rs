//! Stateless Anthropic <-> OpenAI protocol conversion helpers.

use serde_json::{json, Map, Value};

pub fn wants_stream(request: &Value) -> bool {
    request.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

pub fn anthropic_to_openai_chat(request: &Value, model: &str, stream: bool) -> Value {
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
    copy_if_present(request, &mut result, "temperature");
    copy_if_present(request, &mut result, "top_p");
    if let Some(max) = request.get("max_tokens") {
        result["max_tokens"] = max.clone();
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        result["tools"] = Value::Array(tools.iter().filter_map(anthropic_tool_to_openai).collect());
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = anthropic_tool_choice(choice);
    }
    result
}

pub fn anthropic_to_openai_responses(request: &Value, model: &str, stream: bool) -> Value {
    let chat = anthropic_to_openai_chat(request, model, stream);
    let mut result = json!({
        "model": model,
        "input": chat["messages"].clone(),
        "stream": stream,
    });
    if let Some(system) = request.get("system") {
        let instructions = content_text(system);
        if !instructions.is_empty() {
            result["instructions"] = Value::String(instructions);
        }
    }
    copy_if_present(&chat, &mut result, "temperature");
    copy_if_present(&chat, &mut result, "top_p");
    if let Some(max) = request.get("max_tokens") {
        result["max_output_tokens"] = max.clone();
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        result["tools"] = Value::Array(tools.clone());
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = choice.clone();
    }
    result
}

pub fn openai_chat_to_anthropic(value: &Value, fallback_model: &str) -> Value {
    let choice = value.get("choices").and_then(Value::as_array).and_then(|items| items.first());
    let message = choice.and_then(|choice| choice.get("message")).cloned().unwrap_or_else(|| json!({}));
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
        content.push(json!({"type": "text", "text": text}));
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
    json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type": "message",
        "role": "assistant",
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "content": content,
        "stop_reason": if finish == Some("tool_calls") { "tool_use" } else { "end_turn" },
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": value.pointer("/usage/prompt_tokens").and_then(Value::as_i64).unwrap_or(0),
            "output_tokens": value.pointer("/usage/completion_tokens").and_then(Value::as_i64).unwrap_or(0),
        }
    })
}

pub fn openai_responses_to_anthropic(value: &Value, fallback_model: &str) -> Value {
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
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                content.push(json!({"type": "text", "text": text}));
            }
        }
    }
    let has_tool = content.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
    json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_proxy"),
        "type": "message",
        "role": "assistant",
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "content": content,
        "stop_reason": if has_tool { "tool_use" } else { "end_turn" },
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": value.pointer("/usage/input_tokens").and_then(Value::as_i64).unwrap_or(0),
            "output_tokens": value.pointer("/usage/output_tokens").and_then(Value::as_i64).unwrap_or(0),
        }
    })
}

/// Convert a completed Anthropic message into a standards-shaped Anthropic SSE
/// sequence. Upstream OpenAI streaming is requested non-streaming so tool-call
/// arguments and stop reasons remain coherent for every compatible provider.
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
                Some("text") | Some("thinking") => text_blocks.push(json!({"type":"text","text":block.get("text").and_then(Value::as_str).unwrap_or("")})),
                Some("image") => if let Some(url) = image_url(block) { text_blocks.push(json!({"type":"image_url","image_url":{"url":url}})); },
                Some("tool_use") => tool_calls.push(json!({"id":block.get("id").and_then(Value::as_str).unwrap_or("tool_call"),"type":"function","function":{"name":block.get("name").and_then(Value::as_str).unwrap_or("tool"),"arguments":serde_json::to_string(block.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_string())}})),
                Some("tool_result") => tool_results.push(json!({"role":"tool","tool_call_id":block.get("tool_use_id").and_then(Value::as_str).unwrap_or("tool_call"),"content":content_text(block.get("content").unwrap_or(&Value::Null))})),
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

fn anthropic_tool_to_openai(tool: &Value) -> Option<Value> {
    Some(json!({"type":"function","function":{"name":tool.get("name")?.as_str()?,"description":tool.get("description").and_then(Value::as_str).unwrap_or(""),"parameters":tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}}))}}))
}

fn anthropic_tool_choice(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("tool") => json!({"type":"function","function":{"name":value.get("name").and_then(Value::as_str).unwrap_or("")}}),
        Some("any") => Value::String("required".to_string()),
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
    fn chat_conversion_preserves_tools_images_and_model() {
        let request = json!({
            "model": "ignored", "max_tokens": 32, "stream": true,
            "messages": [{"role":"user","content":[
                {"type":"text","text":"look"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}
            ]}],
            "tools":[{"name":"lookup","description":"Find data","input_schema":{"type":"object"}}]
        });
        let output = anthropic_to_openai_chat(&request, "gpt-test", false);
        assert_eq!(output["model"], "gpt-test");
        assert_eq!(output["stream"], false);
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
        assert!(sse.contains("event: input_json_delta"));
        assert!(sse.contains("event: message_stop"));
    }

    #[test]
    fn responses_conversion_preserves_function_call() {
        let upstream = json!({"id":"resp_1","model":"gpt-test","output":[
            {"type":"message","content":[{"type":"output_text","text":"done"}]},
            {"type":"function_call","call_id":"call_2","name":"save","arguments":"{\"ok\":true}"}
        ],"usage":{"input_tokens":4,"output_tokens":5}});
        let message = openai_responses_to_anthropic(&upstream, "fallback");
        assert_eq!(message["content"][0]["text"], "done");
        assert_eq!(message["content"][1]["name"], "save");
        assert_eq!(message["usage"]["output_tokens"], 5);
    }
}
