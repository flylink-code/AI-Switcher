//! Provider quota and balance detection module.

pub mod balance;
pub mod coding_plan;
pub mod detector;
pub mod official;
pub mod types;

use chrono::Utc;
use std::time::Duration;

pub use detector::{query_official_quota, query_provider_quota};
pub use types::*;

/// Get current timestamp in milliseconds.
pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

/// Convert timestamp in milliseconds to RFC 3339 (ISO 8601) string.
pub fn millis_to_iso8601(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    let secs = ms / 1000;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
}

/// Create a dedicated reqwest HTTP client for quota queries.
pub fn quota_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}
