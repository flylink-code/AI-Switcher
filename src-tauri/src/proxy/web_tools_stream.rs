/// Server tools must finish before the CLI consumes their result. Ordinary
/// conversations retain the existing streaming path without buffering.
pub async fn materialize_stream(
    response: axum::response::Response, tools: &[Value],
) -> axum::response::Response {
    if tools.is_empty() || !response.status().is_success() { return response; }
    let (mut parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, super::LOCAL_LISTENER_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => return super::json_error(axum::http::StatusCode::BAD_GATEWAY, "读取联网工具响应失败"),
    };
    let mut message = match collect_message(&bytes) {
        Some(message) => message,
        None => return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes)),
    };
    if !materialize_web_tool_uses(&mut message, tools).await {
        return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    axum::response::Response::from_parts(parts,
        axum::body::Body::from(super::convert::anthropic_message_to_sse(&message)))
}

fn collect_message(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut message = Value::Null;
    let mut blocks = std::collections::BTreeMap::<usize, Value>::new();
    let mut inputs = std::collections::HashMap::<usize, String>::new();
    let mut stopped = false;
    for line in text.lines().filter_map(|line| line.strip_prefix("data:")) {
        let event: Value = serde_json::from_str(line.trim()).ok()?;
        let index = event["index"].as_u64().unwrap_or(0) as usize;
        match event["type"].as_str()? {
            "message_start" => message = event["message"].clone(),
            "content_block_start" => { blocks.insert(index, event["content_block"].clone()); }
            "content_block_delta" => {
                let delta = &event["delta"];
                match delta["type"].as_str() {
                    Some("input_json_delta") => inputs.entry(index).or_default()
                        .push_str(delta["partial_json"].as_str()?),
                    Some("text_delta") => {
                        let block = blocks.get_mut(&index)?;
                        let text = block["text"].as_str().unwrap_or("").to_string();
                        block["text"] = json!(text + delta["text"].as_str()?);
                    }
                    Some("thinking_delta") | Some("signature_delta") => {
                        let field = if delta["type"] == "thinking_delta" { "thinking" } else { "signature" };
                        let block = blocks.get_mut(&index)?;
                        let text = block[field].as_str().unwrap_or("").to_string();
                        block[field] = json!(text + delta[field].as_str()?);
                    }
                    _ => return None,
                }
            }
            "message_delta" => {
                message["stop_reason"] = event["delta"]["stop_reason"].clone();
                message["stop_sequence"] = event["delta"]["stop_sequence"].clone();
                if let Some(usage) = event["usage"].as_object() {
                    for (key, value) in usage { message["usage"][key] = value.clone(); }
                }
            }
            "message_stop" => stopped = true,
            "error" => return None,
            _ => {}
        }
    }
    if !stopped || message.is_null() { return None; }
    for (index, input) in inputs {
        blocks.get_mut(&index)?["input"] = serde_json::from_str(&input).ok()?;
    }
    message["content"] = Value::Array(blocks.into_values().collect());
    Some(message)
}
