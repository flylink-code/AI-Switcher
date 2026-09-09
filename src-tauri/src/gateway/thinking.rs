//! Apply mode-level ThinkingConfig onto protocol-specific request bodies.

use serde_json::{json, Value};

use crate::provider::{ProtocolType, ThinkingConfig};

pub fn effort_to_budget(effort: &str) -> Option<u32> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some(2048),
        "medium" => Some(8192),
        "high" | "xhigh" | "max" | "ultra" => Some(16384),
        _ => None,
    }
}

pub fn budget_to_effort(budget: u32) -> &'static str {
    if budget <= 4096 {
        "low"
    } else if budget <= 16384 {
        "medium"
    } else {
        "high"
    }
}

pub fn apply_to_body(body: &mut Value, protocol: ProtocolType, thinking: &ThinkingConfig) {
    match protocol {
        ProtocolType::Anthropic => apply_anthropic(body, thinking),
        ProtocolType::OpenAiChat | ProtocolType::Proxy => apply_openai_chat(body, thinking),
        ProtocolType::OpenAiResponses => apply_openai_responses(body, thinking),
    }
}

fn apply_anthropic(body: &mut Value, thinking: &ThinkingConfig) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    if thinking.is_disabled() {
        object.remove("thinking");
        return;
    }
    let Some(budget) = thinking
        .resolved_budget_tokens()
        .or_else(|| thinking.reasoning_effort.as_deref().and_then(effort_to_budget))
    else {
        return;
    };
    object.insert(
        "thinking".into(),
        json!({ "type": "enabled", "budget_tokens": budget }),
    );
    let max_tokens = object
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if max_tokens < u64::from(budget) + 1 {
        object.insert("max_tokens".into(), json!(budget.saturating_add(4096)));
    }
}

fn apply_openai_chat(body: &mut Value, thinking: &ThinkingConfig) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    if thinking.is_disabled() {
        object.remove("reasoning_effort");
        object.remove("enable_thinking");
        return;
    }
    if let Some(effort) = thinking.resolved_reasoning_effort() {
        object.insert("reasoning_effort".into(), json!(effort));
    }
    if let Some(enabled) = thinking.resolved_enable_thinking() {
        object.insert("enable_thinking".into(), json!(enabled));
    }
}

fn apply_openai_responses(body: &mut Value, thinking: &ThinkingConfig) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    if thinking.is_disabled() {
        object.remove("reasoning");
        return;
    }
    let Some(effort) = thinking.resolved_reasoning_effort() else {
        return;
    };
    let mut reasoning = object
        .get("reasoning")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    reasoning.insert("effort".into(), json!(effort));
    object.insert("reasoning".into(), Value::Object(reasoning));
}

pub fn apply_gemini_thinking_budget(body: &mut Value, thinking: &ThinkingConfig) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let mut config = object
        .get("thinkingConfig")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if thinking.is_disabled() {
        config.insert("thinkingBudget".into(), json!(0));
        object.insert("thinkingConfig".into(), Value::Object(config));
        return;
    }
    if let Some(budget) = thinking.resolved_gemini_thinking_budget() {
        config.insert("thinkingBudget".into(), json!(budget));
        object.insert("thinkingConfig".into(), Value::Object(config));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: &str, effort: Option<&str>, budget: Option<u32>) -> ThinkingConfig {
        ThinkingConfig {
            mode: Some(mode.into()),
            reasoning_effort: effort.map(str::to_string),
            budget_tokens: budget,
            prefix_thought: None,
        }
    }

    #[test]
    fn anthropic_writes_budget_and_raises_max_tokens() {
        let mut body = json!({ "max_tokens": 128 });
        apply_to_body(&mut body, ProtocolType::Anthropic, &cfg("effort", Some("high"), None));
        assert_eq!(body["thinking"]["budget_tokens"], 16384);
        assert!(body["max_tokens"].as_u64().unwrap() > 16384);
    }

    #[test]
    fn disabled_omits_thinking_fields() {
        let mut anthropic = json!({ "thinking": { "type": "enabled", "budget_tokens": 2048 } });
        apply_to_body(&mut anthropic, ProtocolType::Anthropic, &cfg("disabled", None, None));
        assert!(anthropic.get("thinking").is_none());

        let mut chat = json!({ "reasoning_effort": "high" });
        apply_to_body(&mut chat, ProtocolType::OpenAiChat, &cfg("disabled", None, None));
        assert!(chat.get("reasoning_effort").is_none());

        let mut gemini = json!({});
        apply_gemini_thinking_budget(&mut gemini, &cfg("disabled", None, None));
        assert_eq!(gemini["thinkingConfig"]["thinkingBudget"], 0);
    }

    #[test]
    fn responses_writes_reasoning_effort() {
        let mut body = json!({});
        apply_to_body(&mut body, ProtocolType::OpenAiResponses, &cfg("effort", Some("medium"), None));
        assert_eq!(body["reasoning"]["effort"], "medium");
    }
}
