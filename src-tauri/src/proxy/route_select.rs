use crate::catalog::CatalogRequestError;

#[derive(Debug)]
pub(crate) enum GatewaySelectionError {
    Catalog(CatalogRequestError),
    App(crate::error::AppError),
}

impl From<crate::error::AppError> for GatewaySelectionError {
    fn from(error: crate::error::AppError) -> Self { Self::App(error) }
}

impl std::fmt::Display for GatewaySelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Catalog(error) => error.fmt(f), Self::App(error) => error.fmt(f) }
    }
}

pub(crate) fn select_gateway_runtime_provider_with(
    state: &ProxyState,
    requested_model: &str,
    _force_catalog_subagent: bool,
    _incoming: &Value,
    _request_path: &str,
    _headers: &HeaderMap,
) -> Result<Option<(Provider, String, bool, crate::gateway::RouteDecision, crate::gateway::RouteExecutionPlan)>, GatewaySelectionError> {
    let style = crate::catalog::catalog_style_for(state.target);
    let (providers, entries) = load_gateway_catalog(state, style)?;
    let profile = state.db.with_read_conn(|conn| crate::database::dao::gateway::current_profile(conn, state.target)).ok().flatten();
    let original_model = requested_model.trim().to_string();
    let requested_model = if state.target == ProviderTarget::ClaudeDesktop {
        // 只调用用户选的模型：Desktop 自带的标题 / 侧查询 / 裸角色名不另选模型。
        let last_selected = desktop_alias::last_selected(&state.db);
        desktop_alias::resolve_selected(&original_model, &entries, last_selected.as_deref())
            .map_err(|unresolved| {
                GatewaySelectionError::Catalog(crate::catalog::CatalogRequestError::StalePublicId(unresolved))
            })?
    } else {
        let session = conversation_model::session_key(_headers);
        let mut remembered =
            conversation_model::recall_for_target(state.target, session.as_deref());
        if state.target == ProviderTarget::VscodeCopilot && remembered.is_none() {
            remembered = crate::config::vscode_copilot::preferred_selected_model_id(&entries);
        }
        let pins = if state.target == ProviderTarget::ClaudeCode {
            configured_model_pins(state, profile.as_ref())
        } else {
            Vec::new()
        };
        let stripped_original = crate::catalog::strip_client_model_prefix(&original_model);
        let role_followed = if state.target == ProviderTarget::VscodeCopilot {
            let is_aux_turn = _force_catalog_subagent
                || is_copilot_subtask_or_subagent_payload(_incoming, _headers, stripped_original);
            crate::catalog::follow_copilot_conversation_model(
                stripped_original,
                &entries,
                remembered.as_deref(),
                is_aux_turn,
            )
        } else {
            crate::catalog::follow_conversation_model(
                stripped_original,
                &entries,
                remembered.as_deref(),
            )
        };
        let mut followed = crate::catalog::follow_model_pin(
            &role_followed,
            remembered.as_deref(),
            &pins,
        )
        .unwrap_or(role_followed);
        if state.target == ProviderTarget::VscodeCopilot
            && !crate::gateway::is_auto_model_id(&followed)
            && !crate::catalog::catalog_in_entries(&entries, &followed)
        {
            if let Some( fallback ) = remembered
                .as_deref()
                .filter(|m| crate::catalog::catalog_in_entries(&entries, m))
                .map(str::to_string)
                .or_else(|| crate::config::vscode_copilot::preferred_selected_model_id(&entries))
                .or_else(|| entries.first().map(|e| e.public_id.clone()))
            {
                log::info!(
                    "VS Code Copilot 未启用或未知模型 {} 自动回退到 {}",
                    original_model,
                    fallback
                );
                followed = fallback;
            }
        }
        let pin = crate::catalog::is_configured_model_pin(stripped_original, &pins);
        let avoid_remembering_aux = state.target == ProviderTarget::VscodeCopilot
            && crate::catalog::is_copilot_auxiliary_model(stripped_original)
            && remembered
                .as_deref()
                .is_some_and(|main| !crate::catalog::is_copilot_auxiliary_model(main));
        if followed.eq_ignore_ascii_case(stripped_original)
            && !pin
            && !avoid_remembering_aux
            && crate::catalog::is_explicit_catalog_passthrough(&entries, stripped_original)
        {
            conversation_model::remember_for_target(
                state.target,
                session.as_deref(),
                stripped_original,
            );
        } else if followed != original_model {
            log::info!(
                "Model {original_model} follows conversation model {followed}"
            );
        }
        followed
    };
    let explicit = if crate::gateway::is_auto_model_id(&requested_model) {
        entries.first().map(|entry| entry.public_id.clone()).unwrap_or_default()
    } else {
        requested_model.clone()
    };
    if explicit.is_empty() {
        return Ok(None);
    }
    let resolved = crate::catalog::resolve_request_strict(&entries, &providers, &explicit)
        .map_err(GatewaySelectionError::Catalog)?;
    let Some((provider_id, upstream)) = resolved else {
        return Ok(None);
    };
    let Some(provider) = providers.into_iter().find(|item| item.id == provider_id) else {
        return Ok(None);
    };
    let source = if crate::gateway::is_auto_model_id(&requested_model) {
        crate::gateway::RouteSource::Auto
    } else {
        crate::gateway::RouteSource::Explicit
    };
    let decision = crate::gateway::RouteDecision {
        requested_model: requested_model.to_string(),
        normalized_model: upstream.clone(),
        source,
        reason: source.as_reason().to_string(),
        profile_id: profile.as_ref().map(|item| item.id.clone()),
        upstream_id: Some(provider.id.clone()),
        diagnostics: Vec::new(),
        thinking: None,
        rewrites: Vec::new(),
        mode_id: None,
    };
    let plan = crate::gateway::build_execution_plan(
        profile.as_ref(),
        &upstream,
        Some(&provider.id),
        source,
    );
    let Some(mut provider) = hydrate_provider_credential(state, provider)? else {
        return Ok(None);
    };
    provider.model = upstream.clone();
    log::info!(
        "Catalog route client={} normalized={} provider={} upstream={}",
        decision.requested_model, decision.normalized_model, provider.name, upstream
    );
    Ok(Some((provider, upstream, false, decision, plan)))
}

