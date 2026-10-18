//! Shared request and response shape for Anthropic Messages, OpenAI Chat, and
//! Responses. Same-protocol traffic stays a passthrough at the call site.
//! This round keeps text, images, tool calls, thinking text, stop, and usage.

mod sse;

use serde_json::{json, Map, Value};

use crate::provider::ProtocolType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Anthropic,
    Chat,
    Responses,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Image {
        media_type: String,
        data: String,
        url: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        output: String,
    },
    Thinking(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    EndTurn,
    MaxTokens,
    ToolUse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<Value>,
    pub top_p: Option<Value>,
    pub stream: bool,
    pub tools: Vec<ToolDef>,
    pub tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub id: String,
    pub model: String,
    pub parts: Vec<Part>,
    pub stop: Stop,
    pub usage: Usage,
}

pub fn transcode_request(from: Api, to: Api, body: &Value, model: &str, stream: bool) -> Value {
    if from == to {
        let mut cloned = body.clone();
        if let Some(object) = cloned.as_object_mut() {
            if !model.is_empty() {
                object.insert("model".to_string(), json!(model));
            }
            object.insert("stream".to_string(), json!(stream));
        }
        return cloned;
    }
    let mut request = parse_request(from, body);
    if !model.is_empty() {
        request.model = model.to_string();
    }
    request.stream = stream;
    build_request(to, &request)
}

pub fn client_anthropic_message(
    protocol: ProtocolType,
    upstream: &Value,
    client_model: &str,
) -> Value {
    if protocol == ProtocolType::OpenAiResponses
        && upstream.get("status").and_then(Value::as_str) == Some("failed")
    {
        let message = upstream
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| upstream.pointer("/error/code").and_then(Value::as_str))
            .unwrap_or("上游 Responses 请求失败");
        return json!({
            "type": "error",
            "error": { "type": "api_error", "message": message }
        });
    }
    if protocol == ProtocolType::Anthropic {
        let mut response = upstream.clone();
        if !client_model.trim().is_empty() {
            response["model"] = json!(client_model);
        }
        return response;
    }
    let api = if protocol == ProtocolType::OpenAiResponses {
        Api::Responses
    } else {
        Api::Chat
    };
    build_anthropic_response(&parse_response(api, upstream), client_model)
}

pub fn parse_request(api: Api, body: &Value) -> Request {
    match api {
        Api::Anthropic => parse_anthropic(body),
        Api::Chat => parse_chat(body),
        Api::Responses => parse_responses(body),
    }
}

pub fn build_request(api: Api, request: &Request) -> Value {
    match api {
        Api::Anthropic => build_anthropic(request),
        Api::Chat => build_chat(request),
        Api::Responses => build_responses(request),
    }
}

fn parse_anthropic(body: &Value) -> Request {
    let mut messages = Vec::new();
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        messages.push(Message {
            role: if role == "assistant" {
                "assistant".into()
            } else {
                "user".into()
            },
            parts: parts_from_anthropic_content(message.get("content").unwrap_or(&Value::Null)),
        });
    }
    Request {
        model: string_field(body, "model"),
        system: content_text(body.get("system").unwrap_or(&Value::Null)),
        messages,
        max_tokens: body.get("max_tokens").and_then(Value::as_u64),
        temperature: body.get("temperature").cloned(),
        top_p: body.get("top_p").cloned(),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: anthropic_tools(body),
        tool_choice: body.get("tool_choice").map(anthropic_tool_choice),
    }
}

fn build_anthropic(request: &Request) -> Value {
    let mut messages = Vec::new();
    for message in &request.messages {
        let blocks = anthropic_blocks(&message.parts);
        if blocks.is_empty() {
            continue;
        }
        messages.push(json!({
            "role": message.role,
            "content": blocks,
        }));
    }
    let mut result = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if !request.system.is_empty() {
        result["system"] = json!(request.system);
    }
    if let Some(max) = request.max_tokens {
        result["max_tokens"] = json!(max);
    }
    copy_option(&mut result, "temperature", request.temperature.as_ref());
    copy_option(&mut result, "top_p", request.top_p.as_ref());
    if !request.tools.is_empty() {
        result["tools"] = Value::Array(request.tools.iter().map(anthropic_tool_json).collect());
    }
    if let Some(choice) = &request.tool_choice {
        result["tool_choice"] = anthropic_choice_json(choice);
    }
    result
}

