async fn proxy_handler(
    State(mut state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if let Err(error) = validate_listener_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    if state.listener_kind == ListenerKind::SmartGateway {
        if let Some(target) = resolve_binding_target(&state, &headers) { state.target = target; }
    }
    state.request_path = uri.path().to_string();
    state.correlation = Some(crate::gateway::correlation::resolve(
        &headers,
        if state.listener_kind == ListenerKind::SmartGateway {
            crate::gateway::correlation::HOP_SMART_GATEWAY
        } else { crate::gateway::correlation::HOP_AGENT_PROXY },
        Some(state.target.as_str()),
    ));
    let mut pending = response_lifecycle::PendingRequestGuard::new(&mut state);
    let permit = if state.listener_kind == ListenerKind::SmartGateway {
        match acquire_smart_gateway_inbound().await {
            Ok(permit) => Some(permit),
            Err(response) => { pending.disarm(); return response; }
        }
    } else {
        None
    };
    let response = proxy_handler_inner(state, method, uri, headers, body).await;
    pending.disarm();
    response_lifecycle::hold_response_guard(response, permit)
}

async fn proxy_handler_inner(
    mut state: ProxyState,
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    body: Body,
) -> Response {
    if state.listener_kind == ListenerKind::SmartGateway {
        if let Some(target) = resolve_binding_target(&state, &headers) {
            state.target = target;
        }
    }
    state.request_path = uri.path().to_string();
    let started = Instant::now();

    // Resolve a seed provider so body-read failures can still be logged.
    let provider: Option<Provider> = match state.db.with_conn(|conn| {
        if crate::catalog::enabled_for_conn(conn, state.target) {
            let listed = list_providers(conn, state.target)?;
            Ok(listed
                .iter()
                .find(|item| item.is_current)
                .cloned()
                .or_else(|| listed.first().cloned()))
        } else {
            get_current_provider(conn, state.target)
        }
    }) {
        Ok(p) => p,
        Err(e) => {
            log::error!("代理读取当前供应商失败: {e}");
            None
        }
    };
    let Some(mut provider) = provider else {
        log_early_failure(&state, uri.path(), "provider", Some(503), started.elapsed().as_millis() as i64);
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "没有激活的第三方供应商");
    };
    if provider.is_codex_oauth() {
        match crate::codex_oauth::manager().get_valid_token(Some(&provider.auth_binding)) {
            Ok((token, account_id)) => {
                provider.api_key = token;
                provider.auth_binding = account_id;
                provider.base_url = crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string();
                provider.protocol_type = ProtocolType::OpenAiResponses;
            }
            Err(error) => {
                log::error!("代理读取 ChatGPT OAuth 凭据失败: {error}");
                return json_error(StatusCode::SERVICE_UNAVAILABLE, "ChatGPT 登录已失效");
            }
        }
    } else {
        provider.api_key = match crate::database::dao::provider_runtime_api_key(&provider.api_key) {
        Ok(Some(key)) => key,
        Ok(None) => {
            log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("credential"));
            return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商未配置 API Key");
        }
        Err(e) => {
            log::error!("代理读取供应商凭据失败: {e}");
            log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("credential"));
            return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商凭据不可用");
        }
        };
    }
    if provider.base_url.trim().is_empty() {
        log_request(&state, &provider, None, started.elapsed().as_millis() as i64, uri.path(), false, Some("configuration"));
        return json_error(StatusCode::SERVICE_UNAVAILABLE, "当前供应商未配置 Base URL");
    }

    // Read and optionally rewrite the request body.
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            log_request(&state, &provider, Some(400), started.elapsed().as_millis() as i64, uri.path(), false, Some("request"));
            return json_error(StatusCode::BAD_REQUEST, format!("读取请求体失败: {e}"));
        }
    };

    let incoming: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        Err(_) => {
            log_request(&state, &provider, Some(400), started.elapsed().as_millis() as i64, uri.path(), false, Some("request"));
            return json_error(StatusCode::BAD_REQUEST, "请求体不是有效 JSON");
        }
    };
    let incoming_stream = convert::wants_stream(&incoming);
    let mut incoming = incoming;
    web_tools::rewrite_server_tools(&mut incoming);
    let mut requested_model = incoming
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Echo this id on the Anthropic envelope. Routing may replace the body
    // with an upstream slug; Claude Code must still see the id it sent.
    let client_model = requested_model.clone();

    // Claude Desktop 1.49585.0+ introduces a hard 10-second client-side health probe
    // (max_tokens: 1, single user message with content ".") on start / profile load.
    // Flaky upstream / sub2api latencies frequently breach 10s, triggering an unwanted
    // "Gateway was unreachable" modal. If credentials and base URL are valid, fast-respond
    // to the probe locally with an Anthropic message structure.
    if is_claude_desktop_inference_probe(state.target, uri.path(), &incoming, incoming_stream) {
        let probe_model = if requested_model.is_empty() {
            "claude-haiku-4-5".to_string()
        } else {
            requested_model
        };
        log::info!("Claude Desktop 推理探活快速响应: model={probe_model}");
        let probe_response = serde_json::json!({
            "id": format!("msg_probe_{}", uuid::Uuid::new_v4().simple()),
            "type": "message",
            "role": "assistant",
            "model": probe_model,
            "content": [
                {
                    "type": "text",
                    "text": "."
                }
            ],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1
            }
        });
        log_request(
            &state,
            &provider,
            Some(200),
            started.elapsed().as_millis() as i64,
            uri.path(),
            false,
            None,
        );
        return axum::Json(probe_response).into_response();
    }
    if state.listener_kind == ListenerKind::SmartGateway {
        if let Err((message, retry_after)) =
            crate::gateway::budget::apply_to_request(&state.db, &mut requested_model)
        {
            return json_error_with_retry_after(
                StatusCode::TOO_MANY_REQUESTS,
                message,
                Some(retry_after),
            );
        }
        if let Some(object) = incoming.as_object_mut() {
            object.insert("model".to_string(), Value::String(requested_model.clone()));
        }
    }
    let mut body_bytes = Bytes::from(serde_json::to_vec(&incoming).unwrap_or_else(|_| body_bytes.to_vec()));
    let mut is_catalog_subagent = false;
    let mut route_decision: Option<crate::gateway::RouteDecision> = None;
    let mut route_plan: Option<crate::gateway::RouteExecutionPlan> = None;
    let mut attempt_index: i64 = 0;
    if gateway_catalog_enabled(&state) {
        let force_subagent = is_claude_code_subagent_request(&headers, &requested_model);
        match select_gateway_runtime_provider_with(&state, &requested_model, force_subagent, &incoming, uri.path(), &headers) {
            Ok(Some((selected, upstream, routed_subagent, decision, plan))) => {
                provider = selected;
                requested_model = upstream.clone();
                is_catalog_subagent = routed_subagent;
                crate::gateway::rules::apply_rewrites(&mut incoming, &mut headers, &decision.rewrites);
                if let Some(thinking) = decision.thinking.as_ref() {
                    crate::gateway::thinking::apply_to_body(&mut incoming, provider.protocol_type, thinking);
                    if provider.is_antigravity() {
                        crate::gateway::thinking::apply_gemini_thinking_budget(&mut incoming, thinking);
                    }
                }
                route_decision = Some(decision);
                route_plan = Some(plan);
                if let Some(object) = incoming.as_object_mut() {
                    object.insert("model".to_string(), Value::String(upstream.clone()));
                }
                body_bytes = Bytes::from(serde_json::to_vec(&incoming).unwrap_or_else(|_| rewrite_json_model(&body_bytes, &upstream)));
            }
            Ok(None) => {
                log_early_failure(
                    &state,
                    uri.path(),
                    "provider",
                    Some(503),
                    started.elapsed().as_millis() as i64,
                );
                return json_error(StatusCode::SERVICE_UNAVAILABLE, "没有可路由的第三方供应商");
            }
            Err(error) => {
                log::error!("按模型选择供应商失败: {error}");
                match error {
                    crate::proxy::GatewaySelectionError::Catalog(catalog_error) => {
                        log_early_failure(
                            &state,
                            uri.path(),
                            "model",
                            Some(400),
                            started.elapsed().as_millis() as i64,
                        );
                        return json_error(StatusCode::BAD_REQUEST, catalog_error.to_string());
                    }
                    crate::proxy::GatewaySelectionError::App(_) => {
                        log_early_failure(
                            &state,
                            uri.path(),
                            "configuration",
                            Some(500),
                            started.elapsed().as_millis() as i64,
                        );
                        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录");
                    }
                }
            }
        }
    }
    let prepared = match prepare_upstream_request(&state, &mut provider, &method, &headers, &incoming, &body_bytes, incoming_stream, &client_model) {
        Ok(request) => request,
        Err(error) => {
            log_request(
                &state,
                &provider,
                Some(400),
                started.elapsed().as_millis() as i64,
                uri.path(),
                incoming_stream,
                Some("configuration"),
            );
            return json_error(StatusCode::BAD_REQUEST, error.to_string());
        }
    };
    let mut translated = prepared.translated;
    let mut retry_without_stream_options = compatible_stream_retry(&provider, &prepared, incoming_stream);
    let mut failover_trace: Vec<String> = Vec::new();
    let explicit_models: Vec<String> = route_plan.as_ref()
        .filter(|plan| !plan.explicit_pinned)
        .map(|plan| plan.attempts.iter().skip(1).map(|attempt| attempt.model.clone()).collect())
        .unwrap_or_default();
    let allow_cross_provider_failover = !route_plan.as_ref().is_some_and(|plan| plan.explicit_pinned);
    let has_explicit_chain = !explicit_models.is_empty();
    let mut explicit_models = explicit_models.into_iter();
    let mut legacy_models = if route_plan.is_none() {
        provider.failover_models.clone().into_iter()
    } else {
        Vec::new().into_iter()
    };
    let mut excluded = vec![provider.id.clone()];
    let mut outgoing = apply_catalog_subagent_signal(prepared.builder, is_catalog_subagent)
        .body(prepared.outgoing_body);
    let mut attempts: Vec<ProxyRequestAttempt> = Vec::new();
    let mut upstream_resp = loop {
        let attempt_start = Instant::now();
        note_gateway_inflight(&state, &provider, &requested_model, incoming_stream, route_decision.as_ref(), &attempts);
        let (result, queue_wait_ms) = upstream_health::send_observed_upstream_with_timing(outgoing, &provider, Some(&state), incoming_stream).await;
        let attempt_duration = attempt_start.elapsed().as_millis() as i64;
        attempts.push(build_request_attempt(
            attempts.len(),
            &provider,
            &requested_model,
            attempt_duration,
            &result,
        ).with_queue_wait_ms(queue_wait_ms));
        let can_explicit = allow_cross_provider_failover && match &result {
            Ok(response) => should_try_explicit_response(&provider, response),
            Err(_) => true,
        };
        let can_generic = allow_cross_provider_failover && match &result {
            Ok(response) => is_retryable_upstream_status(&state, response.status())
                && should_failover_upstream_status(&provider, response.status()),
            Err(_) => !provider.is_kiro(),
        };
        let mut next = None;
        if can_explicit && has_explicit_chain {
            for model in explicit_models.by_ref() {
                if let Ok(Some((candidate, slug))) = resolve_explicit_fallback(&state, &model) {
                    if candidate.id == provider.id && slug == requested_model { continue; }
                    if !crate::gateway::health::is_available(&candidate.id, Some(&slug)) { continue; }
                    next = Some((candidate, slug));
                    break;
                }
            }
        } else if can_generic && !has_explicit_chain {
            if let Some(model) = legacy_models.next() {
                next = Some((provider.clone(), model));
            } else if attempt_index < FAILOVER_MAX_HOPS as i64 {
                if let Ok(Some(candidate)) = next_failover_provider(&state, &excluded, &requested_model) {
                    next = Some((candidate, requested_model.clone()));
                }
            }
        }
        let Some((mut candidate, model)) = next else {
            match result {
                Ok(response) => break response,
                Err(error) => {
                    let fail_log_id = log_request_with_diagnostic(&state, &provider, Some(502),
                        started.elapsed().as_millis() as i64, uri.path(), incoming_stream,
                        Some("network"), Some(&format!("故障降级失败: {}", failover_trace.join(" → "))));
                    let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
                    if let Some(id) = fail_log_id.as_deref() {
                        if let Some(decision) = route_decision.as_ref() {
                            patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
                        } else if !attempts.is_empty() {
                            update_proxy_log_attempts(&state, id, &attempts_json);
                        }
                    }
                    return if translated {
                        anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502))
                    } else {
                        json_error(StatusCode::BAD_GATEWAY, format!("转发到上游失败: {error}"))
                    };
                }
            }
        };
        failover_trace.push(format!("{}({}) {} → {}", provider.name, provider.id,
            result.as_ref().map(|response| response.status().to_string()).unwrap_or_else(|_| "网络错误".into()), candidate.name));
        if let Some(object) = incoming.as_object_mut() {
            object.insert("model".into(), Value::String(model.clone()));
        }
        body_bytes = Bytes::from(rewrite_json_model(&body_bytes, &model));
        let fallback_prepared = match prepare_upstream_request(&state, &mut candidate, &method,
            &headers, &incoming, &body_bytes, incoming_stream, &client_model) {
            Ok(prepared) => prepared,
            Err(error) => return json_error(StatusCode::BAD_REQUEST, error.to_string()),
        };
        translated = fallback_prepared.translated;
        retry_without_stream_options = compatible_stream_retry(&candidate, &fallback_prepared, incoming_stream);
        outgoing = apply_catalog_subagent_signal(fallback_prepared.builder, is_catalog_subagent)
            .body(fallback_prepared.outgoing_body);
        requested_model = model;
        excluded.push(candidate.id.clone());
        provider = candidate;
        attempt_index = attempt_index.saturating_add(1);
    };

    if let Some(retry_request) = retry_without_stream_options {
        let rejected_status = upstream_resp.status();
        if matches!(
            rejected_status,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ) {
            let rejected_body = match upstream_resp.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let log_id = log_request(
                        &state,
                        &provider,
                        Some(502),
                        started.elapsed().as_millis() as i64,
                        uri.path(),
                        incoming_stream,
                        Some("network"),
                    );
                    update_log_diagnostic(
                        &state,
                        log_id.as_deref(),
                        "network",
                        "读取 stream_options 兼容性错误响应失败",
                    );
                    log::warn!("读取 OpenAI stream_options 兼容性错误失败: {error}");
                    return anthropic_error(
                        StatusCode::BAD_GATEWAY,
                        convert::openai_error_to_anthropic(502),
                    );
                }
            };
            if explicitly_rejects_stream_options(&rejected_body) {
                log::info!(
                    "上游明确不支持 stream_options.include_usage，移除该字段后兼容重试一次"
                );
                let retry_start = Instant::now();
                note_gateway_inflight(&state, &provider, &requested_model, incoming_stream, route_decision.as_ref(), &attempts);
                let (retry_result, queue_wait_ms) = upstream_health::send_observed_upstream_with_timing(retry_request, &provider, Some(&state), incoming_stream).await;
                let retry_duration = retry_start.elapsed().as_millis() as i64;
                attempts.push(build_request_attempt(
                    attempts.len(),
                    &provider,
                    &requested_model,
                    retry_duration,
                    &retry_result,
                ).with_queue_wait_ms(queue_wait_ms));
                upstream_resp = match retry_result {
                    Ok(response) => response,
                    Err(error) => {
                        let log_id = log_request(
                            &state,
                            &provider,
                            Some(502),
                            started.elapsed().as_millis() as i64,
                            uri.path(),
                            incoming_stream,
                            Some("network"),
                        );
                        update_log_diagnostic(
                            &state,
                            log_id.as_deref(),
                            "network",
                            "移除 stream_options 后的兼容重试连接失败",
                        );
                        let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
                        if let Some(id) = log_id.as_deref() {
                            if let Some(decision) = route_decision.as_ref() {
                                patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
                            } else if !attempts.is_empty() {
                                update_proxy_log_attempts(&state, id, &attempts_json);
                            }
                        }
                        log::warn!("OpenAI stream_options 兼容重试失败: {error}");
                        return anthropic_error(
                            StatusCode::BAD_GATEWAY,
                            convert::openai_error_to_anthropic(502),
                        );
                    }
                };
            } else {
                let log_id = log_request(
                    &state,
                    &provider,
                    Some(rejected_status.as_u16() as i64),
                    started.elapsed().as_millis() as i64,
                    uri.path(),
                    incoming_stream,
                    Some(upstream_error_category(rejected_status)),
                );
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    upstream_error_category(rejected_status),
                    &sanitized_upstream_diagnostic(rejected_status, &rejected_body),
                );
                let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
                if let Some(id) = log_id.as_deref() {
                    if let Some(decision) = route_decision.as_ref() {
                        patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
                    } else if !attempts.is_empty() {
                        update_proxy_log_attempts(&state, id, &attempts_json);
                    }
                }
                return anthropic_error(
                    rejected_status,
                    convert::openai_error_to_anthropic(rejected_status.as_u16()),
                );
            }
        }
    }

    let status = upstream_resp.status();
    let duration_ms = started.elapsed().as_millis() as i64;
    let error_category = (!status.is_success()).then(|| upstream_error_category(status));
    let failover_diag = if !failover_trace.is_empty() {
        Some(format!("故障降级: {}", failover_trace.join(" → ")))
    } else {
        None
    };
    let log_id = log_request_with_diagnostic(
        &state,
        &provider,
        Some(status.as_u16() as i64),
        duration_ms,
        uri.path(),
        incoming_stream,
        error_category,
        failover_diag.as_deref(),
    );
    let attempts_json = serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string());
    if let Some(id) = log_id.as_deref() {
        if let Some(decision) = route_decision.as_ref() {
            patch_route_log(&state, id, decision, attempt_index, Some(&attempts_json));
        } else if !attempts.is_empty() {
            update_proxy_log_attempts(&state, id, &attempts_json);
        }
    }

    // OpenAI upstreams are normalized into Anthropic JSON. For an Anthropic
    // streaming request, keep the OpenAI upstream stream open and translate each
    // SSE frame as it arrives rather than waiting for a completed response.
    if translated {
        if incoming_stream && status.is_success() {
            let protocol = match provider.protocol_type {
                ProtocolType::OpenAiResponses => convert::OpenAiStreamProtocol::Responses,
                _ => convert::OpenAiStreamProtocol::Chat,
            };
            let db = Arc::clone(&state.db);
            let decoder = UpstreamSseDecoder::default();
            let converter = convert::OpenAiSseConverter::new(protocol, client_model.trim());
            let stream_log_id = log_id.clone();
            let target_app = state.target.as_str().to_string();
            let provider_id = provider.id.clone();
            let idle = Duration::from_secs(
                state
                    .db
                    .with_conn(load_streaming_idle_timeout_secs)
                    .unwrap_or(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS),
            );
            let upstream_stream = upstream_resp.bytes_stream();
            let stream = futures_util::stream::unfold(
                (upstream_stream, decoder, converter, false),
                move |(mut upstream_stream, mut decoder, mut converter, done)| {
                    let db = Arc::clone(&db);
                    let stream_log_id = stream_log_id.clone();
                    let target_app = target_app.clone();
                    let provider_id = provider_id.clone();
                    async move {
                        if done {
                            return None;
                        }
                        let next = tokio::time::timeout(idle, upstream_stream.next()).await;
                        let (output, done) = match next {
                            Ok(Some(Ok(bytes))) => {
                                let mut output = Vec::new();
                                let mut terminal_error = false;
                                for item in decoder.push(&bytes) {
                                    match item {
                                        UpstreamSseItem::Json(event) => {
                                            response_lifecycle::record_response_event(
                                                &db, stream_log_id.as_deref(), &event,
                                            );
                                            output.extend(converter.push_event(&event));
                                            if converter.took_terminal_error() {
                                                terminal_error = true;
                                                break;
                                            }
                                        }
                                        UpstreamSseItem::Done => {
                                            response_lifecycle::record_sse_terminal(
                                                &db, stream_log_id.as_deref(), "[DONE]",
                                            );
                                            output.extend(converter.finish_stream())
                                        }
                                    }
                                }
                                if terminal_error {
                                    mark_passthrough_midstream_error(
                                        &db,
                                        stream_log_id.as_deref(),
                                        "midstream_error",
                                        "上游服务返回流式错误",
                                    );
                                }
                                (output, terminal_error)
                            }
                            Ok(Some(Err(_))) => {
                                if let Some(id) = stream_log_id.as_deref() {
                                    let _ = db.with_conn(|conn| {
                                        update_proxy_log_stream_outcome(
                                            conn,
                                            id,
                                            "midstream_error",
                                            None,
                                            Some("midstream_error"),
                                            Some("上游流式响应中途中断"),
                                        )
                                    });
                                }
                                (converter.error_event("上游流式响应中断"), true)
                            }
                            Ok(None) => {
                                let output = converter.finish_stream();
                                if let Some(id) = stream_log_id.as_deref() {
                                    let _ = db.with_conn(|conn| {
                                        update_proxy_log_stream_outcome(
                                            conn,
                                            id,
                                            "complete",
                                            None,
                                            None,
                                            None,
                                        )
                                    });
                                }
                                (output, true)
                            }
                            Err(_) => {
                                if let Some(id) = stream_log_id.as_deref() {
                                    let _ = db.with_conn(|conn| {
                                        update_proxy_log_stream_outcome(
                                            conn,
                                            id,
                                            "midstream_error",
                                            None,
                                            Some("timeout"),
                                            Some("流式响应空闲超时"),
                                        )
                                    });
                                }
                                (converter.error_event("流式响应空闲超时"), true)
                            }
                        };
                        if let (Some(id), Some(usage)) =
                            (stream_log_id.as_deref(), converter.usage())
                        {
                            if let Err(error) = db.with_conn(|conn| {
                                update_proxy_log_usage_idempotent(
                                    conn,
                                    id,
                                    Some(target_app.as_str()),
                                    Some(provider_id.as_str()),
                                    usage.envelope_id.as_deref(),
                                    usage.input_tokens,
                                    usage.cache_read_input_tokens,
                                    usage.cache_creation_input_tokens,
                                    usage.output_tokens,
                                )
                            }) {
                                log::error!("更新代理请求 Token 用量失败: {error}");
                            } else {
                                crate::usage_events::notify_log_recorded();
                            }
                        }
                        if output.is_empty() && done {
                            return None;
                        }
                        Some((
                            Ok::<Bytes, Infallible>(Bytes::from(output)),
                            (upstream_stream, decoder, converter, done),
                        ))
                    }
                },
            );
            if status.is_success() {
                remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
            }
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header("x-accel-buffering", "no")
                .body(response_lifecycle::track_stream_body(
                    Body::from_stream(stream), Arc::clone(&state.db), log_id.clone(),
                ))
                .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"));
        }
        let response_bytes = match upstream_resp.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "network",
                    "读取 OpenAI 上游响应失败",
                );
                return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
            }
        };
        if !status.is_success() {
            update_log_diagnostic(
                &state,
                log_id.as_deref(),
                upstream_error_category(status),
                &sanitized_upstream_diagnostic(status, &response_bytes),
            );
            return anthropic_error(status, convert::openai_error_to_anthropic(status.as_u16()));
        }
        let upstream: Value = match serde_json::from_slice(&response_bytes) {
            Ok(value) => value,
            Err(_) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "conversion",
                    "OpenAI 上游返回了无法转换的非 JSON 成功响应",
                );
                return anthropic_error(StatusCode::BAD_GATEWAY, convert::openai_error_to_anthropic(502));
            }
        };
        if provider.protocol_type == ProtocolType::OpenAiResponses {
            if let Some(failed) = convert::responses_failed_anthropic_error(&upstream) {
                if !provider.is_antigravity() && !provider.is_kiro() {
                    record_provider_failure(&state, &provider.id);
                }
                if let Some(attempt) = attempts.last_mut() {
                    attempt.success = false;
                    attempt.error_category = Some("upstream_envelope".into());
                    attempt.diagnostic = Some("Responses status=failed".into());
                }
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "upstream_envelope",
                    "Responses status=failed",
                );
                for _ in attempt_index as usize..FAILOVER_MAX_HOPS {
                    if !allow_cross_provider_failover || attempt_index >= FAILOVER_MAX_HOPS as i64 { break; }
                    let next = if has_explicit_chain {
                        let mut selected = None;
                        for model in explicit_models.by_ref() {
                            if let Ok(Some((candidate, slug))) = resolve_explicit_fallback(&state, &model) {
                                if candidate.id == provider.id && slug == requested_model { continue; }
                                if !crate::gateway::health::is_available(&candidate.id, Some(&slug)) { continue; }
                                selected = Some((candidate, slug));
                                break;
                            }
                        }
                        selected
                    } else if !provider.is_kiro() {
                        next_failover_provider(&state, &excluded, &requested_model)
                            .ok().flatten().map(|candidate| (candidate, requested_model.clone()))
                    } else {
                        None
                    };
                    let Some((mut fallback, model)) = next else { break; };
                    excluded.push(fallback.id.clone());
                    requested_model = model;
                    if let Some(object) = incoming.as_object_mut() {
                        object.insert("model".into(), Value::String(requested_model.clone()));
                    }
                    body_bytes = Bytes::from(rewrite_json_model(&body_bytes, &requested_model));
                    log::warn!(
                        "供应商 {} 返回 Responses failed envelope，尝试故障切换到 {}",
                        provider.id,
                        fallback.id
                    );
                    let Ok(fallback_prepared) = prepare_upstream_request(
                        &state,
                        &mut fallback,
                        &method,
                        &headers,
                        &incoming,
                        &body_bytes,
                        incoming_stream,
                        &client_model,
                    ) else {
                        continue;
                    };
                    let fallback_start = Instant::now();
                    note_gateway_inflight(&state, &fallback, &fallback.model, false, route_decision.as_ref(), &attempts);
                    let (result, queue_wait_ms) = upstream_health::send_observed_upstream_with_timing(
                        apply_catalog_subagent_signal(fallback_prepared.builder, is_catalog_subagent)
                            .body(fallback_prepared.outgoing_body),
                        &fallback,
                        Some(&state),
                        false,
                    ).await;
                    attempt_index = attempt_index.saturating_add(1);
                    attempts.push(build_request_attempt(
                        attempts.len(), &fallback, &fallback.model,
                        fallback_start.elapsed().as_millis() as i64, &result,
                    ).with_queue_wait_ms(queue_wait_ms));
                    provider = fallback.clone();
                    match result {
                        Ok(response) if response.status().is_success() => {
                            let fallback_bytes = match response.bytes().await {
                                Ok(bytes) => bytes,
                                Err(_) => {
                                    if !fallback.is_antigravity() && !fallback.is_kiro() {
                                        record_provider_failure(&state, &fallback.id);
                                    }
                                    mark_last_attempt_failure(&mut attempts, "network", "读取备用响应失败");
                                    continue;
                                }
                            };
                            let fallback_json: Value = match serde_json::from_slice(&fallback_bytes)
                            {
                                Ok(value) => value,
                                Err(_) => {
                                    mark_last_attempt_failure(&mut attempts, "conversion", "备用返回非 JSON 响应");
                                    continue;
                                }
                            };
                            if fallback.protocol_type == ProtocolType::OpenAiResponses
                                && convert::responses_failed_anthropic_error(&fallback_json)
                                    .is_some()
                            {
                                if !fallback.is_antigravity() && !fallback.is_kiro() {
                                    record_provider_failure(&state, &fallback.id);
                                }
                                mark_last_attempt_failure(&mut attempts, "upstream_envelope", "Responses status=failed");
                                continue;
                            }
                            provider = fallback;
                            let anthropic = match provider.protocol_type {
                                ProtocolType::Anthropic => {
                                    let mut response = fallback_json;
                                    response["model"] = Value::String(client_model.trim().to_string());
                                    response
                                }
                                ProtocolType::OpenAiResponses => {
                                    convert::openai_responses_to_anthropic(
                                        &fallback_json,
                                        client_model.trim(),
                                    )
                                }
                                _ => convert::openai_chat_to_anthropic(
                                    &fallback_json,
                                    client_model.trim(),
                                ),
                            };
                            let mut anthropic = anthropic;
                            let _ = web_tools::materialize_web_tool_uses(&mut anthropic);
                            record_provider_success(&state, &provider.id);
                            remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
                            patch_completed_fallback_log(&state, log_id.as_deref(), &provider,
                                200, started.elapsed().as_millis() as i64, attempt_index, &attempts, true);
                            if let Some(id) = log_id.as_deref() {
                                update_log_usage(
                                    &state,
                                    &provider,
                                    id,
                                    extract_usage_from_json(
                                        &serde_json::to_vec(&anthropic).unwrap_or_default(),
                                    ),
                                );
                            }
                            if incoming_stream {
                                return Response::builder()
                                    .status(StatusCode::OK)
                                    .header(header::CONTENT_TYPE, "text/event-stream")
                                    .header(header::CACHE_CONTROL, "no-cache")
                                    .body(Body::from(convert::anthropic_message_to_sse(&anthropic)))
                                    .unwrap_or_else(|_| {
                                        json_error(
                                            StatusCode::INTERNAL_SERVER_ERROR,
                                            "构造流式响应失败",
                                        )
                                    });
                            }
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "application/json")
                                .body(Body::from(
                                    serde_json::to_vec(&anthropic).unwrap_or_default(),
                                ))
                                .unwrap_or_else(|_| {
                                    json_error(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "构造响应失败",
                                    )
                                });
                        }
                        Ok(_) | Err(_) => {
                            // 共同观察器已分类；本地准入拒绝不得再次污染健康状态。
                        }
                    }
                }
                patch_completed_fallback_log(&state, log_id.as_deref(), &provider,
                    502, started.elapsed().as_millis() as i64, attempt_index, &attempts, false);
                return anthropic_error(StatusCode::BAD_GATEWAY, failed);
            }
        }
        let anthropic = match provider.protocol_type {
            ProtocolType::OpenAiResponses => convert::openai_responses_to_anthropic(&upstream, client_model.trim()),
            _ => convert::openai_chat_to_anthropic(&upstream, client_model.trim()),
        };
        let mut anthropic = anthropic;
        let _ = web_tools::materialize_web_tool_uses(&mut anthropic);
        if let Some(id) = log_id.as_deref() {
            update_log_usage(
                &state,
                &provider,
                id,
                extract_usage_from_json(&serde_json::to_vec(&anthropic).unwrap_or_default()),
            );
        }
        remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
        if incoming_stream {
            return Response::builder().status(status).header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(convert::anthropic_message_to_sse(&anthropic)))
                .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造流式响应失败"));
        }
        return Response::builder().status(status).header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&anthropic).unwrap_or_default()))
            .unwrap_or_else(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "构造响应失败"));
    }

    // Build the response, preserving upstream headers. Non-streaming responses are
    // inspected directly; streaming responses update usage when the final SSE
    // message_delta event arrives.
    let mut resp_builder = Response::builder().status(status);
    for (name, value) in upstream_resp.headers() {
        if is_hop_by_hop_header(name.as_str()) {
            continue;
        }
        resp_builder = resp_builder.header(name, value);
    }

    let is_streaming = upstream_resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));

    if !is_streaming {
        let response_bytes = match upstream_resp.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "network",
                    "读取 Anthropic 上游响应失败",
                );
                return json_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {e}"));
            }
        };
        let response_bytes = if let Ok(mut json) = serde_json::from_slice::<Value>(&response_bytes) {
            if web_tools::materialize_web_tool_uses(&mut json) {
                Bytes::from(serde_json::to_vec(&json).unwrap_or_else(|_| response_bytes.to_vec()))
            } else {
                response_bytes
            }
        } else {
            response_bytes
        };
        if !status.is_success() {
            update_log_diagnostic(
                &state,
                log_id.as_deref(),
                upstream_error_category(status),
                &sanitized_upstream_diagnostic(status, &response_bytes),
            );
        }
        if let Some(id) = log_id.as_deref() {
            update_log_usage(&state, &provider, id, extract_usage_from_json(&response_bytes));
        }
        if status.is_success() {
            remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
        }
        return resp_builder
            .body(Body::from(response_bytes))
            .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")));
    }

    let db = Arc::clone(&state.db);
    let sse_buffer = Vec::new();
    let target_app = state.target.as_str().to_string();
    let provider_id = provider.id.clone();
    let idle = Duration::from_secs(
        state
            .db
            .with_conn(load_streaming_idle_timeout_secs)
            .unwrap_or(DEFAULT_STREAMING_IDLE_TIMEOUT_SECS),
    );
    let upstream_stream = upstream_resp.bytes_stream();
    let stream_log_id = log_id.clone();
    let client_model = client_model.clone();
    let stream = futures_util::stream::unfold(
        (
            upstream_stream,
            sse_buffer,
            false,
            PassthroughCursor::default(),
        ),
        move |(mut upstream_stream, mut sse_buffer, done, mut cursor)| {
            let db = Arc::clone(&db);
            let target_app = target_app.clone();
            let provider_id = provider_id.clone();
            let stream_log_id = stream_log_id.clone();
            let client_model = client_model.clone();
            async move {
                loop {
                    if done {
                        return None;
                    }
                    let (output, done) =
                        match tokio::time::timeout(idle, upstream_stream.next()).await {
                            Ok(Some(Ok(bytes))) => {
                                sse_buffer.extend_from_slice(&bytes);
                                let drained = drain_passthrough_frames(&mut sse_buffer, &mut cursor);
                                let mut output = Vec::new();
                                for frame in &drained.frames {
                                    output.extend_from_slice(frame);
                                    if let Some(json) = sse_frame_data_json(frame) {
                                        response_lifecycle::record_response_event(
                                            &db, stream_log_id.as_deref(), &json,
                                        );
                                    }
                                    record_passthrough_usage(
                                        &db,
                                        stream_log_id.as_deref(),
                                        &target_app,
                                        &provider_id,
                                        frame,
                                    );
                                }
                                if drained.terminal_error {
                                    let message = drained
                                        .terminal_message
                                        .as_deref()
                                        .unwrap_or("上游服务返回流式错误");
                                    mark_passthrough_midstream_error(
                                        &db,
                                        stream_log_id.as_deref(),
                                        "midstream_error",
                                        message,
                                    );
                                    output.extend(passthrough_abort(&client_model, &cursor, message));
                                    (output, true)
                                } else {
                                    if drained.finished {
                                        if let Some(id) = stream_log_id.as_deref() {
                                            let _ = db.with_conn(|conn| update_proxy_log_stream_outcome(
                                                conn, id, "complete", None, None, None,
                                            ));
                                        }
                                    }
                                    (output, drained.finished)
                                }
                            }
                            Ok(Some(Err(_))) | Ok(None) => {
                                if cursor.saw_message_stop {
                                    return None;
                                }
                                sse_buffer.clear();
                                mark_passthrough_midstream_error(
                                    &db,
                                    stream_log_id.as_deref(),
                                    "midstream_error",
                                    "上游流式响应中途中断",
                                );
                                (
                                    passthrough_abort(&client_model, &cursor, "上游流式响应中途中断"),
                                    true,
                                )
                            }
                            Err(_) => {
                                if cursor.saw_message_stop {
                                    return None;
                                }
                                sse_buffer.clear();
                                mark_passthrough_midstream_error(
                                    &db,
                                    stream_log_id.as_deref(),
                                    "timeout",
                                    "流式响应空闲超时",
                                );
                                (
                                    passthrough_abort(&client_model, &cursor, "流式响应空闲超时"),
                                    true,
                                )
                            }
                        };
                    if output.is_empty() && !done {
                        continue;
                    }
                    if output.is_empty() && done {
                        return None;
                    }
                    return Some((
                        Ok::<Bytes, Infallible>(Bytes::from(output)),
                        (upstream_stream, sse_buffer, done, cursor),
                    ));
                }
            }
        },
    );
    let body = response_lifecycle::track_stream_body(
        Body::from_stream(stream), Arc::clone(&state.db), log_id.clone(),
    );
    if status.is_success() {
        remember_gateway_success_upstream(&state, &headers, &incoming, &provider.id, is_catalog_subagent);
    }

    resp_builder
        .body(body)
        .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")))
}

