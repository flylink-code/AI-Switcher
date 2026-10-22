//! Default model metadata inferred from well-known id prefixes.
//! Only fills missing fields; never overwrites user-edited values.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModelMeta {
    #[serde(default)]
    pub priority_supported: Option<bool>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub max_context_window: Option<u64>,
    pub effective_context_window_percent: Option<u64>,
}

impl DiscoveredModelMeta {
    pub fn from_model(value: &Value) -> Self {
        let positive = |value: Option<&Value>| value.and_then(Value::as_u64).filter(|v| *v > 0);
        let context = positive(value.get("context_window"))
            .or_else(|| positive(value.get("context_length")))
            .or_else(|| positive(value.pointer("/limit/context")));
        let input = positive(value.get("max_input_tokens"))
            .or_else(|| positive(value.pointer("/limit/input")));
        Self {
            priority_supported: value.get("service_tiers").and_then(Value::as_array)
                .or_else(|| value.get("additional_speed_tiers").and_then(Value::as_array))
                .map(|tiers| tiers.iter().any(|tier| {
                    matches!(tier.as_str().or_else(|| tier.get("id").and_then(Value::as_str)), Some("priority" | "fast"))
                })),
            context_window: match (input, context) {
                (Some(input), Some(context)) => Some(input.min(context)),
                _ => input.or(context),
            },
            max_output_tokens: positive(value.get("max_output_tokens"))
                .or_else(|| positive(value.pointer("/limit/output"))),
            max_context_window: positive(value.get("max_context_window")),
            effective_context_window_percent: positive(value.get("effective_context_window_percent"))
                .filter(|value| *value <= 100),
        }
    }
}

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
    let bare = strip_gateway_routing_prefix(&lower);
    let mut meta = InferredMeta {
        display_name: id.to_string(),
        context_window: Some(200_000),
        max_output_tokens: Some(32_000),
        reasoning_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
        capabilities: json!({ "tools": true, "vision": false, "web_search": false }),
    };
    if bare.contains("gemini")
        || bare.contains("gpt-4")
        || bare.contains("gpt-5")
        || bare.contains("gpt-6")
        || bare.contains("claude")
        || bare.contains("grok")
    {
        meta.capabilities["vision"] = json!(true);
    }
    if bare.contains("gemini")
        || bare.contains("gpt-5")
        || bare.contains("gpt-6")
        || bare.contains("sonar")
    {
        meta.capabilities["web_search"] = json!(true);
    }
    if !bare.contains("gemini")
        && (bare.contains("flash")
            || bare.contains("haiku")
            || bare.contains("mini")
            || bare.contains("chat"))
    {
        meta.reasoning_levels = vec!["off".into(), "low".into()];
    }
    if has_window_token(&lower, "1m") {
        meta.context_window = Some(1_000_000);
    } else if has_window_token(&lower, "200k") {
        meta.context_window = Some(200_000);
    } else if bare.contains("gemini") {
        meta.context_window = Some(1_000_000);
        meta.max_output_tokens = Some(65_536);
    }
    if bare.contains("deepseek") && bare.contains("reasoner") {
        meta.reasoning_levels = vec!["off".into(), "high".into()];
    }
    meta
}

/// Strip smart-gateway routing prefixes (`claude.<model>` or `claude.<provider>.<model>`)
/// so downstream family detection inspects the underlying upstream model id.
fn strip_gateway_routing_prefix(lower: &str) -> &str {
    let Some(rest) = lower.strip_prefix("claude.") else {
        return lower;
    };
    if let Some((prefix, model)) = rest.split_once('.') {
        let is_version_fragment = prefix.ends_with(|c: char| c.is_ascii_digit())
            && model.starts_with(|c: char| c.is_ascii_digit());
        if !is_version_fragment && !model.is_empty() {
            return model;
        }
    }
    rest
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
    fn fast_mode_capability_requires_upstream_evidence() {
        for (value, expected) in [
            (json!({"id": "gpt-5.5"}), None),
            (json!({"service_tiers": []}), Some(false)),
            (json!({"service_tiers": [{"id": "priority"}]}), Some(true)),
            (json!({"service_tiers": ["fast"]}), Some(true)),
            (json!({"additional_speed_tiers": ["fast"]}), Some(true)),
            (json!({"service_tiers": ["flex"]}), Some(false)),
        ] {
            assert_eq!(DiscoveredModelMeta::from_model(&value).priority_supported, expected);
        }
        let old: DiscoveredModelMeta = serde_json::from_value(json!({})).unwrap();
        assert_eq!(old.priority_supported, None);
    }

    #[test]
    fn window_marker_must_be_a_whole_token() {
        assert_eq!(context_window_for("claude-opus-5[1m]"), 1_000_000);
        assert_eq!(context_window_for("some-model-1m"), 1_000_000);
        assert_eq!(context_window_for("some-model-200k"), 200_000);
        // 子串 "1m" 不是窗口标记。
        assert_eq!(context_window_for("foo-v1m-bar"), 200_000);
        assert_eq!(context_window_for("claude-sonnet-5-5-medium"), 200_000);
        assert_eq!(context_window_for("gemini-3.8-flash-high"), 1_000_000);
        assert_eq!(context_window_for("gemini-4-argon-medium"), 1_000_000);
        assert_eq!(context_window_for("claude.gemini-4-argon-medium"), 1_000_000);
        assert_eq!(
            context_window_for("claude.antigravity--built-in.gemini-4-argon-high"),
            1_000_000
        );
    }

    #[test]
    fn gemini_flash_and_argon_infer_full_reasoning_levels() {
        let flash = infer("gemini-3.8-flash-high");
        assert_eq!(
            flash.reasoning_levels,
            vec![
                "off".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string()
            ]
        );
        let argon = infer("gemini-4-argon-medium");
        assert_eq!(
            argon.reasoning_levels,
            vec![
                "off".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string()
            ]
        );
        // Non-Gemini flash/haiku/mini models keep their compact ["off", "low"] levels.
        let ds_flash = infer("deepseek-v4-flash");
        assert_eq!(
            ds_flash.reasoning_levels,
            vec!["off".to_string(), "low".to_string()]
        );
    }
}