fn parse_chat(body: &Value) -> Request {
    let mut system = String::new();
    let mut messages = Vec::new();
    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "system" {
            let text = content_text(message.get("content").unwrap_or(&Value::Null));
            if !system.is_empty() && !text.is_empty() {
                system.push('\n');
            }
            system.push_str(&text);
            continue;
        }
        if role == "tool" {
            messages.push(Message {
                role: "user".into(),
                parts: vec![Part::ToolResult {
                    id: message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string(),
                    output: content_text(message.get("content").unwrap_or(&Value::Null)),
                }],
            });
            continue;
        }
        let mut parts = parts_from_chat_content(message.get("content").unwrap_or(&Value::Null));
        if let Some(thinking) = message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            parts.insert(0, Part::Thinking(thinking.to_string()));
        }
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function").cloned().unwrap_or_else(|| json!({}));
            parts.push(Part::ToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("tool_call")
                    .to_string(),
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                arguments: function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_string(),
            });
        }
        if parts.is_empty() {
            continue;
        }
        messages.push(Message {
            role: if role == "assistant" {
                "assistant".into()
            } else {
                "user".into()
            },
            parts,
        });
    }
    Request {
        model: string_field(body, "model"),
        system,
        messages,
        max_tokens: body.get("max_tokens").and_then(Value::as_u64),
        temperature: body.get("temperature").cloned(),
        top_p: body.get("top_p").cloned(),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: chat_tools(body),
        tool_choice: body.get("tool_choice").map(chat_tool_choice),
    }
}

fn build_chat(request: &Request) -> Value {
    let mut messages = Vec::new();
    if !request.system.is_empty() {
        messages.push(json!({"role": "system", "content": request.system}));
    }
    for message in &request.messages {
        let mut text_and_images = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();
        let mut thinking = String::new();
        for part in &message.parts {
            match part {
                Part::Text(text) => text_and_images.push(json!({"type": "text", "text": text})),
                Part::Image {
                    media_type,
                    data,
                    url,
                } => {
                    text_and_images.push(json!({"type": "image_url", "image_url": {"url": chat_image_url(media_type, data, url)}}));
                }
                Part::Thinking(text) => {
                    if !thinking.is_empty() {
                        thinking.push('\n');
                    }
                    thinking.push_str(text);
                }
                Part::ToolCall {
                    id,
                    name,
                    arguments,
                } => tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments }
                })),
                Part::ToolResult { id, output } => tool_results.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": output
                })),
            }
        }
        let is_assistant = message.role == "assistant";
        if !is_assistant {
            messages.extend(std::mem::take(&mut tool_results));
        }
        if !text_and_images.is_empty() || !tool_calls.is_empty() || !thinking.is_empty() {
            let mut row = Map::new();
            row.insert(
                "role".into(),
                json!(if is_assistant {
                    "assistant"
                } else {
                    "user"
                }),
            );
            if text_and_images.len() == 1
                && text_and_images[0].get("type").and_then(Value::as_str) == Some("text")
            {
                row.insert(
                    "content".into(),
                    text_and_images[0].get("text").cloned().unwrap_or(json!("")),
                );
            } else if text_and_images.is_empty() {
                row.insert("content".into(), Value::Null);
            } else {
                row.insert("content".into(), Value::Array(text_and_images));
            }
            if !thinking.is_empty() {
                row.insert("reasoning_content".into(), json!(thinking));
            }
            if !tool_calls.is_empty() {
                row.insert("tool_calls".into(), Value::Array(tool_calls));
            }
            messages.push(Value::Object(row));
        }
        if is_assistant {
            messages.extend(tool_results);
        }
    }
    let mut result = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if request.stream {
        result["stream_options"] = json!({"include_usage": true});
    }
    if let Some(max) = request.max_tokens {
        result["max_tokens"] = json!(max);
    }
    copy_option(&mut result, "temperature", request.temperature.as_ref());
    copy_option(&mut result, "top_p", request.top_p.as_ref());
    if !request.tools.is_empty() {
        result["tools"] = Value::Array(request.tools.iter().map(chat_tool_json).collect());
    }
    if let Some(choice) = &request.tool_choice {
        result["tool_choice"] = chat_choice_json(choice);
    }
    result
}

