pub(crate) fn log_request_with_diagnostic(
    state: &ProxyState,
    provider: &Provider,
    status: Option<i64>,
    duration_ms: i64,
    route: &str,
    is_stream: bool,
    error_category: Option<&str>,
    custom_diagnostic: Option<&str>,
) -> Option<String> {
    let model = if provider.model.trim().is_empty() {
        None
    } else {
        Some(provider.model.trim())
    };
    let diag = custom_diagnostic
        .map(str::to_string)
        .or_else(|| error_category.map(error_diagnostic).map(str::to_string));
    match state.db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            Some(&provider.id),
            Some(&provider.name),
            model,
            status,
            duration_ms,
            Some(state.target.as_str()),
            Some(provider.protocol_type.as_str()),
            Some(route),
            is_stream,
            error_category,
            diag.as_deref(),
        )
    }) {
        Ok(id) => {
            let hop = state
                .correlation
                .as_ref()
                .map(|item| item.hop)
                .unwrap_or(match state.listener_kind {
                    ListenerKind::SmartGateway => crate::gateway::correlation::HOP_SMART_GATEWAY,
                    ListenerKind::Agent => crate::gateway::correlation::HOP_AGENT_PROXY,
                });
            let correlation_id = state.correlation.as_ref().map(|item| item.id.as_str());
            let _ = state.db.with_conn(|conn| {
                update_proxy_log_hop(conn, &id, correlation_id, Some(hop))
            });
            crate::usage_events::notify_log_recorded();
            Some(id)
        }
        Err(e) => {
            log::error!("写入代理请求日志失败: {e}");
            None
        }
    }
}

pub(crate) fn patch_route_log(
    state: &ProxyState,
    id: &str,
    decision: &crate::gateway::RouteDecision,
    attempt_index: i64,
) {
    if let Err(error) = state.db.with_conn(|conn| {
        update_proxy_log_route(
            conn,
            id,
            decision.profile_id.as_deref(),
            Some(decision.reason.as_str()),
            attempt_index,
            Some(decision.requested_model.as_str()),
            decision.upstream_id.as_deref(),
        )
    }) {
        log::warn!("写入网关路由观测失败: {error}");
    }
}

pub(crate) fn log_request(
    state: &ProxyState,
    provider: &Provider,
    status: Option<i64>,
    duration_ms: i64,
    route: &str,
    is_stream: bool,
    error_category: Option<&str>,
) -> Option<String> {
    log_request_with_diagnostic(
        state,
        provider,
        status,
        duration_ms,
        route,
        is_stream,
        error_category,
        None,
    )
}

fn validate_listener_auth(state: &ProxyState, headers: &HeaderMap) -> AppResult<()> {
    if state.listener_kind == ListenerKind::SmartGateway {
        return validate_smart_gateway_binding_auth(state, headers);
    }
    validate_desktop_gateway_auth(state, headers)
}

fn validate_smart_gateway_binding_auth(state: &ProxyState, headers: &HeaderMap) -> AppResult<()> {
    let presented = presented_listener_token(headers);
    if presented.is_empty() {
        return Err(AppError::Config("网关入口凭据无效".to_string()));
    }
    let accepted = state.db.with_conn(|conn| {
        if crate::gateway::service::api_key_matches(conn, &presented) {
            return Ok(true);
        }
        Ok(crate::database::dao::gateway::binding_by_token(conn, &presented)?.is_some())
    })?;
    if accepted {
        Ok(())
    } else {
        Err(AppError::Config("网关入口凭据无效".to_string()))
    }
}

