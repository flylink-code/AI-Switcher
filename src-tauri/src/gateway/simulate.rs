//! Dry-run routing: decide without sending an upstream request.

use serde::{Deserialize, Serialize};

use crate::catalog::{catalog_style_for, resolve_request_strict, CatalogStyle};
use crate::database::dao::gateway::{
    current_profile, get_profile, list_route_rules, list_upstream_providers,
    list_visible_upstream_model_ids, profile_allows_upstream, resolve_profile_id,
    SHARED_PROFILE_ID,
};
use crate::database::Database;
use crate::error::AppResult;
use crate::gateway::budget;
use crate::gateway::health;
use crate::gateway::modes::{self, ModeSignals};
use crate::gateway::rules::match_rules;
use crate::gateway::{
    estimate_request_tokens, request_has_web_search, resolve_gateway_route_with_modes_strict,
    RouteDecision, RouteExecutionPlan, RouteHints,
};
use crate::provider::{Provider, ProviderTarget};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateCandidate {
    pub index: usize,
    pub model: String,
    pub upstream_id: Option<String>,
    pub upstream_name: Option<String>,
    pub upstream_model: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_remaining_secs: Option<u64>,
    pub selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<RouteExecutionPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<SimulateCandidate>,
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

    let mut steps = Vec::new();

    // 1. 预算阶段：日预算限额门禁
    let budget_decision = budget::evaluate(db);
    let mut requested = input
        .requested_model
        .clone()
        .or_else(|| {
            body.get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "auto".into());

    match budget_decision {
        budget::BudgetDecision::Reject { spent, cap } => {
            steps.push(RouteTraceStep {
                stage: "budget".into(),
                id: "daily_budget".into(),
                matched: true,
                detail: format!("日预算已超限 ({spent:.4}/{cap:.2} USD)，请求被拦截 (429)"),
            });
            return Ok(SimulateRouteResult {
                estimated_tokens: signals.token_count,
                decision: None,
                upstream_model: None,
                provider_id: None,
                provider_name: None,
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
                plan: None,
                candidates: Vec::new(),
            });
        }
        budget::BudgetDecision::Fallback { spent, cap, model } => {
            steps.push(RouteTraceStep {
                stage: "budget".into(),
                id: "daily_budget".into(),
                matched: true,
                detail: format!("日预算超限 ({spent:.4}/{cap:.2} USD)，改写到降级模型 {model}"),
            });
            requested = model;
        }
        budget::BudgetDecision::Warn { spent, cap } => {
            steps.push(RouteTraceStep {
                stage: "budget".into(),
                id: "daily_budget".into(),
                matched: false,
                detail: format!("日预算告警：已用 {spent:.4}/{cap:.2} USD"),
            });
        }
        budget::BudgetDecision::Allow => {
            let bsettings = budget::load_from_db(db);
            if bsettings.is_active() {
                let spent = budget::today_spend_usd(db);
                steps.push(RouteTraceStep {
                    stage: "budget".into(),
                    id: "daily_budget".into(),
                    matched: false,
                    detail: format!("日预算正常：已用 {spent:.4}/{:.2} USD", bsettings.daily_budget_usd),
                });
            } else {
                steps.push(RouteTraceStep {
                    stage: "budget".into(),
                    id: "daily_budget".into(),
                    matched: false,
                    detail: "未启用日预算限额".into(),
                });
            }
        }
    }

    let style = catalog_style_for(target);
    let (all_providers, providers, entries, all_entries, profile, modes, rules) = db.with_conn(|conn| {
        let mut all_providers = list_upstream_providers(conn, false)?;
        all_providers.retain(|provider| !provider.is_smart_gateway());
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
        let mut providers = all_providers.clone();
        if let Some(profile) = profile.as_ref() {
            if !profile.allowed_upstream_ids.is_empty() {
                providers.retain(|provider| {
                    profile_allows_upstream(profile, &provider.id)
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
        let mut all_pairs = Vec::with_capacity(all_providers.len());
        for provider in &all_providers {
            let cached = list_visible_upstream_model_ids(conn, &provider.id).unwrap_or_default();
            all_pairs.push((provider.clone(), cached));
        }
        let all_entries = crate::catalog::with_auto_entry_from_modes(
            style,
            crate::catalog::build_catalog_with(style, &all_pairs, hide_official),
            &modes,
        );
        let rules = list_route_rules(conn, profile_id).unwrap_or_default();
        Ok((all_providers, providers, entries, all_entries, profile, modes, rules))
    })?;

    // 2. Allowlist 阶段记录
    if profile.as_ref().is_some_and(|p| !p.allowed_upstream_ids.is_empty()) {
        let total_all = all_providers.len();
        let allowed_count = providers.len();
        let filtered_count = total_all.saturating_sub(allowed_count);
        steps.push(RouteTraceStep {
            stage: "allowlist".into(),
            id: "profile_allowlist".into(),
            matched: true,
            detail: format!("当前档案启用了允许列表，放行 {allowed_count} 个上游，过滤 {filtered_count} 个上游"),
        });
    }

    // 3. 显式/规则/模式推导说明
    let in_catalog = crate::catalog::is_explicit_catalog_passthrough(&entries, &requested);
    let in_all_catalog = crate::catalog::is_explicit_catalog_passthrough(&all_entries, &requested);

    if !in_catalog && in_all_catalog {
        let raw_resolved = resolve_request_strict(&all_entries, &all_providers, &requested);
        if let Ok(Some((raw_id, raw_slug))) = raw_resolved {
            let prov_name = all_providers.iter().find(|p| p.id == raw_id).map(|p| p.name.clone()).unwrap_or(raw_id);
            steps.push(RouteTraceStep {
                stage: "allowlist".into(),
                id: requested.clone(),
                matched: false,
                detail: format!("请求的模型对应上游 {} 不在当前档案白名单中", prov_name),
            });
            steps.push(RouteTraceStep {
                stage: "candidate".into(),
                id: requested.clone(),
                matched: false,
                detail: "没有可路由的供应商（被白名单过滤）".into(),
            });
            let candidates = vec![SimulateCandidate {
                index: 0,
                model: requested.clone(),
                upstream_id: None,
                upstream_name: Some(prov_name),
                upstream_model: Some(raw_slug),
                status: "rejected_by_allowlist".into(),
                cooldown_remaining_secs: None,
                selected: false,
                skip_reason: Some("上游不在当前档案白名单中".into()),
            }];
            return Ok(SimulateRouteResult {
                estimated_tokens: signals.token_count,
                decision: None,
                upstream_model: None,
                provider_id: None,
                provider_name: None,
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
                plan: None,
                candidates,
            });
        }
    }

    let role_explicit = !signals.is_subagent
        && crate::catalog::is_sticky_remap_role_id(&requested);
    let route_steps = explain_route(
        &requested,
        in_catalog,
        role_explicit,
        &signals,
        &modes,
        &rules,
    );
    steps.extend(route_steps);

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
    let routed = resolve_gateway_route_with_modes_strict(
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

    // 4. 候选评测与备用链执行模拟（纳入健康与冷却快照，无副作用、不出网、不抢 permit）
    let mut candidates = Vec::new();
    let (decision, upstream_model, provider_id, provider_name, execution_plan) = match routed {
        Ok(Some((primary_provider, primary_upstream, mut decision, plan, _is_subagent))) => {
            let primary_available = health::is_available(&primary_provider.id, Some(&primary_upstream));
            let primary_model_display = plan.primary_model.clone();

            if primary_available {
                candidates.push(SimulateCandidate {
                    index: 0,
                    model: primary_model_display.clone(),
                    upstream_id: Some(primary_provider.id.clone()),
                    upstream_name: Some(primary_provider.name.clone()),
                    upstream_model: Some(primary_upstream.clone()),
                    status: "healthy".into(),
                    cooldown_remaining_secs: None,
                    selected: true,
                    skip_reason: None,
                });
                steps.push(RouteTraceStep {
                    stage: "candidate".into(),
                    id: format!("{}/{}", primary_provider.name, primary_upstream),
                    matched: true,
                    detail: "首选候选健康可用".into(),
                });

                for attempt in plan.attempts.iter().skip(1) {
                    let fallback_model = &attempt.model;
                    let resolved = resolve_request_strict(&entries, &providers, fallback_model);
                    match resolved {
                        Ok(Some((cand_id, cand_slug))) => {
                            let cand_prov = providers.iter().find(|p| p.id == cand_id);
                            let is_cand_avail = health::is_available(&cand_id, Some(&cand_slug));
                            let rem = health::min_cooldown_remaining_secs(&cand_id, Some(&cand_slug));
                            let status = if is_cand_avail { "healthy".into() } else { "cooling".into() };
                            candidates.push(SimulateCandidate {
                                index: attempt.index,
                                model: fallback_model.clone(),
                                upstream_id: Some(cand_id),
                                upstream_name: cand_prov.map(|p| p.name.clone()),
                                upstream_model: Some(cand_slug),
                                status,
                                cooldown_remaining_secs: rem,
                                selected: false,
                                skip_reason: Some("首选可用，无需切换".into()),
                            });
                        }
                        _ => {
                            candidates.push(SimulateCandidate {
                                index: attempt.index,
                                model: fallback_model.clone(),
                                upstream_id: None,
                                upstream_name: None,
                                upstream_model: None,
                                status: "unknown".into(),
                                cooldown_remaining_secs: None,
                                selected: false,
                                skip_reason: Some("首选可用，无需切换".into()),
                            });
                        }
                    }
                    steps.push(RouteTraceStep {
                        stage: "fallback".into(),
                        id: fallback_model.clone(),
                        matched: false,
                        detail: "备用候选就绪（首选可用，无需切换）".into(),
                    });
                }

                (
                    Some(decision),
                    Some(primary_upstream),
                    Some(primary_provider.id),
                    Some(primary_provider.name),
                    Some(plan),
                )
            } else {
                let rem_secs = health::min_cooldown_remaining_secs(&primary_provider.id, Some(&primary_upstream));
                let h_info = health::lookup(&primary_provider.id);
                let status_str = h_info.as_ref().map(|h| h.status.clone()).unwrap_or_else(|| "cooling".into());
                let skip_reason = format!(
                    "上游处于 {} 状态{}",
                    status_str,
                    rem_secs.map(|s| format!("（冷却剩余 {}s）", s)).unwrap_or_default()
                );

                candidates.push(SimulateCandidate {
                    index: 0,
                    model: primary_model_display.clone(),
                    upstream_id: Some(primary_provider.id.clone()),
                    upstream_name: Some(primary_provider.name.clone()),
                    upstream_model: Some(primary_upstream.clone()),
                    status: status_str,
                    cooldown_remaining_secs: rem_secs,
                    selected: false,
                    skip_reason: Some(skip_reason.clone()),
                });
                steps.push(RouteTraceStep {
                    stage: "candidate".into(),
                    id: format!("{}/{}", primary_provider.name, primary_upstream),
                    matched: false,
                    detail: format!("首选不可用: {skip_reason}"),
                });

                if plan.explicit_pinned {
                    steps.push(RouteTraceStep {
                        stage: "fallback".into(),
                        id: "explicit_pinned".into(),
                        matched: false,
                        detail: "显式模型已锁定 (pinned)，禁止备用切换".into(),
                    });
                    decision.reason = format!("{}（首选 {} 不可用且显式锁定，禁止备用切换）", decision.reason, primary_provider.name);
                    (Some(decision), None, None, None, Some(plan))
                } else {
                    let mut fallback_winner: Option<(Provider, String, String)> = None;
                    for attempt in plan.attempts.iter().skip(1) {
                        let fallback_model = &attempt.model;
                        let resolved = resolve_request_strict(&entries, &providers, fallback_model);
                        match resolved {
                            Ok(Some((cand_id, cand_slug))) => {
                                let cand_provider = providers.iter().find(|p| p.id == cand_id).cloned();
                                if cand_id == primary_provider.id && cand_slug == primary_upstream {
                                    candidates.push(SimulateCandidate {
                                        index: attempt.index,
                                        model: fallback_model.clone(),
                                        upstream_id: Some(cand_id),
                                        upstream_name: cand_provider.as_ref().map(|p| p.name.clone()),
                                        upstream_model: Some(cand_slug),
                                        status: "skipped".into(),
                                        cooldown_remaining_secs: None,
                                        selected: false,
                                        skip_reason: Some("与首选相同，跳过".into()),
                                    });
                                    continue;
                                }
                                let is_avail = health::is_available(&cand_id, Some(&cand_slug));
                                let cand_rem = health::min_cooldown_remaining_secs(&cand_id, Some(&cand_slug));
                                if !is_avail {
                                    let cand_status = health::lookup(&cand_id)
                                        .map(|h| h.status)
                                        .unwrap_or_else(|| "cooling".into());
                                    let cand_skip = format!("备用上游处于 {} 状态{}", cand_status, cand_rem.map(|s| format!("（冷却剩余 {}s）", s)).unwrap_or_default());
                                    candidates.push(SimulateCandidate {
                                        index: attempt.index,
                                        model: fallback_model.clone(),
                                        upstream_id: Some(cand_id),
                                        upstream_name: cand_provider.as_ref().map(|p| p.name.clone()),
                                        upstream_model: Some(cand_slug),
                                        status: cand_status,
                                        cooldown_remaining_secs: cand_rem,
                                        selected: false,
                                        skip_reason: Some(cand_skip.clone()),
                                    });
                                    steps.push(RouteTraceStep {
                                        stage: "fallback".into(),
                                        id: fallback_model.clone(),
                                        matched: false,
                                        detail: cand_skip,
                                    });
                                } else if fallback_winner.is_none() {
                                    let prov = cand_provider.unwrap();
                                    candidates.push(SimulateCandidate {
                                        index: attempt.index,
                                        model: fallback_model.clone(),
                                        upstream_id: Some(prov.id.clone()),
                                        upstream_name: Some(prov.name.clone()),
                                        upstream_model: Some(cand_slug.clone()),
                                        status: "healthy".into(),
                                        cooldown_remaining_secs: None,
                                        selected: true,
                                        skip_reason: None,
                                    });
                                    steps.push(RouteTraceStep {
                                        stage: "fallback".into(),
                                        id: fallback_model.clone(),
                                        matched: true,
                                        detail: format!("首选冷却，已切换到备用模型 {} (上游 {})", fallback_model, prov.name),
                                    });
                                    fallback_winner = Some((prov, cand_slug, fallback_model.clone()));
                                } else {
                                    candidates.push(SimulateCandidate {
                                        index: attempt.index,
                                        model: fallback_model.clone(),
                                        upstream_id: Some(cand_id),
                                        upstream_name: cand_provider.as_ref().map(|p| p.name.clone()),
                                        upstream_model: Some(cand_slug),
                                        status: "healthy".into(),
                                        cooldown_remaining_secs: None,
                                        selected: false,
                                        skip_reason: Some("已有可用备用，无需切换".into()),
                                    });
                                }
                            }
                            _ => {
                                let raw_resolved = resolve_request_strict(&all_entries, &all_providers, fallback_model);
                                let (status, skip_detail) = match raw_resolved {
                                    Ok(Some((raw_id, _))) => {
                                        let prov_name = all_providers.iter().find(|p| p.id == raw_id).map(|p| p.name.as_str()).unwrap_or(&raw_id);
                                        ("rejected_by_allowlist".to_string(), format!("上游 {} 不在当前档案白名单中", prov_name))
                                    }
                                    _ => ("unknown_model".to_string(), "目录中不存在该备用模型".to_string()),
                                };
                                candidates.push(SimulateCandidate {
                                    index: attempt.index,
                                    model: fallback_model.clone(),
                                    upstream_id: None,
                                    upstream_name: None,
                                    upstream_model: None,
                                    status,
                                    cooldown_remaining_secs: None,
                                    selected: false,
                                    skip_reason: Some(skip_detail.clone()),
                                });
                                steps.push(RouteTraceStep {
                                    stage: "fallback".into(),
                                    id: fallback_model.clone(),
                                    matched: false,
                                    detail: skip_detail,
                                });
                            }
                        }
                    }

                    if let Some((winner_prov, winner_slug, winner_model)) = fallback_winner {
                        decision.reason = format!("{}（首选 {} 冷却中，切换到备用 {}）", decision.reason, primary_provider.name, winner_model);
                        decision.upstream_id = Some(winner_prov.id.clone());
                        decision.normalized_model = winner_slug.clone();
                        (
                            Some(decision),
                            Some(winner_slug),
                            Some(winner_prov.id),
                            Some(winner_prov.name),
                            Some(plan),
                        )
                    } else {
                        decision.reason = format!("{}（首选及所有备用候选均不可用/冷却中）", decision.reason);
                        (Some(decision), None, None, None, Some(plan))
                    }
                }
            }
        }
        _ => {
            let raw_resolved = resolve_request_strict(&all_entries, &all_providers, &requested);
            match raw_resolved {
                Ok(Some((raw_id, _))) => {
                    let prov_name = all_providers.iter().find(|p| p.id == raw_id).map(|p| p.name.as_str()).unwrap_or(&raw_id);
                    steps.push(RouteTraceStep {
                        stage: "allowlist".into(),
                        id: requested.clone(),
                        matched: false,
                        detail: format!("模型对应的上游 {} 被当前档案白名单过滤", prov_name),
                    });
                    steps.push(RouteTraceStep {
                        stage: "candidate".into(),
                        id: requested.clone(),
                        matched: false,
                        detail: "没有可路由的供应商（被白名单过滤）".into(),
                    });
                }
                _ => {
                    steps.push(RouteTraceStep {
                        stage: "candidate".into(),
                        id: requested.clone(),
                        matched: false,
                        detail: "未能在目录中找到匹配模型".into(),
                    });
                }
            }
            (None, None, None, None, None)
        }
    };

    if let Some(id) = provider_id.as_deref() {
        let pressure = db.gateway_upstream_limiter.snapshot_or_default(id);
        let policy = &pressure.policy;
        let limited = (policy.max_concurrency > 0 && pressure.active >= policy.max_concurrency)
            || (policy.rpm > 0 && pressure.current_rpm >= policy.rpm)
            || pressure.queue_len > 0;
        let availability = if !limited {
            "当前可直接准入"
        } else if policy.queue_capacity == 0 || policy.queue_timeout_ms == 0
            || pressure.queue_len >= policy.queue_capacity
        {
            "当前将立即拒绝"
        } else {
            "当前需要排队"
        };
        let has_fallback = execution_plan.as_ref().is_some_and(|plan| plan.attempts.len() > 1);
        steps.push(RouteTraceStep {
            stage: "admission".into(),
            id: id.to_string(),
            matched: !limited,
            detail: format!(
                "{availability}：活跃 {}，排队 {}，近 60 秒出站 {}；{}。只读快照，不入队、不扣 RPM，不保证实际执行结果。",
                pressure.active, pressure.queue_len, pressure.current_rpm,
                if has_fallback { "已配置备用，是否切换仍按现有策略" } else { "无显式备用" },
            ),
        });
    }

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
        plan: execution_plan,
        candidates,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dao::gateway::{
        ensure_profile_for_target, patch_profile, patch_route_mode, replace_upstream_models,
        upsert_upstream, GatewayProfilePatch, RouteModePatch,
    };
    use crate::gateway::budget::BudgetSettings;
    use crate::provider::{
        ClaudeModelMapping, ProtocolType, ProviderInput, ProviderKind, ProviderTarget,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn test_id(prefix: &str) -> String {
        format!("{prefix}_{}_{}", std::process::id(), TEST_SEQ.fetch_add(1, Ordering::Relaxed))
    }

    fn test_provider_input(
        id: &str,
        target: ProviderTarget,
        base_url: &str,
        model: &str,
        name: &str,
    ) -> ProviderInput {
        ProviderInput {
            id: Some(id.to_string()),
            name: name.to_string(),
            base_url: base_url.to_string(),
            // 试跑只读决策，不需要真实或测试凭据，也不应依赖系统凭据库。
            api_key: String::new(),
            clear_api_key: false,
            model: model.to_string(),
            model_context_window: Some(200_000),
            auto_review_model_override: None,
            web_search_enabled: Some(false),
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: target,
            notes: String::new(),
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
        }
    }

    #[test]
    fn test_simulate_budget_reject() {
        let db = Database::memory().expect("test db");
        let now = chrono::Utc::now().timestamp_millis();
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model, currency, input_price_per_million, output_price_per_million) VALUES ('test-model', 'USD', 1000.0, 1000.0);",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (id, created_at, model, input_tokens, output_tokens, data_source) VALUES ('log_test', ?1, 'test-model', 1000000, 1000000, 'proxy');",
                [now],
            )?;
            Ok(())
        }).expect("seed");

        budget::invalidate_cache();
        budget::persist(&db, &BudgetSettings {
            daily_budget_usd: 10.0,
            action: "reject".into(),
            fallback_model: String::new(),
        }).expect("persist budget");

        let input = SimulateRouteInput {
            requested_model: Some("auto".into()),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        assert!(result.decision.is_none());
        assert!(result.upstream_model.is_none());
        assert!(result.provider_id.is_none());
        let budget_step = result.steps.iter().find(|s| s.stage == "budget").expect("budget step");
        assert!(budget_step.matched);
        assert!(budget_step.detail.contains("429") || budget_step.detail.contains("拦截"));
    }

    #[test]
    fn test_simulate_budget_fallback() {
        let db = Database::memory().expect("test db");
        let now = chrono::Utc::now().timestamp_millis();
        let fallback_id = test_id("up_fallback");
        db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let fb_prov = upsert_upstream(conn, &test_provider_input(&fallback_id, ProviderTarget::ClaudeCode, "https://fb.test", "claude-haiku-fb", "FB Provider"))?;
            replace_upstream_models(conn, &fb_prov.id, &["claude-haiku-fb".into()])?;
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model, currency, input_price_per_million, output_price_per_million) VALUES ('test-model', 'USD', 1000.0, 1000.0);",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (id, created_at, model, input_tokens, output_tokens, data_source) VALUES ('log_test', ?1, 'test-model', 1000000, 1000000, 'proxy');",
                [now],
            )?;
            Ok(())
        }).expect("seed");

        budget::invalidate_cache();
        budget::persist(&db, &BudgetSettings {
            daily_budget_usd: 10.0,
            action: "fallback".into(),
            fallback_model: "claude-haiku-fb".into(),
        }).expect("persist budget");

        let input = SimulateRouteInput {
            requested_model: Some("auto".into()),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        let budget_step = result.steps.iter().find(|s| s.stage == "budget").expect("budget step");
        assert!(budget_step.matched);
        assert!(budget_step.detail.contains("claude-haiku-fb"));
        assert_eq!(result.upstream_model.as_deref(), Some("claude-haiku-fb"));
        assert_eq!(result.provider_id.as_deref(), Some(fallback_id.as_str()));
    }

    #[test]
    fn test_simulate_healthy_primary_selects_primary_and_standby_fallbacks() {
        let db = Database::memory().expect("test db");
        let primary_id = test_id("up_pri");
        let backup_id = test_id("up_bak");
        let (_primary_public, _backup_public) = db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let p1 = upsert_upstream(conn, &test_provider_input(&primary_id, ProviderTarget::ClaudeCode, "https://p1.test", "primary-slug", "P1"))?;
            let p2 = upsert_upstream(conn, &test_provider_input(&backup_id, ProviderTarget::ClaudeCode, "https://p2.test", "backup-slug", "P2"))?;
            replace_upstream_models(conn, &p1.id, &["primary-slug".into()])?;
            replace_upstream_models(conn, &p2.id, &["backup-slug".into()])?;
            let pairs = vec![(p1, vec!["primary-slug".into()]), (p2, vec!["backup-slug".into()])];
            let entries = crate::catalog::build_catalog_with(CatalogStyle::Claude, &pairs, false);
            let pri_pub = entries.iter().find(|e| e.provider_id == primary_id).unwrap().public_id.clone();
            let bak_pub = entries.iter().find(|e| e.provider_id == backup_id).unwrap().public_id.clone();
            patch_route_mode(conn, "default", &RouteModePatch {
                enabled: Some(true),
                model: Some(pri_pub.clone()),
                fallback_models: Some(vec![bak_pub.clone()]),
                ..RouteModePatch::default()
            }, Some(SHARED_PROFILE_ID))?;
            Ok((pri_pub, bak_pub))
        }).expect("seed");

        health::record_success(&primary_id, Some(20));
        health::record_success(&backup_id, Some(30));

        let input = SimulateRouteInput {
            requested_model: Some("auto".into()),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let before_pressure = db.gateway_upstream_limiter.snapshot_all();
        let result = simulate(&db, input).expect("simulate");
        assert_eq!(db.gateway_upstream_limiter.snapshot_all(), before_pressure);
        assert!(result.steps.iter().any(|s| s.stage == "admission" && s.detail.contains("只读快照")));
        assert_eq!(result.provider_id.as_deref(), Some(primary_id.as_str()));
        assert_eq!(result.upstream_model.as_deref(), Some("primary-slug"));

        assert_eq!(result.candidates.len(), 2);
        assert!(result.candidates[0].selected);
        assert_eq!(result.candidates[0].status, "healthy");
        assert!(!result.candidates[1].selected);
        assert_eq!(result.candidates[1].status, "healthy");
        assert!(result.candidates[1].skip_reason.as_deref().unwrap().contains("首选可用"));

        assert!(result.steps.iter().any(|s| s.stage == "candidate" && s.matched));
        assert!(result.steps.iter().any(|s| s.stage == "fallback" && !s.matched));
    }

    #[test]
    fn test_simulate_cooling_primary_falls_over_to_backup() {
        let db = Database::memory().expect("test db");
        let primary_id = test_id("up_pri_cool");
        let backup_id = test_id("up_bak_cool");
        let (_primary_public, _backup_public) = db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let p1 = upsert_upstream(conn, &test_provider_input(&primary_id, ProviderTarget::ClaudeCode, "https://p1.test", "primary-slug", "P1"))?;
            let p2 = upsert_upstream(conn, &test_provider_input(&backup_id, ProviderTarget::ClaudeCode, "https://p2.test", "backup-slug", "P2"))?;
            replace_upstream_models(conn, &p1.id, &["primary-slug".into()])?;
            replace_upstream_models(conn, &p2.id, &["backup-slug".into()])?;
            let pairs = vec![(p1, vec!["primary-slug".into()]), (p2, vec!["backup-slug".into()])];
            let entries = crate::catalog::build_catalog_with(CatalogStyle::Claude, &pairs, false);
            let pri_pub = entries.iter().find(|e| e.provider_id == primary_id).unwrap().public_id.clone();
            let bak_pub = entries.iter().find(|e| e.provider_id == backup_id).unwrap().public_id.clone();
            patch_route_mode(conn, "default", &RouteModePatch {
                enabled: Some(true),
                model: Some(pri_pub.clone()),
                fallback_models: Some(vec![bak_pub.clone()]),
                ..RouteModePatch::default()
            }, Some(SHARED_PROFILE_ID))?;
            Ok((pri_pub, bak_pub))
        }).expect("seed");

        // Mark primary as in cooldown / circuit open
        health::record_failure(&primary_id);
        health::record_failure(&primary_id);
        assert!(!health::is_available(&primary_id, Some("primary-slug")));

        health::record_success(&backup_id, Some(30));
        assert!(health::is_available(&backup_id, Some("backup-slug")));

        let input = SimulateRouteInput {
            requested_model: Some("auto".into()),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        assert_eq!(result.provider_id.as_deref(), Some(backup_id.as_str()));
        assert_eq!(result.upstream_model.as_deref(), Some("backup-slug"));

        assert_eq!(result.candidates.len(), 2);
        assert!(!result.candidates[0].selected);
        assert_eq!(result.candidates[0].status, "cooling");
        assert!(result.candidates[1].selected);
        assert_eq!(result.candidates[1].status, "healthy");

        let reason = result.decision.as_ref().unwrap().reason.clone();
        assert!(reason.contains("切换到备用") || reason.contains("冷却中"));

        assert!(result.steps.iter().any(|s| s.stage == "candidate" && !s.matched));
        assert!(result.steps.iter().any(|s| s.stage == "fallback" && s.matched));
    }

    #[test]
    fn test_simulate_pinned_explicit_blocks_failover() {
        let db = Database::memory().expect("test db");
        let primary_id = test_id("up_pri_pin");
        let backup_id = test_id("up_bak_pin");
        let (primary_public, _backup_public) = db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let p1 = upsert_upstream(conn, &test_provider_input(&primary_id, ProviderTarget::ClaudeCode, "https://p1.test", "primary-slug", "P1"))?;
            let p2 = upsert_upstream(conn, &test_provider_input(&backup_id, ProviderTarget::ClaudeCode, "https://p2.test", "backup-slug", "P2"))?;
            replace_upstream_models(conn, &p1.id, &["primary-slug".into()])?;
            replace_upstream_models(conn, &p2.id, &["backup-slug".into()])?;
            let pairs = vec![(p1, vec!["primary-slug".into()]), (p2, vec!["backup-slug".into()])];
            let entries = crate::catalog::build_catalog_with(CatalogStyle::Claude, &pairs, false);
            let pri_pub = entries.iter().find(|e| e.provider_id == primary_id).unwrap().public_id.clone();
            let bak_pub = entries.iter().find(|e| e.provider_id == backup_id).unwrap().public_id.clone();
            patch_profile(conn, SHARED_PROFILE_ID, &GatewayProfilePatch {
                explicit_fallback_enabled: Some(false),
                fallback_models: Some(vec![bak_pub.clone()]),
                ..GatewayProfilePatch::default()
            })?;
            Ok((pri_pub, bak_pub))
        }).expect("seed");

        health::record_failure(&primary_id);
        health::record_failure(&primary_id);

        let input = SimulateRouteInput {
            requested_model: Some(primary_public),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        assert!(result.upstream_model.is_none());
        assert!(result.provider_id.is_none());
        assert!(result.steps.iter().any(|s| s.stage == "fallback" && s.id == "explicit_pinned"));
    }

    #[test]
    fn test_simulate_allowlist_filtering() {
        let db = Database::memory().expect("test db");
        let allowed_id = test_id("up_allowed");
        let blocked_id = test_id("up_blocked");
        let (_allowed_public, blocked_public) = db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let p1 = upsert_upstream(conn, &test_provider_input(&allowed_id, ProviderTarget::ClaudeCode, "https://p1.test", "allowed-slug", "P1"))?;
            let p2 = upsert_upstream(conn, &test_provider_input(&blocked_id, ProviderTarget::ClaudeCode, "https://p2.test", "blocked-slug", "P2"))?;
            replace_upstream_models(conn, &p1.id, &["allowed-slug".into()])?;
            replace_upstream_models(conn, &p2.id, &["blocked-slug".into()])?;
            let pairs = vec![(p1, vec!["allowed-slug".into()]), (p2, vec!["blocked-slug".into()])];
            let entries = crate::catalog::build_catalog_with(CatalogStyle::Claude, &pairs, false);
            let allow_pub = entries.iter().find(|e| e.provider_id == allowed_id).unwrap().public_id.clone();
            let block_pub = entries.iter().find(|e| e.provider_id == blocked_id).unwrap().public_id.clone();
            patch_profile(conn, SHARED_PROFILE_ID, &GatewayProfilePatch {
                allowed_upstream_ids: Some(vec![allowed_id.clone()]),
                ..GatewayProfilePatch::default()
            })?;
            Ok((allow_pub, block_pub))
        }).expect("seed");

        let input = SimulateRouteInput {
            requested_model: Some(blocked_public),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        assert!(result.decision.is_none());
        assert!(result.upstream_model.is_none());
        assert!(result.steps.iter().any(|s| s.stage == "allowlist"));
    }

    #[test]
    fn test_simulate_all_candidates_cooling() {
        let db = Database::memory().expect("test db");
        let primary_id = test_id("up_all_cool1");
        let backup_id = test_id("up_all_cool2");
        let (_primary_public, _backup_public) = db.with_conn(|conn| {
            ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
            let p1 = upsert_upstream(conn, &test_provider_input(&primary_id, ProviderTarget::ClaudeCode, "https://p1.test", "primary-slug", "P1"))?;
            let p2 = upsert_upstream(conn, &test_provider_input(&backup_id, ProviderTarget::ClaudeCode, "https://p2.test", "backup-slug", "P2"))?;
            replace_upstream_models(conn, &p1.id, &["primary-slug".into()])?;
            replace_upstream_models(conn, &p2.id, &["backup-slug".into()])?;
            let pairs = vec![(p1, vec!["primary-slug".into()]), (p2, vec!["backup-slug".into()])];
            let entries = crate::catalog::build_catalog_with(CatalogStyle::Claude, &pairs, false);
            let pri_pub = entries.iter().find(|e| e.provider_id == primary_id).unwrap().public_id.clone();
            let bak_pub = entries.iter().find(|e| e.provider_id == backup_id).unwrap().public_id.clone();
            patch_route_mode(conn, "default", &RouteModePatch {
                enabled: Some(true),
                model: Some(pri_pub.clone()),
                fallback_models: Some(vec![bak_pub.clone()]),
                ..RouteModePatch::default()
            }, Some(SHARED_PROFILE_ID))?;
            Ok((pri_pub, bak_pub))
        }).expect("seed");

        health::record_failure(&primary_id);
        health::record_failure(&primary_id);
        health::record_failure(&backup_id);
        health::record_failure(&backup_id);

        let input = SimulateRouteInput {
            requested_model: Some("auto".into()),
            body_json: None,
            token_count: Some(100),
            has_web_search: None,
            has_vision: None,
            has_thinking: None,
            is_subagent: None,
            is_image_gen: None,
            tool_names: None,
            recent_write_tool: None,
            path: None,
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        };

        let result = simulate(&db, input).expect("simulate");
        assert!(result.upstream_model.is_none());
        assert!(result.provider_id.is_none());
        assert_eq!(result.candidates.len(), 2);
        assert!(!result.candidates[0].selected);
        assert!(!result.candidates[1].selected);
        assert_eq!(result.candidates[0].status, "cooling");
        assert_eq!(result.candidates[1].status, "cooling");
        let reason = result.decision.as_ref().unwrap().reason.clone();
        assert!(reason.contains("不可用") || reason.contains("冷却中"));
    }
}