fn parse_responses(body: &Value) -> Request {
    let mut messages: Vec<Message> = Vec::new();
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (role, parts) = match item.get("type").and_then(Value::as_str) {
            Some("function_call") => (
                "assistant",
                vec![Part::ToolCall {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    arguments: item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}")
                        .to_string(),
                }],
            ),
            Some("function_call_output") => (
                "user",
                vec![Part::ToolResult {
                    id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string(),
                    output: content_text(item.get("output").unwrap_or(&Value::Null)),
                }],
            ),
            Some("reasoning") => {
                let text = reasoning_text(item);
                if text.is_empty() {
                    continue;
                }
                ("assistant", vec![Part::Thinking(text)])
            }
            _ => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                let role = if role == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                let parts = parts_from_responses_content(item.get("content").unwrap_or(item));
                if parts.is_empty() {
                    continue;
                }
                (role, parts)
            }
        };
        if let Some(last) = messages.last_mut() {
            if last.role == role {
                last.parts.extend(parts);
                continue;
            }
        }
        messages.push(Message {
            role: role.into(),
            parts,
        });
    }
    Request {
        model: string_field(body, "model"),
        system: content_text(body.get("instructions").unwrap_or(&Value::Null)),
        messages,
        max_tokens: body
            .get("max_output_tokens")
            .or_else(|| body.get("max_tokens"))
            .and_then(Value::as_u64),
        temperature: body.get("temperature").cloned(),
        top_p: body.get("top_p").cloned(),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: responses_tools(body),
        tool_choice: body.get("tool_choice").map(responses_tool_choice),
    }
}

fn build_responses(request: &Request) -> Value {
    let mut input = Vec::new();
    for message in &request.messages {
        let assistant = message.role == "assistant";
        let text_type = if assistant {
            "output_text"
        } else {
            "input_text"
        };
        let mut content = Vec::new();
        let flush_content = |input: &mut Vec<Value>, content: &mut Vec<Value>| {
            if content.is_empty() {
                return;
            }
            input.push(json!({
                "type": "message",
                "role": if assistant { "assistant" } else { "user" },
                "content": std::mem::take(content),
            }));
        };
        for part in &message.parts {
            match part {
                Part::Text(text) => content.push(json!({"type": text_type, "text": text})),
                Part::Image {
                    media_type,
                    data,
                    url,
                } if !assistant => {
                    content.push(json!({"type": "input_image", "image_url": chat_image_url(media_type, data, url)}));
                }
                Part::Image { .. } => {}
                Part::Thinking(text) => {
                    flush_content(&mut input, &mut content);
                    input.push(json!({
                        "type": "reasoning",
                        "summary": [{ "type": "summary_text", "text": text }]
                    }));
                }
                Part::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    flush_content(&mut input, &mut content);
                    input.push(json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": arguments
                    }));
                }
                Part::ToolResult { id, output } => {
                    if assistant {
                        flush_content(&mut input, &mut content);
                    }
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": id,
                        "output": output
                    }));
                }
            }
        }
        flush_content(&mut input, &mut content);
    }
    let mut result = json!({
        "model": request.model,
        "input": input,
        "stream": request.stream,
    });
    if !request.system.is_empty() {
        result["instructions"] = json!(request.system);
    }
    if let Some(max) = request.max_tokens {
        result["max_output_tokens"] = json!(max);
    }
    copy_option(&mut result, "temperature", request.temperature.as_ref());
    copy_option(&mut result, "top_p", request.top_p.as_ref());
    if !request.tools.is_empty() {
        result["tools"] = Value::Array(request.tools.iter().map(responses_tool_json).collect());
    }
    if let Some(choice) = &request.tool_choice {
        result["tool_choice"] = responses_choice_json(choice);
    }
    result
}

fn parse_response(api: Api, body: &Value) -> Response {
    match api {
        Api::Chat => parse_chat_response(body),
        Api::Responses => parse_responses_response(body),
        Api::Anthropic => parse_anthropic_response(body),
    }
}

fn parse_chat_response(body: &Value) -> Response {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let message = choice
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut parts = Vec::new();
    if let Some(thinking) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        parts.push(Part::Thinking(thinking.to_string()));
    }
    parts.extend(parts_from_chat_content(
        message.get("content").unwrap_or(&Value::Null),
    ));
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = call.get("function").cloned().unwrap_or_else(|| json!({}));
        parts.push(Part::ToolCall {
            id: call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string(),
            name: function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
        });
    }
    let finish = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    Response {
        id: string_field(body, "id"),
        model: string_field(body, "model"),
        stop: match finish {
            Some("tool_calls") | Some("function_call") => Stop::ToolUse,
            Some("length") => Stop::MaxTokens,
            _ => {
                if parts
                    .iter()
                    .any(|part| matches!(part, Part::ToolCall { .. }))
                {
                    Stop::ToolUse
                } else {
                    Stop::EndTurn
                }
            }
        },
        usage: chat_usage(body),
        parts,
    }
}

