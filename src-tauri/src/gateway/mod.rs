//! Smart gateway routing: decisions, execution plans, and loop detection.
//!
//! Protocol adaptation still lives in `crate::proxy`. This module decides *which*
//! upstream model to call and records why.

pub mod correlation;
pub mod metadata;
pub mod service;
pub mod health;
pub mod inbound;
pub mod upstream_limits;
pub mod count_tokens;

pub const PARENT_SESSION_HEADER: &str = "x-cs-parent-session-id";

use serde::{Deserialize, Serialize};
#[cfg(test)]
use ts_rs::TS;

use crate::database::dao::gateway::GatewayProfile;
use crate::provider::{ProviderKind, ThinkingConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum RouteSource {
    Explicit,
    Rule,
    Auto,
    RolePlan,
    RoleExecute,
    RoleSubagent,
    Plan,
    Think,
    Edit,
    LongContext,
    WebSearch,
    Vision,
    ImageGen,
    ProfileDefault,
}

impl RouteSource {
    pub fn as_reason(self) -> &'static str {
        match self {
            RouteSource::Explicit => "explicit_model",
            RouteSource::Rule => "rule",
            RouteSource::Auto => "auto",
            RouteSource::RolePlan | RouteSource::Plan => "plan",
            RouteSource::RoleExecute => "edit",
            RouteSource::RoleSubagent => "background",
            RouteSource::Think => "think",
            RouteSource::Edit => "edit",
            RouteSource::LongContext => "long_context",
            RouteSource::WebSearch => "web_search",
            RouteSource::Vision => "vision",
            RouteSource::ImageGen => "image_gen",
            RouteSource::ProfileDefault => "profile_default",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub requested_model: String,
    pub normalized_model: String,
    pub source: RouteSource,
    pub reason: String,
    pub profile_id: Option<String>,
    pub upstream_id: Option<String>,
    pub diagnostics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub rewrites: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAttemptPlan {
    pub index: usize,
    pub model: String,
    pub upstream_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteExecutionPlan {
    pub attempts: Vec<RouteAttemptPlan>,
    pub fallback_mode: String,
    pub primary_model: String,
    #[serde(default)]
    pub explicit_pinned: bool,
}

/// Loopback URLs that point at this app's gateway listeners, not Antigravity (15830).
pub fn is_self_referential_upstream(url: &str, gateway_ports: &[u16]) -> bool {
    let trimmed = url.trim().to_ascii_lowercase();
    let Some(hostport) = extract_host_port(&trimmed) else {
        return false;
    };
    gateway_ports.iter().any(|port| {
        hostport == format!("127.0.0.1:{port}")
            || hostport == format!("localhost:{port}")
            || hostport == format!("[::1]:{port}")
            || hostport == format!("::1:{port}")
    })
}

fn extract_host_port(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let hostport = rest.split('/').next()?.trim();
    if hostport.is_empty() {
        return None;
    }
    Some(hostport.to_string())
}

pub fn default_gateway_ports() -> [u16; 8] {
    [15821, 15822, 15823, 15824, 15825, 15826, 15827, SMART_GATEWAY_PORT]
}

pub const SMART_GATEWAY_PORT: u16 = 15828;

pub fn reserved_listener_ports() -> [u16; 10] {
    [15821, 15822, 15823, 15824, 15825, 15826, 15827, SMART_GATEWAY_PORT, 15830, 15831]
}

pub fn assert_not_self_referential(url: &str) -> crate::error::AppResult<()> {
    if is_self_referential_upstream(url, &default_gateway_ports()) {
        return Err(crate::error::AppError::Config(
            "不能把智能网关自身地址加为上游（检测到本机网关端口循环）".to_string(),
        ));
    }
    Ok(())
}

pub fn assert_not_managed_gateway_kind(kind: ProviderKind) -> crate::error::AppResult<()> {
    if kind == ProviderKind::SmartGateway {
        return Err(crate::error::AppError::Config(
            "智能网关 Auto 卡不能作为上游".to_string(),
        ));
    }
    Ok(())
}

pub fn is_auto_model_id(model: &str) -> bool {
    crate::catalog::is_auto_public_id(model) || model.trim().is_empty()
}

/// Canonical Auto id stored on the Auto card / in SQLite (`auto`, not `claude.auto`).
/// Leftover `opusplan` is treated as auto; an explicit catalog id is kept.
pub fn normalize_live_model(model: &str) -> String {
    let trimmed = model.trim();
    if is_auto_model_id(trimmed) || trimmed.eq_ignore_ascii_case("opusplan") {
        crate::catalog::AUTO_PUBLIC_ID.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Live model written to an Agent. Auto and retired Opus Plan are blank so the
/// caller can substitute the selected or first catalog model.
pub fn normalize_live_model_for(
    _target: crate::provider::ProviderTarget,
    model: &str,
) -> String {
    let trimmed = model.trim();
    if is_auto_model_id(trimmed) || trimmed.eq_ignore_ascii_case("opusplan") {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// Plaintext gateway entry token. Keyring refs and empty strings are not usable live credentials.
pub fn resolved_gateway_token(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("kr://") {
        None
    } else {
        Some(trimmed)
    }
}

pub fn smart_gateway_provider_id(target: crate::provider::ProviderTarget) -> String {
    format!("sgw_{}", target.as_str())
}

pub fn smart_gateway_live_endpoint(
    target: crate::provider::ProviderTarget,
    port: u16,
) -> (crate::provider::ProtocolType, String) {
    use crate::provider::{ProtocolType, ProviderTarget};
    match target {
        ProviderTarget::Codex | ProviderTarget::Cline => {
            (ProtocolType::OpenAiResponses, format!("http://127.0.0.1:{port}/v1"))
        }
        ProviderTarget::OpenCode => (ProtocolType::Anthropic, format!("http://127.0.0.1:{port}/v1")),
        _ => (ProtocolType::Anthropic, format!("http://127.0.0.1:{port}")),
    }
}

pub fn estimate_request_tokens(body: &serde_json::Value) -> u32 {
    let mut total = 0u32;
    for key in ["messages", "system", "input", "instructions", "tools"] {
        if let Some(value) = body.get(key) {
            accumulate_text_tokens(value, &mut total);
        }
    }
    total.max(1)
}

fn accumulate_text_tokens(value: &serde_json::Value, total: &mut u32) {
    match value {
        serde_json::Value::String(text) => {
            *total = total.saturating_add(estimate_text_tokens(text));
        }
        serde_json::Value::Array(items) => {
            for item in items {
                accumulate_text_tokens(item, total);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "type" | "role" | "id" | "model" | "cache_control" | "signature"
                ) {
                    continue;
                }
                accumulate_text_tokens(child, total);
            }
        }
        _ => {}
    }
}

fn estimate_text_tokens(text: &str) -> u32 {
    let mut tokens = 0u32;
    let mut ascii_run = 0u32;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii_run = ascii_run.saturating_add(1);
            continue;
        }
        if ascii_run > 0 {
            tokens = tokens.saturating_add(ascii_run.div_ceil(4));
            ascii_run = 0;
        }
        tokens = tokens.saturating_add(1);
    }
    if ascii_run > 0 {
        tokens = tokens.saturating_add(ascii_run.div_ceil(4));
    }
    tokens
}

fn chain_for_source(profile: &GatewayProfile, source: RouteSource) -> &[String] {
    match source {
        RouteSource::RolePlan | RouteSource::Plan => &profile.plan_fallback,
        RouteSource::RoleExecute | RouteSource::Edit => &profile.execute_fallback,
        RouteSource::RoleSubagent => &profile.subagent_fallback,
        RouteSource::Explicit if profile.explicit_fallback_enabled => &profile.fallback_models,
        RouteSource::Explicit
        | RouteSource::Rule
        | RouteSource::Auto
        | RouteSource::Think
        | RouteSource::LongContext
        | RouteSource::WebSearch
        | RouteSource::Vision
        | RouteSource::ImageGen
        | RouteSource::ProfileDefault => &profile.fallback_models,
    }
}

pub fn build_execution_plan(
    profile: Option<&GatewayProfile>,
    primary_model: &str,
    primary_upstream_id: Option<&str>,
    source: RouteSource,
) -> RouteExecutionPlan {
    build_execution_plan_with_chain(profile, primary_model, primary_upstream_id, source, None)
}

pub fn build_execution_plan_with_chain(
    profile: Option<&GatewayProfile>,
    primary_model: &str,
    primary_upstream_id: Option<&str>,
    source: RouteSource,
    extra_chain: Option<&[String]>,
) -> RouteExecutionPlan {
    let fallback_mode = profile
        .map(|profile| profile.fallback_mode.as_str())
        .unwrap_or("off");
    let mut attempts = vec![RouteAttemptPlan {
        index: 0,
        model: primary_model.to_string(),
        upstream_id: primary_upstream_id.map(str::to_string),
    }];
    let explicit_pinned = matches!(source, RouteSource::Explicit)
        && profile.map(|profile| !profile.explicit_fallback_enabled).unwrap_or(true);
    let mode_chain = extra_chain.filter(|items| !items.is_empty());
    if !explicit_pinned && (mode_chain.is_some() || fallback_mode == "model_chain") {
        let chain: Vec<String> = mode_chain
            .map(|items| items.to_vec())
            .or_else(|| {
                profile.map(|profile| chain_for_source(profile, source).to_vec())
            })
            .unwrap_or_default();
        for model in chain {
            let model = model.trim();
            if model.is_empty() || attempts.iter().any(|attempt| attempt.model.eq_ignore_ascii_case(model)) {
                continue;
            }
            if attempts.len() >= 3 {
                break;
            }
            attempts.push(RouteAttemptPlan {
                index: attempts.len(),
                model: model.to_string(),
                upstream_id: None,
            });
        }
    }
    RouteExecutionPlan {
        attempts,
        fallback_mode: fallback_mode.to_string(),
        primary_model: primary_model.to_string(),
        explicit_pinned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_gateway_ports_are_self_referential() {
        assert!(is_self_referential_upstream(
            "http://127.0.0.1:15821",
            &default_gateway_ports()
        ));
        assert!(is_self_referential_upstream(
            "http://localhost:15823/v1",
            &default_gateway_ports()
        ));
        assert!(is_self_referential_upstream(
            "http://127.0.0.1:15828",
            &default_gateway_ports()
        ));
        assert!(!is_self_referential_upstream(
            "http://127.0.0.1:15830",
            &default_gateway_ports()
        ));
        assert!(!is_self_referential_upstream(
            "https://api.example.test/v1",
            &default_gateway_ports()
        ));
    }

    #[test]
    fn mode_chain_works_without_profile_fallback_and_deduplicates() {
        let chain = vec!["primary".into(), "backup".into(), "BACKUP".into(), "last".into(), "extra".into()];
        let plan = build_execution_plan_with_chain(None, "primary", Some("p1"), RouteSource::Auto, Some(&chain));
        assert_eq!(plan.fallback_mode, "off");
        assert_eq!(plan.attempts.iter().map(|attempt| attempt.model.as_str()).collect::<Vec<_>>(), vec!["primary", "backup", "last"]);
        let pinned = build_execution_plan_with_chain(None, "primary", Some("p1"), RouteSource::Explicit, Some(&chain));
        assert!(pinned.explicit_pinned);
        assert_eq!(pinned.attempts.len(), 1);
    }

    #[test]
    fn retry_fallback_does_not_invent_model_chain() {
        let profile = GatewayProfile {
            id: "gprof_claude_code".into(),
            name: "t".into(),
            target_app: crate::provider::ProviderTarget::ClaudeCode,
            default_model: String::new(),
            plan_model: String::new(),
            execute_model: String::new(),
            subagent_model: String::new(),
            allowed_upstream_ids: vec![],
            role_routing_enabled: false,
            explicit_fallback_enabled: false,
            fallback_mode: "retry".into(),
            fallback_models: vec!["other".into()],
            hide_official: false,
            entry_token: String::new(),
            entry_token_set: false,
            plan_fallback: vec![],
            execute_fallback: vec![],
            subagent_fallback: vec![],
            long_context_model: String::new(),
            long_context_tokens: 0,
            web_search_model: String::new(),
            created_at: 0,
            updated_at: 0,
        };
        let plan = build_execution_plan(Some(&profile), "kimi-k2", Some("p1"), RouteSource::Explicit);
        assert_eq!(plan.attempts.len(), 1);
        assert_eq!(plan.fallback_mode, "retry");
    }

    #[test]
    fn model_chain_uses_role_fallback_not_global_list() {
        let profile = GatewayProfile {
            id: "gprof_claude_code".into(),
            name: "t".into(),
            target_app: crate::provider::ProviderTarget::ClaudeCode,
            default_model: String::new(),
            plan_model: "plan-a".into(),
            execute_model: String::new(),
            subagent_model: String::new(),
            allowed_upstream_ids: vec![],
            role_routing_enabled: true,
            explicit_fallback_enabled: false,
            fallback_mode: "model_chain".into(),
            fallback_models: vec!["global-b".into()],
            hide_official: false,
            entry_token: String::new(),
            entry_token_set: false,
            plan_fallback: vec!["plan-b".into()],
            execute_fallback: vec![],
            subagent_fallback: vec![],
            long_context_model: String::new(),
            long_context_tokens: 0,
            web_search_model: String::new(),
            created_at: 0,
            updated_at: 0,
        };
        let plan = build_execution_plan(Some(&profile), "plan-a", Some("p1"), RouteSource::RolePlan);
        assert_eq!(plan.attempts.len(), 2);
        assert_eq!(plan.attempts[1].model, "plan-b");
    }

    #[test]
    fn live_auto_id_is_claude_prefixed_for_code() {
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::ClaudeCode, "auto"),
            ""
        );
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::ClaudeDesktop, ""),
            ""
        );
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::Codex, "claude.example.model"),
            "claude.example.model"
        );
        assert_eq!(normalize_live_model("claude.auto"), "auto");
        assert!(resolved_gateway_token("gwt_abc").is_some());
        assert!(resolved_gateway_token("kr://sgw_claude_code").is_none());
        assert!(resolved_gateway_token("").is_none());
    }

    #[test]
    fn estimate_tokens_counts_cjk_near_one_per_char() {
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": "你好世界" }]
        });
        assert_eq!(estimate_request_tokens(&body), 4);
    }

    #[test]
    fn estimate_tokens_uses_ascii_four_chars_per_token() {
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": "abcd" }]
        });
        assert_eq!(estimate_request_tokens(&body), 1);
        let long = serde_json::json!({
            "messages": [{ "role": "user", "content": "abcdefgh" }]
        });
        assert_eq!(estimate_request_tokens(&long), 2);
    }

    #[test]
    fn estimate_tokens_mixed_cjk_and_ascii() {
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": "你好abcd" }]
        });
        assert_eq!(estimate_request_tokens(&body), 3);
    }

    #[test]
    fn estimate_tokens_ignores_structural_keys() {
        let body = serde_json::json!({
            "messages": [{
                "role": "user",
                "type": "message",
                "id": "msg_1",
                "content": "ab"
            }]
        });
        assert_eq!(estimate_request_tokens(&body), 1);
    }

}
