//! Persist Antigravity gateway requests into `proxy_request_logs` for the usage dashboard.

use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::database::dao::proxy_logs::{insert_proxy_log, update_proxy_log_usage_idempotent};
use crate::database::Database;

pub const TARGET_APP: &str = "antigravity";
pub const PROVIDER_NAME: &str = "Antigravity";

pub fn protocol_label(is_anthropic: bool) -> &'static str {
    if is_anthropic {
        "anthropic"
    } else {
        "openai"
    }
}

pub fn route_for(is_anthropic: bool) -> &'static str {
    if is_anthropic {
        "/v1/messages"
    } else {
        "/v1/chat/completions"
    }
}

/// Insert a request row and notify the usage dashboard.
pub fn insert_request(
    db: &Arc<Database>,
    account_id: Option<&str>,
    model: &str,
    status_code: Option<i64>,
    started: Instant,
    is_anthropic: bool,
    is_stream: bool,
    error_category: Option<&str>,
) -> Option<String> {
    let duration_ms = started.elapsed().as_millis() as i64;
    let model = model.trim();
    match db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            account_id,
            Some(PROVIDER_NAME),
            if model.is_empty() { None } else { Some(model) },
            status_code,
            duration_ms,
            Some(TARGET_APP),
            Some(protocol_label(is_anthropic)),
            Some(route_for(is_anthropic)),
            is_stream,
            error_category,
            error_category,
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

pub fn tokens_from_gemini(gemini: &Value) -> (i64, i64) {
    let meta = gemini.get("usageMetadata").unwrap_or(&Value::Null);
    let input = meta
        .get("promptTokenCount")
        .and_then(Value::as_i64)
        .or_else(|| {
            meta.get("promptTokenCount")
                .and_then(Value::as_u64)
                .map(|value| value as i64)
        })
        .unwrap_or(0);
    let output = meta
        .get("candidatesTokenCount")
        .and_then(Value::as_i64)
        .or_else(|| {
            meta.get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .map(|value| value as i64)
        })
        .unwrap_or(0);
    (input, output)
}

/// Best-effort token update from a Gemini (or unwrapped v1internal) body.
pub fn update_usage_from_gemini(db: &Arc<Database>, log_id: &str, account_id: Option<&str>, gemini: &Value) {
    let (input, output) = tokens_from_gemini(gemini);
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
