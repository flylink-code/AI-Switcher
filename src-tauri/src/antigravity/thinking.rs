//! Gemini thinkingBudget mapping and 400 rectifier helpers.

use serde_json::{json, Value};

pub const ADAPTIVE_HIGH_BUDGET: u32 = 24_576;
pub const FLASH_BUDGET_CAP: u32 = 24_576;
pub const RECTIFY_BUDGET: u32 = 8_192;
pub const RECTIFY_MIN_MAX_TOKENS: u32 = 16_384;

pub fn extract_budget_tokens(body: &Value) -> Option<u32> {
    let thinking = body.get("thinking")?;
    thinking
        .get("budget_tokens")
        .or_else(|| thinking.get("budgetTokens"))
        .or_else(|| thinking.get("budget"))
        .and_then(json_u32)
        .or_else(|| {
            body.pointer("/thinkingConfig/thinkingBudget")
                .and_then(json_u32)
        })
}

pub fn thinking_kind(body: &Value) -> Option<String> {
    body.get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

pub fn resolve_thinking_budget(body: &Value, model: &str) -> Option<u32> {
    if crate::antigravity::model_catalog::is_gemini_31_pro(model) {
        return None;
    }
    let kind = thinking_kind(body);
    if kind.as_deref() == Some("disabled") {
        return None;
    }
    let mut budget = extract_budget_tokens(body);
    if budget.is_none() && kind.as_deref() == Some("adaptive") {
        budget = Some(ADAPTIVE_HIGH_BUDGET);
    }
    let mut budget = budget?;
    if is_flash_model(model) {
        budget = budget.min(FLASH_BUDGET_CAP);
    }
    Some(budget.max(1))
}

pub fn pad_max_tokens(max_tokens: Option<u64>, budget: u32) -> u64 {
    let need = u64::from(budget).saturating_add(1);
    max_tokens.unwrap_or(0).max(need)
}

pub fn apply_thinking_budget(generation: &mut Value, budget: u32, include_thoughts: bool) {
    generation["thinkingConfig"]["thinkingBudget"] = json!(budget);
    if let Some(config) = generation
        .get_mut("thinkingConfig")
        .and_then(Value::as_object_mut)
    {
        config.remove("thinkingLevel");
        if include_thoughts {
            config.insert("includeThoughts".into(), json!(true));
        }
    }
}

pub fn is_budget_constraint_error(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    (lower.contains("budget")
        || lower.contains("thinkingbudget")
        || lower.contains("thinking_budget"))
        && (lower.contains("invalid")
            || lower.contains("constraint")
            || lower.contains("must be")
            || lower.contains("greater than")
            || lower.contains("less than")
            || lower.contains("exceed"))
}

pub fn rectify_generate_request(request: &mut Value, model: &str) {
    let budget =
        crate::antigravity::model_catalog::thinking_budget_for(model).unwrap_or(RECTIFY_BUDGET);
    let min_max = u64::from(budget)
        .saturating_add(1)
        .max(u64::from(RECTIFY_MIN_MAX_TOKENS));
    let generation = request
        .get_mut("generationConfig")
        .filter(|value| value.is_object());
    let Some(generation) = generation else {
        request["generationConfig"] = json!({
            "thinkingConfig": { "thinkingBudget": budget, "includeThoughts": true },
            "maxOutputTokens": min_max,
        });
        return;
    };
    apply_thinking_budget(generation, budget, true);
    let max = generation
        .get("maxOutputTokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if max <= u64::from(budget) {
        generation["maxOutputTokens"] = json!(min_max);
    }
}

fn is_flash_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("flash") && !lower.contains("image")
}

fn json_u32(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .map(|n| n as u32)
        .or_else(|| value.as_i64().filter(|n| *n > 0).map(|n| n as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_defaults_to_high_budget() {
        let body = json!({ "thinking": { "type": "adaptive" } });
        assert_eq!(
            resolve_thinking_budget(&body, "gemini-3.8-flash-high"),
            Some(FLASH_BUDGET_CAP)
        );
    }

    #[test]
    fn pads_max_tokens_above_budget() {
        assert_eq!(pad_max_tokens(Some(100), 8_192), 8_193);
        assert_eq!(pad_max_tokens(Some(20_000), 8_192), 20_000);
    }

    #[test]
    fn detects_budget_constraint_copy() {
        assert!(is_budget_constraint_error(
            "Invalid thinkingBudget: must be less than maxOutputTokens"
        ));
        assert!(!is_budget_constraint_error("missing thought_signature"));
    }

    #[test]
    fn gemini_31_pro_does_not_resolve_thinking_budget() {
        let body = json!({
            "thinking": { "type": "enabled", "budget_tokens": 8192 },
            "thinkingConfig": { "thinkingBudget": 16384 }
        });
        assert_eq!(resolve_thinking_budget(&body, "gemini-3.1-pro-high"), None);
        assert_eq!(resolve_thinking_budget(&body, "gemini-3.1-pro-low"), None);
    }

    #[test]
    fn apply_thinking_budget_strips_thinking_level() {
        let mut generation = json!({
            "thinkingConfig": { "thinkingLevel": "HIGH", "thinkingBudget": 8192 }
        });
        apply_thinking_budget(&mut generation, 10_001, true);
        assert_eq!(
            generation["thinkingConfig"]["thinkingBudget"],
            json!(10_001)
        );
        assert_eq!(generation["thinkingConfig"]["includeThoughts"], json!(true));
        assert!(generation["thinkingConfig"].get("thinkingLevel").is_none());
    }

    #[test]
    fn rectify_uses_31_pro_budget_not_generic_8192() {
        let mut request = json!({
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "HIGH" },
                "maxOutputTokens": 64
            }
        });
        rectify_generate_request(&mut request, "gemini-3.1-pro-high");
        assert_eq!(
            request["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            json!(10_001)
        );
        assert!(request["generationConfig"]["thinkingConfig"]
            .get("thinkingLevel")
            .is_none());
        assert_eq!(
            request["generationConfig"]["maxOutputTokens"],
            json!(16_384)
        );
    }
}