fn validate_desktop_gateway_auth(state: &ProxyState, headers: &HeaderMap) -> AppResult<()> {
    if state.target != ProviderTarget::ClaudeDesktop {
        return Ok(());
    }
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if crate::config::claude_desktop::validate_gateway_auth_header(auth).is_ok() {
        return Ok(());
    }
    let presented = presented_listener_token(headers);
    if presented.is_empty() {
        return Err(AppError::Config(
            "Claude Desktop gateway 缺少 Authorization 头".to_string(),
        ));
    }
    let accepted = state
        .db
        .with_conn(|conn| {
            let Some(provider) = get_current_provider(conn, ProviderTarget::ClaudeDesktop)? else {
                return Ok(false);
            };
            if !provider.is_smart_gateway() {
                return Ok(false);
            }
            if crate::gateway::service::api_key_matches(conn, &presented) {
                return Ok(true);
            }
            Ok(crate::database::dao::gateway::binding_by_token(conn, &presented)?.is_some())
        })
        .unwrap_or(false);
    if accepted {
        Ok(())
    } else {
        Err(AppError::Config(
            "Claude Desktop gateway token 无效".to_string(),
        ))
    }
}

fn gateway_auth_error(error: AppError) -> Response {
    json_error(StatusCode::UNAUTHORIZED, error.to_string())
}

pub(crate) fn log_early_failure(
    state: &ProxyState,
    route: &str,
    error_category: &str,
    status: Option<i64>,
    duration_ms: i64,
) {
    if let Err(error) = state.db.with_conn(|conn| {
        insert_proxy_log(
            conn,
            None,
            None,
            None,
            status,
            duration_ms,
            Some(state.target.as_str()),
            None,
            Some(route),
            false,
            Some(error_category),
            Some(error_diagnostic(error_category)),
        )
        .map(|_| ())
    }) {
        log::error!("写入代理早期失败日志失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

fn error_diagnostic(category: &str) -> &'static str {
    match category {
        "credential" => "credential unavailable",
        "configuration" => "provider configuration invalid",
        "request" => "request could not be parsed",
        "network" => "upstream connection failed",
        "upstream" => "upstream returned an error status",
        "upstream_429" => "upstream rate limited the request",
        "conversion" => "upstream response conversion failed",
        "provider" => "no active provider",
        _ => "proxy request failed",
    }
}

fn upstream_error_category(status: StatusCode) -> &'static str {
    if status == StatusCode::TOO_MANY_REQUESTS {
        "upstream_429"
    } else {
        "upstream"
    }
}

fn update_log_diagnostic(
    state: &ProxyState,
    id: Option<&str>,
    category: &str,
    diagnostic: &str,
) {
    let Some(id) = id else {
        return;
    };
    if let Err(error) = state.db.with_conn(|conn| {
        update_proxy_log_diagnostic(conn, id, category, diagnostic)
    }) {
        log::error!("更新代理错误诊断失败: {error}");
    }
}

fn sanitized_upstream_diagnostic(status: StatusCode, bytes: &[u8]) -> String {
    let limited = &bytes[..bytes.len().min(MAX_UPSTREAM_ERROR_BYTES)];
    let value = serde_json::from_slice::<Value>(limited).ok();
    let mut fields = Vec::new();
    if let Some(value) = value.as_ref() {
        let error = value.get("error").unwrap_or(value);
        for (label, candidate) in [
            ("type", error.get("type")),
            ("code", error.get("code")),
            ("message", error.get("message").or_else(|| value.get("message"))),
            (
                "request_id",
                value
                    .get("request_id")
                    .or_else(|| value.get("requestId"))
                    .or_else(|| error.get("request_id")),
            ),
        ] {
            let text = candidate.and_then(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            });
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                fields.push(format!("{label}={}", sanitize_diagnostic_text(&text)));
            }
        }
    }
    if fields.is_empty() {
        format!("上游返回 HTTP {}，未提供可安全展示的错误摘要", status.as_u16())
    } else {
        format!("上游 HTTP {}；{}", status.as_u16(), fields.join("；"))
    }
}

fn explicitly_rejects_stream_options(bytes: &[u8]) -> bool {
    let limited = &bytes[..bytes.len().min(MAX_UPSTREAM_ERROR_BYTES)];
    let text = String::from_utf8_lossy(limited).to_lowercase();
    text.contains("stream_options")
        && [
            "unknown",
            "unsupported",
            "unrecognized",
            "not allowed",
            "extra field",
            "不支持",
            "未知",
        ]
        .iter()
        .any(|marker| text.contains(marker))
}