fn configured_model_pins(
    _state: &ProxyState,
    _profile: Option<&crate::database::dao::gateway::GatewayProfile>,
) -> Vec<String> {
    // Catalog mode no longer writes `ANTHROPIC_MODEL` into `~/.claude/settings.json`,
    // so any concrete catalog model sent by Claude Code is the user's `/model` choice.
    // Treating `sgw_claude_code.model` (the first catalog entry) as a pin caused
    // selecting the first catalog model (e.g. `claude.gemini-3.8-flash`) to be
    // overridden by the previously used model.
    Vec::new()
}

fn is_copilot_subtask_or_subagent_payload(
    incoming: &Value,
    headers: &HeaderMap,
    requested_model: &str,
) -> bool {
    if headers.contains_key(CS_SUBAGENT_HEADER) {
        return true;
    }
    if crate::catalog::is_copilot_auxiliary_model(requested_model) {
        // VS Code Copilot 联网搜索、标题生成、意图识别与辅助子任务默认使用 -luna / -mini 模型。
        return true;
    }
    let tools_empty = incoming
        .get("tools")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty);
    if tools_empty {
        return true;
    }
    false
}

pub(crate) fn remember_gateway_success_upstream(
    state: &ProxyState,
    _headers: &HeaderMap,
    incoming: &Value,
    _provider_id: &str,
    _is_subagent: bool,
) {
    if state.listener_kind != ListenerKind::SmartGateway || state.target != ProviderTarget::ClaudeDesktop {
        return;
    }
    // 只记录完成过的、网关自己列出的别名；失败的切换试探不会改变 Desktop 的默认模型。
    if let Some(model) = incoming.get("model").and_then(Value::as_str) {
        desktop_alias::remember_selected(&state.db, model);
    }
}

