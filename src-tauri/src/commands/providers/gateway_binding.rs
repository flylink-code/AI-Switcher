
/// Upgrade a legacy Desktop profile ID or model route list in the background
/// without a network preflight. The provider row and credential remain unchanged.
pub async fn repair_current_desktop_profile(state: &AppState) -> AppResult<()> {
    let applied_id = claude_desktop::current_applied_id()?;
    let legacy_profile = applied_id.as_deref() == Some(claude_desktop::LEGACY_PROFILE_ID);
    let managed_profile = applied_id.as_deref() == Some(claude_desktop::PROFILE_ID);
    if !legacy_profile && !managed_profile {
        return Ok(());
    }
    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::ClaudeDesktop))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let legacy_routes = managed_profile
        && provider.requires_local_proxy()
        && claude_desktop::current_profile_uses_legacy_role_routes()?;
    if !legacy_profile && !legacy_routes {
        return Ok(());
    }
    let _snapshot = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
    log::info!("Claude Desktop managed profile upgraded");
    Ok(())
}

/// Reapply the active Codex provider when the managed `ai_switcher` entry is
/// out of sync with the current routing mode.
pub async fn repair_codex_managed_proxy_endpoint(state: &AppState) -> AppResult<()> {
    if gateway_catalog_on(state, ProviderTarget::Codex) {
        let current = state
            .db
            .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::Codex))?;
        if current.is_none() {
            return Ok(());
        }
        let port = saved_smart_gateway_port(state);
        if let Some(current_base) = codex::managed_provider_base_url() {
            if is_local_proxy_base_url_for_port(&current_base, port) {
                return Ok(());
            }
        }
        sync_gateway_catalog_target(ProviderTarget::Codex, None::<&tauri::AppHandle>, state).await?;
        log::info!("Codex 智能网关：已把 managed provider 指向独立网关 {port}");
        return Ok(());
    }

    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::Codex))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let port = get_saved_proxy_port(state, ProviderTarget::Codex);
    let Some(current_base) = codex::managed_provider_base_url() else {
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!("Codex managed provider entry missing; reapplied current provider");
        return Ok(());
    };
    let needs_proxy = provider.requires_local_proxy() || provider.is_codex_oauth();
    if needs_proxy {
        if is_local_proxy_base_url_for_port(&current_base, port) {
            return Ok(());
        }
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!(
            "Codex managed base_url was `{current_base}`; reapplied local proxy on port {port}"
        );
        return Ok(());
    }
    if is_loopback_v1_base_url(&current_base) {
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!(
            "Codex managed base_url was local proxy `{current_base}`; reapplied direct upstream"
        );
    }
    Ok(())
}

fn is_local_proxy_base_url_for_port(base_url: &str, expected_port: u16) -> bool {
    let trimmed = base_url.trim().trim_end_matches('/');
    let expected = format!("http://127.0.0.1:{expected_port}/v1");
    let expected_localhost = format!("http://localhost:{expected_port}/v1");
    trimmed.eq_ignore_ascii_case(&expected) || trimmed.eq_ignore_ascii_case(&expected_localhost)
}

fn is_loopback_v1_base_url(base_url: &str) -> bool {
    let trimmed = base_url.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    (lower.starts_with("http://127.0.0.1:") || lower.starts_with("http://localhost:"))
        && lower.ends_with("/v1")
}

/// Reapply an active Claude Code provider only when the live model fields use
/// the pre-display-name or pre-role-alias format.
pub async fn repair_current_code_model_fields(state: &AppState) -> AppResult<()> {
    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::ClaudeCode))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let catalog = live_uses_gateway_catalog(state, &provider);
    let uses_proxy = target_starts_agent_proxy(
        ProviderTarget::ClaudeCode,
        catalog,
        &provider,
    );
    let port = if catalog {
        saved_smart_gateway_port(state)
    } else {
        get_saved_proxy_port(state, ProviderTarget::ClaudeCode)
    };
    let expected = expected_code_fields(&provider, uses_proxy || catalog, catalog, port);
    let current = code_managed_fields()?;
    let model_keys = [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "ANTHROPIC_REASONING_MODEL",
    ];
    if model_keys
        .iter()
        .all(|key| current.get(*key) == expected.get(*key))
    {
        return Ok(());
    }
    let _snapshot = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
    log::info!("Claude Code live model fields upgraded");
    Ok(())
}

async fn test_provider_impl(provider: &Provider, state: &AppState) -> AppResult<ConnectionTestResult> {
    let key = state.db.with_conn(|conn| dao::resolve_api_key(conn, &provider.id));
    let key = match key {
        Ok(Some(key)) => key,
        Ok(None) => String::new(),
        Err(_) => {
            let result = ConnectionTestResult {
                ok: false,
                category: "credential".to_string(),
                message: "无法读取系统凭据库中的 API Key".to_string(),
                checked_at: Utc::now().timestamp_millis(),
                latency_ms: None,
            };
            persist_provider_health(provider, &result, state.db.as_ref())?;
            return Ok(result);
        }
    };
    test_provider_with_key(provider, key, state.db.as_ref(), true).await
}

