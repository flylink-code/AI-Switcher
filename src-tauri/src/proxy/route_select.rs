pub(crate) fn select_gateway_runtime_provider_with(
    state: &ProxyState,
    requested_model: &str,
    force_catalog_subagent: bool,
    incoming: &Value,
    request_path: &str,
    headers: &HeaderMap,
) -> AppResult<Option<(Provider, String, bool, crate::gateway::RouteDecision, crate::gateway::RouteExecutionPlan)>> {
    let style = crate::catalog::catalog_style_for(state.target);
    let (providers, entries) = load_gateway_catalog(state, style)?;
    let profile = state
        .db
        .with_read_conn(|conn| crate::database::dao::gateway::current_profile(conn, state.target))
        .ok()
        .flatten();
    let modes = match state.db.with_read_conn(crate::gateway::modes::load_modes) {
        Ok(modes) => modes,
        Err(error) => {
            log::warn!("failed to load route_modes: {error}");
            Vec::new()
        }
    };
    let rules = state
        .db
        .with_read_conn(|conn| {
            crate::database::dao::gateway::list_route_rules(
                conn,
                crate::database::dao::gateway::SHARED_PROFILE_ID,
            )
        })
        .unwrap_or_default();
    let tool_names = crate::gateway::modes::extract_tool_names(incoming);
    let recent_write_tool = crate::gateway::modes::extract_recent_write_tool(incoming);
    if matches!(state.listener_kind, ListenerKind::SmartGateway) {
        log::info!(
            "tool-signal-probe tools=[{}] write={} path={} target={}",
            tool_names.join(","),
            recent_write_tool.as_deref().unwrap_or("-"),
            request_path,
            state.target.as_str()
        );
    }
    let hints = crate::gateway::RouteHints {
        token_count: crate::gateway::estimate_request_tokens(incoming),
        has_web_search: crate::gateway::request_has_web_search(incoming),
        has_vision: crate::gateway::modes::has_vision_content(incoming),
        has_thinking: crate::gateway::modes::has_thinking_signal(incoming),
        is_image_gen: request_path.contains("/images/generations"),
        tool_names,
        recent_write_tool,
        path: request_path.to_string(),
        target: Some(state.target),
    };
    let session_key = crate::gateway::sticky::session_key_from(
        incoming,
        session_prompt_cache_hint(headers).as_deref(),
    );
    let Some((provider, upstream, decision, plan, is_catalog_subagent)) =
        crate::gateway::resolve_gateway_route_with_modes(
            style,
            &entries,
            &providers,
            &crate::gateway::sticky::rewrite_requested(
                state.target,
                requested_model,
                &entries,
                &session_key,
            ),
            force_catalog_subagent,
            profile.as_ref(),
            &hints,
            &modes,
            &rules,
        )
    else {
        return Ok(None);
    };
    let Some(mut provider) = hydrate_provider_credential(state, provider)? else {
        return Ok(None);
    };
    provider.model = upstream.clone();
    log::info!(
        "Catalog route client={} normalized={} reason={} provider={} upstream={} subagent={}",
        decision.requested_model,
        decision.normalized_model,
        decision.reason,
        provider.name,
        upstream,
        is_catalog_subagent
    );
    Ok(Some((provider, upstream, is_catalog_subagent, decision, plan)))
}