#[derive(Debug, Default)]
struct PassthroughCursor {
    saw_message_start: bool,
    saw_message_stop: bool,
    open_blocks: Vec<usize>,
    next_block_index: usize,
    emitted_body: bool,
}

struct PassthroughDrain {
    frames: Vec<Vec<u8>>,
    /// Upstream `event: error` (or `data.type=error`) replaced by a normal close.
    terminal_error: bool,
    /// Human-readable error returned by the upstream, when present.
    terminal_message: Option<String>,
    /// `message_stop` already forwarded; do not read further.
    finished: bool,
}

fn drain_passthrough_frames(buffer: &mut Vec<u8>, cursor: &mut PassthroughCursor) -> PassthroughDrain {
    let mut frames = Vec::new();
    while let Some((end, delimiter_len)) = find_sse_frame_end(buffer) {
        let frame = buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
        if passthrough_frame_is_terminal_error(&frame) {
            let message = passthrough_frame_error_message(&frame);
            buffer.clear();
            if cursor.saw_message_stop {
                return PassthroughDrain {
                    frames,
                    terminal_error: false,
                    terminal_message: None,
                    finished: true,
                };
            }
            return PassthroughDrain {
                frames,
                terminal_error: true,
                terminal_message: message,
                finished: true,
            };
        }
        note_passthrough_frame(&frame, cursor);
        frames.push(frame);
        if cursor.saw_message_stop {
            buffer.clear();
            return PassthroughDrain {
                frames,
                terminal_error: false,
                terminal_message: None,
                finished: true,
            };
        }
    }
    PassthroughDrain {
        frames,
        terminal_error: false,
        terminal_message: None,
        finished: false,
    }
}

