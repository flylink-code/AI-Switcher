//! Configured route rules (model-prefix / condition). No JS scripts.

use serde_json::Value;

use crate::database::dao::gateway::RouteRule;
use crate::provider::ThinkingConfig;
use crate::gateway::modes::ModeSignals;

const PROTECTED_HEADERS: &[&str] = &[
    "authorization",
    "api-key",
    "x-api-key",
    "cookie",
    "host",
    "content-length",
    "connection",
];

#[derive(Debug, Clone)]
pub struct RuleHit {
    pub rule_id: String,
    pub target_model: String,
    pub thinking: Option<ThinkingConfig>,
    pub rewrites: Vec<Value>,
}

pub fn match_rules(rules: &[RouteRule], requested_model: &str, signals: &ModeSignals) -> Option<RuleHit> {
    let mut ranked = rules.to_vec();
    ranked.sort_by_key(|rule| rule.sort_index);
    for rule in ranked {
        if !rule.enabled {
            continue;
        }
        if rule_matches(&rule, requested_model, signals) {
            let thinking = serde_json::from_str(&rule.thinking_config_json).ok();
            let rewrites = serde_json::from_str::<Vec<Value>>(&rule.rewrites_json).unwrap_or_default();
            return Some(RuleHit {
                rule_id: rule.id,
                target_model: rule.target_model,
                thinking,
                rewrites,
            });
        }
    }
    None
}

fn rule_matches(rule: &RouteRule, requested_model: &str, signals: &ModeSignals) -> bool {
    match rule.rule_type.as_str() {
        "model-prefix" => {
            let pattern = rule.pattern.trim();
            !pattern.is_empty() && requested_model.starts_with(pattern)
        }
        "condition" => evaluate_condition(&rule.condition_json, signals),
        _ => false,
    }
}

fn evaluate_condition(raw: &str, signals: &ModeSignals) -> bool {
    let Ok(condition) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    let left = condition.get("left").and_then(Value::as_str).unwrap_or("");
    let operator = condition.get("operator").and_then(Value::as_str).unwrap_or("==");
    let right = condition
        .get("right")
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|n| n.to_string()))
                .or_else(|| value.as_bool().map(|flag| flag.to_string()))
        })
        .unwrap_or_default();
    let actual = match left {
        "token_count" => signals.token_count.to_string(),
        "thinking" => signals.has_thinking.to_string(),
        "web_search" => signals.has_web_search.to_string(),
        "vision" => signals.has_vision.to_string(),
        "tool" => signals.tool_names.join(","),
        "path" => signals.path.clone(),
        "target_app" => signals
            .target
            .map(|target| target.as_str().to_string())
            .unwrap_or_default(),
        _ => return false,
    };
    match operator {
        "==" => actual.eq_ignore_ascii_case(&right),
        "!=" => !actual.eq_ignore_ascii_case(&right),
        "contains" => actual.to_ascii_lowercase().contains(&right.to_ascii_lowercase()),
        "starts-with" => actual.starts_with(&right),
        ">" => actual.parse::<i64>().ok().zip(right.parse::<i64>().ok()).is_some_and(|(a, b)| a > b),
        ">=" => actual.parse::<i64>().ok().zip(right.parse::<i64>().ok()).is_some_and(|(a, b)| a >= b),
        "<" => actual.parse::<i64>().ok().zip(right.parse::<i64>().ok()).is_some_and(|(a, b)| a < b),
        "<=" => actual.parse::<i64>().ok().zip(right.parse::<i64>().ok()).is_some_and(|(a, b)| a <= b),
        _ => false,
    }
}

pub fn apply_rewrites(body: &mut Value, headers: &mut axum::http::HeaderMap, rewrites: &[Value]) {
    for rewrite in rewrites {
        let path = rewrite.get("path").and_then(Value::as_str).unwrap_or("").trim();
        let Some(value) = rewrite.get("value") else {
            continue;
        };
        if let Some(header_name) = path.strip_prefix("request.headers.") {
            if is_protected_rewrite_header(header_name) {
                continue;
            }
            if let Ok(name) = axum::http::HeaderName::try_from(header_name) {
                if let Some(text) = value.as_str() {
                    if let Ok(header_value) = axum::http::HeaderValue::from_str(text) {
                        headers.insert(name, header_value);
                    }
                }
            }
            continue;
        }
        if let Some(body_path) = path.strip_prefix("request.body.") {
            if let Some(object) = body.as_object_mut() {
                object.insert(body_path.to_string(), value.clone());
            }
        }
    }
}

pub fn is_protected_rewrite_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    PROTECTED_HEADERS.contains(&lower.as_str())
        || lower.starts_with("x-auth-")
        || lower.starts_with("x-aisw-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dao::gateway::RouteRule;

    #[test]
    fn prefix_rule_hits_before_unrelated() {
        let rule = RouteRule {
            id: "r1".into(),
            profile_id: "gprof_shared".into(),
            enabled: true,
            sort_index: 0,
            rule_type: "model-prefix".into(),
            condition_json: "{}".into(),
            pattern: "deepseek.".into(),
            target_model: "openai.gpt".into(),
            thinking_config_json: "{}".into(),
            rewrites_json: "[]".into(),
        };
        let hit = match_rules(&[rule], "deepseek.chat", &ModeSignals::default());
        assert_eq!(hit.unwrap().target_model, "openai.gpt");
    }

    #[test]
    fn protected_headers_cannot_be_rewritten() {
        assert!(is_protected_rewrite_header("Authorization"));
        assert!(is_protected_rewrite_header("x-aisw-request-id"));
        assert!(!is_protected_rewrite_header("x-target-provider"));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("authorization", axum::http::HeaderValue::from_static("secret"));
        let mut body = serde_json::json!({});
        apply_rewrites(
            &mut body,
            &mut headers,
            &[serde_json::json!({"path": "request.headers.authorization", "value": "hacked"})],
        );
        assert_eq!(headers.get("authorization").unwrap(), "secret");
    }
}
