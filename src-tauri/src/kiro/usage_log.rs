//! Persist Kiro gateway requests. `count_tokens` never reaches this module.

use std::sync::Arc;
use std::time::Instant;

use axum::http::HeaderMap;

use crate::database::dao::proxy_logs::{
    insert_proxy_log, update_proxy_log_hop, update_proxy_log_usage_idempotent,
};
use crate::database::Database;
use crate::gateway::correlation::{self, HOP_KIRO};

use super::map::WireProtocol;

pub const PROVIDER_NAME: &str = "Kiro";

pub fn insert_request(
    db: &Arc<Database>,
    account_id: Option<&str>,
    model: &str,
    status_code: Option<i64>,
    started: Instant,
    protocol: WireProtocol,
    is_stream: bool,
    error_category: Option<&str>,
    headers: Option<&HeaderMap>,
) -> Option<String> {
    let duration_ms = started.elapsed().as_millis() as i64;
    let correlation = correlation::resolve(
        headers.unwrap_or(&HeaderMap::new()),
        HOP_KIRO,
        Some("kiro"),
    );
    let target = correlation
        .target_app
        .clone()
        .unwrap_or_else(|| "kiro".to_string());
    let route = match protocol {
        WireProtocol::Anthropic => "/v1/messages",
        WireProtocol::OpenAiChat => "/v1/chat/completions",
        WireProtocol::OpenAiResponses => "/v1/responses",
    };
    let protocol_label = match protocol {
        WireProtocol::Anthropic => "anthropic",
        WireProtocol::OpenAiChat | WireProtocol::OpenAiResponses => "openai",
    };
    match db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            account_id,
            Some(PROVIDER_NAME),
            Some(model),
            status_code,
            duration_ms,
            Some(&target),
            Some(protocol_label),
            Some(route),
            is_stream,
            error_category,
            error_category,
        )
    }) {
        Ok(id) => {
            let _ = db.with_conn(|conn| {
                update_proxy_log_hop(conn, &id, Some(&correlation.id), Some(HOP_KIRO))
            });
            crate::usage_events::notify_log_recorded();
            Some(id)
        }
        Err(error) => {
            log::error!("写入 Kiro 用量日志失败: {error}");
            None
        }
    }
}

pub fn write_usage(db: &Arc<Database>, id: &str, input: i64, output: i64, cache_read: i64, cache_write: i64) {
    let _ = db.with_conn(|conn| {
        update_proxy_log_usage_idempotent(
            conn,
            id,
            Some("kiro"),
            None,
            None,
            input,
            cache_read,
            cache_write,
            output,
        )
    });
}
