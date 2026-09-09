//! Cross-hop request correlation for usage de-duplication.

use axum::http::HeaderMap;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: &str = "x-aisw-request-id";
pub const TARGET_APP_HEADER: &str = "x-aisw-target-app";

pub const HOP_AGENT_PROXY: &str = "agent_proxy";
pub const HOP_SMART_GATEWAY: &str = "smart_gateway";
pub const HOP_ANTIGRAVITY: &str = "antigravity";

#[derive(Debug, Clone)]
pub struct Correlation {
    pub id: String,
    pub target_app: Option<String>,
    pub hop: &'static str,
    pub transit: bool,
}

pub fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn resolve(
    headers: &HeaderMap,
    hop: &'static str,
    fallback_target: Option<&str>,
) -> Correlation {
    let id = header_value(headers, REQUEST_ID_HEADER)
        .unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()));
    let target_app = header_value(headers, TARGET_APP_HEADER)
        .or_else(|| fallback_target.map(str::to_string));
    Correlation {
        id,
        target_app,
        hop,
        transit: false,
    }
}

pub fn is_internal_upstream(url: &str) -> bool {
    crate::gateway::is_self_referential_upstream(url, &crate::gateway::reserved_listener_ports())
}
