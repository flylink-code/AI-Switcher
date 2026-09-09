//! Smart gateway routing: decisions, execution plans, and loop detection.
//!
//! Protocol adaptation still lives in `crate::proxy`. This module decides *which*
//! upstream model to call and records why.

use serde::{Deserialize, Serialize};

use crate::catalog::{
    catalog_in_entries, normalize_client_request_with, resolve_request, CatalogEntry, CatalogStyle,
};
use crate::database::dao::gateway::{
    profile_allows_upstream, GatewayProfile,
};
use crate::provider::{Provider, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSource {
    Explicit,
    Auto,
    RolePlan,
    RoleExecute,
    RoleSubagent,
    LongContext,
    WebSearch,
    ProfileDefault,
}

impl RouteSource {
    pub fn as_reason(self) -> &'static str {
        match self {
            RouteSource::Explicit => "explicit_model",
            RouteSource::Auto => "auto",
            RouteSource::RolePlan => "role_plan",
            RouteSource::RoleExecute => "role_execute",
            RouteSource::RoleSubagent => "role_subagent",
            RouteSource::LongContext => "long_context",
            RouteSource::WebSearch => "web_search",
            RouteSource::ProfileDefault => "profile_default",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub requested_model: String,
    pub normalized_model: String,
    pub source: RouteSource,
    pub reason: String,
    pub profile_id: Option<String>,
    pub upstream_id: Option<String>,
    pub diagnostics: Vec<String>,
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
}

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub decision: RouteDecision,
    pub plan: RouteExecutionPlan,
    pub provider: Provider,
    pub upstream_model: String,
    pub is_subagent: bool,
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

pub fn default_gateway_ports() -> [u16; 7] {
    [15821, 15822, 15823, 15824, 15825, 15826, 15827]
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

/// Live default written to an Agent. Claude discovery requires `claude.auto`.
pub fn normalize_live_model_for(
    target: crate::provider::ProviderTarget,
    model: &str,
) -> String {
    let trimmed = model.trim();
    if is_auto_model_id(trimmed) || trimmed.eq_ignore_ascii_case("opusplan") {
        crate::catalog::auto_public_id(crate::catalog::catalog_style_for(target)).to_string()
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

pub fn default_listener_port(target: crate::provider::ProviderTarget) -> u16 {
    match target {
        crate::provider::ProviderTarget::ClaudeCode => 15821,
        crate::provider::ProviderTarget::ClaudeDesktop => 15822,
        crate::provider::ProviderTarget::Codex => 15823,
        crate::provider::ProviderTarget::OpenCode => 15824,
        crate::provider::ProviderTarget::Pi => 15825,
        crate::provider::ProviderTarget::Dsh => 15826,
        crate::provider::ProviderTarget::Cline => 15827,
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

#[derive(Debug, Clone, Default)]
pub struct RouteHints {
    pub token_count: u32,
    pub has_web_search: bool,
}

pub fn estimate_request_tokens(body: &serde_json::Value) -> u32 {
    let blob = serde_json::json!({
        "messages": body.get("messages"),
        "system": body.get("system"),
        "input": body.get("input"),
        "instructions": body.get("instructions"),
        "tools": body.get("tools"),
    });
    let chars = blob.to_string().chars().count() as u32;
    (chars / 4).max(1)
}

pub fn request_has_web_search(body: &serde_json::Value) -> bool {
    let Some(tools) = body.get("tools").and_then(|value| value.as_array()) else {
        return false;
    };
    tools.iter().any(|tool| {
        let name = tool
            .get("name")
            .or_else(|| tool.pointer("/function/name"))
            .or_else(|| tool.pointer("/web_search/name"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let kind = tool.get("type").and_then(|value| value.as_str()).unwrap_or("");
        let lower = name.to_ascii_lowercase();
        kind.eq_ignore_ascii_case("web_search")
            || kind.eq_ignore_ascii_case("web_fetch")
            || lower.contains("web_search")
            || lower.contains("web_fetch")
            || lower.contains("websearch")
    })
}

pub fn classify_route_source(
    requested: &str,
    normalized: &str,
    force_subagent: bool,
    role_routing: bool,
    plan: Option<&str>,
    execute: Option<&str>,
    subagent: Option<&str>,
) -> RouteSource {
    if force_subagent {
        return RouteSource::RoleSubagent;
    }
    let requested = requested.trim();
    if requested.eq_ignore_ascii_case(normalized) {
        return RouteSource::Explicit;
    }
    if role_routing {
        if let Some(plan) = plan {
            if normalized.eq_ignore_ascii_case(plan) {
                return RouteSource::RolePlan;
            }
        }
        if let Some(execute) = execute {
            if normalized.eq_ignore_ascii_case(execute) {
                return RouteSource::RoleExecute;
            }
        }
    }
    if let Some(subagent) = subagent {
        if normalized.eq_ignore_ascii_case(subagent) {
            return RouteSource::RoleSubagent;
        }
    }
    if requested.is_empty() || is_auto_model_id(requested) {
        return RouteSource::Auto;
    }
    RouteSource::ProfileDefault
}

fn chain_for_source(profile: &GatewayProfile, source: RouteSource) -> &[String] {
    match source {
        RouteSource::RolePlan => &profile.plan_fallback,
        RouteSource::RoleExecute => &profile.execute_fallback,
        RouteSource::RoleSubagent => &profile.subagent_fallback,
        RouteSource::Explicit if profile.explicit_fallback_enabled => &profile.fallback_models,
        RouteSource::Explicit
        | RouteSource::Auto
        | RouteSource::LongContext
        | RouteSource::WebSearch
        | RouteSource::ProfileDefault => &profile.fallback_models,
    }
}

pub fn build_execution_plan(
    profile: Option<&GatewayProfile>,
    primary_model: &str,
    primary_upstream_id: Option<&str>,
    source: RouteSource,
) -> RouteExecutionPlan {
    let fallback_mode = profile
        .map(|profile| profile.fallback_mode.as_str())
        .unwrap_or("off");
    let mut attempts = vec![RouteAttemptPlan {
        index: 0,
        model: primary_model.to_string(),
        upstream_id: primary_upstream_id.map(str::to_string),
    }];
    if fallback_mode == "model_chain" {
        if let Some(profile) = profile {
            for model in chain_for_source(profile, source) {
                let model = model.trim();
                if model.is_empty() || model.eq_ignore_ascii_case(primary_model) {
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
    }
    RouteExecutionPlan {
        attempts,
        fallback_mode: fallback_mode.to_string(),
        primary_model: primary_model.to_string(),
    }
}

pub fn slot_diagnostics(profile: &GatewayProfile, entries: &[CatalogEntry]) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let slots = [
        ("default", profile.default_model.as_str()),
        ("plan", profile.plan_model.as_str()),
        ("execute", profile.execute_model.as_str()),
        ("subagent", profile.subagent_model.as_str()),
        ("long_context", profile.long_context_model.as_str()),
        ("web_search", profile.web_search_model.as_str()),
    ];
    for (label, value) in slots {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let known = catalog_in_entries(entries, value)
            || entries
                .iter()
                .any(|entry| entry.upstream_slug.eq_ignore_ascii_case(value));
        if !known {
            diagnostics.push(format!("槽位 {label} 引用了目录中不存在的模型 {value}"));
        }
    }
    diagnostics
}

pub fn resolve_gateway_route(
    style: CatalogStyle,
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested_model: &str,
    force_subagent: bool,
    profile: Option<&GatewayProfile>,
) -> Option<(Provider, String, RouteDecision, RouteExecutionPlan, bool)> {
    resolve_gateway_route_with(
        style,
        entries,
        providers,
        requested_model,
        force_subagent,
        profile,
        &RouteHints::default(),
    )
}

pub fn resolve_gateway_route_with(
    style: CatalogStyle,
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested_model: &str,
    force_subagent: bool,
    profile: Option<&GatewayProfile>,
    hints: &RouteHints,
) -> Option<(Provider, String, RouteDecision, RouteExecutionPlan, bool)> {
    let hide_official = profile.map(|profile| profile.hide_official).unwrap_or(false);
    let subagent = profile
        .map(|profile| profile.subagent_model.clone())
        .filter(|value| !value.trim().is_empty());
    // Opus Plan / role remap is removed: leftover plan/execute columns are unused.
    let role_routing = false;
    let plan = None;
    let execute = None;
    let (lookup_model, auto_source) = auto_slot_rewrite(requested_model, force_subagent, profile, hints);
    let normalized = normalize_client_request_with(
        style,
        entries,
        providers,
        &lookup_model,
        hide_official,
        subagent.as_deref(),
        force_subagent,
        plan,
        execute,
        role_routing,
    );
    let (provider_id, upstream) = resolve_request(entries, providers, &normalized)?;
    if let Some(profile) = profile {
        if !profile_allows_upstream(profile, &provider_id) {
            return None;
        }
    }
    let provider = providers
        .iter()
        .find(|provider| provider.id == provider_id && !provider.is_smart_gateway())?
        .clone();
    let mut source = classify_route_source(
        requested_model,
        &normalized,
        force_subagent,
        role_routing,
        plan.as_deref(),
        execute.as_deref(),
        subagent.as_deref(),
    );
    let in_catalog = catalog_in_entries(entries, requested_model);
    if in_catalog && !force_subagent && !is_auto_model_id(requested_model) {
        source = RouteSource::Explicit;
    } else if let Some(auto_source) = auto_source {
        source = auto_source;
    }
    let diagnostics = profile
        .map(|profile| slot_diagnostics(profile, entries))
        .unwrap_or_default();
    let decision = RouteDecision {
        requested_model: requested_model.to_string(),
        normalized_model: normalized.clone(),
        source,
        reason: source.as_reason().to_string(),
        profile_id: profile.map(|profile| profile.id.clone()),
        upstream_id: Some(provider_id.clone()),
        diagnostics,
    };
    let plan = build_execution_plan(profile, &upstream, Some(&provider_id), source);
    let is_subagent = matches!(source, RouteSource::RoleSubagent) || force_subagent;
    Some((provider, upstream, decision, plan, is_subagent))
}

fn auto_slot_rewrite(
    requested_model: &str,
    force_subagent: bool,
    profile: Option<&GatewayProfile>,
    hints: &RouteHints,
) -> (String, Option<RouteSource>) {
    if force_subagent || !is_auto_model_id(requested_model) {
        return (requested_model.to_string(), None);
    }
    let Some(profile) = profile else {
        return (String::new(), Some(RouteSource::Auto));
    };
    if hints.has_web_search {
        let model = profile.web_search_model.trim();
        if !model.is_empty() {
            return (model.to_string(), Some(RouteSource::WebSearch));
        }
    }
    if profile.long_context_tokens > 0 && i64::from(hints.token_count) >= profile.long_context_tokens
    {
        let model = profile.long_context_model.trim();
        if !model.is_empty() {
            return (model.to_string(), Some(RouteSource::LongContext));
        }
    }
    let default = profile.default_model.trim();
    if !default.is_empty() {
        return (default.to_string(), Some(RouteSource::Auto));
    }
    (String::new(), Some(RouteSource::Auto))
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
    fn auto_slots_ignore_explicit_catalog_ids() {
        let profile = GatewayProfile {
            id: "gprof_claude_code".into(),
            name: "t".into(),
            target_app: crate::provider::ProviderTarget::ClaudeCode,
            default_model: "default-m".into(),
            plan_model: String::new(),
            execute_model: String::new(),
            subagent_model: String::new(),
            allowed_upstream_ids: vec![],
            role_routing_enabled: false,
            explicit_fallback_enabled: false,
            fallback_mode: "off".into(),
            fallback_models: vec![],
            hide_official: false,
            entry_token: String::new(),
            entry_token_set: false,
            plan_fallback: vec![],
            execute_fallback: vec![],
            subagent_fallback: vec![],
            long_context_model: "long-m".into(),
            long_context_tokens: 100,
            web_search_model: "web-m".into(),
            created_at: 0,
            updated_at: 0,
        };
        let hints = RouteHints {
            token_count: 9_000,
            has_web_search: true,
        };
        let (lookup, source) = auto_slot_rewrite("claude.kimi.k2", false, Some(&profile), &hints);
        assert_eq!(lookup, "claude.kimi.k2");
        assert!(source.is_none());
        let (lookup, source) = auto_slot_rewrite("auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "web-m");
        assert_eq!(source, Some(RouteSource::WebSearch));
        let hints = RouteHints {
            token_count: 9_000,
            has_web_search: false,
        };
        let (lookup, source) = auto_slot_rewrite("auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "long-m");
        assert_eq!(source, Some(RouteSource::LongContext));
        let (lookup, source) = auto_slot_rewrite("claude.auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "long-m");
        assert_eq!(source, Some(RouteSource::LongContext));
    }

    #[test]
    fn live_auto_id_is_claude_prefixed_for_code() {
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::ClaudeCode, "auto"),
            "claude.auto"
        );
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::ClaudeDesktop, ""),
            "claude.auto"
        );
        assert_eq!(
            normalize_live_model_for(crate::provider::ProviderTarget::Codex, "auto"),
            "auto"
        );
        assert_eq!(normalize_live_model("claude.auto"), "auto");
        assert!(resolved_gateway_token("gwt_abc").is_some());
        assert!(resolved_gateway_token("kr://sgw_claude_code").is_none());
        assert!(resolved_gateway_token("").is_none());
    }
}
