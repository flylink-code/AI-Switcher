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
            if let Some(slot) = &state.request_log { slot.record(&id); }
            let hop = state.correlation.as_ref().map(|item| item.hop).unwrap_or(
                match state.listener_kind {
                    ListenerKind::SmartGateway => crate::gateway::correlation::HOP_SMART_GATEWAY,
                    ListenerKind::Agent => crate::gateway::correlation::HOP_AGENT_PROXY,
                },
            );
            let correlation_id = state.correlation.as_ref().map(|item| item.id.as_str());
            let _ = state
                .db
                .with_conn(|conn| update_proxy_log_hop(conn, &id, correlation_id, Some(hop)));
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
    attempts_json: Option<&str>,
) {
    let final_upstream_id = attempts_json
        .and_then(|raw| serde_json::from_str::<Vec<ProxyRequestAttempt>>(raw).ok())
        .and_then(|attempts| attempts.into_iter().last())
        .and_then(|attempt| attempt.upstream_id);
    if let Err(error) = state.db.with_conn(|conn| {
        update_proxy_log_route(
            conn,
            id,
            decision.profile_id.as_deref(),
            Some(decision.reason.as_str()),
            attempt_index,
            Some(decision.requested_model.as_str()),
            final_upstream_id.as_deref().or(decision.upstream_id.as_deref()),
            decision.mode_id.as_deref(),
            attempts_json,
        )
    }) {
        log::warn!("写入网关路由观测失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

pub(crate) fn update_proxy_log_attempts(state: &ProxyState, id: &str, attempts_json: &str) {
    if let Err(error) = state.db.with_conn(|conn| {
        crate::database::dao::proxy_logs::update_proxy_log_attempts(conn, id, attempts_json)
    }) {
        log::warn!("写入请求尝试日志失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

fn mark_last_attempt_failure(attempts: &mut [ProxyRequestAttempt], category: &str, diagnostic: &str) {
    if let Some(attempt) = attempts.last_mut() {
        attempt.success = false;
        attempt.error_category = Some(category.to_string());
        attempt.diagnostic = Some(diagnostic.to_string());
    }
}

// 非流式信封失败后仍复用同一计费行，更新为最终实际出站的供应商。
fn patch_completed_fallback_log(
    state: &ProxyState,
    id: Option<&str>,
    provider: &Provider,
    status: i64,
    duration_ms: i64,
    attempt_index: i64,
    attempts: &[ProxyRequestAttempt],
    success: bool,
) {
    let Some(id) = id else { return; };
    let attempts_json = serde_json::to_string(attempts).unwrap_or_else(|_| "[]".into());
    let result = state.db.with_conn(|conn| {
        conn.execute(
            "UPDATE proxy_request_logs SET provider_id=?1, provider_name=?2, model=?3,
             protocol=?4, upstream_id=?1, status_code=?5, duration_ms=?6, attempt_index=?7,
             attempts_json=?8, error_category=?9, diagnostic=?10 WHERE id=?11",
            rusqlite::params![provider.id, provider.name, provider.model, provider.protocol_type.as_str(),
                status, duration_ms, attempt_index, attempts_json,
                if success { None } else { Some("upstream_envelope") },
                if success { "Responses 信封失败后备用接管成功" } else { "Responses 信封失败，未获得有效备用响应" }, id],
        )?;
        Ok(())
    });
    if let Err(error) = result {
        log::warn!("更新信封备用结果失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
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
    let result = state.db.with_conn(|conn| {
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
    });
    match result {
        Ok(id) => {
            if let Some(slot) = &state.request_log { slot.record(&id); }
            crate::usage_events::notify_log_recorded();
        }
        Err(error) => log::error!("写入代理早期失败日志失败: {error}"),
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
        "auth" => "account authorization failed",
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

fn update_log_diagnostic(state: &ProxyState, id: Option<&str>, category: &str, diagnostic: &str) {
    let Some(id) = id else {
        return;
    };
    if let Err(error) = state
        .db
        .with_conn(|conn| update_proxy_log_diagnostic(conn, id, category, diagnostic))
    {
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
            (
                "message",
                error.get("message").or_else(|| value.get("message")),
            ),
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
        format!(
            "上游返回 HTTP {}，未提供可安全展示的错误摘要",
            status.as_u16()
        )
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
        .next_back()
}

fn usage_from_value(value: &Value) -> Option<UsageCounts> {
    let usage = value
        .get("usage")
        .or_else(|| value.pointer("/response/usage"))?;
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
    let cache_read = anthropic_style_cache.or(details_cache).unwrap_or(0).max(0);
    let fresh_input = if anthropic_style_cache.is_some() {
        // Anthropic: `input_tokens` is already non-cached / fresh.
        input_tokens_field
            .unwrap_or_else(|| reported_input.saturating_sub(cache_read.min(reported_input)))
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
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        envelope_id: extract_usage_envelope_id(value),
    })
}

pub(crate) fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    json_error_with_retry_after(status, message, None)
}

pub(crate) fn json_error_with_retry_after(
    status: StatusCode,
    message: impl Into<String>,
    retry_after_secs: Option<u64>,
) -> Response {
    let body = serde_json::json!({"error": message.into()});
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(secs) = retry_after_secs.filter(|value| *value > 0) {
        builder = builder.header(header::RETRY_AFTER, secs.to_string());
    }
    builder
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

    pub(super) fn circuit_test_state() -> ProxyState {
        ProxyState {
            db: Arc::new(Database::memory().unwrap()),
            client: Client::new(),
            codex_history: Arc::new(super::codex_history::CodexHistoryStore::default()),
            target: ProviderTarget::ClaudeCode,
            listener_kind: ListenerKind::Agent,
            port: DEFAULT_PORT,
            started_at: Instant::now(),
            correlation: None,
            request_log: None,
            request_path: String::new(),
        }
    }

    struct TestServer(tokio::task::JoinHandle<()>);

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    #[tokio::test]
    async fn responses_envelope_fallback_obeys_admission_and_updates_single_log() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower::ServiceExt;
        for (blocked, _explicit, pinned) in [
            (false, false, false), (true, false, false),
            (false, true, false), (true, true, false),
            (false, false, true),
        ] {
            let mut state = circuit_test_state();
            state.listener_kind = ListenerKind::SmartGateway;
            state.client = Client::builder().no_proxy().build().unwrap();
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let primary_id = format!("envelope_primary_{suffix}");
            let backup_id = format!("envelope_backup_{suffix}");
            let hits = Arc::new(AtomicUsize::new(0));
            let count = hits.clone();
            let mock = Router::new()
                .route("/primary/v1/responses", axum::routing::post(|| async {
                    axum::Json(serde_json::json!({"status":"failed","error":{"message":"private upstream body"}}))
                }))
                .route("/backup/v1/messages", axum::routing::post(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        axum::Json(serde_json::json!({"id":"msg_backup","type":"message","role":"assistant",
                            "model":"raw-model","content":[{"type":"text","text":"backup works"}],
                            "stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}))
                    }
                }));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); });
            let _server_guard = TestServer(server);
            let credential = crate::secrets::test_credentials::Credential::new("mock-key");
            // 仅内存数据库、合成凭据和 loopback，不访问系统凭据或用户配置。
            state.db.with_conn(|conn| {
                crate::database::dao::gateway::ensure_profile_for_target(conn, state.target)?;
                conn.execute("INSERT INTO providers (id,name,base_url,api_key,model,is_current) VALUES ('seed','seed','http://127.0.0.1:1',?1,'model',1)", [credential.reference()])?;
                for (id, route, protocol, sort) in [(&primary_id, "primary", "openai_responses", 0), (&backup_id, "backup", "anthropic", 1)] {
                    conn.execute("INSERT INTO upstreams (id,name,base_url,api_key,model,protocol_type,sort_index) VALUES (?1,?2,?3,?6,'model',?4,?5)",
                        rusqlite::params![id, route, format!("http://{address}/{route}"), protocol, sort, credential.reference()])?;
                }
                let fallback_models = if pinned {
                    "[]"
                } else {
                    "[\"claude.backup.model\"]"
                };
                conn.execute(
                    "UPDATE gateway_profiles SET fallback_mode=?1, fallback_models_json=?2, explicit_fallback_enabled=0",
                    rusqlite::params![if pinned { "off" } else { "model_chain" }, fallback_models],
                )?;
                Ok(())
            }).unwrap();
            state.db.gateway_upstream_limiter.apply(&backup_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
                max_concurrency: 1, queue_capacity: 0, ..Default::default()
            });
            let held = if blocked { Some(state.db.gateway_upstream_limiter.acquire(&backup_id).await.unwrap()) } else { None };
            let key = crate::gateway::service::ensure_api_key(&state.db);
            let client_model = if pinned { "claude.primary.model" } else { "claude.auto" };
            let request = http::Request::builder().method("POST").uri("/v1/messages")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({
                    "model": client_model, "messages": [{"role":"user","content":"hello"}],
                    "max_tokens":16,"stream":false
                }).to_string())).unwrap();
            let response = smart_gateway_router(state.db.clone(), 0).oneshot(request).await.unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
            assert_eq!(
                status,
                if blocked || pinned { StatusCode::BAD_GATEWAY } else { StatusCode::OK },
                "unexpected gateway response: {}",
                String::from_utf8_lossy(&body)
            );
            if !blocked && !pinned {
                let value: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(value["content"][0]["text"], "backup works");
                assert_eq!(value["model"], "claude.auto");
            }
            assert_eq!(hits.load(Ordering::SeqCst), usize::from(!blocked && !pinned));
            state.db.with_read_conn(|conn| {
                let count: i64 = conn.query_row("SELECT count(*) FROM proxy_request_logs", [], |row| row.get(0))?;
                assert_eq!(count, 1);
                let (provider, upstream, status, category, raw, input, output, diagnostic): (String,String,i64,Option<String>,String,i64,i64,String) = conn.query_row(
                    "SELECT provider_id,upstream_id,status_code,error_category,attempts_json,input_tokens,output_tokens,diagnostic FROM proxy_request_logs", [],
                    |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)))?;
                assert_eq!(provider, if pinned { primary_id.clone() } else { backup_id.clone() });
                assert_eq!(upstream, provider);
                assert_eq!(status, if blocked || pinned { 502 } else { 200 });
                assert_eq!(category.is_some(), blocked || pinned);
                assert!(!diagnostic.contains("private upstream body"));
                let attempts: Vec<ProxyRequestAttempt> = serde_json::from_str(&raw).unwrap();
                assert_eq!(attempts.len(), if pinned { 1 } else { 2 });
                assert!(!attempts[0].success);
                if pinned {
                    assert_eq!(attempts[0].error_category.as_deref(), Some("upstream_envelope"));
                } else {
                    assert_eq!(attempts[1].success, !blocked);
                    assert!(attempts[1].queue_wait_ms.is_some());
                    if blocked {
                        assert_eq!(attempts[1].error_category.as_deref(), Some("local_queue_full"));
                        assert!(crate::gateway::health::is_available(&backup_id, Some("model")));
                    } else {
                        assert_eq!((input, output), (3, 2));
                    }
                }
                Ok(())
            }).unwrap();
            drop(held);
        }
    }

    fn seed_loopback_gateway(
        db: &Database,
        address: std::net::SocketAddr,
        primary_id: &str,
        backup_id: &str,
        credential: &str,
        protocol: &str,
        target: ProviderTarget,
        fallback: bool,
    ) {
        db.with_conn(|conn| {
            crate::database::dao::gateway::ensure_profile_for_target(conn, target)?;
            conn.execute(
                "INSERT INTO providers (id,name,base_url,api_key,model,is_current,target_app) VALUES ('seed','seed','http://127.0.0.1:1',?1,'model',1,?2)",
                rusqlite::params![credential, target.as_str()],
            )?;
            for (id, route, sort) in [(primary_id, "primary", 0), (backup_id, "backup", 1)] {
                conn.execute(
                    "INSERT INTO upstreams (id,name,base_url,api_key,model,protocol_type,sort_index) VALUES (?1,?2,?3,?4,'model',?5,?6)",
                    rusqlite::params![id, route, format!("http://{address}/{route}"), credential, protocol, sort],
                )?;
            }
            let backup = if target == ProviderTarget::Codex { "backup.model" } else { "claude.backup.model" };
            let fallback_models = if fallback {
                format!("[\"{backup}\"]")
            } else {
                "[]".to_string()
            };
            conn.execute(
                "UPDATE gateway_profiles SET fallback_mode=?1, fallback_models_json=?2, explicit_fallback_enabled=0",
                rusqlite::params![if fallback { "model_chain" } else { "off" }, fallback_models],
            )?;
            Ok(())
        }).unwrap();
    }

    #[tokio::test]
    async fn first_output_deadline_switches_before_client_commit_for_code_and_codex() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower::ServiceExt;
        for target in [ProviderTarget::ClaudeCode, ProviderTarget::Codex] {
            for fallback in [false, true] {
                let db = Arc::new(Database::memory().unwrap());
                let primary_id = format!("prefetch_primary_{}", uuid::Uuid::new_v4().simple());
                let backup_id = format!("prefetch_backup_{}", uuid::Uuid::new_v4().simple());
                let credential = crate::secrets::test_credentials::Credential::new("synthetic-key");
                let hits = Arc::new(AtomicUsize::new(0));
                let count = hits.clone();
                let mock = Router::new()
                    .route("/primary/v1/responses", axum::routing::post(|| async {
                        let stream = futures_util::stream::once(async {
                            Ok::<_, Infallible>("data: {\"type\":\"response.created\"}\n\n: ping\n\n")
                        }).chain(futures_util::stream::pending());
                        Response::builder().header("content-type", "text/event-stream")
                            .body(Body::from_stream(stream)).unwrap()
                    }))
                    .route("/backup/v1/responses", axum::routing::post(move || {
                        let count = count.clone();
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            Response::builder().header("content-type", "text/event-stream")
                                .body(Body::from(concat!(
                                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"backup works\",\"output_index\":0,\"content_index\":0}\n\n",
                                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_backup\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n"
                                ))).unwrap()
                        }
                    }));
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let _server = TestServer(tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); }));
                seed_loopback_gateway(&db, address, &primary_id, &backup_id, &credential.reference(), "openai_responses", target, fallback);
                db.gateway_upstream_limiter.apply(&primary_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
                    first_output_timeout_ms: 100, ..Default::default()
                });
                let key = crate::gateway::service::ensure_api_key(&db);
                let (path, body) = if target == ProviderTarget::Codex {
                    ("/v1/responses", serde_json::json!({"model":"auto","input":"hello","stream":true}))
                } else {
                    ("/v1/messages", serde_json::json!({"model":"claude.auto","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true}))
                };
                let request = http::Request::builder().method("POST").uri(path)
                    .header("authorization", format!("Bearer {key}"))
                    .header("x-ai-switcher-target", target.as_str())
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string())).unwrap();
                let response = tokio::time::timeout(Duration::from_secs(5), smart_gateway_router(db.clone(), 0).oneshot(request)).await.unwrap().unwrap();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
                let text = String::from_utf8_lossy(&body);
                assert_eq!(status, if fallback { StatusCode::OK } else { StatusCode::GATEWAY_TIMEOUT }, "target={target:?} fallback={fallback}: {text}");
                if fallback {
                    assert_eq!(text.matches("backup works").count(), 1, "{text}");
                    assert!(!text.contains(": ping"));
                    if target == ProviderTarget::ClaudeCode { assert!(text.contains("claude.auto"), "{text}"); }
                }
                assert_eq!(hits.load(Ordering::SeqCst), usize::from(fallback));
                db.with_read_conn(|conn| {
                    let logs = crate::database::dao::proxy_logs::list_proxy_request_logs(conn, &Default::default(), 0, 10)?;
                    assert_eq!(logs.data.len(), 1);
                    let attempts = logs.data[0].parse_attempts();
                    assert_eq!(attempts.len(), if fallback { 2 } else { 1 });
                    assert_eq!(attempts[0].error_category.as_deref(), Some("first_output_timeout"));
                    assert_eq!(logs.data[0].upstream_id.as_deref(), Some(if fallback { backup_id.as_str() } else { primary_id.as_str() }));
                    Ok(())
                }).unwrap();
                assert_eq!(db.gateway_upstream_limiter.snapshot(&primary_id).unwrap().active, 0);
            }
        }
    }

    #[tokio::test]
    async fn tcp_disconnect_cancels_gateway_queue_and_prefetch_without_backup() {
        use std::io::Write;
        use std::sync::atomic::{AtomicUsize, Ordering};
        for target in [ProviderTarget::ClaudeCode, ProviderTarget::Codex] {
            for queued in [true, false] {
                let db = Arc::new(Database::memory().unwrap());
                let primary_id = format!("cancel_primary_{}", uuid::Uuid::new_v4().simple());
                let backup_id = format!("cancel_backup_{}", uuid::Uuid::new_v4().simple());
                let credential = crate::secrets::test_credentials::Credential::new("synthetic-key");
                let primary_hits = Arc::new(AtomicUsize::new(0));
                let backup_hits = Arc::new(AtomicUsize::new(0));
                let (primary, backup) = (primary_hits.clone(), backup_hits.clone());
                let mock = Router::new()
                    .route("/primary/v1/responses", axum::routing::post(move || {
                        let primary = primary.clone();
                        async move {
                            primary.fetch_add(1, Ordering::SeqCst);
                            let stream = futures_util::stream::once(async {
                                Ok::<_, Infallible>("data: {\"type\":\"response.created\"}\n\n")
                            }).chain(futures_util::stream::pending());
                            Response::builder().header("content-type", "text/event-stream")
                                .body(Body::from_stream(stream)).unwrap()
                        }
                    }))
                    .route("/backup/v1/responses", axum::routing::post(move || {
                        let backup = backup.clone();
                        async move {
                            backup.fetch_add(1, Ordering::SeqCst);
                            axum::Json(serde_json::json!({"status":"completed"}))
                        }
                    }));
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let _mock = TestServer(tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); }));
                seed_loopback_gateway(&db, address, &primary_id, &backup_id, &credential.reference(), "openai_responses", target, true);
                db.gateway_upstream_limiter.apply(&primary_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
                    max_concurrency: 1, queue_timeout_ms: 2000,
                    first_output_timeout_ms: 1000, ..Default::default()
                });
                let held = if queued { Some(db.gateway_upstream_limiter.acquire(&primary_id).await.unwrap()) } else { None };
                let key = crate::gateway::service::ensure_api_key(&db);
                let app = smart_gateway_router(db.clone(), 0);
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let gateway_address = listener.local_addr().unwrap();
                let _gateway = TestServer(tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); }));
                let (path, body) = if target == ProviderTarget::Codex {
                    ("/v1/responses", serde_json::json!({"model":"auto","input":"hello","stream":true}))
                } else {
                    ("/v1/messages", serde_json::json!({"model":"claude.auto","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true}))
                };
                let payload = body.to_string();
                let request = format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nx-ai-switcher-target: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}", target.as_str(), payload.len());
                let socket = tokio::task::spawn_blocking(move || {
                    let mut socket = std::net::TcpStream::connect(gateway_address).unwrap();
                    socket.write_all(request.as_bytes()).unwrap();
                    socket
                }).await.unwrap();
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        if queued {
                            if db.gateway_upstream_limiter.snapshot(&primary_id).unwrap().queue_len == 1 { break; }
                        } else if primary_hits.load(Ordering::SeqCst) == 1 { break; }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }).await.unwrap();
                socket.shutdown(std::net::Shutdown::Both).unwrap();
                drop(socket);
                tokio::time::timeout(Duration::from_millis(750), async {
                    loop {
                        let snapshot = db.gateway_upstream_limiter.snapshot(&primary_id).unwrap();
                        if snapshot.queue_len == 0 && snapshot.active == u32::from(queued) { break; }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }).await.expect("断连应在排队或首输出截止前即时释放请求");
                drop(held);
                tokio::time::sleep(Duration::from_millis(1100)).await;
                assert_eq!(primary_hits.load(Ordering::SeqCst), usize::from(!queued));
                assert_eq!(backup_hits.load(Ordering::SeqCst), 0);
                let snapshot = db.gateway_upstream_limiter.snapshot(&primary_id).unwrap();
                assert_eq!(snapshot.active, 0);
                assert_eq!(snapshot.current_rpm, u32::from(!queued));
                assert!(crate::gateway::health::is_available(&primary_id, Some("model")));
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let outcome = db.with_read_conn(|conn| {
                            Ok(conn.query_row("SELECT stream_outcome FROM proxy_request_logs LIMIT 1", [], |row| row.get::<_, Option<String>>(0)).ok().flatten())
                        }).unwrap();
                        if outcome.as_deref() == Some("cancelled") { break; }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }).await.expect("头前断连应记录取消终态");
                db.with_read_conn(|conn| {
                    assert_eq!(conn.query_row("SELECT count(*) FROM proxy_request_logs", [], |row| row.get::<_, i64>(0))?, 1);
                    let (provider_id, upstream_id, route, attempts_json): (Option<String>, Option<String>, Option<String>, Option<String>) = conn.query_row(
                        "SELECT provider_id, upstream_id, route, attempts_json FROM proxy_request_logs LIMIT 1",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?;
                    assert_eq!(provider_id.as_deref(), Some(primary_id.as_str()));
                    assert_eq!(upstream_id.as_deref(), Some(primary_id.as_str()));
                    assert_eq!(route.as_deref(), Some(if target == ProviderTarget::Codex { "/v1/responses" } else { "/v1/messages" }));
                    let attempts: Vec<crate::database::dao::proxy_logs::ProxyRequestAttempt> = serde_json::from_str(attempts_json.as_deref().unwrap_or("[]")).unwrap();
                    assert_eq!(attempts.last().and_then(|attempt| attempt.upstream_id.as_deref()), Some(primary_id.as_str()));
                    Ok(())
                }).unwrap();
            }
        }
    }


    #[tokio::test]
    async fn first_output_deadline_covers_anthropic_chat_limit_and_post_commit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tower::ServiceExt;
        for protocol in ["anthropic", "openai_chat"] {
            let db = Arc::new(Database::memory().unwrap());
            let primary_id = format!("proto_primary_{}", uuid::Uuid::new_v4().simple());
            let backup_id = format!("proto_backup_{}", uuid::Uuid::new_v4().simple());
            let credential = crate::secrets::test_credentials::Credential::new("synthetic-key");
            let backup_hits = Arc::new(AtomicUsize::new(0));
            let hits = backup_hits.clone();
            let (primary_path, backup_path, stall, ready) = if protocol == "anthropic" {
                ("/primary/v1/messages", "/backup/v1/messages",
                    "data: {\"type\":\"message_start\"}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"backup works\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n")
            } else {
                ("/primary/v1/chat/completions", "/backup/v1/chat/completions",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"backup works\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            };
            let stall = stall.to_string();
            let ready = ready.to_string();
            let mock = Router::new()
                .route(primary_path, axum::routing::post({
                    let stall = stall.clone();
                    move || {
                        let stall = stall.clone();
                        async move {
                            let stream = futures_util::stream::once(async move { Ok::<_, Infallible>(stall) }).chain(futures_util::stream::pending());
                            Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(stream)).unwrap()
                        }
                    }
                }))
                .route(backup_path, axum::routing::post({
                    let hits = hits.clone();
                    let ready = ready.clone();
                    move || {
                        let hits = hits.clone();
                        let ready = ready.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            Response::builder().header("content-type", "text/event-stream").body(Body::from(ready)).unwrap()
                        }
                    }
                }));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let _server = TestServer(tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); }));
            seed_loopback_gateway(&db, address, &primary_id, &backup_id, &credential.reference(), protocol, ProviderTarget::ClaudeCode, true);
            db.gateway_upstream_limiter.apply(&primary_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
                first_output_timeout_ms: 100, ..Default::default()
            });
            let key = crate::gateway::service::ensure_api_key(&db);
            let body = serde_json::json!({"model":"claude.auto","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true});
            let request = http::Request::builder().method("POST").uri("/v1/messages")
                .header("authorization", format!("Bearer {key}"))
                .header("x-ai-switcher-target", "claude_code")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())).unwrap();
            let response = tokio::time::timeout(Duration::from_secs(5), smart_gateway_router(db.clone(), 0).oneshot(request)).await.unwrap().unwrap();
            let status = response.status();
            let text = String::from_utf8_lossy(&axum::body::to_bytes(response.into_body(), 65536).await.unwrap()).to_string();
            assert_eq!(status, StatusCode::OK, "{protocol}: {text}");
            assert!(text.contains("backup works"), "{protocol}: {text}");
            assert_eq!(backup_hits.load(Ordering::SeqCst), 1, "{protocol}");
        }

        let db = Arc::new(Database::memory().unwrap());
        let primary_id = format!("limit_primary_{}", uuid::Uuid::new_v4().simple());
        let backup_id = format!("limit_backup_{}", uuid::Uuid::new_v4().simple());
        let credential = crate::secrets::test_credentials::Credential::new("synthetic-key");
        let backup_hits = Arc::new(AtomicUsize::new(0));
        let hits = backup_hits.clone();
        let mock = Router::new()
            .route("/primary/v1/messages", axum::routing::post(|| async {
                let mut bytes = b"data: {\"type\":\"message_start\"}\n".to_vec();
                bytes.extend(std::iter::repeat(b'x').take(256 * 1024));
                Response::builder().header("content-type", "text/event-stream").body(Body::from(bytes)).unwrap()
            }))
            .route("/backup/v1/messages", axum::routing::post(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder().header("content-type", "text/event-stream").body(Body::from(
                        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"backup works\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n"
                    )).unwrap()
                }
            }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = TestServer(tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); }));
        seed_loopback_gateway(&db, address, &primary_id, &backup_id, &credential.reference(), "anthropic", ProviderTarget::ClaudeCode, true);
        db.gateway_upstream_limiter.apply(&primary_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
            first_output_timeout_ms: 1000, ..Default::default()
        });
        let key = crate::gateway::service::ensure_api_key(&db);
        let body = serde_json::json!({"model":"claude.auto","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true});
        let request = http::Request::builder().method("POST").uri("/v1/messages")
            .header("authorization", format!("Bearer {key}"))
            .header("x-ai-switcher-target", "claude_code")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), smart_gateway_router(db.clone(), 0).oneshot(request)).await.unwrap().unwrap();
        let status = response.status();
        let text = String::from_utf8_lossy(&axum::body::to_bytes(response.into_body(), 65536).await.unwrap()).to_string();
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(backup_hits.load(Ordering::SeqCst), 1, "{text}");
        db.with_read_conn(|conn| {
            let logs = crate::database::dao::proxy_logs::list_proxy_request_logs(conn, &Default::default(), 0, 5)?;
            assert!(logs.data.iter().flat_map(|log| log.parse_attempts()).any(|attempt| attempt.error_category.as_deref() == Some("first_output_buffer_limit")));
            Ok(())
        }).unwrap();

        let db = Arc::new(Database::memory().unwrap());
        let primary_id = format!("commit_primary_{}", uuid::Uuid::new_v4().simple());
        let backup_id = format!("commit_backup_{}", uuid::Uuid::new_v4().simple());
        let credential = crate::secrets::test_credentials::Credential::new("synthetic-key");
        let backup_hits = Arc::new(AtomicUsize::new(0));
        let hits = backup_hits.clone();
        let mock = Router::new()
            .route("/primary/v1/messages", axum::routing::post(|| async {
                let stream = futures_util::stream::once(async {
                    Ok::<_, Infallible>("data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"committed\"}}\n\n")
                }).chain(futures_util::stream::pending());
                Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(stream)).unwrap()
            }))
            .route("/backup/v1/messages", axum::routing::post(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({"status":"backup"}))
                }
            }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = TestServer(tokio::spawn(async move { axum::serve(listener, mock).await.unwrap(); }));
        seed_loopback_gateway(&db, address, &primary_id, &backup_id, &credential.reference(), "anthropic", ProviderTarget::ClaudeCode, true);
        db.gateway_upstream_limiter.apply(&primary_id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
            first_output_timeout_ms: 100, ..Default::default()
        });
        let key = crate::gateway::service::ensure_api_key(&db);
        let body = serde_json::json!({"model":"claude.auto","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true});
        let request = http::Request::builder().method("POST").uri("/v1/messages")
            .header("authorization", format!("Bearer {key}"))
            .header("x-ai-switcher-target", "claude_code")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), smart_gateway_router(db, 0).oneshot(request)).await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        use http_body_util::BodyExt;
        let mut body = response.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame()).await.unwrap().unwrap().unwrap();
        let bytes = frame.into_data().unwrap_or_default();
        assert!(String::from_utf8_lossy(&bytes).contains("committed"));
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(backup_hits.load(Ordering::SeqCst), 0);
        drop(body);
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
        assert!(should_failover_upstream_status(
            &ag,
            StatusCode::BAD_GATEWAY
        ));
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

        let first = next_failover_provider(&state, std::slice::from_ref(&current_id), "claude-opus-5")
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

        let matched = next_failover_provider(&state, &[current_id, group0_id], "gpt-4o-mini")
            .unwrap()
            .expect("group1 whitelist match");
        assert_eq!(matched.name, "Group1");
    }

    #[test]
    fn upstream_sse_decoder_reassembles_split_json_frames() {
        let mut decoder = UpstreamSseDecoder::default();
        assert!(decoder
            .push(b"data: {\"type\":\"response.output_text")
            .is_empty());
        let events = decoder.push(b".delta\",\"delta\":\"hi\"}\n\n");
        assert_eq!(events.len(), 1);
        let UpstreamSseItem::Json(event) = &events[0] else {
            panic!("expected JSON SSE event");
        };
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
                let (body, _) = encode_upstream_request(
                    &provider,
                    &incoming,
                    &original,
                    false,
                    &HeaderMap::new(),
                );
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
        assert_eq!(upstream_error_category(StatusCode::BAD_GATEWAY), "upstream");
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
        let kimi =
            br#"{"usage":{"input_tokens":321,"cache_read_input_tokens":12,"output_tokens":45}}"#;
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