fn passthrough_frame_is_terminal_error(frame: &[u8]) -> bool {
    if sse_frame_is_event(frame, "error") {
        return true;
    }
    sse_frame_data_json(frame)
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_string))
        .is_some_and(|kind| kind == "error")
}

fn passthrough_frame_error_message(frame: &[u8]) -> Option<String> {
    let value = sse_frame_data_json(frame)?;
    let error = value.get("error")?;
    error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

fn note_passthrough_frame(frame: &[u8], cursor: &mut PassthroughCursor) {
    let json = sse_frame_data_json(frame);
    let kind = json
        .as_ref()
        .and_then(|value| value.get("type").and_then(Value::as_str))
        .map(str::to_string);
    if sse_frame_is_event(frame, "message_start") || kind.as_deref() == Some("message_start") {
        cursor.saw_message_start = true;
    }
    if sse_frame_is_event(frame, "message_stop") || kind.as_deref() == Some("message_stop") {
        cursor.saw_message_stop = true;
    }
    if sse_frame_is_event(frame, "content_block_start") || kind.as_deref() == Some("content_block_start")
    {
        let index = json
            .as_ref()
            .and_then(|value| value.get("index").and_then(Value::as_u64))
            .map(|index| index as usize)
            .unwrap_or(cursor.next_block_index);
        if !cursor.open_blocks.contains(&index) {
            cursor.open_blocks.push(index);
        }
        cursor.next_block_index = cursor.next_block_index.max(index.saturating_add(1));
    }
    if sse_frame_is_event(frame, "content_block_stop") || kind.as_deref() == Some("content_block_stop")
    {
        if let Some(index) = json
            .as_ref()
            .and_then(|value| value.get("index").and_then(Value::as_u64))
        {
            let index = index as usize;
            cursor.open_blocks.retain(|open| *open != index);
            cursor.next_block_index = cursor.next_block_index.max(index.saturating_add(1));
        }
    }
    if sse_frame_is_event(frame, "content_block_delta") || kind.as_deref() == Some("content_block_delta")
    {
        let text = json.as_ref().and_then(|value| {
            value
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        });
        if text.is_some() {
            cursor.emitted_body = true;
        }
    }
}

fn sse_frame_data_json(frame: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(frame).ok()?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "[DONE]")
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return None;
    }
    serde_json::from_str(&data).ok()
}