fn prepare_upstream_request(
    state: &ProxyState,
    provider: &mut Provider,
    method: &Method,
    headers: &HeaderMap,
    incoming: &Value,
    body_bytes: &Bytes,
    incoming_stream: bool,
) -> AppResult<PreparedUpstreamRequest> {
    let requested_model = incoming.get("model").and_then(Value::as_str).unwrap_or("");
    provider.model = resolve_upstream_model(provider, requested_model);
    let image_gen = state.request_path.contains("/images/generations");
    let endpoint_path = if image_gen {
        "/v1/images/generations"
    } else {
        protocol_endpoint_path_for_provider(provider)
    };
    let target_url = api_endpoint_url(&provider.base_url, endpoint_path)?;
    let (outgoing_body, translated) = if image_gen {
        (body_bytes.clone(), false)
    } else {
        encode_upstream_request(provider, incoming, body_bytes, incoming_stream, headers)
    };
    let mut builder = state.client.request(method.clone(), target_url).header(header::CONTENT_TYPE, "application/json");
    if !provider.is_codex_oauth() {
        for (name, value) in headers.iter() {
            let name_str = name.as_str();
            if is_hop_by_hop_header(name_str)
                || name_str.eq_ignore_ascii_case("host")
                || name_str.eq_ignore_ascii_case("content-length")
                || name_str.eq_ignore_ascii_case("content-type")
                || name_str.eq_ignore_ascii_case("authorization")
                || name_str.eq_ignore_ascii_case("x-api-key")
            {
                continue;
            }
            builder = builder.header(name, value);
        }
    }
    if let Some(correlation) = state.correlation.as_ref() {
        builder = builder.header(
            crate::gateway::correlation::REQUEST_ID_HEADER,
            correlation.id.as_str(),
        );
        if let Some(target) = correlation.target_app.as_deref() {
            builder = builder.header(crate::gateway::correlation::TARGET_APP_HEADER, target);
        }
    }
    let key = provider.api_key.trim();
    builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    if provider.is_codex_oauth() {
        builder = builder
            .header("originator", crate::codex_oauth::ORIGINATOR)
            .header("version", crate::codex_oauth::CLIENT_VERSION)
            .header("Chatgpt-Account-Id", provider.auth_binding.trim());
    } else {
        builder = builder.header("x-api-key", key);
    }
    if let Some(ref custom_headers) = provider.custom_headers {
        for (k, v) in custom_headers {
            if !is_hop_by_hop_header(k)
                && !k.eq_ignore_ascii_case("host")
                && !k.eq_ignore_ascii_case("content-length")
            {
                builder = builder.header(k.as_str(), v.as_str());
            }
        }
    }
    Ok(PreparedUpstreamRequest { builder, outgoing_body, translated })
}

fn compatible_stream_retry(provider: &Provider, prepared: &PreparedUpstreamRequest, incoming_stream: bool) -> Option<reqwest::RequestBuilder> {
    if !prepared.translated || !incoming_stream || !matches!(provider.protocol_type, ProtocolType::OpenAiChat | ProtocolType::Proxy) {
        return None;
    }
    serde_json::from_slice::<Value>(&prepared.outgoing_body)
        .ok()
        .and_then(|mut value| {
            value.as_object_mut()?.remove("stream_options")?;
            serde_json::to_vec(&value).ok()
        })
        .and_then(|body| prepared.builder.try_clone().map(|builder| builder.body(body)))
}

pub(crate) fn is_retryable_upstream_status(state: &ProxyState, status: StatusCode) -> bool {
    let codes = state
        .db
        .with_conn(|conn| load_retryable_status_codes(conn))
        .unwrap_or_else(|_| default_retryable_status_codes());
    codes.contains(&status.as_u16())
}

/// Antigravity already rotates its account pool on rate limits and timeouts.
/// Failing over to
/// unrelated Desktop providers (Kimi / DeepSeek / …) with a Gemini model id
/// produces extra 4xx/502 rows, and Claude Desktop then retries 502 immediately.
pub(crate) fn should_failover_upstream_status(provider: &Provider, status: StatusCode) -> bool {
    should_failover_upstream_status_ex(provider, status, false)
}

/// `catalog_cross_provider_429` lets Codex unified-catalog mode fail over an
/// Antigravity 429 onto the next catalog provider (with a rewritten model).
pub(crate) fn should_failover_upstream_status_ex(
    provider: &Provider,
    status: StatusCode,
    catalog_cross_provider_429: bool,
) -> bool {
    if provider.is_antigravity() && status == StatusCode::GATEWAY_TIMEOUT {
        return false;
    }
    if catalog_cross_provider_429 {
        return true;
    }
    !(provider.is_antigravity() && status == StatusCode::TOO_MANY_REQUESTS)
}

/// Explicit mode backup chain may switch on AG 429/504; generic vendor walk
/// still uses [`should_failover_upstream_status`].
pub(crate) fn should_try_route_plan_fallback(_provider: &Provider, status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::GATEWAY_TIMEOUT
        || default_retryable_status_codes().contains(&status.as_u16())
}

pub(crate) fn route_plan_followup_attempts(
    plan: Option<&crate::gateway::RouteExecutionPlan>,
) -> Vec<crate::gateway::RouteAttemptPlan> {
    plan.filter(|plan| plan.attempts.len() > 1)
        .map(|plan| plan.attempts.iter().skip(1).cloned().collect())
        .unwrap_or_default()
}

