#[derive(Default)]
struct UpstreamSseDecoder {
    buffer: Vec<u8>,
}

enum UpstreamSseItem {
    Json(Value),
    Done,
}

impl UpstreamSseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Vec<UpstreamSseItem> {
        self.buffer.extend_from_slice(bytes);
        let mut items = Vec::new();
        while let Some((end, delimiter_len)) = find_sse_frame_end(&self.buffer) {
            let frame = self.buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
            let Ok(frame) = std::str::from_utf8(&frame) else { continue; };
            let data = frame.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() { continue; }
            if data == "[DONE]" {
                items.push(UpstreamSseItem::Done);
            } else if let Ok(value) = serde_json::from_str::<Value>(&data) {
                items.push(UpstreamSseItem::Json(value));
            }
        }
        items
    }
}

fn find_sse_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len().saturating_sub(1) {
        if buffer[index..].starts_with(b"\n\n") {
            return Some((index, 2));
        }
        if buffer[index..].starts_with(b"\r\n\r\n") {
            return Some((index, 4));
        }
    }
    None
}

fn rewrite_body(provider: &Provider, original: &Bytes) -> Bytes {
    let mut value: Value = match serde_json::from_slice(original) {
        Ok(v) => v,
        Err(_) => return original.clone(),
    };

    let body_model = value.get("model").and_then(Value::as_str).unwrap_or("").trim();
    let smart_gateway = provider.is_smart_gateway();
    let fill_gateway_default = smart_gateway && crate::gateway::is_auto_model_id(body_model);
    if smart_gateway {
        // Desktop picks a catalog alias. The local proxy must forward that id
        // so the smart gateway can map it. Only an empty or Auto id uses the
        // card default.
        if fill_gateway_default && !provider.model.trim().is_empty() {
            value["model"] = Value::String(provider.model.trim().to_string());
        }
    } else if !provider.model.trim().is_empty() && value.get("model").is_some() {
        value["model"] = Value::String(provider.model.trim().to_string());
    }

    if let Some(cfg) = provider.thinking_config.as_ref() {
        if cfg.is_disabled() {
            if let Some(obj) = value.as_object_mut() {
                obj.remove("thinking");
            }
        } else if let Some(budget) = cfg.resolved_budget_tokens() {
            let needs_inject = match value.get("thinking") {
                None => true,
                Some(t) => {
                    t.get("type").and_then(Value::as_str) == Some("enabled")
                        && t.get("budget_tokens").is_none()
                }
            };
            if needs_inject {
                value["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget
                });
                if let Some(max_tokens) = value.get("max_tokens").and_then(Value::as_u64) {
                    if max_tokens <= budget as u64 {
                        value["max_tokens"] = serde_json::json!(budget as u64 + 1024);
                    }
                }
            }
        }
    }

    if !smart_gateway {
        if let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) {
            for msg in messages.iter_mut() {
                let Some(blocks) = msg.get_mut("content").and_then(Value::as_array_mut) else {
                    continue;
                };
                for block in blocks.iter_mut() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("document") => {
                            if let Some(doc_text) =
                                crate::proxy::convert::extract_anthropic_document_text(block)
                            {
                                *block = serde_json::json!({
                                    "type": "text",
                                    "text": doc_text
                                });
                            }
                        }
                        Some("tool_use") => {
                            if !block.get("input").is_some_and(Value::is_object) {
                                if let Some(obj) = block.as_object_mut() {
                                    obj.insert("input".to_string(), serde_json::json!({}));
                                }
                            }
                        }
                        Some("tool_result" | "tool_search_tool_result") => {
                            if let Some(nested) =
                                block.get_mut("content").and_then(Value::as_array_mut)
                            {
                                for item in nested.iter_mut() {
                                    if item.get("type").and_then(Value::as_str) == Some("document")
                                    {
                                        if let Some(doc_text) =
                                            crate::proxy::convert::extract_anthropic_document_text(
                                                item,
                                            )
                                        {
                                            *item = serde_json::json!({
                                                "type": "text",
                                                "text": doc_text
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            let needs_sanitize = messages.iter().any(|msg| {
                msg.get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|blocks| {
                        blocks.iter().any(|b| {
                            matches!(
                                b.get("type").and_then(Value::as_str),
                                Some("tool_use" | "tool_result" | "tool_search_tool_result")
                            )
                        })
                    })
            });
            if needs_sanitize {
                crate::proxy::codex_anthropic::sanitize_anthropic_messages(messages);
            }
        }
        let effective_model = value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(&provider.model)
            .to_string();
        if provider
            .thinking_config
            .as_ref()
            .is_some_and(|cfg| cfg.is_disabled())
        {
            if let Some(obj) = value.as_object_mut() {
                obj.remove("thinking");
            }
        }
        crate::proxy::codex_anthropic::ensure_deepseek_assistant_thinking(
            &mut value,
            &effective_model,
            Some(&provider.base_url),
        );
    }

    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| original.clone())
}

pub(crate) fn session_prompt_cache_hint(headers: &HeaderMap) -> Option<String> {
    for name in [
        "x-session-id",
        "x-chatgpt-session-id",
        "x-conversation-id",
        "session_id",
    ] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn encode_upstream_request(
    provider: &Provider,
    incoming: &Value,
    original: &Bytes,
    stream: bool,
    headers: &HeaderMap,
) -> (Bytes, bool) {
    match provider.protocol_type {
        ProtocolType::OpenAiChat | ProtocolType::Proxy => {
            let mut request = wire::transcode_request(
                wire::Api::Anthropic,
                wire::Api::Chat,
                incoming,
                provider.model.trim(),
                stream,
            );
            overlay_legacy_thinking(&mut request, incoming, stream, provider, false);
            convert::reinject_chat_prompt_cache_key(
                &mut request,
                incoming
                    .get("prompt_cache_key")
                    .and_then(Value::as_str),
                session_prompt_cache_hint(headers).as_deref(),
                convert::chat_prompt_cache_allowed_for_base_url(&provider.base_url),
            );
            (
                Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                true,
            )
        }
        ProtocolType::OpenAiResponses => {
            let mut request = wire::transcode_request(
                wire::Api::Anthropic,
                wire::Api::Responses,
                incoming,
                provider.model.trim(),
                stream,
            );
            overlay_legacy_thinking(&mut request, incoming, stream, provider, true);
            convert::sanitize_codex_responses_body(&mut request);
            if provider.is_codex_oauth() {
                convert::apply_codex_oauth_response_body(&mut request);
            }
            (
                Bytes::from(serde_json::to_vec(&request).unwrap_or_default()),
                true,
            )
        }
        ProtocolType::Anthropic => (rewrite_body(provider, original), false),
    }
}

fn overlay_legacy_thinking(
    request: &mut Value,
    incoming: &Value,
    stream: bool,
    provider: &Provider,
    responses: bool,
) {
    if provider.thinking_config.is_none()
        && convert::extract_anthropic_thinking(incoming).is_none()
    {
        return;
    }
    let legacy = if responses {
        convert::anthropic_to_openai_responses(
            incoming,
            provider.model.trim(),
            stream,
            provider.thinking_config.as_ref(),
        )
    } else {
        convert::anthropic_to_openai_chat(
            incoming,
            provider.model.trim(),
            stream,
            provider.thinking_config.as_ref(),
        )
    };
    let Some(object) = request.as_object_mut() else {
        return;
    };
    let Some(legacy) = legacy.as_object() else {
        return;
    };
    for key in ["reasoning_effort", "enable_thinking", "instructions", "reasoning", "tool_choice"] {
        if let Some(value) = legacy.get(key) {
            object.insert(key.to_string(), value.clone());
        }
    }
}

