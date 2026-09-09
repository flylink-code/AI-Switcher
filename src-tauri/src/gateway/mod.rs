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
use crate::provider::Provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSource {
    Explicit,
    RolePlan,
    RoleExecute,
    RoleSubagent,
    ProfileDefault,
}

impl RouteSource {
    pub fn as_reason(self) -> &'static str {
        match self {
            RouteSource::Explicit => "explicit_model",
            RouteSource::RolePlan => "role_plan",
            RouteSource::RoleExecute => "role_execute",
            RouteSource::RoleSubagent => "role_subagent",
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
    if requested.is_empty() {
        return RouteSource::ProfileDefault;
    }
    RouteSource::ProfileDefault
}

fn chain_for_source(profile: &GatewayProfile, source: RouteSource) -> &[String] {
    match source {
        RouteSource::RolePlan => &profile.plan_fallback,
        RouteSource::RoleExecute => &profile.execute_fallback,
        RouteSource::RoleSubagent => &profile.subagent_fallback,
        RouteSource::Explicit if profile.explicit_fallback_enabled => &profile.fallback_models,
        RouteSource::Explicit | RouteSource::ProfileDefault => &profile.fallback_models,
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
    let hide_official = profile.map(|profile| profile.hide_official).unwrap_or(false);
    let subagent = profile
        .map(|profile| profile.subagent_model.clone())
        .filter(|value| !value.trim().is_empty());
    let role_routing = profile
        .map(|profile| profile.role_routing_enabled)
        .unwrap_or(false);
    let plan = if role_routing {
        profile
            .map(|profile| profile.plan_model.clone())
            .filter(|value| !value.trim().is_empty())
    } else {
        None
    };
    let execute = if role_routing {
        profile
            .map(|profile| profile.execute_model.clone())
            .filter(|value| !value.trim().is_empty())
    } else {
        None
    };
    let normalized = normalize_client_request_with(
        style,
        entries,
        providers,
        requested_model,
        hide_official,
        subagent.as_deref(),
        force_subagent,
        plan.as_deref(),
        execute.as_deref(),
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
        .find(|provider| provider.id == provider_id)?
        .clone();
    let source = classify_route_source(
        requested_model,
        &normalized,
        force_subagent,
        role_routing,
        plan.as_deref(),
        execute.as_deref(),
        subagent.as_deref(),
    );
    let in_catalog = catalog_in_entries(entries, requested_model);
    let source = if in_catalog && !force_subagent {
        RouteSource::Explicit
    } else {
        source
    };
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
            created_at: 0,
            updated_at: 0,
        };
        let plan = build_execution_plan(Some(&profile), "plan-a", Some("p1"), RouteSource::RolePlan);
        assert_eq!(plan.attempts.len(), 2);
        assert_eq!(plan.attempts[1].model, "plan-b");
    }
}
