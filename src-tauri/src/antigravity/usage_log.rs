//! Persist Antigravity gateway requests into `proxy_request_logs` for the usage dashboard.

use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::database::dao::proxy_logs::{insert_proxy_log, update_proxy_log_usage_idempotent};
use crate::database::Database;

pub const TARGET_APP: &str = "antigravity";
pub const PROVIDER_NAME: &str = "Antigravity";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireProtocol {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
}

pub fn protocol_label(protocol: WireProtocol) -> &'static str {
    match protocol {
        WireProtocol::Anthropic => "anthropic",
        WireProtocol::OpenAiChat | WireProtocol::OpenAiResponses => "openai",
    }
}

pub fn route_for(protocol: WireProtocol) -> &'static str {
    match protocol {
        WireProtocol::Anthropic => "/v1/messages",
        WireProtocol::OpenAiChat => "/v1/chat/completions",
        WireProtocol::OpenAiResponses => "/v1/responses",
    }
}

/// Insert a request row and notify the usage dashboard.
pub fn insert_request(
    db: &Arc<Database>,
    account_id: Option<&str>,
    model: &str,
    status_code: Option<i64>,
    started: Instant,
    protocol: WireProtocol,
    is_stream: bool,
    error_category: Option<&str>,
    diagnostic: Option<&str>,
) -> Option<String> {
    let duration_ms = started.elapsed().as_millis() as i64;
    let model = model.trim();
    let diagnostic = diagnostic
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let clipped: String = value.chars().take(400).collect();
            clipped
        });
    match db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            account_id,
            Some(PROVIDER_NAME),
            if model.is_empty() { None } else { Some(model) },
            status_code,
            duration_ms,
            Some(TARGET_APP),
            Some(protocol_label(protocol)),
            Some(route_for(protocol)),
            is_stream,
            error_category,
            diagnostic.as_deref().or(error_category),
        )
    }) {
        Ok(id) => {
            crate::usage_events::notify_log_recorded();
            Some(id)
        }
        Err(error) => {
            log::error!("写入 Antigravity 用量日志失败: {error}");
            None
        }
    }
}

/// Parsed Gemini `usageMetadata` for wire-protocol translation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeminiUsage {
    pub input: i64,
    pub output: i64,
    pub thoughts: i64,
    pub cached: i64,
}

impl GeminiUsage {
    pub fn parse(gemini: &Value) -> Self {
        let meta = gemini.get("usageMetadata").unwrap_or(&Value::Null);
        let input = meta_count(meta, "promptTokenCount");
        let candidates = meta_count(meta, "candidatesTokenCount");
        let thoughts = meta_count(meta, "thoughtsTokenCount");
        let cached = meta_count(meta, "cachedContentTokenCount");
        let total = meta_count(meta, "totalTokenCount");
        let total_output = meta_count(meta, "totalOutputTokenCount")
            .max(meta_count(meta, "total_output_tokens"));
        // Classic Gemini usageMetadata: candidatesTokenCount already includes
        // thought tokens. Newer Interactions-style payloads expose a separate
        // total_output that does not, so thoughts must be added there only.
        let mut output = if total_output > 0 {
            total_output.saturating_add(thoughts)
        } else {
            candidates
        };
        if output == 0 && total > input {
            output = total.saturating_sub(input);
        }
        Self {
            input,
            output,
            thoughts,
            cached,
        }
    }

