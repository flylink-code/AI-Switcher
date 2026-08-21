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
    if include_thoughts {
        generation["thinkingConfig"]["includeThoughts"] = json!(true);
    }
}

pub fn is_budget_constraint_error(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    (lower.contains("budget") || lower.contains("thinkingbudget") || lower.contains("thinking_budget"))
        && (lower.contains("invalid")
            || lower.contains("constraint")
            || lower.contains("must be")
            || lower.contains("greater than")
            || lower.contains("less than")
            || lower.contains("exceed"))
}

pub fn rectify_generate_request(request: &mut Value) {
    let generation = request
        .get_mut("generationConfig")
        .filter(|value| value.is_object());
    let Some(generation) = generation else {
        request["generationConfig"] = json!({
            "thinkingConfig": { "thinkingBudget": RECTIFY_BUDGET, "includeThoughts": true },
            "maxOutputTokens": RECTIFY_MIN_MAX_TOKENS,
        });
        return;
    };
    generation["thinkingConfig"]["thinkingBudget"] = json!(RECTIFY_BUDGET);
    let max = generation
        .get("maxOutputTokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if max <= u64::from(RECTIFY_BUDGET) {
        generation["maxOutputTokens"] = json!(RECTIFY_MIN_MAX_TOKENS);
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
            resolve_thinking_budget(&body, "gemini-3.7-flash-high"),
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
}
