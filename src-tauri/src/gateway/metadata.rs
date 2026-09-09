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
    if lower.contains("gemini") || lower.contains("gpt-4") || lower.contains("gpt-5") || lower.contains("gpt-6")
        || lower.contains("claude") || lower.contains("grok")
    {
        meta.capabilities["vision"] = json!(true);
    }
    if lower.contains("gemini") || lower.contains("gpt-5") || lower.contains("gpt-6") || lower.contains("sonar")
    {
        meta.capabilities["web_search"] = json!(true);
    }
    if lower.contains("flash") || lower.contains("haiku") || lower.contains("mini") || lower.contains("chat")
    {
        meta.reasoning_levels = vec!["off".into(), "low".into()];
    }
    if lower.contains("1m") || lower.contains("[1m]") {
        meta.context_window = Some(1_000_000);
    } else if lower.contains("200k") || lower.contains("[200k]") {
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
