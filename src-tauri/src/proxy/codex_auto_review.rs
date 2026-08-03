//! Codex auto-review model override for guardian / auto_review subagent requests.

use axum::http::HeaderMap;
use bytes::Bytes;

const SUBAGENT_HEADER: &str = "x-openai-subagent";

fn is_auto_review_subagent(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "guardian" | "auto_review"
    )
}

/// When `x-openai-subagent` is `guardian` or `auto_review` (case-insensitive) and
/// `model_override` is set, rewrite the JSON request body's `model` field.
///
/// Callers that fail over must pass the *target* provider's override, not the
/// originally selected provider's.
pub fn apply_auto_review_model_override(
    headers: &HeaderMap,
    body: &[u8],
    model_override: Option<&str>,
) -> Bytes {
    let Some(override_model) = model_override.map(str::trim).filter(|model| !model.is_empty()) else {
        return Bytes::copy_from_slice(body);
    };

    let triggers_override = headers
        .get(SUBAGENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_auto_review_subagent);
    if !triggers_override {
        return Bytes::copy_from_slice(body);
    }

    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(object) = value.as_object_mut() else {
        return Bytes::copy_from_slice(body);
    };
    object.insert(
        "model".to_string(),
        serde_json::Value::String(override_model.to_string()),
    );
    match serde_json::to_vec(&value) {
        Ok(bytes) => Bytes::from(bytes),
        Err(_) => Bytes::copy_from_slice(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with_subagent(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            SUBAGENT_HEADER,
            HeaderValue::from_str(value).expect("valid header value"),
        );
        headers
    }

    #[test]
    fn rewrite_skipped_without_override_or_matching_header() {
        let body = br#"{"model":"gpt-5","stream":false}"#;
        let headers = headers_with_subagent("guardian");

        assert_eq!(
            apply_auto_review_model_override(&headers, body, None),
            Bytes::from_static(body)
        );
        assert_eq!(
            apply_auto_review_model_override(&HeaderMap::new(), body, Some("cheap-model")),
            Bytes::from_static(body)
        );
        assert_eq!(
            apply_auto_review_model_override(&headers_with_subagent("other"), body, Some("cheap-model")),
            Bytes::from_static(body)
        );
    }

    #[test]
    fn rewrite_applies_for_guardian_and_auto_review_headers() {
        let body = br#"{"model":"gpt-5","stream":false}"#;
        for subagent in ["guardian", "Guardian", "auto_review", "AUTO_REVIEW"] {
            let headers = headers_with_subagent(subagent);
            let rewritten = apply_auto_review_model_override(&headers, body, Some("gpt-5.4-mini"));
            let value: serde_json::Value = serde_json::from_slice(&rewritten).unwrap();
            assert_eq!(value.get("model").and_then(serde_json::Value::as_str), Some("gpt-5.4-mini"));
            assert_eq!(value.get("stream").and_then(serde_json::Value::as_bool), Some(false));
        }
    }

    #[test]
    fn rewrite_leaves_invalid_json_unchanged() {
        let body = b"not-json";
        let headers = headers_with_subagent("guardian");
        assert_eq!(
            apply_auto_review_model_override(&headers, body, Some("cheap-model")),
            Bytes::from_static(body)
        );
    }
}
