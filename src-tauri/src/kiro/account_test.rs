//! Pin one Kiro account and send a tiny generateAssistantResponse probe.
//!
//! Does not go through the local :15831 pool, does not rotate accounts, and
//! does not write usage logs.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{AppError, AppResult};

use super::account::store;
use super::event_stream;
use super::map::{prepare, CollectedReply, WireProtocol};
use super::models::DEFAULT_MODEL;
use super::token::{ensure_access_token, should_force_refresh};
use super::upstream;

pub const DEFAULT_PROMPT: &str = "hello";
const MAX_REPLY_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KiroAccountTestResult {
    pub ok: bool,
    pub category: String,
    pub status: Option<u16>,
    pub model: String,
    pub latency_ms: u64,
    pub reply: Option<String>,
    pub error: Option<String>,
}

pub async fn test_account(
    account_id: &str,
    model: Option<String>,
    prompt: Option<String>,
) -> AppResult<KiroAccountTestResult> {
    let started = Instant::now();
    let model = {
        let trimmed = model.unwrap_or_default();
        let trimmed = trimmed.trim();
        if trimmed.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            trimmed.to_string()
        }
    };
    let prompt = {
        let trimmed = prompt.unwrap_or_default();
        let trimmed = trimmed.trim();
        if trimmed.is_empty() {
            DEFAULT_PROMPT.to_string()
        } else {
            trimmed.to_string()
        }
    };
    let Some(account) = store().get(account_id) else {
        return Ok(result(false, "error", None, model, started, None, Some("Kiro 账号不存在".into())));
    };
    let incoming = json!({
        "model": model,
        "max_tokens": 64,
        "stream": true,
        "thinking": { "type": "disabled" },
        "messages": [{ "role": "user", "content": prompt }]
    });
    let prepared = match prepare(WireProtocol::Anthropic, &incoming, &account) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Ok(result(false, "error", None, model, started, None, Some(error)));
        }
    };
    let mut account = match ensure_access_token(&account, false).await {
        Ok(account) => account,
        Err(error) => {
            let category = if matches!(error, AppError::Network(_)) {
                "network"
            } else {
                "auth"
            };
            return Ok(result(false, category, None, prepared.client_model, started, None, Some(error.to_string())));
        }
    };
    let mut refreshed = false;
    loop {
        let reply = upstream::generate(&account, &prepared.body).await;
        if let Some(message) = reply.network_error {
            return Ok(result(
                false,
                "network",
                None,
                prepared.client_model,
                started,
                None,
                Some(message),
            ));
        }
        let text = String::from_utf8_lossy(&reply.body).into_owned();
        if should_force_refresh(reply.status, &text, refreshed) {
            refreshed = true;
            match ensure_access_token(&account, true).await {
                Ok(updated) => {
                    account = updated;
                    continue;
                }
                Err(error) => {
                    let category = if matches!(error, AppError::Network(_)) {
                        "network"
                    } else {
                        "auth"
                    };
                    return Ok(result(
                        false,
                        category,
                        Some(401),
                        prepared.client_model,
                        started,
                        None,
                        Some(error.to_string()),
                    ));
                }
            }
        }
        if reply.status == 429 {
            return Ok(result(
                true,
                "rate_limit",
                Some(429),
                prepared.client_model,
                started,
                None,
                Some("上游限流，账号仍然可用".into()),
            ));
        }
        if reply.status == 401 {
            return Ok(result(
                false,
                "auth",
                Some(401),
                prepared.client_model,
                started,
                None,
                Some("Kiro 鉴权失败".into()),
            ));
        }
        if !(200..300).contains(&reply.status) {
            let category = if text.contains("MONTHLY_REQUEST_COUNT")
                || text.contains("OVERAGE_REQUEST_LIMIT_EXCEEDED")
            {
                "quota"
            } else {
                "error"
            };
            return Ok(result(
                false,
                category,
                Some(reply.status),
                prepared.client_model,
                started,
                None,
                Some(truncate(&text, 400)),
            ));
        }
        let collected = collect_reply(&reply.body);
        let reply_text = truncate(&collected.text, MAX_REPLY_CHARS);
        return Ok(result(
            true,
            "ok",
            Some(reply.status),
            prepared.client_model,
            started,
            Some(reply_text),
            None,
        ));
    }
}

fn collect_reply(body: &[u8]) -> CollectedReply {
    let mut reply = CollectedReply::default();
    let mut offset = 0;
    while offset < body.len() {
        match event_stream::parse_frame(&body[offset..]) {
            Ok(Some((frame, used))) => {
                reply.push_frame(&frame);
                offset += used;
            }
            Ok(None) | Err(_) => break,
        }
    }
    reply
}

fn result(
    ok: bool,
    category: &str,
    status: Option<u16>,
    model: String,
    started: Instant,
    reply: Option<String>,
    error: Option<String>,
) -> KiroAccountTestResult {
    KiroAccountTestResult {
        ok,
        category: category.to_string(),
        status,
        model,
        latency_ms: started.elapsed().as_millis() as u64,
        reply,
        error,
    }
}

fn truncate(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiro::event_stream::encode_frame;

    #[test]
    fn collects_assistant_text_from_event_stream() {
        let frame = encode_frame(
            "assistantResponseEvent",
            br#"{"content":"hello back"}"#,
        );
        let reply = collect_reply(&frame);
        assert_eq!(reply.text, "hello back");
    }
}