fn passthrough_abort(model: &str, cursor: &PassthroughCursor, message: &str) -> Vec<u8> {
    convert::anthropic_sse_abort(
        message,
        model,
        cursor.saw_message_start,
        &cursor.open_blocks,
        cursor.next_block_index,
        cursor.emitted_body,
    )
}

fn record_passthrough_usage(
    db: &Database,
    log_id: Option<&str>,
    target_app: &str,
    provider_id: &str,
    frame: &[u8],
) {
    let Some(id) = log_id else {
        return;
    };
    let Some(usage) = extract_usage_from_sse(frame) else {
        return;
    };
    if let Err(error) = db.with_conn(|conn| {
        update_proxy_log_usage_idempotent(
            conn,
            id,
            Some(target_app),
            Some(provider_id),
            usage.envelope_id.as_deref(),
            usage.input_tokens,
            usage.cache_read_input_tokens,
            usage.cache_creation_input_tokens,
            usage.output_tokens,
        )
    }) {
        log::error!("更新代理请求 Token 用量失败: {error}");
    } else {
        crate::usage_events::notify_log_recorded();
    }
}

fn sse_frame_is_event(frame: &[u8], name: &str) -> bool {
    let Ok(text) = std::str::from_utf8(frame) else {
        return false;
    };
    text.lines().any(|line| {
        line.strip_prefix("event:")
            .map(str::trim)
            == Some(name)
    })
}

