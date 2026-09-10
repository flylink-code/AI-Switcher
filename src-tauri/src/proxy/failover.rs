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

    if !provider.model.trim().is_empty() && value.get("model").is_some() {
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
            let mut request =
                convert::anthropic_to_openai_chat(incoming, provider.model.trim(), stream, provider.thinking_config.as_ref());
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
            let mut request =
                convert::anthropic_to_openai_responses(incoming, provider.model.trim(), stream, provider.thinking_config.as_ref());
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