fn parse_responses_response(body: &Value) -> Response {
    let mut parts = Vec::new();
    for item in body
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => parts.push(Part::ToolCall {
                id: item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool_call")
                    .to_string(),
                name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                arguments: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_string(),
            }),
            Some("reasoning") => {
                let text = reasoning_text(item);
                if !text.is_empty() {
                    parts.push(Part::Thinking(text));
                }
            }
            _ => parts.extend(parts_from_responses_content(
                item.get("content").unwrap_or(&Value::Null),
            )),
        }
    }
    let incomplete = body.get("status").and_then(Value::as_str) == Some("incomplete")
        && body
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
            == Some("max_output_tokens");
    let stop = if parts
        .iter()
        .any(|part| matches!(part, Part::ToolCall { .. }))
    {
        Stop::ToolUse
    } else if incomplete {
        Stop::MaxTokens
    } else {
        Stop::EndTurn
    };
    Response {
        id: string_field(body, "id"),
        model: string_field(body, "model"),
        parts,
        stop,
        usage: responses_usage(body),
    }
}

fn parse_anthropic_response(body: &Value) -> Response {
    Response {
        id: string_field(body, "id"),
        model: string_field(body, "model"),
        parts: parts_from_anthropic_content(body.get("content").unwrap_or(&Value::Null)),
        stop: match body.get("stop_reason").and_then(Value::as_str) {
            Some("tool_use") => Stop::ToolUse,
            Some("max_tokens") => Stop::MaxTokens,
            _ => Stop::EndTurn,
        },
        usage: Usage {
            input_tokens: body
                .pointer("/usage/input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            output_tokens: body
                .pointer("/usage/output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            cache_read_input_tokens: body
                .pointer("/usage/cache_read_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        },
    }
}

fn build_anthropic_response(response: &Response, client_model: &str) -> Value {
    let model = if client_model.trim().is_empty() {
        response.model.clone()
    } else {
        client_model.to_string()
    };
    let fresh = response.usage.input_tokens.max(0);
    json!({
        "id": if response.id.is_empty() { "msg_proxy".to_string() } else { response.id.clone() },
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": anthropic_blocks(&response.parts),
        "stop_reason": match response.stop {
            Stop::ToolUse => "tool_use",
            Stop::MaxTokens => "max_tokens",
            Stop::EndTurn => "end_turn",
        },
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": fresh,
            "cache_read_input_tokens": response.usage.cache_read_input_tokens.max(0),
            "cache_creation_input_tokens": 0,
            "output_tokens": response.usage.output_tokens.max(0),
        }
    })
}

fn parts_from_anthropic_content(content: &Value) -> Vec<Part> {
    match content {
        Value::String(text) if !text.is_empty() => vec![Part::Text(text.clone())],
        Value::Array(blocks) => blocks.iter().filter_map(anthropic_block).collect(),
        _ => Vec::new(),
    }
}

fn anthropic_block(block: &Value) -> Option<Part> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| Part::Text(text.to_string())),
        Some("thinking") => block
            .get("thinking")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| Part::Thinking(text.to_string())),
        Some("image") => image_from_anthropic(block),
        Some("tool_use") => Some(Part::ToolCall {
            id: block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string(),
            name: block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: serde_json::to_string(block.get("input").unwrap_or(&json!({})))
                .unwrap_or_else(|_| "{}".to_string()),
        }),
        Some("tool_result") | Some("tool_search_tool_result") => Some(Part::ToolResult {
            id: block
                .get("tool_use_id")
                .or_else(|| block.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string(),
            output: content_text(block.get("content").unwrap_or(&Value::Null)),
        }),
        _ => None,
    }
}

fn image_from_anthropic(block: &Value) -> Option<Part> {
    let source = block.get("source")?;
    if source.get("type").and_then(Value::as_str) == Some("base64") {
        Some(Part::Image {
            media_type: source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png")
                .to_string(),
            data: source
                .get("data")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            url: String::new(),
        })
    } else {
        source
            .get("url")
            .and_then(Value::as_str)
            .map(|url| Part::Image {
                media_type: String::new(),
                data: String::new(),
                url: url.to_string(),
            })
    }
}