async fn test_provider_with_key(
    provider: &Provider,
    key: String,
    db: &crate::database::Database,
    persist_health: bool,
) -> AppResult<ConnectionTestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let result = if !key.trim().is_empty() && !provider.model.trim().is_empty() {
            let (endpoint, payload) = protocol_test_request(provider);
            let endpoint_url = api_endpoint_url(&provider.base_url, endpoint)?;
            let client = discovery_http_client(&endpoint_url)?;
            let mut request = client
                .post(endpoint_url)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header("x-api-key", key);
            if matches!(provider.protocol_type, ProtocolType::Anthropic) {
                request = request.header("anthropic-version", "2023-06-01");
            }
            if let Some(ref custom_headers) = provider.custom_headers {
                for (k, v) in custom_headers {
                    if !crate::proxy::is_hop_by_hop_header(k)
                        && !k.eq_ignore_ascii_case("host")
                        && !k.eq_ignore_ascii_case("content-length")
                    {
                        request = request.header(k.as_str(), v.as_str());
                    }
                }
            }
            let started = std::time::Instant::now();
            let response = request.body(serde_json::to_vec(&payload)?).send().await;
            let latency_ms = Some(started.elapsed().as_millis() as u64);
            classify_test_response(response, checked_at, provider.protocol_type, latency_ms).await
    } else if key.trim().is_empty() {
        ConnectionTestResult { ok: false, category: "authentication".to_string(), message: "供应商未配置 API Key".to_string(), checked_at, latency_ms: None }
    } else {
        ConnectionTestResult { ok: false, category: "model".to_string(), message: "请先填写模型名称".to_string(), checked_at, latency_ms: None }
    };
    if persist_health {
        persist_provider_health(provider, &result, db)?;
    }
    Ok(result)
}

fn persist_provider_health(
    provider: &Provider,
    result: &ConnectionTestResult,
    db: &crate::database::Database,
) -> AppResult<()> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO provider_health (provider_id, status, detail, checked_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET status = excluded.status, detail = excluded.detail, checked_at = excluded.checked_at",
            rusqlite::params![
                provider.id,
                if result.ok { "healthy" } else { "error" },
                match result.latency_ms {
                    Some(ms) => format!("{}|latency_ms={ms}", result.message),
                    None => result.message.clone(),
                },
                result.checked_at
            ],
        )?;
        Ok(())
    })
}

fn temporary_provider(input: &ProviderInput, state: &AppState) -> AppResult<Provider> {
    validate_target_protocol(input.target_app, input.protocol_type)?;
    let key = if !input.api_key.trim().is_empty() {
        input.api_key.clone()
    } else if let Some(id) = input.id.as_deref() {
        state.db.with_conn(|conn| dao::resolve_api_key(conn, id))?.unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Provider {
        id: input.id.clone().unwrap_or_else(|| "temporary-form-provider".to_string()),
        name: input.name.clone(), base_url: normalize_base_url(&input.base_url)?, api_key: key,
        api_key_set: !input.api_key.trim().is_empty(), model: input.model.clone(),
        model_context_window: input.model_context_window,
        auto_review_model_override: normalized_auto_review_model_override(
            input.target_app,
            input.auto_review_model_override.clone(),
        ),
        web_search_enabled: input.web_search_enabled,
        model_mapping: normalized_model_mapping(input.target_app, input.model_mapping.clone()),
        protocol_type: input.protocol_type, notes: input.notes.clone(), target_app: input.target_app,
        provider_kind: input.provider_kind, auth_binding: input.auth_binding.clone(),
        sort_index: 0, failover_group: input.failover_group,
        failover_models: input.failover_models.clone(),
        hidden_models: input.hidden_models.clone(),
        thinking_config: input.thinking_config.clone(),
        custom_headers: input.custom_headers.clone(),
        is_current: false, created_at: 0,
        health_status: None, health_checked_at: None, health_latency_ms: None,
    })
}

async fn classify_test_response(
    response: Result<reqwest::Response, reqwest::Error>,
    checked_at: i64,
    protocol: ProtocolType,
    latency_ms: Option<u64>,
) -> ConnectionTestResult {
    match response {
        Ok(response) if response.status().is_success() => ConnectionTestResult {
            ok: true,
            category: "ok".to_string(),
            message: match latency_ms {
                Some(ms) => format!("连接验证成功（{ms} ms）"),
                None => "连接验证成功".to_string(),
            },
            checked_at,
            latency_ms,
        },
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            let detail = extract_upstream_error_detail(&body);
            let (category, message) = match status {
                401 | 403 => ("authentication", "API Key 被拒绝".to_string()),
                404 | 405 => ("protocol", protocol_endpoint_message(protocol).to_string()),
                400 | 422 => ("model", "模型不可用或不兼容".to_string()),
                _ => (
                    "upstream",
                    if detail.is_empty() {
                        format!("上游服务返回错误（HTTP {status}）")
                    } else {
                        format!("上游服务返回错误（HTTP {status}）：{detail}")
                    },
                ),
            };
            ConnectionTestResult {
                ok: false,
                category: category.to_string(),
                message,
                checked_at,
                latency_ms,
            }
        }
        Err(error) if error.is_timeout() => ConnectionTestResult {
            ok: false,
            category: "network".to_string(),
            message: "连接测试超时".to_string(),
            checked_at,
            latency_ms,
        },
        Err(error) => ConnectionTestResult {
            ok: false,
            category: "network".to_string(),
            message: format!(
                "无法连接供应商服务（{}）",
                sanitize_network_error(&error)
            ),
            checked_at,
            latency_ms,
        },
    }
}