fn sanitize_diagnostic_text(value: &str) -> String {
    crate::log_redact::redact_secrets(value)
}

fn update_log_usage(state: &ProxyState, provider: &Provider, id: &str, usage: Option<UsageCounts>) {
    let Some(usage) = usage else {
        return;
    };
    if let Err(e) = state.db.with_conn(|conn| {
        update_proxy_log_usage_idempotent(
            conn,
            id,
            Some(state.target.as_str()),
            Some(provider.id.as_str()),
            usage.envelope_id.as_deref(),
            usage.input_tokens,
            usage.cache_read_input_tokens,
            usage.cache_creation_input_tokens,
            usage.output_tokens,
        )
    }) {
        log::error!("更新代理请求 Token 用量失败: {e}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

pub(crate) fn extract_usage_from_json(bytes: &[u8]) -> Option<UsageCounts> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    usage_from_value(&value)
}

pub(crate) fn extract_usage_from_sse(bytes: &[u8]) -> Option<UsageCounts> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|value| usage_from_value(&value))
        .last()
}

fn usage_from_value(value: &Value) -> Option<UsageCounts> {
    let usage = value.get("usage").or_else(|| value.pointer("/response/usage"))?;
    let input_tokens_field = usage.get("input_tokens").and_then(Value::as_i64);
    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
    let reported_input = input_tokens_field.or(prompt_tokens)?;
    // Anthropic exposes fresh input via `input_tokens` + separate `cache_read_input_tokens`.
    // OpenAI Chat / Responses expose total input and put cache under `*_tokens_details`.
    let anthropic_style_cache = usage.get("cache_read_input_tokens").and_then(Value::as_i64);
    let details_cache = usage
        .pointer("/input_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cached_tokens"))
        .and_then(Value::as_i64);
    let cache_read = anthropic_style_cache
        .or(details_cache)
        .unwrap_or(0)
        .max(0);
    let fresh_input = if anthropic_style_cache.is_some() {
        // Anthropic: `input_tokens` is already non-cached / fresh.
        input_tokens_field.unwrap_or_else(|| reported_input.saturating_sub(cache_read.min(reported_input)))
    } else {
        // Chat Completions / Responses: reported input is total (fresh + cached).
        let cache = cache_read.min(reported_input);
        reported_input.saturating_sub(cache)
    };
    let cache_read = if anthropic_style_cache.is_some() {
        cache_read
    } else {
        cache_read.min(reported_input)
    };
    Some(UsageCounts {
        input_tokens: fresh_input,
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        output_tokens: usage.get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        envelope_id: extract_usage_envelope_id(value),
    })
}

pub(crate) fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = serde_json::json!({"error": message.into()});
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| status.into_response())
}

fn anthropic_error(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| status.into_response())
}