fn parts_from_chat_content(content: &Value) -> Vec<Part> {
    match content {
        Value::String(text) if !text.is_empty() => vec![Part::Text(text.clone())],
        Value::Array(blocks) => blocks.iter().filter_map(chat_block).collect(),
        _ => Vec::new(),
    }
}

fn chat_block(block: &Value) -> Option<Part> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") | Some("output_text") => block
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| Part::Text(text.to_string())),
        Some("refusal") => block
            .get("refusal")
            .or_else(|| block.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| Part::Text(text.to_string())),
        Some("image_url") => {
            let url = block
                .pointer("/image_url/url")
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(image_from_url(url))
        }
        Some("thinking") | Some("reasoning") => block
            .get("thinking")
            .or_else(|| block.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| Part::Thinking(text.to_string())),
        _ => None,
    }
}

fn parts_from_responses_content(content: &Value) -> Vec<Part> {
    match content {
        Value::String(text) if !text.is_empty() => vec![Part::Text(text.clone())],
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("input_text") | Some("output_text") | Some("text") => block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| Part::Text(text.to_string())),
                Some("refusal") => block
                    .get("refusal")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| Part::Text(text.to_string())),
                Some("input_image") => {
                    let url = block
                        .get("image_url")
                        .and_then(Value::as_str)
                        .or_else(|| block.pointer("/image_url/url").and_then(Value::as_str))
                        .unwrap_or("");
                    Some(image_from_url(url))
                }
                Some("reasoning_text") | Some("summary_text") => block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| Part::Thinking(text.to_string())),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn image_from_url(url: &str) -> Part {
    if let Some((media_type, data)) = split_data_url(url) {
        Part::Image {
            media_type,
            data,
            url: String::new(),
        }
    } else {
        Part::Image {
            media_type: String::new(),
            data: String::new(),
            url: url.to_string(),
        }
    }
}

fn anthropic_blocks(parts: &[Part]) -> Vec<Value> {
    parts.iter().filter_map(|part| match part {
        Part::Text(text) => Some(json!({"type": "text", "text": text})),
        Part::Thinking(text) => Some(json!({"type": "thinking", "thinking": text})),
        Part::Image { media_type, data, url } => {
            if !data.is_empty() {
                Some(json!({"type": "image", "source": {"type": "base64", "media_type": if media_type.is_empty() { "image/png" } else { media_type }, "data": data}}))
            } else if !url.is_empty() {
                Some(json!({"type": "image", "source": {"type": "url", "url": url}}))
            } else {
                None
            }
        }
        Part::ToolCall { id, name, arguments } => {
            let input = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}));
            Some(json!({"type": "tool_use", "id": id, "name": name, "input": input}))
        }
        Part::ToolResult { id, output } => Some(json!({"type": "tool_result", "tool_use_id": id, "content": output})),
    }).collect()
}

fn anthropic_tools(body: &Value) -> Vec<ToolDef> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let name = tool_name(tool)?;
            Some(ToolDef {
                name: name.to_string(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                parameters: tool
                    .get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            })
        })
        .collect()
}

fn chat_tools(body: &Value) -> Vec<ToolDef> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())?;
            Some(ToolDef {
                name: name.to_string(),
                description: function
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                parameters: function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            })
        })
        .collect()
}

fn responses_tools(body: &Value) -> Vec<ToolDef> {
    chat_tools(body)
}

fn tool_name(tool: &Value) -> Option<&str> {
    if let Some(name) = tool.get("name").and_then(Value::as_str) {
        if !name.trim().is_empty() {
            return Some(name);
        }
    }
    if let Some(tool_type) = tool.get("type").and_then(Value::as_str) {
        if super::convert::is_claude_server_tool_type(tool_type) {
            return Some(tool_type);
        }
    }
    None
}

fn anthropic_tool_json(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.parameters,
    })
}

fn chat_tool_json(tool: &ToolDef) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

fn responses_tool_json(tool: &ToolDef) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
    })
}

