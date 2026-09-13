//! Dry-run routing: decide without sending an upstream request.

use serde::{Deserialize, Serialize};

use crate::catalog::{catalog_style_for, CatalogStyle};
use crate::database::dao::gateway::{
    current_profile, get_profile, list_route_rules, list_upstream_providers,
    list_visible_upstream_model_ids, resolve_profile_id, SHARED_PROFILE_ID,
};
use crate::database::Database;
use crate::error::AppResult;
use crate::gateway::modes::{self, ModeSignals};
use crate::gateway::rules::match_rules;
use crate::gateway::{
    estimate_request_tokens, request_has_web_search, resolve_gateway_route_with_modes,
    RouteDecision, RouteHints,
};
use crate::provider::ProviderTarget;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateRouteInput {
    pub requested_model: Option<String>,
    pub body_json: Option<String>,
    pub token_count: Option<u32>,
    pub has_web_search: Option<bool>,
    pub has_vision: Option<bool>,
    pub has_thinking: Option<bool>,
    pub is_subagent: Option<bool>,
    pub is_image_gen: Option<bool>,
    pub tool_names: Option<Vec<String>>,
    pub recent_write_tool: Option<String>,
    pub path: Option<String>,
    pub target: Option<ProviderTarget>,
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTraceStep {
    pub stage: String,
    pub id: String,
    pub matched: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateRouteResult {
    pub estimated_tokens: u32,
    pub decision: Option<RouteDecision>,
    pub upstream_model: Option<String>,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub signals: SimulateSignals,
    pub steps: Vec<RouteTraceStep>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateSignals {
    pub token_count: u32,
    pub has_web_search: bool,
    pub has_vision: bool,
    pub has_thinking: bool,
    pub is_subagent: bool,
    pub is_image_gen: bool,
    pub tool_names: Vec<String>,
    pub recent_write_tool: Option<String>,
    pub path: String,
    pub target: String,
}

pub fn simulate(db: &Database, input: SimulateRouteInput) -> AppResult<SimulateRouteResult> {
    let target = input.target.unwrap_or(ProviderTarget::ClaudeCode);
    let body: serde_json::Value = input
        .body_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(serde_json::Value::Null);
    let estimated = if body.is_null() {
        input.token_count.unwrap_or(1)
    } else {
        estimate_request_tokens(&body)
    };
    let path = input
        .path
        .clone()
        .unwrap_or_else(|| "/v1/messages".into());
    let extracted_tools = modes::extract_tool_names(&body);
    let extracted_write = modes::extract_recent_write_tool(&body);
    let signals = ModeSignals {
        token_count: input.token_count.unwrap_or(estimated),
        has_web_search: input
            .has_web_search
            .unwrap_or_else(|| request_has_web_search(&body)),
        has_vision: input
            .has_vision
            .unwrap_or_else(|| modes::has_vision_content(&body)),
        has_thinking: input
            .has_thinking
            .unwrap_or_else(|| modes::has_thinking_signal(&body)),
        is_subagent: input.is_subagent.unwrap_or(false),
        is_image_gen: input
            .is_image_gen
            .unwrap_or_else(|| path.contains("/images/generations")),
        tool_names: input.tool_names.clone().unwrap_or(extracted_tools),
        recent_write_tool: input.recent_write_tool.clone().or(extracted_write),
        target: Some(target),
        path: path.clone(),
    };
    let requested = input
        .requested_model
        .clone()
        .or_else(|| {
            body.get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "auto".into());

    let style = catalog_style_for(target);
    let (providers, entries, profile, modes, rules) = db.with_conn(|conn| {
        let mut providers = list_upstream_providers(conn, false)?;
        providers.retain(|provider| !provider.is_smart_gateway());
        let profile = if let Some(profile_id) = input
            .profile_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let resolved = resolve_profile_id(conn, Some(profile_id))?;
            get_profile(conn, &resolved)?
        } else {
            current_profile(conn, target).ok().flatten()
        };
        if let Some(profile) = profile.as_ref() {
            if !profile.allowed_upstream_ids.is_empty() {
                providers.retain(|provider| {
                    crate::database::dao::gateway::profile_allows_upstream(profile, &provider.id)
                });
            }
        }
        let hide_official = profile
            .as_ref()
            .map(|item| item.hide_official)
            .unwrap_or_else(|| crate::catalog::hide_official_for_conn(conn, target));
        let profile_id = profile
            .as_ref()
            .map(|item| item.id.as_str())
            .unwrap_or(SHARED_PROFILE_ID);
        let modes = crate::database::dao::gateway::list_route_modes(conn, profile_id)
            .unwrap_or_default();
        let mut pairs = Vec::with_capacity(providers.len());
        for provider in &providers {
            let cached = list_visible_upstream_model_ids(conn, &provider.id).unwrap_or_default();
            pairs.push((provider.clone(), cached));
        }
        let entries = crate::catalog::with_auto_entry_from_modes(
            style,
            crate::catalog::build_catalog_with(style, &pairs, hide_official),
            &modes,
        );
        let rules = list_route_rules(conn, profile_id).unwrap_or_default();
        Ok((providers, entries, profile, modes, rules))
    })?;

    let in_catalog = crate::catalog::is_explicit_catalog_passthrough(&entries, &requested);
    let role_explicit = !signals.is_subagent
        && crate::catalog::is_sticky_remap_role_id(&requested);
    let steps = explain_route(
        &requested,
        in_catalog,
        role_explicit,
        &signals,
        &modes,
        &rules,
    );
    let hints = RouteHints {
        token_count: signals.token_count,
        has_web_search: signals.has_web_search,
        has_vision: signals.has_vision,
        has_thinking: signals.has_thinking,
        is_image_gen: signals.is_image_gen,
        tool_names: signals.tool_names.clone(),
        recent_write_tool: signals.recent_write_tool.clone(),
        path: signals.path.clone(),
        target: signals.target,
    };
    let routed = resolve_gateway_route_with_modes(
        style,
        &entries,
        &providers,
        &requested,
        signals.is_subagent,
        profile.as_ref(),
        &hints,
        &modes,
        &rules,
    );
    let (decision, upstream_model, provider_id, provider_name) = match routed {
        Some((provider, upstream, decision, _, _)) => (
            Some(decision),
            Some(upstream),
            Some(provider.id.clone()),
            Some(provider.name),
        ),
        None => (None, None, None, None),
    };
    let _style: CatalogStyle = style;
    Ok(SimulateRouteResult {
        estimated_tokens: signals.token_count,
        decision,
        upstream_model,
        provider_id,
        provider_name,
        signals: SimulateSignals {
            token_count: signals.token_count,
            has_web_search: signals.has_web_search,
            has_vision: signals.has_vision,
            has_thinking: signals.has_thinking,
            is_subagent: signals.is_subagent,
            is_image_gen: signals.is_image_gen,
            tool_names: signals.tool_names.clone(),
            recent_write_tool: signals.recent_write_tool.clone(),
            path: signals.path.clone(),
            target: target.as_str().to_string(),
        },
        steps,
    })
}

fn explain_route(
    requested_model: &str,
    in_catalog: bool,
    role_explicit: bool,
    signals: &ModeSignals,
    modes: &[crate::database::dao::gateway::RouteMode],
    rules: &[crate::database::dao::gateway::RouteRule],
) -> Vec<RouteTraceStep> {
    let mut steps = Vec::new();
    let skip_modes = in_catalog || role_explicit;
    steps.push(RouteTraceStep {
        stage: "explicit".into(),
        id: requested_model.to_string(),
        matched: skip_modes,
        detail: if in_catalog {
            "目录中的显式模型，模式与规则让路".into()
        } else if role_explicit {
            "官方角色（Sonnet/Opus/Fable），跳过模式检测，落到默认模型".into()
        } else {
            "不是目录里的显式模型（或 auto / 子代理）".into()
        },
    });
    if skip_modes {
        return steps;
    }
    let mut ranked = rules.to_vec();
    ranked.sort_by_key(|rule| rule.sort_index);
    let hit = match_rules(rules, requested_model, signals);
    for rule in ranked {
        let matched = hit.as_ref().is_some_and(|item| item.rule_id == rule.id);
        steps.push(RouteTraceStep {
            stage: "rule".into(),
            id: rule.id.clone(),
            matched,
            detail: if !rule.enabled {
                "未启用".into()
            } else if matched {
                format!("命中，改写到 {}", rule.target_model)
            } else {
                "未命中".into()
            },
        });
        if matched {
            return steps;
        }
    }
    explain_modes(modes, signals, &mut steps);
    steps
}

fn explain_modes(
    modes: &[crate::database::dao::gateway::RouteMode],
    signals: &ModeSignals,
    steps: &mut Vec<RouteTraceStep>,
) {
    let selected = modes::select_mode(modes, signals);
    let selected_id = selected.map(|mode| mode.id.as_str());
    let enabled = |id: &str| {
        modes
            .iter()
            .find(|mode| mode.id == id)
    };
    let push = |steps: &mut Vec<RouteTraceStep>, id: &str, detail: String, matched: bool| {
        steps.push(RouteTraceStep {
            stage: "mode".into(),
            id: id.into(),
            matched,
            detail,
        });
    };
    if signals.is_subagent {
        let bg = enabled("background");
        let matched = selected_id == Some("background");
        push(
            steps,
            "background",
            if bg.map(|mode| !mode.enabled || mode.model.trim().is_empty()).unwrap_or(true) {
                "子代理信号存在，但后台行未启用或未选模型".into()
            } else {
                "子代理/Haiku 独占后台".into()
            },
            matched,
        );
        if matched {
            return;
        }
        push(
            steps,
            "default",
            "后台未命中，落到默认".into(),
            selected_id == Some("default"),
        );
        return;
    }
    let checks: [(&str, bool, &str); 8] = [
        ("image_gen", signals.is_image_gen, "图像生成路径"),
        ("web_search", signals.has_web_search, "含 web_search/web_fetch"),
        ("vision", signals.has_vision, "最新一轮用户消息含图"),
        ("plan", modes::looks_like_plan(&signals.tool_names, signals.target), "tools 含规划工具"),
        ("think", signals.has_thinking, "请求含 thinking/reasoning"),
        ("edit", modes::looks_like_edit(signals.recent_write_tool.as_deref())
            && !modes::looks_like_plan(&signals.tool_names, signals.target), "最近一轮写工具"),
        ("long_context", true, "估算 token 达到阈值"),
        ("default", true, "兜底"),
    ];
    for (id, signal, why) in checks {
        let mode = enabled(id);
        let ready = mode.is_some_and(|item| item.enabled && !item.model.trim().is_empty());
        let matched = selected_id == Some(id);
        let detail = if id == "long_context" {
            let threshold = mode.map(|item| item.threshold).unwrap_or(0);
            if !ready {
                "未启用或未选模型".into()
            } else if threshold <= 0 {
                "阈值 ≤ 0，不按长度分流".into()
            } else if (signals.token_count as i64) < threshold {
                format!("估算 {} < 阈值 {}", signals.token_count, threshold)
            } else {
                format!("估算 {} ≥ 阈值 {}", signals.token_count, threshold)
            }
        } else if !signal {
            format!("信号未出现：{why}")
        } else if !ready {
            format!("信号在，但该行未启用或未选模型（{why}）")
        } else {
            why.to_string()
        };
        push(steps, id, detail, matched);
        if matched && id != "default" {
            return;
        }
    }
}
