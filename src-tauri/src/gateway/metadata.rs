//! Default model metadata inferred from well-known id prefixes.
//! Only fills missing fields; never overwrites user-edited values.

use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct InferredMeta {
    pub display_name: String,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub reasoning_levels: Vec<String>,
    pub capabilities: Value,
}

pub fn infer(model_id: &str) -> InferredMeta {
    let id = model_id.trim();
    let lower = id.to_ascii_lowercase();
    let mut meta = InferredMeta {
        display_name: id.to_string(),
        context_window: Some(200_000),
        max_output_tokens: Some(32_000),
        reasoning_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
        capabilities: json!({ "tools": true, "vision": false, "web_search": false }),
    };
    if lower.contains("gemini")
        || lower.contains("gpt-4")
        || lower.contains("gpt-5")
        || lower.contains("gpt-6")
        || lower.contains("claude")
        || lower.contains("grok")
    {
        meta.capabilities["vision"] = json!(true);
    }
    if lower.contains("gemini")
        || lower.contains("gpt-5")
        || lower.contains("gpt-6")
        || lower.contains("sonar")
    {
        meta.capabilities["web_search"] = json!(true);
    }
    if lower.contains("flash")
        || lower.contains("haiku")
        || lower.contains("mini")
        || lower.contains("chat")
    {
        meta.reasoning_levels = vec!["off".into(), "low".into()];
    }
    if has_window_token(&lower, "1m") {
        meta.context_window = Some(1_000_000);
    } else if has_window_token(&lower, "200k") {
        meta.context_window = Some(200_000);
    } else if lower.contains("gemini") {
        meta.context_window = Some(1_000_000);
        meta.max_output_tokens = Some(65_536);
    }
    if lower.contains("deepseek") && lower.contains("reasoner") {
        meta.reasoning_levels = vec!["off".into(), "high".into()];
    }
    meta
}

/// Whole-token match for a window marker such as `1m` / `200k`. A bare substring
/// test would also hit ids like `foo-v1m-bar`.
fn has_window_token(lower: &str, token: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
}

pub fn context_window_for(model_id: &str) -> u64 {
    infer(model_id)
        .context_window
        .filter(|value| *value > 0)
        .unwrap_or(200_000) as u64
}

pub fn max_output_for(model_id: &str, context: u64) -> u64 {
    infer(model_id)
        .max_output_tokens
        .filter(|value| *value > 0)
        .map(|value| value as u64)
        .unwrap_or(32_000)
        .min(context.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_marker_must_be_a_whole_token() {
        assert_eq!(context_window_for("claude-opus-5[1m]"), 1_000_000);
        assert_eq!(context_window_for("some-model-1m"), 1_000_000);
        assert_eq!(context_window_for("some-model-200k"), 200_000);
        // 子串 "1m" 不是窗口标记。
        assert_eq!(context_window_for("foo-v1m-bar"), 200_000);
        assert_eq!(context_window_for("claude-sonnet-5-5-medium"), 200_000);
        assert_eq!(context_window_for("gemini-3.8-flash-high"), 1_000_000);
    }
}