fn anthropic_tool_choice(value: &Value) -> ToolChoice {
    match value.get("type").and_then(Value::as_str) {
        Some("none") => ToolChoice::None,
        Some("any") => ToolChoice::Required,
        Some("tool") => ToolChoice::Named(
            value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        _ => ToolChoice::Auto,
    }
}

fn chat_tool_choice(value: &Value) -> ToolChoice {
    match value {
        Value::String(text) if text == "none" => ToolChoice::None,
        Value::String(text) if text == "required" => ToolChoice::Required,
        Value::Object(_) => ToolChoice::Named(
            value
                .pointer("/function/name")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        _ => ToolChoice::Auto,
    }
}

fn responses_tool_choice(value: &Value) -> ToolChoice {
    match value {
        Value::String(text) if text == "none" => ToolChoice::None,
        Value::String(text) if text == "required" => ToolChoice::Required,
        Value::Object(_) => ToolChoice::Named(
            value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        _ => ToolChoice::Auto,
    }
}

fn anthropic_choice_json(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::None => json!({"type": "none"}),
        ToolChoice::Required => json!({"type": "any"}),
        ToolChoice::Named(name) => json!({"type": "tool", "name": name}),
        ToolChoice::Auto => json!({"type": "auto"}),
    }
}

fn chat_choice_json(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Named(name) => json!({"type": "function", "function": {"name": name}}),
        ToolChoice::Auto => json!("auto"),
    }
}

fn responses_choice_json(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Named(name) => json!({"type": "function", "name": name}),
        ToolChoice::Auto => json!("auto"),
    }
}

pub(super) fn chat_usage(body: &Value) -> Usage {
    let total = body
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = body
        .pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, total);
    Usage {
        input_tokens: total.saturating_sub(cached),
        output_tokens: body
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_input_tokens: cached,
    }
}

