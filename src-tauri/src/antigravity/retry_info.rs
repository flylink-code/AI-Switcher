//! Parse Google RPC RetryInfo from Cloud Code error bodies.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartRetry {
    /// Wait on the same account / same model, then retry.
    InPlaceWait(Duration),
    /// SKU RPM — rotate account after existing short cooldown.
    RateLimitRotate,
    /// Model capacity exhausted — do not walk the rest of the pool.
    CapacityNoPoolWalk,
    None,
}

pub fn classify_retry(body: &str, retry_after_header: Option<u64>) -> SmartRetry {
    let lower = body.to_ascii_lowercase();
    if lower.contains("model_capacity_exhausted") || lower.contains("model capacity exhausted")
    {
        return SmartRetry::CapacityNoPoolWalk;
    }
    let delay = parse_retry_delay(body)
        .or_else(|| retry_after_header.map(Duration::from_secs))
        .unwrap_or_else(|| Duration::from_secs(1));
    if lower.contains("rate_limit_exceeded") || lower.contains("rate limit exceeded") {
        if delay <= Duration::from_secs(5) {
            return SmartRetry::InPlaceWait(delay);
        }
        return SmartRetry::RateLimitRotate;
    }
    if delay > Duration::ZERO && delay <= Duration::from_secs(5) && looks_like_rpc_retry(body) {
        return SmartRetry::InPlaceWait(delay);
    }
    SmartRetry::None
}

fn looks_like_rpc_retry(body: &str) -> bool {
    body.contains("RetryInfo") || body.contains("retryDelay") || body.contains("retry_delay")
}

fn parse_retry_delay(body: &str) -> Option<Duration> {
    for key in ["\"retryDelay\"", "\"retry_delay\""] {
        if let Some(value) = json_string_after(body, key) {
            return parse_proto_duration(&value);
        }
    }
    None
}

fn json_string_after(body: &str, key: &str) -> Option<String> {
    let idx = body.find(key)?;
    let rest = &body[idx + key.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_proto_duration(raw: &str) -> Option<Duration> {
    let trimmed = raw.trim();
    if let Some(secs) = trimmed.strip_suffix('s') {
        let value: f64 = secs.parse().ok()?;
        if value < 0.0 {
            return None;
        }
        return Some(Duration::from_millis((value * 1000.0) as u64));
    }
    trimmed.parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_retry_info_is_in_place() {
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"1.5s"}]}}"#;
        match classify_retry(body, None) {
            SmartRetry::InPlaceWait(delay) => assert_eq!(delay, Duration::from_millis(1500)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn capacity_exhausted_does_not_walk_pool() {
        let body = "MODEL_CAPACITY_EXHAUSTED: no capacity";
        assert_eq!(classify_retry(body, Some(30)), SmartRetry::CapacityNoPoolWalk);
    }

    #[test]
    fn long_rate_limit_rotates() {
        let body = r#"{"error":{"status":"RATE_LIMIT_EXCEEDED","details":[{"retryDelay":"20s"}]}}"#;
        assert_eq!(classify_retry(body, None), SmartRetry::RateLimitRotate);
    }
}
