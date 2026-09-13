//! Smart gateway routing: decisions, execution plans, and loop detection.
//!
//! Protocol adaptation still lives in `crate::proxy`. This module decides *which*
//! upstream model to call and records why.

pub mod correlation;
pub mod metadata;
pub mod modes;
pub mod rules;
pub mod service;
pub mod thinking;
pub mod health;
pub mod inbound;
pub mod budget;
pub mod simulate;
pub mod count_tokens;
pub mod sticky;

use serde::{Deserialize, Serialize};
#[cfg(test)]
use ts_rs::TS;

use crate::catalog::{
    catalog_in_entries, is_explicit_catalog_passthrough, is_sticky_remap_role_id,
    normalize_client_request_with, resolve_request, CatalogEntry, CatalogStyle,
};
use crate::database::dao::gateway::{
    profile_allows_upstream, GatewayProfile, RouteMode, RouteRule,
};
use crate::provider::{Provider, ProviderKind, ThinkingConfig};

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

    pub fn from_mode_id(id: &str) -> Self {
        match id {
            "background" => Self::RoleSubagent,
            "plan" => Self::Plan,
            "think" => Self::Think,
            "edit" => Self::Edit,
            "long_context" => Self::LongContext,
            "web_search" => Self::WebSearch,
            "vision" => Self::Vision,
            "image_gen" => Self::ImageGen,
            "default" => Self::Auto,
            _ => Self::ProfileDefault,
        }
    }

    pub fn mode_id(self) -> Option<&'static str> {
        match self {
            RouteSource::Explicit | RouteSource::Rule => None,
            RouteSource::Auto | RouteSource::ProfileDefault => Some("default"),
            RouteSource::RolePlan | RouteSource::Plan => Some("plan"),
            RouteSource::RoleExecute | RouteSource::Edit => Some("edit"),
            RouteSource::RoleSubagent => Some("background"),
            RouteSource::Think => Some("think"),
            RouteSource::LongContext => Some("long_context"),
            RouteSource::WebSearch => Some("web_search"),
            RouteSource::Vision => Some("vision"),
            RouteSource::ImageGen => Some("image_gen"),
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

pub fn reserved_listener_ports() -> [u16; 9] {
    [15821, 15822, 15823, 15824, 15825, 15826, 15827, SMART_GATEWAY_PORT, 15830]
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
    pub has_vision: bool,
    pub has_thinking: bool,
    pub is_image_gen: bool,
    pub tool_names: Vec<String>,
    pub recent_write_tool: Option<String>,
    pub path: String,
    pub target: Option<crate::provider::ProviderTarget>,
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
    if fallback_mode == "model_chain" {
        let chain: Vec<String> = extra_chain
            .map(|items| items.to_vec())
            .or_else(|| {
                profile.map(|profile| chain_for_source(profile, source).to_vec())
            })
            .unwrap_or_default();
        for model in chain {
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

fn default_mode_lookup(
    modes: &[RouteMode],
    profile: Option<&GatewayProfile>,
) -> Option<String> {
    modes
        .iter()
        .find(|mode| mode.id == "default" && mode.enabled && !mode.model.trim().is_empty())
        .map(|mode| mode.model.clone())
        .or_else(|| {
            profile
                .map(|profile| profile.default_model.clone())
                .filter(|value| !value.trim().is_empty())
        })
}

pub fn resolve_gateway_route_with_modes(
    style: CatalogStyle,
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested_model: &str,
    force_subagent: bool,
    profile: Option<&GatewayProfile>,
    hints: &RouteHints,
    modes: &[RouteMode],
    rules: &[RouteRule],
) -> Option<(Provider, String, RouteDecision, RouteExecutionPlan, bool)> {
    let hide_official = profile.map(|profile| profile.hide_official).unwrap_or(false);
    let subagent = modes::enabled_background_model(modes).or_else(|| {
        profile
            .map(|profile| profile.subagent_model.clone())
            .filter(|value| !value.trim().is_empty())
    });
    let role_routing = false;
    let plan = None;
    let execute = None;
    let signals = modes::ModeSignals {
        token_count: hints.token_count,
        has_web_search: hints.has_web_search,
        has_vision: hints.has_vision,
        has_thinking: hints.has_thinking,
        is_subagent: force_subagent,
        is_image_gen: hints.is_image_gen || hints.path.contains("/images/generations"),
        tool_names: hints.tool_names.clone(),
        recent_write_tool: hints.recent_write_tool.clone(),
        target: hints.target,
        path: hints.path.clone(),
    };
    let in_catalog = is_explicit_catalog_passthrough(entries, requested_model);
    let role_explicit = !force_subagent && is_sticky_remap_role_id(requested_model);
    let mut thinking: Option<ThinkingConfig> = None;
    let mut rewrites = Vec::new();
    let mut extra_chain: Option<Vec<String>> = None;
    let (lookup_model, mut source, mut reason) = if in_catalog {
        (
            requested_model.to_string(),
            RouteSource::Explicit,
            RouteSource::Explicit.as_reason().to_string(),
        )
    } else if role_explicit {
        let lookup = default_mode_lookup(modes, profile)
            .unwrap_or_else(|| requested_model.to_string());
        (
            lookup.clone(),
            RouteSource::Explicit,
            format!("显式模型（角色 {requested_model} → {lookup}）"),
        )
    } else if let Some(hit) = rules::match_rules(rules, requested_model, &signals) {
        thinking = hit.thinking;
        rewrites = hit.rewrites;
        (
            hit.target_model,
            RouteSource::Rule,
            format!("命中规则 {}（优先于模式）", hit.rule_id),
        )
    } else if !modes.is_empty() {
        if let Some(mode) = modes::select_mode(modes, &signals) {
            thinking = serde_json::from_str(&mode.thinking_config_json).ok();
            extra_chain = Some(mode.fallback_models.clone());
            let source = RouteSource::from_mode_id(&mode.id);
            (
                mode.model.clone(),
                source,
                mode_reason(&mode.id, &signals),
            )
        } else {
            auto_slot_rewrite(requested_model, force_subagent, profile, hints)
        }
    } else {
        auto_slot_rewrite(requested_model, force_subagent, profile, hints)
    };
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
    let mut provider = providers
        .iter()
        .find(|provider| provider.id == provider_id && !provider.is_smart_gateway())?
        .clone();
    if thinking.as_ref().is_some_and(|cfg| !cfg.is_empty()) {
        provider.thinking_config = thinking.clone();
    }
    if in_catalog {
        source = RouteSource::Explicit;
        reason = source.as_reason().to_string();
    }
    let diagnostics = profile
        .map(|profile| slot_diagnostics(profile, entries))
        .unwrap_or_default();
    let decision = RouteDecision {
        requested_model: requested_model.to_string(),
        normalized_model: normalized.clone(),
        source,
        reason,
        profile_id: profile.map(|profile| profile.id.clone()),
        upstream_id: Some(provider_id.clone()),
        diagnostics,
        thinking,
        rewrites,
        mode_id: source.mode_id().map(str::to_string),
    };
    let plan = build_execution_plan_with_chain(
        profile,
        &upstream,
        Some(&provider_id),
        source,
        extra_chain.as_deref(),
    );
    let is_subagent = matches!(source, RouteSource::RoleSubagent)
        || (force_subagent && !in_catalog);
    Some((provider, upstream, decision, plan, is_subagent))
}

fn mode_reason(mode_id: &str, signals: &modes::ModeSignals) -> String {
    match mode_id {
        "plan" => {
            let tool = signals
                .tool_names
                .iter()
                .find(|name| modes::looks_like_plan(std::slice::from_ref(name), signals.target))
                .cloned()
                .unwrap_or_else(|| "ExitPlanMode".into());
            format!("命中规划模式（依据：tools 含 {tool}）")
        }
        "edit" => {
            let tool = signals
                .recent_write_tool
                .clone()
                .unwrap_or_else(|| "Edit".into());
            format!("命中改内容模式（依据：最近一轮调用了 {tool}）")
        }
        "background" => "命中后台/辅助模式（依据：子代理或 Haiku 角色）".into(),
        "think" => "命中思考模式（依据：请求含 thinking/reasoning）".into(),
        "long_context" => format!(
            "命中长上下文模式（依据：估算 token {} 超过阈值）",
            signals.token_count
        ),
        "web_search" => "命中联网模式（依据：tools 含 web_search/web_fetch）".into(),
        "vision" => "命中视觉模式（依据：content 含 image block）".into(),
        "image_gen" => "命中图像生成模式（依据：路径 /v1/images/generations）".into(),
        "default" => "命中默认模式".into(),
        other => format!("命中模式 {other}"),
    }
}

fn auto_slot_rewrite(
    requested_model: &str,
    force_subagent: bool,
    profile: Option<&GatewayProfile>,
    _hints: &RouteHints,
) -> (String, RouteSource, String) {
    if force_subagent || !is_auto_model_id(requested_model) {
        return (
            requested_model.to_string(),
            if force_subagent {
                RouteSource::RoleSubagent
            } else {
                RouteSource::Explicit
            },
            if force_subagent {
                "background".into()
            } else {
                "explicit_model".into()
            },
        );
    }
    log::warn!("route_modes is empty; falling back to profile.default_model");
    let Some(profile) = profile else {
        return (String::new(), RouteSource::Auto, "auto".into());
    };
    let default = profile.default_model.trim();
    if !default.is_empty() {
        return (default.to_string(), RouteSource::Auto, "auto".into());
    }
    (String::new(), RouteSource::Auto, "auto".into())
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
            ..RouteHints::default()
        };
        let (lookup, source, _) = auto_slot_rewrite("claude.kimi.k2", false, Some(&profile), &hints);
        assert_eq!(lookup, "claude.kimi.k2");
        assert_eq!(source, RouteSource::Explicit);
        let (lookup, source, _) = auto_slot_rewrite("auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "default-m");
        assert_eq!(source, RouteSource::Auto);
        let hints = RouteHints {
            token_count: 9_000,
            has_web_search: false,
            ..RouteHints::default()
        };
        let (lookup, source, _) = auto_slot_rewrite("auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "default-m");
        assert_eq!(source, RouteSource::Auto);
        let (lookup, source, _) = auto_slot_rewrite("claude.auto", false, Some(&profile), &hints);
        assert_eq!(lookup, "default-m");
        assert_eq!(source, RouteSource::Auto);
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

    fn test_provider(id: &str, name: &str, model: &str) -> Provider {
        use crate::provider::{ClaudeModelMapping, ProtocolType, ProviderKind, ProviderTarget};
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: model.into(),
            model_context_window: Some(200_000),
            auto_review_model_override: None,
            web_search_enabled: Some(true),
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    fn catalog_entry(public_id: &str, slug: &str, provider_id: &str, window: u64) -> CatalogEntry {
        CatalogEntry {
            public_id: public_id.into(),
            display_name: public_id.into(),
            upstream_slug: slug.into(),
            provider_id: provider_id.into(),
            context_window: window,
            anthropic_upstream: false,
            web_search_enabled: false,
        }
    }

    fn route_mode(id: &str, model: &str, threshold: i64) -> RouteMode {
        RouteMode {
            id: id.into(),
            profile_id: "gprof_shared".into(),
            enabled: true,
            model: model.into(),
            thinking_config_json: "{}".into(),
            fallback_models: vec![],
            threshold,
            sort_index: 0,
        }
    }

    fn routing_fixture() -> (Vec<CatalogEntry>, Vec<Provider>, Vec<RouteMode>) {
        let astra = test_provider("sub2api", "sub2api", "gpt-6-astra");
        let ag = test_provider("ag", "Antigravity", "gemini-3.8-flash-high");
        let providers = vec![astra, ag];
        let entries = vec![
            catalog_entry(
                "claude.sub2api.gpt-6-astra",
                "gpt-6-astra",
                "sub2api",
                200_000,
            ),
            catalog_entry(
                "claude.ag.gemini-3.8-flash-high",
                "gemini-3.8-flash-high",
                "ag",
                1_000_000,
            ),
            catalog_entry(
                "claude.ag.gemini-3.8-flash-low",
                "gemini-3.8-flash-low",
                "ag",
                1_000_000,
            ),
        ];
        let modes = vec![
            route_mode("default", "claude.ag.gemini-3.8-flash-high", 0),
            route_mode(
                "long_context",
                "claude.ag.gemini-3.8-flash-high",
                20_000,
            ),
            route_mode("background", "claude.ag.gemini-3.8-flash-low", 0),
        ];
        let entries =
            crate::catalog::with_auto_entry_from_modes(CatalogStyle::Claude, entries, &modes);
        (entries, providers, modes)
    }

    fn route(
        requested: &str,
        force_subagent: bool,
        tokens: u32,
    ) -> Option<(Provider, String, RouteDecision, RouteExecutionPlan, bool)> {
        let (entries, providers, modes) = routing_fixture();
        resolve_gateway_route_with_modes(
            CatalogStyle::Claude,
            &entries,
            &providers,
            requested,
            force_subagent,
            None,
            &RouteHints {
                token_count: tokens,
                ..RouteHints::default()
            },
            &modes,
            &[],
        )
    }

    #[test]
    fn explicit_catalog_id_skips_long_context_mode() {
        let routed = route("claude.sub2api.gpt-6-astra", false, 42_611).unwrap();
        assert_eq!(routed.1, "gpt-6-astra");
        assert_eq!(routed.2.source, RouteSource::Explicit);
        assert_eq!(routed.2.reason, "explicit_model");
    }

    #[test]
    fn auto_over_threshold_hits_long_context() {
        let routed = route("claude.auto", false, 42_611).unwrap();
        assert_eq!(routed.1, "gemini-3.8-flash-high");
        assert_eq!(routed.2.source, RouteSource::LongContext);
        assert!(routed.2.reason.contains("长上下文"));
    }

    #[test]
    fn injected_sonnet_role_without_sticky_skips_modes() {
        let routed = route("claude-sonnet-5", false, 42_611).unwrap();
        assert_eq!(routed.1, "gemini-3.8-flash-high");
        assert_eq!(routed.2.source, RouteSource::Explicit);
        assert!(routed.2.reason.contains("显式模型"));
        assert!(routed.2.reason.contains("claude-sonnet-5"));
        assert_ne!(routed.2.source, RouteSource::LongContext);
    }

    #[test]
    fn subagent_header_on_sonnet_role_still_uses_background() {
        let routed = route("claude-sonnet-5", true, 42_611).unwrap();
        assert_eq!(routed.1, "gemini-3.8-flash-low");
        assert_eq!(routed.2.source, RouteSource::RoleSubagent);
        assert!(routed.4);
    }

    #[test]
    fn sticky_sonnet_role_stays_on_explicit_catalog_model() {
        let _guard = sticky::test_lock();
        sticky::reset_for_tests();
        let (entries, providers, modes) = routing_fixture();
        let rewritten = sticky::rewrite_requested(
            crate::provider::ProviderTarget::ClaudeCode,
            "claude.sub2api.gpt-6-astra",
            &entries,
            "sess-b",
        );
        assert_eq!(rewritten, "claude.sub2api.gpt-6-astra");
        let rewritten = sticky::rewrite_requested(
            crate::provider::ProviderTarget::ClaudeCode,
            "claude-sonnet-5",
            &entries,
            "sess-b",
        );
        let routed = resolve_gateway_route_with_modes(
            CatalogStyle::Claude,
            &entries,
            &providers,
            &rewritten,
            false,
            None,
            &RouteHints {
                token_count: 42_611,
                ..RouteHints::default()
            },
            &modes,
            &[],
        )
        .unwrap();
        assert_eq!(routed.1, "gpt-6-astra");
        assert_eq!(routed.2.source, RouteSource::Explicit);
        sticky::reset_for_tests();
    }

    #[test]
    fn haiku_role_uses_background_slot() {
        let routed = route("claude-haiku-4-5", true, 42_611).unwrap();
        assert_eq!(routed.1, "gemini-3.8-flash-low");
        assert_eq!(routed.2.source, RouteSource::RoleSubagent);
        assert!(routed.4);
    }

    #[test]
    fn subagent_header_does_not_steal_explicit_catalog_id() {
        let routed = route("claude.sub2api.gpt-6-astra", true, 42_611).unwrap();
        assert_eq!(routed.1, "gpt-6-astra");
        assert_eq!(routed.2.source, RouteSource::Explicit);
        assert!(!routed.4);
    }
}
