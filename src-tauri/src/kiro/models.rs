//! Public Kiro model ids and the normalization used on the way out.
//!
//! The id the client sent is echoed back on the response. Only the upstream
//! payload uses the normalized id. `-thinking` is a request flag, not a catalog row.

pub const DEFAULT_MODEL: &str = "claude-sonnet-4.6";
pub const THINKING_BUDGET_CAP: u32 = 24_576;

const FAMILIES: [&str; 5] = ["sonnet", "opus", "haiku", "fable", "mythos"];

/// Models the local `/v1/models` list exposes. No synthetic `-thinking` ids.
pub fn catalog_ids() -> &'static [&'static str] {
    &[
        "claude-sonnet-4.5",
        "claude-sonnet-4.6",
        "claude-sonnet-4.8",
        "claude-opus-4.5",
        "claude-opus-4.6",
        "claude-opus-4.7",
        "claude-opus-4.8",
        "claude-haiku-4.5",
        "claude-fable-5",
    ]
}

pub fn preferred_default_model() -> String {
    DEFAULT_MODEL.to_string()
}

pub fn preferred_opus() -> String {
    "claude-opus-4.6".to_string()
}

pub fn preferred_haiku() -> String {
    "claude-haiku-4.5".to_string()
}

/// Strip thinking/latest/date suffixes and rewrite dated Claude ids.
/// Non-Claude ids pass through unchanged when they are non-empty.
pub fn map_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.chars().any(|ch| ch.is_control()) {
        return None;
    }
    normalize_claude_model(trimmed).or_else(|| Some(trimmed.to_string()))
}

pub fn normalize_claude_model(model: &str) -> Option<String> {
    let mut normalized = model.to_ascii_lowercase();
    loop {
        let mut stripped = false;
        for suffix in ["-thinking", "-latest"] {
            if let Some(next) = normalized.strip_suffix(suffix) {
                normalized = next.to_string();
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    if let Some((base, suffix)) = normalized.rsplit_once('-') {
        if suffix.len() == 8 && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            normalized = base.to_string();
        }
    }
    let body = normalized.strip_prefix("claude-")?;
    for family in FAMILIES {
        if let Some(rest) = body.strip_prefix(family) {
            let rest = rest
                .strip_prefix('-')
                .or_else(|| rest.strip_prefix('.'))
                .unwrap_or(rest);
            let parts: Vec<&str> = rest.split('-').filter(|part| !part.is_empty()).collect();
            let version = canonical_version(&parts)?;
            return Some(format!("claude-{family}-{version}"));
        }
    }
    let parts: Vec<&str> = body.split('-').collect();
    let family_index = parts.iter().position(|part| FAMILIES.contains(part))?;
    if family_index == 0 || family_index + 1 != parts.len() {
        return None;
    }
    let version = canonical_version(&parts[..family_index])?;
    Some(format!("claude-{}-{version}", parts[family_index]))
}

fn canonical_version(parts: &[&str]) -> Option<String> {
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    if !parts.iter().all(|part| part.chars().all(|ch| ch.is_ascii_digit())) {
        return None;
    }
    Some(parts.join("."))
}

/// `enabled` / `adaptive` count. `disabled` and an empty object do not.
pub fn thinking_requested(value: &serde_json::Value) -> bool {
    let thinking = value.get("thinking").filter(|item| !item.is_null());
    match thinking {
        Some(serde_json::Value::Object(map)) if map.is_empty() => false,
        Some(item) => {
            let kind = item.get("type").and_then(|kind| kind.as_str()).unwrap_or("");
            !kind.eq_ignore_ascii_case("disabled")
        }
        None => value
            .get("model")
            .and_then(|model| model.as_str())
            .is_some_and(|model| model.to_ascii_lowercase().ends_with("-thinking")),
    }
}

pub fn clamped_thinking_budget(value: &serde_json::Value) -> Option<u32> {
    if !thinking_requested(value) {
        return None;
    }
    let raw = value
        .pointer("/thinking/budget_tokens")
        .and_then(|item| item.as_u64())
        .unwrap_or(20_000);
    Some(u32::try_from(raw).unwrap_or(THINKING_BUDGET_CAP).min(THINKING_BUDGET_CAP))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dated_sonnet_and_thinking_suffix_normalize() {
        assert_eq!(
            map_model("claude-3-5-sonnet-20241022").as_deref(),
            Some("claude-sonnet-3.5")
        );
        assert_eq!(
            map_model("claude-sonnet-4-6-thinking").as_deref(),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(map_model("glm-5").as_deref(), Some("glm-5"));
    }

    #[test]
    fn thinking_budget_caps_and_ignores_disabled() {
        assert_eq!(
            clamped_thinking_budget(&json!({"thinking": {"type": "enabled", "budget_tokens": 99999}})),
            Some(THINKING_BUDGET_CAP)
        );
        assert_eq!(
            clamped_thinking_budget(&json!({"thinking": {"type": "disabled"}})),
            None
        );
        assert_eq!(clamped_thinking_budget(&json!({"thinking": {}})), None);
        assert_eq!(
            clamped_thinking_budget(&json!({"model": "claude-sonnet-4.6-thinking"})),
            Some(20_000)
        );
    }
}