pub(crate) fn load_gateway_attempt_provider(
    state: &ProxyState,
    upstream_id: &str,
    model: &str,
) -> AppResult<Option<Provider>> {
    let style = crate::catalog::catalog_style_for(state.target);
    let (providers, _) = load_gateway_catalog(state, style)?;
    let Some(provider) = providers
        .into_iter()
        .find(|provider| provider.id == upstream_id && !provider.is_smart_gateway())
    else {
        return Ok(None);
    };
    let Some(mut provider) = hydrate_provider_credential(state, provider)? else {
        return Ok(None);
    };
    provider.model = model.to_string();
    Ok(Some(provider))
}

pub fn default_retryable_status_codes() -> Vec<u16> {
    let mut codes = Vec::new();
    codes.extend(400..=404);
    codes.push(408);
    codes.push(429);
    codes.extend(500..=599);
    codes
}

pub fn load_retryable_status_codes(conn: &rusqlite::Connection) -> AppResult<Vec<u16>> {
    let Some(raw) = get_setting(conn, PROXY_RETRYABLE_STATUS_CODES_KEY)? else {
        return Ok(default_retryable_status_codes());
    };
    parse_retryable_status_codes(&raw)
}

pub fn parse_retryable_status_codes(raw: &str) -> AppResult<Vec<u16>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(default_retryable_status_codes());
    }
    let mut codes = Vec::new();
    for part in trimmed.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: u16 = start
                .trim()
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码范围: {part}")))?;
            let end: u16 = end
                .trim()
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码范围: {part}")))?;
            if start > end || start < 100 || end > 599 {
                return Err(AppError::Config(format!("无效的重试状态码范围: {part}")));
            }
            codes.extend(start..=end);
        } else {
            let code: u16 = part
                .parse()
                .map_err(|_| AppError::Config(format!("无效的重试状态码: {part}")))?;
            if !(100..=599).contains(&code) {
                return Err(AppError::Config(format!("无效的重试状态码: {part}")));
            }
            codes.push(code);
        }
    }
    codes.sort_unstable();
    codes.dedup();
    if codes.is_empty() {
        Ok(default_retryable_status_codes())
    } else {
        Ok(codes)
    }
}

pub fn format_retryable_status_codes(codes: &[u16]) -> String {
    if codes.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut start = codes[0];
    let mut prev = codes[0];
    for &code in &codes[1..] {
        if code == prev + 1 {
            prev = code;
            continue;
        }
        if start == prev {
            parts.push(start.to_string());
        } else {
            parts.push(format!("{start}-{prev}"));
        }
        start = code;
        prev = code;
    }
    if start == prev {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}-{prev}"));
    }
    parts.join(",")
}

pub fn load_streaming_idle_timeout_secs(conn: &rusqlite::Connection) -> AppResult<u64> {
    let Some(raw) = get_setting(conn, PROXY_STREAMING_IDLE_TIMEOUT_KEY)? else {
        return Ok(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS);
    };
    let value = raw
        .trim()
        .parse::<u64>()
        .map_err(|_| AppError::Config("流式空闲超时必须是正整数秒".to_string()))?;
    Ok(value.clamp(5, 3600))
}

async fn health_handler(State(state): State<ProxyState>) -> impl IntoResponse {
    let provider = state.db.with_conn(|conn| get_current_provider(conn, state.target));
    let (status, provider_id, protocol, credential_ready, upstream_status, checked_at) = match provider {
        Ok(Some(provider)) => {
            let credential_ready = matches!(
                state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)),
                Ok(Some(key)) if !key.trim().is_empty()
            );
            let status = if credential_ready { "ok" } else { "degraded" };
            (
                status,
                Some(provider.id),
                Some(provider.protocol_type.as_str()),
                credential_ready,
                provider.health_status,
                provider.health_checked_at,
            )
        }
        Ok(None) => ("degraded", None, None, false, None, None),
        Err(error) => {
            log::error!("健康检查读取当前供应商失败: {error}");
            ("degraded", None, None, false, None, None)
        }
    };
    axum::Json(serde_json::json!({
        "status": status,
        "proxyListening": true,
        "targetApp": state.target.as_str(),
        "port": state.port,
        "uptimeSeconds": state.started_at.elapsed().as_secs(),
        "providerId": provider_id,
        "protocol": protocol,
        "credentialReady": credential_ready,
        "lastUpstreamCheck": {"status": upstream_status, "checkedAt": checked_at},
    }))
}