fn note_gateway_inflight(
    state: &ProxyState,
    provider: &Provider,
    model: &str,
    is_stream: bool,
    decision: Option<&crate::gateway::RouteDecision>,
    attempts: &[crate::database::dao::proxy_logs::ProxyRequestAttempt],
) {
    let Some(slot) = state.request_log.as_ref() else { return; };
    let mut pending = attempts.to_vec();
    pending.push(crate::database::dao::proxy_logs::ProxyRequestAttempt::new(
        pending.len(),
        Some(provider.id.clone()),
        Some(provider.name.clone()),
        model.to_string(),
        None,
        0,
        Some("cancelled".into()),
        Some("客户端在响应提交前断开".into()),
        false,
    ));
    slot.note_inflight(response_lifecycle::InflightLog {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        model: model.to_string(),
        protocol: provider.protocol_type.as_str().to_string(),
        is_stream,
        profile_id: decision.and_then(|item| item.profile_id.clone()),
        route_reason: decision.map(|item| item.reason.clone()),
        requested_model: decision.map(|item| item.requested_model.clone()),
        upstream_id: Some(provider.id.clone()),
        route_mode: decision.and_then(|item| item.mode_id.clone()),
        attempts_json: serde_json::to_string(&pending).unwrap_or_else(|_| "[]".into()),
    });
}

/// 显式备用仅解析目录，不重新检测模式，避免备用再次落回主模型。
pub(crate) fn resolve_explicit_fallback(
    state: &ProxyState,
    model: &str,
) -> AppResult<Option<(Provider, String)>> {
    let style = crate::catalog::catalog_style_for(state.target);
    let (providers, entries) = load_gateway_catalog(state, style)?;
    let resolved = crate::catalog::resolve_request_strict(&entries, &providers, model)
        .map_err(|error| AppError::Config(error.to_string()))?;
    let Some((id, upstream)) = resolved else { return Ok(None); };
    let Some(provider) = providers.into_iter().find(|provider| provider.id == id) else {
        return Ok(None);
    };
    Ok(hydrate_provider_credential(state, provider)?.map(|mut provider| {
        provider.model = upstream.clone();
        (provider, upstream)
    }))
}

pub(crate) fn should_try_explicit_fallback(provider: &Provider, status: StatusCode) -> bool {
    if provider.is_kiro() && matches!(status, StatusCode::TOO_MANY_REQUESTS | StatusCode::GATEWAY_TIMEOUT) {
        return false;
    }
    matches!(status, StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS)
        || status.is_server_error()
}

fn should_forward_inbound_header(name: &str) -> bool {
    !(is_hop_by_hop_header(name)
        || name.eq_ignore_ascii_case("host")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("content-type")
        // 网关按明文解析上游响应；压缩编码由客户端协商会导致 body/头不一致（见下方 identity）。
        || name.eq_ignore_ascii_case("accept-encoding")
        || name.eq_ignore_ascii_case("authorization")
        || name.eq_ignore_ascii_case("x-api-key")
        || name.eq_ignore_ascii_case(crate::gateway::correlation::CLIENT_MODEL_HEADER)
        || name.eq_ignore_ascii_case(crate::gateway::PARENT_SESSION_HEADER))
}