fn mark_passthrough_midstream_error(
    db: &Database,
    log_id: Option<&str>,
    error_category: &str,
    diagnostic: &str,
) {
    let Some(id) = log_id else {
        return;
    };
    let _ = db.with_conn(|conn| {
        update_proxy_log_stream_outcome(
            conn,
            id,
            "midstream_error",
            None,
            Some(error_category),
            Some(diagnostic),
        )
    });
}

fn is_claude_desktop_inference_probe(
    target: ProviderTarget,
    uri_path: &str,
    incoming: &Value,
    incoming_stream: bool,
) -> bool {
    let is_desktop = target == ProviderTarget::ClaudeDesktop
        || uri_path.starts_with(crate::config::claude_desktop::CLAUDE_DESKTOP_PROXY_PREFIX);
    if !is_desktop || incoming_stream {
        return false;
    }
    let max_tokens = incoming.get("max_tokens").and_then(Value::as_i64).unwrap_or(0);
    if max_tokens != 1 {
        return false;
    }
    let Some(messages) = incoming.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.len() != 1 {
        return false;
    }
    let Some(first_msg) = messages.first() else {
        return false;
    };
    if first_msg.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let content = first_msg.get("content");
    let is_dot_str = content.and_then(Value::as_str) == Some(".");
    let is_dot_array = content.and_then(Value::as_array).is_some_and(|blocks| {
        blocks.len() == 1
            && blocks[0].get("text").and_then(Value::as_str) == Some(".")
    });
    is_dot_str || is_dot_array
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    fn detects_claude_desktop_probe_request() {
        let probe_json = serde_json::json!({
            "model": "claude-haiku-4-5",
            "max_tokens": 1,
            "messages": [
                {
                    "role": "user",
                    "content": "."
                }
            ]
        });
        assert!(is_claude_desktop_inference_probe(
            ProviderTarget::ClaudeDesktop,
            "/v1/messages",
            &probe_json,
            false
        ));
        assert!(is_claude_desktop_inference_probe(
            ProviderTarget::ClaudeDesktop,
            "/claude-desktop/v1/messages",
            &probe_json,
            false
        ));
        // Stream should not be treated as probe
        assert!(!is_claude_desktop_inference_probe(
            ProviderTarget::ClaudeDesktop,
            "/v1/messages",
            &probe_json,
            true
        ));
        // Claude Code target should not trigger desktop probe
        assert!(!is_claude_desktop_inference_probe(
            ProviderTarget::ClaudeCode,
            "/v1/messages",
            &probe_json,
            false
        ));
        // Real user query should not trigger probe
        let normal_chat = serde_json::json!({
            "model": "claude-sonnet-5",
            "max_tokens": 4096,
            "messages": [
                {
                    "role": "user",
                    "content": "Hello world"
                }
            ]
        });
        assert!(!is_claude_desktop_inference_probe(
            ProviderTarget::ClaudeDesktop,
            "/v1/messages",
            &normal_chat,
            false
        ));
    }

    #[test]
    fn sse_frame_is_event_reads_event_line() {
        assert!(sse_frame_is_event(
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            "message_stop"
        ));
        assert!(sse_frame_is_event(
            b"event:message_start\ndata: {}\n\n",
            "message_start"
        ));
        assert!(!sse_frame_is_event(
            b"event: message_delta\ndata: {}\n\n",
            "message_stop"
        ));
    }

    #[test]
    fn passthrough_holds_partial_frame() {
        let mut buffer = b"event: message_start\ndata: {\"type\":\"message_start\"}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\"".to_vec();
        let mut cursor = PassthroughCursor::default();
        let drained = drain_passthrough_frames(&mut buffer, &mut cursor);
        let terminal_error = drained.terminal_error;
        let forwarded = String::from_utf8(drained.frames.into_iter().flatten().collect()).unwrap();
        assert!(!terminal_error);
        assert!(forwarded.contains("message_start"));
        assert!(!forwarded.contains("content_block_delta"));
        assert!(cursor.saw_message_start);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn passthrough_replaces_error_event_and_closes_open_block() {
        let mut buffer = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"model down\"}}\n\n",
        )
        .as_bytes()
        .to_vec();
        let mut cursor = PassthroughCursor::default();
        let drained = drain_passthrough_frames(&mut buffer, &mut cursor);
        let forwarded = String::from_utf8(drained.frames.iter().flatten().copied().collect()).unwrap();
        assert!(drained.terminal_error);
        assert!(buffer.is_empty());
        assert!(forwarded.contains("message_start"));
        assert!(forwarded.contains("content_block_start"));
        assert!(!forwarded.contains("event: error"));
        assert_eq!(cursor.open_blocks, vec![0]);
        let abort = String::from_utf8(passthrough_abort(
            "gpt-5.6-sol",
            &cursor,
            drained.terminal_message.as_deref().unwrap_or("上游服务返回流式错误"),
        ))
        .unwrap();
        assert!(abort.contains("\"index\":0"));
        assert!(abort.contains("model down"));
        assert!(abort.contains("event: message_stop"));
        assert!(!abort.contains("event: message_start"));
        assert!(!abort.contains("event: error"));
        let fresh = String::from_utf8(passthrough_abort(
            "gpt-5.6-sol",
            &PassthroughCursor::default(),
            "上游流式响应中断",
        ))
        .unwrap();
        assert!(fresh.contains("\"model\":\"gpt-5.6-sol\""));
        assert!(!fresh.contains("\"model\":\"proxy\""));
        assert!(fresh.contains("event: message_stop"));
        assert!(!fresh.contains("event: error"));
    }
}