pub(crate) fn is_hop_by_hop_header(name: &str) -> bool {
    let name = name.as_bytes();
    matches!(
        name,
        b"connection"
            | b"keep-alive"
            | b"transfer-encoding"
            | b"te"
            | b"trailer"
            | b"proxy-authorization"
            | b"proxy-authenticate"
            | b"upgrade"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{ClaudeModelMapping, ProviderKind, ProviderTarget};

    fn provider(protocol_type: ProtocolType) -> Provider {
        Provider {
            id: "mapped".into(),
            name: "Mapped".into(),
            base_url: "https://api.example.test".into(),
            api_key: "secret".into(),
            api_key_set: true,
            model: "opus-upstream".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type,
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
            is_current: true,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    fn circuit_test_state() -> ProxyState {
        ProxyState {
            db: Arc::new(Database::memory().unwrap()),
            client: Client::new(),
            circuits: Arc::new(Mutex::new(std::collections::HashMap::new())),
            codex_history: Arc::new(super::codex_history::CodexHistoryStore::default()),
            target: ProviderTarget::ClaudeCode,
            listener_kind: ListenerKind::Agent,
            port: DEFAULT_PORT,
            started_at: Instant::now(),
            correlation: None,
            request_path: String::new(),
        }
    }

    #[test]
    fn claude_haiku_role_is_subagent_signal() {
        assert!(is_claude_code_subagent_request(
            &HeaderMap::new(),
            "claude-haiku-4-5"
        ));
        assert!(!is_claude_code_subagent_request(
            &HeaderMap::new(),
            "claude.auto"
        ));
        let mut headers = HeaderMap::new();
        headers.insert(CS_SUBAGENT_HEADER, "1".parse().unwrap());
        assert!(is_claude_code_subagent_request(&headers, "claude.auto"));
    }

    #[test]
    fn provider_circuit_opens_after_two_failures_and_resets_on_success() {
        let state = circuit_test_state();
        record_provider_failure(&state, "provider-a");
        assert!(!circuit_is_open(&state, "provider-a"));
        record_provider_failure(&state, "provider-a");
        assert!(circuit_is_open(&state, "provider-a"));
        record_provider_success(&state, "provider-a");
        assert!(!circuit_is_open(&state, "provider-a"));
    }

    #[test]
    fn antigravity_429_does_not_failover_to_other_providers() {
        let ag = Provider {
            provider_kind: ProviderKind::Antigravity,
            ..provider(ProtocolType::Anthropic)
        };
        let standard = provider(ProtocolType::Anthropic);
        assert!(!should_failover_upstream_status(
            &ag,
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!should_failover_upstream_status(
            &ag,
            StatusCode::GATEWAY_TIMEOUT
        ));
        assert!(should_failover_upstream_status(&ag, StatusCode::BAD_GATEWAY));
        assert!(should_failover_upstream_status(
            &standard,
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(should_failover_upstream_status_ex(
            &ag,
            StatusCode::TOO_MANY_REQUESTS,
            true
        ));
        assert!(!should_failover_upstream_status_ex(
            &ag,
            StatusCode::TOO_MANY_REQUESTS,
            false
        ));
        assert!(!should_failover_upstream_status_ex(
            &ag,
            StatusCode::GATEWAY_TIMEOUT,
            true
        ));
    }

    #[test]
    fn next_failover_provider_orders_by_group_and_filters_models() {
        use crate::database::dao::providers::{set_current_provider, upsert_provider};
        use crate::database::dao::settings::set_setting;
        use crate::provider::{ClaudeModelMapping, ProviderInput, ProviderKind};

        let state = circuit_test_state();
        let seeded = state.db.with_conn(|conn| {
            set_setting(conn, PROXY_FAILOVER_ENABLED_KEY, "true")?;
            let current = upsert_provider(
                conn,
                &ProviderInput {
                    id: None,
                    name: "Current".into(),
                    base_url: "https://current.example.test/v1".into(),
                    api_key: "sk-current".into(),
                    clear_api_key: false,
                    model: "default".into(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::OpenAiChat,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 0,
                    failover_models: Vec::new(),
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;
            set_current_provider(conn, &current.id)?;

            let _group1 = upsert_provider(
                conn,
                &ProviderInput {
                    id: None,
                    name: "Group1".into(),
                    base_url: "https://group1.example.test/v1".into(),
                    api_key: "sk-group1".into(),
                    clear_api_key: false,
                    model: "default".into(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::OpenAiChat,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 1,
                    failover_models: vec!["gpt-4o".into()],
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;
            let group0 = upsert_provider(
                conn,
                &ProviderInput {
                    id: None,
                    name: "Group0".into(),
                    base_url: "https://group0.example.test/v1".into(),
                    api_key: "sk-group0".into(),
                    clear_api_key: false,
                    model: "default".into(),
                    model_context_window: None,
                    auto_review_model_override: None,
                    web_search_enabled: None,
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: ProtocolType::OpenAiChat,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::ClaudeCode,
                    notes: String::new(),
                    failover_group: 0,
                    failover_models: Vec::new(),
                    hidden_models: Vec::new(),
                    thinking_config: None,
                    custom_headers: None,
                },
            )?;
            Ok((current.id, group0.id))
        });
        let (current_id, group0_id) = match seeded {
            Ok(ids) => ids,
            Err(error) => {
                let message = error.to_string();
                if message.contains("凭据")
                    || message.contains("keyring")
                    || message.contains("credential")
                {
                    return;
                }
                panic!("{error}");
            }
        };

        let first = next_failover_provider(&state, &[current_id.clone()], "claude-opus-5")
            .unwrap()
            .expect("group 0 candidate");
        assert_eq!(first.id, group0_id);

        let filtered = next_failover_provider(
            &state,
            &[current_id.clone(), group0_id.clone()],
            "claude-opus-5",
        )
        .unwrap();
        assert!(filtered.is_none(), "group1 whitelist should reject opus");

        let ignored = next_failover_provider_ex(
            &state,
            &[current_id.clone(), group0_id.clone()],
            "claude-opus-5",
            true,
        )
        .unwrap()
        .expect("catalog failover ignores model whitelist");
        assert_eq!(ignored.name, "Group1");

        let matched = next_failover_provider(
            &state,
            &[current_id, group0_id],
            "gpt-4o-mini",
        )
        .unwrap()
        .expect("group1 whitelist match");
        assert_eq!(matched.name, "Group1");
    }

    #[test]
    fn upstream_sse_decoder_reassembles_split_json_frames() {
        let mut decoder = UpstreamSseDecoder::default();
        assert!(decoder.push(b"data: {\"type\":\"response.output_text").is_empty());
        let events = decoder.push(b".delta\",\"delta\":\"hi\"}\n\n");
        assert_eq!(events.len(), 1);
        let UpstreamSseItem::Json(event) = &events[0] else { panic!("expected JSON SSE event"); };
        assert_eq!(event["type"], "response.output_text.delta");
        assert_eq!(event["delta"], "hi");
    }

    #[test]
    fn upstream_sse_decoder_accepts_crlf_and_done() {
        let mut decoder = UpstreamSseDecoder::default();
        let events = decoder.push(b"data: [DONE]\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], UpstreamSseItem::Done));
    }

    #[test]
    fn all_protocols_send_the_resolved_model() {
        for requested_model in [
            "claude-opus-5",
            "claude-opus-5[1m]",
            "claude-opus-4-8",
            "claude-opus-4-8[1m]",
        ] {
            let incoming = serde_json::json!({
                "model": requested_model,
                "max_tokens": 32,
                "messages": [{"role": "user", "content": "hello"}],
            });
            let original = Bytes::from(serde_json::to_vec(&incoming).unwrap());

            for protocol in [
                ProtocolType::Anthropic,
                ProtocolType::OpenAiChat,
                ProtocolType::OpenAiResponses,
            ] {
                let provider = provider(protocol);
                let (body, _) =
                    encode_upstream_request(&provider, &incoming, &original, false, &HeaderMap::new());
                let value: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(
                    value["model"], "opus-upstream",
                    "{protocol:?} / {requested_model}"
                );
            }
        }
    }

    #[test]
    fn kimi_chat_encode_reinjects_session_prompt_cache_key() {
        let mut provider = provider(ProtocolType::OpenAiChat);
        provider.base_url = "https://api.moonshot.cn/v1".into();
        let incoming = serde_json::json!({
            "model": "claude-sonnet-5",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hello"}],
        });
        let original = Bytes::from(serde_json::to_vec(&incoming).unwrap());
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", "sess-kimi".parse().unwrap());
        let (body, translated) =
            encode_upstream_request(&provider, &incoming, &original, false, &headers);
        assert!(translated);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["prompt_cache_key"], "sess-kimi");
    }

    #[test]
    fn upstream_diagnostic_keeps_safe_fields_and_redacts_tokens() {
        let body = br#"{"error":{"type":"gateway_error","code":"bad_gateway","message":"Bearer secret-token failed"},"request_id":"req_1"}"#;
        let diagnostic = sanitized_upstream_diagnostic(StatusCode::BAD_GATEWAY, body);
        assert!(diagnostic.contains("HTTP 502"));
        assert!(diagnostic.contains("gateway_error"));
        assert!(diagnostic.contains("req_1"));
        assert!(!diagnostic.contains("secret-token"));
        assert!(diagnostic.contains("[redacted]"));
    }

    #[test]
    fn upstream_429_has_a_specific_log_category() {
        assert_eq!(
            upstream_error_category(StatusCode::TOO_MANY_REQUESTS),
            "upstream_429"
        );
        assert_eq!(
            upstream_error_category(StatusCode::BAD_GATEWAY),
            "upstream"
        );
    }

    #[test]
    fn stream_options_retry_requires_an_explicit_parameter_rejection() {
        assert!(explicitly_rejects_stream_options(
            br#"{"error":{"message":"Unknown field: stream_options"}}"#
        ));
        assert!(!explicitly_rejects_stream_options(
            br#"{"error":{"message":"Temporary upstream failure"}}"#
        ));
        assert!(!explicitly_rejects_stream_options(
            br#"{"error":{"message":"Unknown model"}}"#
        ));
    }

    #[test]
    fn usage_parser_preserves_anthropic_input_and_supports_openai_usage() {
        let anthropic = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 40,
                "cache_creation_input_tokens": 5,
                "output_tokens": 20
            }
        });
        let parsed = usage_from_value(&anthropic).expect("anthropic usage");
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.cache_read_input_tokens, 40);
        assert_eq!(parsed.output_tokens, 20);

        let openai = serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 40 }
            }
        });
        let parsed = usage_from_value(&openai).expect("OpenAI-compatible usage");
        assert_eq!(parsed.input_tokens, 60);
        assert_eq!(parsed.cache_read_input_tokens, 40);
        assert_eq!(parsed.output_tokens, 20);
    }

    #[test]
    fn usage_parser_treats_responses_input_tokens_as_total_with_cache_details() {
        // Codex / OpenAI Responses: input_tokens is TOTAL; cached portion is nested.
        // Must store fresh input so proxy↔session dedup can match session sync rows.
        let responses = serde_json::json!({
            "usage": {
                "input_tokens": 140,
                "input_tokens_details": { "cached_tokens": 40 },
                "output_tokens": 20
            }
        });
        let parsed = usage_from_value(&responses).expect("Responses usage");
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.cache_read_input_tokens, 40);
        assert_eq!(parsed.output_tokens, 20);

        let nested = serde_json::json!({
            "response": {
                "usage": {
                    "input_tokens": 50,
                    "input_tokens_details": { "cached_tokens": 10 },
                    "output_tokens": 7
                }
            }
        });
        let parsed = usage_from_value(&nested).expect("nested Responses usage");
        assert_eq!(parsed.input_tokens, 40);
        assert_eq!(parsed.cache_read_input_tokens, 10);
        assert_eq!(parsed.output_tokens, 7);
    }

    #[test]
    fn kimi_anthropic_usage_and_final_stream_frame_are_preserved() {
        let kimi = br#"{"usage":{"input_tokens":321,"cache_read_input_tokens":12,"output_tokens":45}}"#;
        let parsed = extract_usage_from_json(kimi).expect("Kimi Anthropic usage");
        assert_eq!(parsed.input_tokens, 321);
        assert_eq!(parsed.cache_read_input_tokens, 12);
        assert_eq!(parsed.output_tokens, 45);

        let stream = br#"data: {"type":"content_block_delta"}

data: {"type":"message_delta","usage":{"input_tokens":321,"output_tokens":45}}

data: {"type":"message_delta","usage":{"input_tokens":321,"output_tokens":67}}
"#;
        let parsed = extract_usage_from_sse(stream).expect("final Kimi stream usage");
        assert_eq!(parsed.input_tokens, 321);
        assert_eq!(parsed.output_tokens, 67);
    }
}
