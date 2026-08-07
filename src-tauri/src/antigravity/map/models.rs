//! Model id allow-list and alias mapping for the built-in gateway.

use serde_json::{json, Value};

pub fn map_model_id(requested: &str) -> String {
    let lower = requested.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" => "claude-sonnet-4-6".into(),
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
        | "claude-haiku-4-5-20251001" => "claude-sonnet-4-6".into(),
        "gemini-flash"
        | "gemini-2.5-flash"
        | "gemini-3-flash-preview"
        | "gemini-3.1-flash-lite" => "gemini-3-flash".into(),
        "gemini-pro"
        | "gemini-2.5-pro"
        | "gemini-3-pro"
        | "gemini-3.1-pro"
        | "gemini-3.1-pro-low" => "gemini-3.1-pro-high".into(),
        "gemini-3-pro-high" => "gemini-3.1-pro-high".into(),
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "o3" | "o4-mini" => "claude-sonnet-4-6".into(),
        other => other.to_string(),
    }
}

pub fn list_public_models() -> Value {
    // Keep Claude + the Gemini ids Cloud Code currently exposes (from fetchAvailableModels).
    json!([
        { "id": "claude-sonnet-4-6", "object": "model", "owned_by": "antigravity" },
        { "id": "claude-sonnet-4-6-thinking", "object": "model", "owned_by": "antigravity" },
        { "id": "claude-opus-4-6-thinking", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3-flash", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3-flash-agent", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3.1-pro-high", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-3.1-pro-low", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-2.5-flash", "object": "model", "owned_by": "antigravity" },
        { "id": "gemini-2.5-pro", "object": "model", "owned_by": "antigravity" },
    ])
}

/// Ids seeded into provider forms / failover suggestions for Antigravity presets.
pub fn public_model_ids() -> Vec<&'static str> {
    vec![
        "claude-sonnet-4-6",
        "claude-sonnet-4-6-thinking",
        "claude-opus-4-6-thinking",
        "gemini-3-flash",
        "gemini-3-flash-agent",
        "gemini-3.1-pro-high",
        "gemini-3.1-pro-low",
        "gemini-2.5-flash",
        "gemini-2.5-pro",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_aliases() {
        assert_eq!(map_model_id("claude-sonnet-4-5"), "claude-sonnet-4-6");
        assert_eq!(map_model_id("gpt-4o"), "claude-sonnet-4-6");
        assert_eq!(map_model_id("gemini-flash"), "gemini-3-flash");
        assert_eq!(map_model_id("gemini-3-pro-high"), "gemini-3.1-pro-high");
    }

    #[test]
    fn public_list_includes_gemini() {
        let ids = public_model_ids();
        assert!(ids.iter().any(|id| id.starts_with("gemini-")));
        assert!(ids.iter().any(|id| id.starts_with("claude-")));
    }
}