async fn models_handler(State(state): State<ProxyState>, headers: HeaderMap) -> Response {
    if let Err(error) = validate_listener_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    if gateway_catalog_enabled(&state) || desktop_bound_smart_gateway(&state) {
        let target = if gateway_catalog_enabled(&state) {
            resolve_binding_target(&state, &headers).unwrap_or(state.target)
        } else {
            state.target
        };
        let style = crate::catalog::catalog_style_for(target);
        match load_gateway_catalog(&state, style) {
            Ok((_, entries)) if !entries.is_empty() => {
                let payload = match style {
                    CatalogStyle::Claude => claude_discovery_payload(&entries),
                    CatalogStyle::Codex => openai_models_payload(&entries),
                };
                axum::Json(payload).into_response()
            }
            Ok(_) => json_error(StatusCode::SERVICE_UNAVAILABLE, "没有已配置的第三方供应商"),
            Err(error) => {
                log::error!("读取模型目录失败: {error}");
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录")
            }
        }
    } else {
        match state
            .db
            .with_conn(|conn| get_current_provider(conn, state.target))
        {
            Ok(Some(provider)) => {
                axum::Json(crate::config::claude_desktop::model_list_response(&provider))
                    .into_response()
            }
            Ok(None) => json_error(StatusCode::SERVICE_UNAVAILABLE, "没有激活的第三方供应商"),
            Err(error) => {
                log::error!("读取模型目录失败: {error}");
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录")
            }
        }
    }
}

async fn model_retrieve_handler(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = validate_listener_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    if gateway_catalog_enabled(&state) || desktop_bound_smart_gateway(&state) {
        let target = if gateway_catalog_enabled(&state) {
            resolve_binding_target(&state, &headers).unwrap_or(state.target)
        } else {
            state.target
        };
        let style = crate::catalog::catalog_style_for(target);
        match load_gateway_catalog(&state, style) {
            Ok((_, entries)) if !entries.is_empty() => {
                if let Some(entry) = find_catalog_entry(&entries, &id) {
                    let payload = match style {
                        CatalogStyle::Claude => claude_discovery_model(entry),
                        CatalogStyle::Codex => crate::catalog::openai_discovery_model(entry),
                    };
                    axum::Json(payload).into_response()
                } else {
                    json_error(StatusCode::NOT_FOUND, format!("找不到模型: {id}"))
                }
            }
            Ok(_) => json_error(StatusCode::SERVICE_UNAVAILABLE, "没有已配置的第三方供应商"),
            Err(error) => {
                log::error!("读取模型失败: {error}");
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录")
            }
        }
    } else {
        match state
            .db
            .with_conn(|conn| get_current_provider(conn, state.target))
        {
            Ok(Some(provider)) => {
                let list = crate::config::claude_desktop::model_list_response(&provider);
                let found = list
                    .get("data")
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter().find(|row| {
                            row.get("id")
                                .and_then(Value::as_str)
                                .is_some_and(|item| item.eq_ignore_ascii_case(id.trim()))
                        })
                    })
                    .cloned();
                match found {
                    Some(row) => axum::Json(row).into_response(),
                    None => json_error(StatusCode::NOT_FOUND, format!("找不到模型: {id}")),
                }
            }
            Ok(None) => json_error(StatusCode::SERVICE_UNAVAILABLE, "没有激活的第三方供应商"),
            Err(error) => {
                log::error!("读取模型失败: {error}");
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录")
            }
        }
    }
}

fn desktop_bound_smart_gateway(state: &ProxyState) -> bool {
    if state.target != ProviderTarget::ClaudeDesktop {
        return false;
    }
    state
        .db
        .with_conn(|conn| {
            Ok(get_current_provider(conn, ProviderTarget::ClaudeDesktop)?
                .is_some_and(|provider| provider.is_smart_gateway()))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod route_plan_fallback_tests {
    use super::*;
    use crate::provider::{
        ClaudeModelMapping, ProtocolType, Provider, ProviderKind, ProviderTarget,
    };

    fn sample_provider(kind: ProviderKind) -> Provider {
        Provider {
            id: "p1".into(),
            name: "t".into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "gemini-3.8-flash-high".into(),
            model_context_window: Some(200_000),
            auto_review_model_override: None,
            web_search_enabled: Some(true),
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: kind,
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

    #[test]
    fn plan_fallback_allows_antigravity_429_while_generic_failover_does_not() {
        let provider = sample_provider(ProviderKind::Antigravity);
        assert!(should_try_route_plan_fallback(
            &provider,
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(should_try_route_plan_fallback(
            &provider,
            StatusCode::GATEWAY_TIMEOUT
        ));
        assert!(!should_failover_upstream_status(
            &provider,
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!should_failover_upstream_status(
            &provider,
            StatusCode::GATEWAY_TIMEOUT
        ));
    }
}

