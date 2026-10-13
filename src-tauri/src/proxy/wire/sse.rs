//! Decode provider SSE JSON into one delta list, then encode that list as
//! Anthropic SSE. Incremental live streams that already speak Anthropic stay
//! on the existing converter so a midstream close still ends with message_stop.

use serde_json::{json, Value};

use super::{chat_usage, Stop, Usage};

#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    Text(String),
    Thinking(String),
    Tool {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    Stop(Stop),
    Usage(Usage),
}

pub fn deltas_from_chat_event(event: &Value) -> Vec<Delta> {
    let mut deltas = Vec::new();
    if let Some(choice) = event
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if let Some(text) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            deltas.push(Delta::Thinking(text.to_string()));
        }
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            deltas.push(Delta::Text(text.to_string()));
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function");
            deltas.push(Delta::Tool {
                index: call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
                id: call.get("id").and_then(Value::as_str).map(str::to_string),
                name: function
                    .and_then(|item| item.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                arguments: function
                    .and_then(|item| item.get("arguments"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
            deltas.push(Delta::Stop(match finish {
                "tool_calls" | "function_call" => Stop::ToolUse,
                "length" => Stop::MaxTokens,
                _ => Stop::EndTurn,
            }));
        }
    }
    if event.get("usage").is_some() {
        deltas.push(Delta::Usage(chat_usage(event)));
    }
    deltas
}

pub fn anthropic_sse_from_deltas(deltas: &[Delta], model: &str) -> String {
    let mut output = String::new();
    let mut input_tokens = 0_i64;
    let mut output_tokens = 0_i64;
    let mut stop = Stop::EndTurn;
    let mut next_index = 0_usize;
    let mut text_index: Option<usize> = None;
    let mut thinking_index: Option<usize> = None;
    let mut tool_indexes: Vec<(usize, usize)> = Vec::new();
    let mut open = Vec::new();

    push(
        &mut output,
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_proxy",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": model,
                "stop_reason": Value::Null,
                "stop_sequence": Value::Null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        }),
    );

    for delta in deltas {
        match delta {
            Delta::Thinking(text) => {
                let index = thinking_index.get_or_insert_with(|| {
                    let index = next_index;
                    next_index += 1;
                    open.push(index);
                    push(
                        &mut output,
                        "content_block_start",
                        json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": {"type": "thinking", "thinking": ""}
                        }),
                    );
                    index
                });
                push(
                    &mut output,
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": *index,
                        "delta": {"type": "thinking_delta", "thinking": text}
                    }),
                );
            }
            Delta::Text(text) => {
                let index = text_index.get_or_insert_with(|| {
                    let index = next_index;
                    next_index += 1;
                    open.push(index);
                    push(
                        &mut output,
                        "content_block_start",
                        json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": {"type": "text", "text": ""}
                        }),
                    );
                    index
                });
                push(
                    &mut output,
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": *index,
                        "delta": {"type": "text_delta", "text": text}
                    }),
                );
            }
            Delta::Tool {
                index,
                id,
                name,
                arguments,
            } => {
                let block = if let Some((_, block)) =
                    tool_indexes.iter().find(|(upstream, _)| upstream == index)
                {
                    *block
                } else {
                    let block = next_index;
                    next_index += 1;
                    tool_indexes.push((*index, block));
                    open.push(block);
                    push(
                        &mut output,
                        "content_block_start",
                        json!({
                            "type": "content_block_start",
                            "index": block,
                            "content_block": {
                                "type": "tool_use",
                                "id": id.clone().unwrap_or_else(|| "tool_call".into()),
                                "name": name.clone().unwrap_or_else(|| "tool".into()),
                                "input": {}
                            }
                        }),
                    );
                    block
                };
                if let Some(arguments) = arguments.clone().filter(|text| !text.is_empty()) {
                    push(
                        &mut output,
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": block,
                            "delta": {"type": "input_json_delta", "partial_json": arguments}
                        }),
                    );
                }
            }
            Delta::Stop(reason) => stop = reason.clone(),
            Delta::Usage(usage) => {
                input_tokens = usage.input_tokens;
                output_tokens = usage.output_tokens;
            }
        }
    }

    for index in open {
        push(
            &mut output,
            "content_block_stop",
            json!({"type": "content_block_stop", "index": index}),
        );
    }
    push(
        &mut output,
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": match stop {
                    Stop::ToolUse => "tool_use",
                    Stop::MaxTokens => "max_tokens",
                    Stop::EndTurn => "end_turn",
                },
                "stop_sequence": Value::Null
            },
            "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens}
        }),
    );
    push(&mut output, "message_stop", json!({"type": "message_stop"}));
    output
}

pub fn deltas_from_anthropic_sse(sse: &str) -> Vec<Delta> {
    let mut deltas = Vec::new();
    let mut current_event = String::new();
    for line in sse.lines() {
        if let Some(event) = line.strip_prefix("event: ") {
            current_event = event.trim().to_string();
            continue;
        }
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(current_event.as_str());
        match kind {
            "content_block_start" => {
                let block = value.get("content_block").unwrap_or(&Value::Null);
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    deltas.push(Delta::Tool {
                        index: value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
                        id: block.get("id").and_then(Value::as_str).map(str::to_string),
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        arguments: None,
                    });
                }
            }
            "content_block_delta" => {
                let delta = value.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str) {
                            deltas.push(Delta::Text(text.to_string()));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = delta.get("thinking").and_then(Value::as_str) {
                            deltas.push(Delta::Thinking(text.to_string()));
                        }
                    }
                    Some("input_json_delta") => deltas.push(Delta::Tool {
                        index: value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
                        id: None,
                        name: None,
                        arguments: delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    }),
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    deltas.push(Delta::Stop(match reason {
                        "tool_use" => Stop::ToolUse,
                        "max_tokens" => Stop::MaxTokens,
                        _ => Stop::EndTurn,
                    }));
                }
                if value.get("usage").is_some() {
                    deltas.push(Delta::Usage(Usage {
                        input_tokens: value
                            .pointer("/usage/input_tokens")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                        output_tokens: value
                            .pointer("/usage/output_tokens")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                        cache_read_input_tokens: value
                            .pointer("/usage/cache_read_input_tokens")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                    }));
                }
            }
            _ => {}
        }
    }
    deltas
}

fn push(output: &mut String, event: &str, data: Value) {
    output.push_str("event: ");
    output.push_str(event);
    output.push('\n');
    output.push_str("data: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()));
    output.push_str("\n\n");
}
