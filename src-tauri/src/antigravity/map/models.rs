//! Model id allow-list and alias mapping for the built-in gateway.

use serde_json::{json, Value};

use crate::antigravity::model_catalog;

pub fn map_model_id(requested: &str) -> String {
    let lower = requested.trim().to_ascii_lowercase();
    let lower = strip_claude_catalog_alias(&lower);
    let mapped = match lower {
        "" => model_catalog::preferred_default_model(),
        // Claude 4.x left Cloud Code; legacy aliases land on the 5.5 tiers.
        "claude-sonnet-4"
        | "claude-sonnet-4-5"
        | "claude-sonnet-4-5-20250929"
        | "claude-sonnet-4-5-thinking"
        | "claude-sonnet-4-6"
        | "claude-sonnet-4-6-thinking"
        | "claude-3-5-sonnet-20241022"
        | "claude-3-5-sonnet-latest"
        | "claude-3-7-sonnet-latest" => model_catalog::pick_claude_55_tier("sonnet"),
        "claude-opus-4"
        | "claude-opus-4-5"
        | "claude-opus-4-5-thinking"
        | "claude-opus-4-5-20251101"
        | "claude-opus-4-6"
        | "claude-opus-4.6"
        | "claude-opus-4-6-thinking" => model_catalog::pick_claude_55_tier("opus"),
        "claude-haiku-4"
        | "claude-haiku-4-5"
        | "claude-3-haiku-20240307"
        | "claude-haiku-4-5-20251001" => model_catalog::preferred_gemini_flash()
            .unwrap_or_else(|| "gemini-3.8-flash-high".into()),
        "gemini-flash"
        | "gemini-2.5-flash"
        | "gemini-3-flash"
        | "gemini-3-flash-preview"
        | "gemini-3.5-flash" => model_catalog::preferred_gemini_flash()
            .unwrap_or_else(|| "gemini-3.8-flash-high".into()),
        "gemini-pro" | "gemini-2.5-pro" | "gemini-3-pro" | "gemini-3-pro-high" => {
            model_catalog::preferred_gemini_pro()
                .or(model_catalog::preferred_gemini_flash())
                .unwrap_or_else(|| "gemini-3.8-flash-high".into())
        }
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "o3" | "o4-mini" => {
            model_catalog::preferred_default_model()
        }
        other if model_catalog::should_remap_legacy_gemini(other) => {
            model_catalog::preferred_gemini_flash()
                .unwrap_or_else(|| "gemini-3.8-flash-high".into())
        }
        other => model_catalog::map_claude_55_id(other).unwrap_or_else(|| other.to_string()),
    };
    // Explicit level suffixes pass through; bare Gemini names compose to a
    // catalog variant (high → medium → low fallback) here.
    model_catalog::with_reasoning_level(&mapped)
}

/// Unified catalog ids look like `claude.antigravity--built-in.gemini-3.6-flash-low`.
/// Independent / direct gateway calls may send that public id unchanged.
fn strip_claude_catalog_alias(id: &str) -> &str {
    let Some(rest) = id.strip_prefix("claude.") else {
        return id;
    };
    rest.split_once('.').map(|(_, model)| model).unwrap_or(rest)
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
    let fallback_ids = [
        "claude-sonnet-5-5-medium",
        "claude-sonnet-5-5-high",
        "claude-sonnet-5-5-low",
        "claude-opus-5-5-medium",
        "claude-opus-5-5-high",
        "claude-opus-5-5-low",
        "gemini-3.8-flash-high",
        "gemini-4-argon-high",
        "gemini-4-argon-medium",
        "gemini-4-argon-low",
    ];
    let items: Vec<Value> = fallback_ids
        .into_iter()
        .map(|id| {
            let context_window = crate::gateway::metadata::context_window_for(id);
            let max_output = crate::gateway::metadata::max_output_for(id, context_window);
            json!({
                "id": id,
                "object": "model",
                "owned_by": "antigravity",
                "context_window": context_window,
                "max_input_tokens": context_window,
                "max_output_tokens": max_output,
                "max_tokens": max_output,
            })
        })
        .collect();
    json!(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_aliases() {
        assert_eq!(
            map_model_id("claude-sonnet-4-5"),
            map_model_id("claude-sonnet-5-5")
        );
        assert_eq!(map_model_id("claude-sonnet-4-6"), "claude-sonnet-5-5-medium");
        let flash = map_model_id("gemini-flash");
        assert!(flash.starts_with("gemini-"));
        // 3.1 Pro is gone from Cloud Code: remap to Flash. Gemini 4 Argon is
        // live; the bare name composes to `-high`, explicit tiers pass through.
        assert_eq!(map_model_id("gemini-3.1-pro"), map_model_id("gemini-flash"));
        assert_eq!(map_model_id("gemini-3.1-pro-high"), map_model_id("gemini-flash"));
        assert_eq!(map_model_id("gemini-4-argon"), "gemini-4-argon-high");
        assert_eq!(map_model_id("gemini-4-argon-medium"), "gemini-4-argon-medium");
        assert_eq!(map_model_id("gemini-4-argon-low"), "gemini-4-argon-low");
        // 3.6 / 3.7 left Cloud Code and follow the current Flash; bare 3.8
        // composes to the live Cloud Code variant (`-high`).
        assert_eq!(map_model_id("gemini-3.6-flash"), flash);
        assert_eq!(map_model_id("gemini-3.7-flash-low"), flash);
        assert_eq!(map_model_id("gemini-3.8-flash"), "gemini-3.8-flash-high");
        let haiku = map_model_id("claude-haiku-4-5");
        assert!(
            haiku.ends_with("-high") || haiku.ends_with("flash"),
            "Haiku aliases now follow the main flash SKU, got {haiku}"
        );
        assert_eq!(
            map_model_id("claude.antigravity--built-in.gemini-3.8-flash-low"),
            "gemini-3.8-flash-low"
        );
    }

    #[test]
    fn claude_sonnet_5_is_not_remapped_to_flash() {
        // Bare Claude 5 names are the 5.5 family. They must not fall through to
        // Flash, and they must not leave as the bare id Cloud Code 404s.
        assert_eq!(map_model_id("claude-sonnet-5"), "claude-sonnet-5-5-medium");
        assert!(!map_model_id("claude-sonnet-5").contains("flash"));
    }

    #[test]
    fn claude_55_aliases_keep_explicit_suffix() {
        assert_eq!(map_model_id("claude-opus-5"), "claude-opus-5-5-medium");
        assert_eq!(map_model_id("claude-opus-5.5"), "claude-opus-5-5-medium");
        assert_eq!(map_model_id("claude-opus-5-5"), "claude-opus-5-5-medium");
        assert_eq!(map_model_id("claude-opus-5.5-high"), "claude-opus-5-5-high");
        assert_eq!(map_model_id("claude-opus-5-5-low"), "claude-opus-5-5-low");
        assert_eq!(
            map_model_id("claude-sonnet-5.5-high"),
            "claude-sonnet-5-5-high"
        );
        assert_eq!(map_model_id("Claude-Opus-5-high"), "claude-opus-5-5-high");
        assert_eq!(map_model_id("claude-opus-4-6"), "claude-opus-5-5-medium");
        assert_eq!(map_model_id("claude-sonnet-4-6"), "claude-sonnet-5-5-medium");
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
