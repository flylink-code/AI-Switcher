//! Model id allow-list and alias mapping for the built-in gateway.

use serde_json::{json, Value};

use crate::antigravity::model_catalog;

pub fn map_model_id(requested: &str) -> String {
    let lower = requested.trim().to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "" => model_catalog::preferred_default_model(),
        "claude-sonnet-4-5"
        | "claude-sonnet-4-5-20250929"
        | "claude-3-5-sonnet-20241022"
        | "claude-3-5-sonnet-latest"
        | "claude-3-7-sonnet-latest" => "claude-sonnet-4-6".into(),
        "claude-sonnet-4-5-thinking" => "claude-sonnet-4-6-thinking".into(),
        "claude-opus-4"
        | "claude-opus-4-5"
        | "claude-opus-4-5-thinking"
        | "claude-opus-4-5-20251101" => "claude-opus-4-6-thinking".into(),
        "claude-opus-4-6" | "claude-opus-4.6" => "claude-opus-4-6-thinking".into(),
        "claude-haiku-4"
        | "claude-haiku-4-5"
        | "claude-3-haiku-20240307"
        | "claude-haiku-4-5-20251001" => model_catalog::preferred_gemini_flash()
            .unwrap_or_else(|| "gemini-3.6-flash-high".into()),
        "gemini-flash"
        | "gemini-2.5-flash"
        | "gemini-3-flash"
        | "gemini-3-flash-preview"
        | "gemini-3.5-flash" => {
            model_catalog::preferred_gemini_flash().unwrap_or_else(|| "gemini-3.6-flash-high".into())
        }
        "gemini-pro"
        | "gemini-2.5-pro"
        | "gemini-3-pro"
        | "gemini-3-pro-high"
        | "gemini-3.1-pro"
        | "gemini-3.1-pro-high"
        | "gemini-3.1-pro-low" => model_catalog::preferred_gemini_pro()
            .or(model_catalog::preferred_gemini_flash())
            .unwrap_or_else(|| "gemini-3.6-flash-high".into()),
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "o3" | "o4-mini" => {
            model_catalog::preferred_default_model()
        }
        other if model_catalog::should_remap_legacy_gemini(other) => model_catalog::preferred_gemini_flash()
            .unwrap_or_else(|| "gemini-3.6-flash-high".into()),
        other => other.to_string(),
    };
    // Explicit level suffixes pass through; bare Gemini names compose to a
    // catalog variant (low → medium → high fallback) here.
    model_catalog::with_reasoning_level(&mapped)
}

/// Map an Anthropic effort value (GA `output_config.effort` or the beta
/// top-level `effort`) to a Gemini level suffix. xhigh/max clamp to high —
/// Gemini variants top out at high.
pub fn map_effort_to_suffix(effort: &str) -> Option<&'static str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" | "xhigh" | "max" => Some("high"),
        _ => None,
    }
}

/// OpenAI-compatible model list for `/v1/models` — prefers live Cloud Code catalog.
pub fn list_public_models() -> Value {
    let live = model_catalog::list_openai_models_payload();
    if live.as_array().is_some_and(|items| !items.is_empty()) {
        return live;
    }
    json!([
        { "id": "claude-sonnet-4-6", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3.6-flash-high", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3.6-flash-low", "object": "model", "owned_by": "antigravity" },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_aliases() {
        assert_eq!(map_model_id("claude-sonnet-4-5"), "claude-sonnet-4-6");
        let flash = map_model_id("gemini-flash");
        assert!(flash.starts_with("gemini-"));
        // Bare public names compose to the real upstream variant.
        assert_eq!(map_model_id("gemini-3.1-pro"), "gemini-3.6-flash-high");
        assert_eq!(map_model_id("gemini-3.1-pro-high"), "gemini-3.6-flash-high");
        // Explicit level suffix from the client passes through untouched.
        assert_eq!(map_model_id("gemini-3.6-flash-low"), "gemini-3.6-flash-low");
    }

    #[test]
    fn claude_sonnet_5_is_not_remapped_to_flash() {
        // Desktop role id may still appear on the wire via role routing; AG no
        // longer synthesizes a Flash alias — pass through as a Claude id.
        assert_eq!(map_model_id("claude-sonnet-5"), "claude-sonnet-5");
    }

    #[test]
    fn maps_effort_to_gemini_suffix() {
        assert_eq!(map_effort_to_suffix("low"), Some("low"));
        assert_eq!(map_effort_to_suffix("MEDIUM"), Some("medium"));
        assert_eq!(map_effort_to_suffix("high"), Some("high"));
        // Flash tops out at high — xhigh/max clamp down.
        assert_eq!(map_effort_to_suffix("xhigh"), Some("high"));
        assert_eq!(map_effort_to_suffix("max"), Some("high"));
        assert_eq!(map_effort_to_suffix("bogus"), None);
    }
}