    /// Keep the highest counts seen on a stream (Gemini often only fills metadata late).
    pub fn merge_max(&mut self, other: Self) {
        if other.input > self.input {
            self.input = other.input;
        }
        if other.output > self.output {
            self.output = other.output;
        }
        if other.thoughts > self.thoughts {
            self.thoughts = other.thoughts;
        }
        if other.cached > self.cached {
            self.cached = other.cached;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.input == 0 && self.output == 0
    }

    pub fn anthropic_usage(&self) -> Value {
        let mut usage = serde_json::json!({
            "input_tokens": self.input,
            "output_tokens": self.output,
        });
        if self.cached > 0 {
            usage["cache_read_input_tokens"] = serde_json::json!(self.cached);
        }
        usage
    }

    pub fn openai_usage(&self) -> Value {
        let mut usage = serde_json::json!({
            "prompt_tokens": self.input,
            "completion_tokens": self.output,
            "total_tokens": self.input.saturating_add(self.output),
        });
        if self.cached > 0 {
            usage["prompt_tokens_details"] = serde_json::json!({ "cached_tokens": self.cached });
        }
        usage
    }

    pub fn responses_usage(&self) -> Value {
        serde_json::json!({
            "input_tokens": self.input,
            "output_tokens": self.output,
            "total_tokens": self.input.saturating_add(self.output),
            "input_tokens_details": { "cached_tokens": self.cached.max(0) },
            "output_tokens_details": { "reasoning_tokens": self.thoughts.max(0) },
        })
    }
}

fn meta_count(meta: &Value, key: &str) -> i64 {
    meta.get(key)
        .and_then(Value::as_i64)
        .or_else(|| meta.get(key).and_then(Value::as_u64).map(|value| value as i64))
        .unwrap_or(0)
}

pub fn tokens_from_gemini(gemini: &Value) -> (i64, i64) {
    let usage = GeminiUsage::parse(gemini);
    (usage.input, usage.output)
}

/// Best-effort token update from a Gemini (or unwrapped v1internal) body.
pub fn update_usage_from_gemini(db: &Arc<Database>, log_id: &str, account_id: Option<&str>, gemini: &Value) {
    let usage = GeminiUsage::parse(gemini);
    if usage.is_empty() {
        return;
    }
    if let Err(error) = db.with_conn(|conn| {
        update_proxy_log_usage_idempotent(
            conn,
            log_id,
            Some(TARGET_APP),
            account_id,
            None,
            usage.input,
            usage.cached,
            0,
            usage.output,
        )
    }) {
        log::error!("更新 Antigravity Token 用量失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

pub fn update_usage_tokens(
    db: &Arc<Database>,
    log_id: &str,
    account_id: Option<&str>,
    input: i64,
    output: i64,
) {
    if input == 0 && output == 0 {
        return;
    }
    if let Err(error) = db.with_conn(|conn| {
        update_proxy_log_usage_idempotent(
            conn,
            log_id,
            Some(TARGET_APP),
            account_id,
            None,
            input,
            0,
            0,
            output,
        )
    }) {
        log::error!("更新 Antigravity Token 用量失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

#[cfg(test)]
mod tests {
    use super::GeminiUsage;
    use serde_json::json;

    #[test]
    fn tokens_from_gemini_include_thoughts_and_total_fallback() {
        let with_thoughts = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 20,
                "thoughtsTokenCount": 80,
                "cachedContentTokenCount": 40
            }
        });
        let usage = GeminiUsage::parse(&with_thoughts);
        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 20);
        assert_eq!(usage.thoughts, 80);
        assert_eq!(usage.cached, 40);
        assert_eq!(usage.anthropic_usage()["input_tokens"], 100);
        assert_eq!(usage.anthropic_usage()["output_tokens"], 20);
        assert_eq!(usage.anthropic_usage()["cache_read_input_tokens"], 40);

        let interactions = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "totalOutputTokenCount": 20,
                "thoughtsTokenCount": 80
            }
        });
        let added = GeminiUsage::parse(&interactions);
        assert_eq!(added.output, 100);
        assert_eq!(added.thoughts, 80);

        let total_only = json!({
            "usageMetadata": {
                "promptTokenCount": 80,
                "totalTokenCount": 120
            }
        });
        let fallback = GeminiUsage::parse(&total_only);
        assert_eq!(fallback.input, 80);
        assert_eq!(fallback.output, 40);
        assert_eq!(fallback.thoughts, 0);
    }
}