fn prepare_upstream_request(
    state: &ProxyState,
    provider: &mut Provider,
    method: &Method,
    headers: &HeaderMap,
    incoming: &Value,
    body_bytes: &Bytes,
    incoming_stream: bool,
    client_model: &str,
) -> AppResult<PreparedUpstreamRequest> {
    let requested_model = incoming.get("model").and_then(Value::as_str).unwrap_or("").trim();
    if provider.is_smart_gateway() {
        let concrete = !crate::gateway::is_auto_model_id(requested_model);
        if concrete {
            provider.model = requested_model.to_string();
        }
    } else {
        provider.model = resolve_upstream_model(provider, requested_model);
    }
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
    if state.target == ProviderTarget::ClaudeDesktop
        && provider.is_smart_gateway()
        && provider.model.starts_with("anthropic/aisw-")
    {
        if let Ok((_, entries)) = load_gateway_catalog(state, CatalogStyle::Claude) {
            if let Some(public_id) = desktop_alias::public_id_for_listed(&provider.model, &entries) {
                let readable = entries
                    .iter()
                    .find(|entry| entry.public_id.eq_ignore_ascii_case(&public_id))
                    .map(|entry| entry.upstream_slug.clone())
                    .filter(|slug| !slug.trim().is_empty())
                    .unwrap_or(public_id);
                provider.model = readable;
            }
        }
    }
    // reqwest 未启用自动解压，网关要解析/改写上游 body，必须要求未压缩响应。
    let mut builder = state
        .client
        .request(method.clone(), target_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT_ENCODING, "identity");
    if !provider.is_codex_oauth() {
        for (name, value) in headers.iter() {
            if !should_forward_inbound_header(name.as_str()) {
                continue;
            }
            builder = builder.header(name, value);
        }
    }
    if provider.is_antigravity() || provider.is_kiro() {
        let trimmed = client_model.trim();
        if !trimmed.is_empty() {
            if let Ok(value) = axum::http::HeaderValue::from_str(trimmed) {
                builder = builder.header(crate::gateway::correlation::CLIENT_MODEL_HEADER, value);
            }
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
                && !k.eq_ignore_ascii_case("accept-encoding")
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
        .with_read_conn(load_retryable_status_codes)
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
    if (provider.is_antigravity() || provider.is_kiro()) && status == StatusCode::GATEWAY_TIMEOUT {
        return false;
    }
    if catalog_cross_provider_429 {
        return true;
    }
    !((provider.is_antigravity() || provider.is_kiro()) && status == StatusCode::TOO_MANY_REQUESTS)
}

pub fn default_retryable_status_codes() -> Vec<u16> {
    let mut codes = Vec::new();
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
            let credential_ready = if gateway_catalog_enabled(&state) || provider.is_smart_gateway() {
                state
                    .db
                    .with_conn(|conn| {
                        if let Some(binding) =
                            crate::database::dao::gateway::binding_for_target(conn, state.target)?
                        {
                            if !binding.entry_token.trim().is_empty() {
                                return Ok(true);
                            }
                        }
                        Ok(false)
                    })
                    .unwrap_or(false)
                    || !crate::gateway::service::ensure_api_key(&state.db).trim().is_empty()
            } else if provider.is_antigravity() || provider.is_kiro() {
                true
            } else {
                matches!(
                    state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)),
                    Ok(Some(key)) if !key.trim().is_empty()
                )
            };
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
                let payload = if target == ProviderTarget::ClaudeDesktop {
                    desktop_alias::discovery_payload(&entries)
                } else {
                    match style {
                        CatalogStyle::Claude => claude_discovery_payload(&entries),
                        CatalogStyle::Codex => openai_models_payload(&entries),
                    }
                };
                let revision = crate::catalog::catalog_revision(style, &entries);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ETAG, format!("\"{revision}\""))
                    .header("x-ai-switcher-catalog-revision", revision)
                    .body(Body::from(payload.to_string()))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
                let lookup = if target == ProviderTarget::ClaudeDesktop {
                    desktop_alias::public_id_for_listed(&id, &entries).unwrap_or(id)
                } else {
                    id
                };
                if let Some(entry) = find_catalog_entry(&entries, &lookup) {
                    let payload = if target == ProviderTarget::ClaudeDesktop {
                        desktop_alias::discovery_model(entry, &entries)
                    } else {
                        match style {
                            CatalogStyle::Claude => claude_discovery_model(entry),
                            CatalogStyle::Codex => crate::catalog::openai_discovery_model(entry),
                        }
                    };
                    let revision = crate::catalog::catalog_revision(style, &entries);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::ETAG, format!("\"{revision}\""))
                        .header("x-ai-switcher-catalog-revision", revision)
                        .body(Body::from(payload.to_string()))
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                } else {
                    json_error(StatusCode::NOT_FOUND, format!("找不到模型: {lookup}"))
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
mod route_select_tests {
    use super::*;

    #[test]
    fn claude_code_switching_back_to_first_catalog_model_routes_to_gemini() {
        conversation_model::clear_all();
        let mut state = tests::circuit_test_state();
        state.listener_kind = ListenerKind::SmartGateway;
        state.target = ProviderTarget::ClaudeCode;
        let credential = crate::secrets::test_credentials::Credential::new("test-key");
        state
            .db
            .with_conn(|conn| {
                crate::database::dao::gateway::ensure_profile_for_target(
                    conn,
                    ProviderTarget::ClaudeCode,
                )?;
                conn.execute(
                    "INSERT INTO providers (id, name, base_url, api_key, model, provider_kind, is_current, target_app)
                     VALUES ('sgw_claude_code', '智能网关', 'http://127.0.0.1:15828', '', 'claude.gemini-3.8-flash', 'smart_gateway', 1, 'claude_code')",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO upstreams (id, name, base_url, api_key, model, protocol_type, provider_kind, sort_index, enabled)
                     VALUES ('up_ag', 'Antigravity (Built-in)', 'http://127.0.0.1:15830', '', 'gemini-3.8-flash', 'anthropic', 'antigravity', 0, 1)",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO upstreams (id, name, base_url, api_key, model, protocol_type, provider_kind, sort_index, enabled)
                     VALUES ('up_sub2', 'sub2api', 'https://8tou.com/v1', ?1, 'gpt-5.6-sol', 'openai_responses', 'standard', 1, 1)",
                    [credential.reference()],
                )?;
                Ok(())
            })
            .unwrap();
        crate::catalog::invalidate_view_cache();

        let headers = HeaderMap::new();
        let body = serde_json::json!({"messages": [{"role": "user", "content": "hi"}]});

        // 1. User first uses gpt-5.6-sol in Claude Code (or VS Code Copilot).
        let (p1, m1, _, d1, _) = select_gateway_runtime_provider_with(
            &state,
            "claude.gpt-5.6-sol",
            false,
            &body,
            "/v1/messages",
            &headers,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p1.id, "up_sub2");
        assert_eq!(m1, "gpt-5.6-sol");
        assert_eq!(d1.normalized_model, "gpt-5.6-sol");

        // 2. User selects claude.gemini-3.8-flash (which is also sgw_claude_code.model).
        let (p2, m2, _, d2, _) = select_gateway_runtime_provider_with(
            &state,
            "claude.gemini-3.8-flash",
            false,
            &body,
            "/v1/messages",
            &headers,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p2.id, "up_ag");
        assert_eq!(m2, "gemini-3.8-flash");
        assert_eq!(d2.requested_model, "claude.gemini-3.8-flash");
        assert_eq!(d2.normalized_model, "gemini-3.8-flash");

        // 3. Subsequent role request (e.g. claude-haiku-4-5) follows claude.gemini-3.8-flash.
        let (p3, m3, _, _, _) = select_gateway_runtime_provider_with(
            &state,
            "claude-haiku-4-5",
            false,
            &body,
            "/v1/messages",
            &headers,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p3.id, "up_ag");
        assert_eq!(m3, "gemini-3.8-flash");
        conversation_model::clear_all();
    }
}