fn responses_usage(body: &Value) -> Usage {
    let total = body
        .pointer("/usage/input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = body
        .pointer("/usage/input_tokens_details/cached_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, total.max(0));
    Usage {
        input_tokens: total.saturating_sub(cached),
        output_tokens: body
            .pointer("/usage/output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_input_tokens: cached,
    }
}

fn reasoning_text(item: &Value) -> String {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    let summary = item.get("summary").or_else(|| item.get("content"));
    content_text(summary.unwrap_or(&Value::Null))
}

fn chat_image_url(media_type: &str, data: &str, url: &str) -> String {
    if !data.is_empty() {
        let media = if media_type.is_empty() {
            "image/png"
        } else {
            media_type
        };
        format!("data:{media};base64,{data}")
    } else {
        url.to_string()
    }
}

fn split_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.contains("base64") {
        return None;
    }
    let media = meta.split(';').next().unwrap_or("image/png").to_string();
    Some((media, data.to_string()))
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn copy_option(target: &mut Value, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        target[key] = value.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::sse::{
        anthropic_sse_from_deltas, deltas_from_anthropic_sse, deltas_from_chat_event, Delta,
    };
    use super::*;

    fn sample_anthropic() -> Value {
        json!({
            "model": "claude-sonnet-5",
            "max_tokens": 128,
            "temperature": 0.2,
            "system": "be brief",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc"}},
                    {"type": "image", "source": {"type": "url", "url": "https://example.test/a.png"}}
                ]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "calling"},
                    {"type": "tool_use", "id": "call_1", "name": "lookup", "input": {"q": "x"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "found"}
                ]}
            ],
            "tools": [{"name": "lookup", "description": "Find data", "input_schema": {"type": "object"}}]
        })
    }

    fn assert_roundtrip(request: &Request) {
        assert_eq!(request.system, "be brief");
        assert_eq!(request.max_tokens, Some(128));
        assert_eq!(request.tools[0].name, "lookup");
        let user = &request.messages[0].parts;
        assert_eq!(user[0], Part::Text("look".into()));
        assert_eq!(
            user[1],
            Part::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
                url: String::new()
            }
        );
        assert_eq!(
            user[2],
            Part::Image {
                media_type: String::new(),
                data: String::new(),
                url: "https://example.test/a.png".into()
            }
        );
        let assistant = &request.messages[1].parts;
        assert_eq!(assistant[0], Part::Thinking("hmm".into()));
        assert_eq!(assistant[1], Part::Text("calling".into()));
        assert_eq!(
            assistant[2],
            Part::ToolCall {
                id: "call_1".into(),
                name: "lookup".into(),
                arguments: "{\"q\":\"x\"}".into()
            }
        );
        assert_eq!(
            request.messages[2].parts[0],
            Part::ToolResult {
                id: "call_1".into(),
                output: "found".into()
            }
        );
    }

    #[test]
    fn anthropic_chat_and_responses_roundtrip_text_image_and_tool() {
        let anthropic = sample_anthropic();
        let parsed = parse_request(Api::Anthropic, &anthropic);
        assert_roundtrip(&parsed);

        let chat = build_request(Api::Chat, &parsed);
        assert_eq!(
            chat["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(
            chat["messages"][1]["content"][2]["image_url"]["url"],
            "https://example.test/a.png"
        );
        assert_eq!(chat["tools"][0]["function"]["name"], "lookup");
        let from_chat = parse_request(Api::Chat, &chat);
        assert_roundtrip(&from_chat);

        let responses = build_request(Api::Responses, &from_chat);
        assert_eq!(responses["instructions"], "be brief");
        assert_eq!(responses["max_output_tokens"], 128);
        let from_responses = parse_request(Api::Responses, &responses);
        assert_roundtrip(&from_responses);

        let back = build_request(Api::Anthropic, &from_responses);
        assert_eq!(back["messages"][0]["content"][1]["source"]["data"], "abc");
        assert_eq!(
            back["messages"][0]["content"][2]["source"]["url"],
            "https://example.test/a.png"
        );
        assert_eq!(back["messages"][1]["content"][2]["id"], "call_1");
        assert_eq!(back["messages"][1]["content"][2]["input"]["q"], "x");
    }

    #[test]
    fn response_usage_stop_and_tool_survive_chat_and_responses() {
        let chat = json!({
            "id": "chatcmpl_1",
            "model": "gpt-test",
            "choices": [{"finish_reason": "tool_calls", "message": {
                "content": "calling",
                "reasoning_content": "hmm",
                "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}]
            }}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 1}}
        });
        let anthropic = client_anthropic_message(
            ProtocolType::OpenAiChat,
            &chat,
            "claude.sub2api.gpt-6-astra",
        );
        assert_eq!(anthropic["model"], "claude.sub2api.gpt-6-astra");
        assert_eq!(anthropic["stop_reason"], "tool_use");
        assert_eq!(anthropic["content"][0]["thinking"], "hmm");
        assert_eq!(anthropic["content"][2]["id"], "call_1");
        assert_eq!(anthropic["usage"]["input_tokens"], 4);
        assert_eq!(anthropic["usage"]["cache_read_input_tokens"], 1);
        assert_eq!(anthropic["usage"]["output_tokens"], 2);

        let responses = json!({
            "id": "resp_1",
            "model": "gpt-test",
            "status": "completed",
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "hmm"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "calling"}]},
                {"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"q\":\"x\"}"}
            ],
            "usage": {"input_tokens": 3, "output_tokens": 2}
        });
        let converted =
            client_anthropic_message(ProtocolType::OpenAiResponses, &responses, "alias");
        assert_eq!(converted["content"][0]["thinking"], "hmm");
        assert_eq!(converted["content"][2]["input"]["q"], "x");
        assert_eq!(converted["stop_reason"], "tool_use");
        assert_eq!(converted["usage"]["output_tokens"], 2);
    }

    #[test]
    fn chat_sse_deltas_roundtrip_text_and_tool() {
        let events = [
            json!({"choices":[{"delta":{"content":"hi"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"x\"}"}}]}}]}),
            json!({"choices":[{"finish_reason":"tool_calls"}], "usage":{"prompt_tokens":3,"completion_tokens":2}}),
        ];
        let deltas: Vec<Delta> = events.iter().flat_map(deltas_from_chat_event).collect();
        let sse = anthropic_sse_from_deltas(&deltas, "anthropic/aisw-0000000001");
        assert!(sse.contains("hi"));
        assert!(sse.contains("call_1"));
        assert!(sse.contains("lookup"));
        let back = deltas_from_anthropic_sse(&sse);
        assert!(back
            .iter()
            .any(|delta| matches!(delta, Delta::Text(text) if text == "hi")));
        assert!(back.iter().any(|delta| matches!(delta, Delta::Tool { id: Some(id), name: Some(name), .. } if id == "call_1" && name == "lookup")));
        assert!(back
            .iter()
            .any(|delta| matches!(delta, Delta::Stop(Stop::ToolUse))));
        assert!(back
            .iter()
            .any(|delta| matches!(delta, Delta::Usage(usage) if usage.output_tokens == 2)));
    }
}
