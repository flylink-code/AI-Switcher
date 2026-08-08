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
            .unwrap_or_else(|| "gemini-3-flash".into()),
        "gemini-flash" | "gemini-2.5-flash" | "gemini-3-flash-preview" => {
            model_catalog::preferred_gemini_flash().unwrap_or_else(|| "gemini-3-flash".into())
        }
        "gemini-pro" | "gemini-2.5-pro" | "gemini-3-pro" | "gemini-3-pro-high" => {
            model_catalog::preferred_gemini_pro().unwrap_or_else(|| "gemini-3.1-pro-high".into())
        }
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "o3" | "o4-mini" => {
            model_catalog::preferred_default_model()
        }
        // Claude Desktop effort-slider alias (see GEMINI_FLASH_ALIAS_ID):
        // resolves to the Gemini 3.6 Flash bare base; the reasoning level is
        // chosen by the request's effort field or the bare-name fallback.
        model_catalog::GEMINI_FLASH_ALIAS_ID => model_catalog::preferred_gemini_36_flash_base()
            .unwrap_or_else(|| "gemini-3.6-flash".into()),
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
        { "id": "gemini-3-flash", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3.1-pro-high", "object": "model", "owned_by": "antigravity" },
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
        assert_eq!(map_model_id("gemini-3.1-pro"), "gemini-3.1-pro-high");
        // Explicit level suffix from the client passes through untouched.
        assert_eq!(map_model_id("gemini-3.6-flash-low"), "gemini-3.6-flash-low");
    }

    #[test]
    fn desktop_alias_maps_to_gemini_flash() {
        // Fallback catalog has no gemini-3.6 flash; the alias degrades to the
        // best available flash base (gemini-3-flash here).
        let mapped = map_model_id(model_catalog::GEMINI_FLASH_ALIAS_ID);
        assert!(mapped.starts_with("gemini-"));
        assert!(mapped.contains("flash"));
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