fn extract_upstream_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
        {
            let message = message.trim();
            if !message.is_empty() {
                return truncate_chars(message, 180);
            }
        }
    }
    truncate_chars(trimmed, 120)
}

fn truncate_chars(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else {
        let truncated: String = value.chars().take(max).collect();
        format!("{truncated}…")
    }
}

fn protocol_test_request(provider: &Provider) -> (&'static str, Value) {
    match provider.protocol_type {
        ProtocolType::Anthropic => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiChat | ProtocolType::Proxy => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiResponses => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_output_tokens": 1, "stream": false,
            "input": "ping"
        })),
    }
}

fn protocol_endpoint_message(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "供应商不支持 Anthropic /v1/messages 端点",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => {
            "供应商不支持 OpenAI /v1/chat/completions 端点"
        }
        ProtocolType::OpenAiResponses => "供应商不支持 OpenAI /v1/responses 端点",
    }
}

fn import_live_provider(live: LiveProviderInfo, target: ProviderTarget, state: &AppState) -> AppResult<()> {
    let normalized_base_url = normalize_base_url(&live.base_url)?;
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    if let Some(provider) = existing.iter().find(|p| p.base_url == normalized_base_url) {
        state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        return Ok(());
    }
    let input = ProviderInput {
        id: None,
        name: "当前配置（已导入）".to_string(),
        base_url: normalized_base_url,
        api_key: live.auth_token,
        clear_api_key: false,
        model: live.model,
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: live.model_mapping,
        protocol_type: live.protocol_type,
        provider_kind: ProviderKind::Standard,
        auth_binding: String::new(),
        target_app: target,
        notes: "从当前 Claude Code 配置导入".to_string(),
        failover_group: 0,
        failover_models: Vec::new(),
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    };
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))
}

pub(crate) fn ensure_smart_gateway_provider_row(
    state: &AppState,
    target: ProviderTarget,
) -> AppResult<Provider> {
    let port = crate::gateway::SMART_GATEWAY_PORT;
    let saved = state
        .db
        .with_conn(|conn| crate::database::dao::settings::get_setting(conn, crate::gateway::service::PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(port);
    let (protocol_type, base_url) = crate::gateway::smart_gateway_live_endpoint(target, saved);
    let profile = state.db.with_conn(|conn| {
        crate::database::dao::gateway::ensure_profile_for_target(conn, target)
    })?;
    let hide = catalog::hide_official(state.db.as_ref(), target);
    let pairs = load_gateway_pairs(state, target).unwrap_or_default();
    let catalog_ids: Vec<String> = build_catalog_with(catalog::catalog_style_for(target), &pairs, hide)
        .into_iter()
        .map(|entry| entry.public_id)
        .filter(|id| !catalog::is_auto_public_id(id))
        .collect();
    let existing = state.db.with_conn(|conn| {
        Ok(dao::list_providers(conn, target)?
            .into_iter()
            .find(|provider| provider.is_smart_gateway()))
    })?;
    let id = existing
        .as_ref()
        .map(|provider| provider.id.clone())
        .unwrap_or_else(|| crate::gateway::smart_gateway_provider_id(target));
    let token = state
        .db
        .with_conn(|conn| {
            Ok(crate::database::dao::gateway::binding_for_target(conn, target)?
                .map(|binding| binding.entry_token)
                .filter(|token| !token.trim().is_empty())
                .unwrap_or_else(|| profile.entry_token.clone()))
        })
        .unwrap_or_else(|_| profile.entry_token.clone());
    let model = existing
        .as_ref()
        .map(|provider| crate::gateway::normalize_live_model(&provider.model))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "auto".to_string());
    let input = ProviderInput {
        id: Some(id),
        name: existing
            .as_ref()
            .map(|provider| provider.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "智能网关".to_string()),
        base_url,
        api_key: token,
        clear_api_key: false,
        model,
        model_context_window: existing.as_ref().and_then(|provider| provider.model_context_window),
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: existing
            .as_ref()
            .map(|provider| provider.model_mapping.clone())
            .unwrap_or_default(),
        protocol_type,
        provider_kind: ProviderKind::SmartGateway,
        auth_binding: String::new(),
        target_app: target,
        notes: existing
            .as_ref()
            .map(|provider| provider.notes.clone())
            .unwrap_or_else(|| "托管 Auto 入口，请求经本机智能网关路由".to_string()),
        failover_group: 0,
        failover_models: catalog_ids,
        hidden_models: existing
            .as_ref()
            .map(|provider| provider.hidden_models.clone())
            .unwrap_or_default(),
        thinking_config: existing.as_ref().and_then(|provider| provider.thinking_config.clone()),
        custom_headers: None,
    };
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}
