async fn proxy_handler(
    State(mut state): State<ProxyState>,
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    body: Body,
) -> Response {
    if let Err(error) = validate_listener_auth(&state, &headers) {
        return gateway_auth_error(error);
    }
    if state.listener_kind == ListenerKind::SmartGateway {
        if let Some(target) = resolve_binding_target(&state, &headers) {
            state.target = target;
        }
    }
    let hop = match state.listener_kind {
        ListenerKind::SmartGateway => crate::gateway::correlation::HOP_SMART_GATEWAY,
        ListenerKind::Agent => crate::gateway::correlation::HOP_AGENT_PROXY,
    };
    state.correlation = Some(crate::gateway::correlation::resolve(
        &headers,
        hop,
        Some(state.target.as_str()),
    ));
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
        provider.api_key = match state.db.with_conn(|conn| resolve_api_key(conn, &provider.id)) {
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
    let mut body_bytes = Bytes::from(serde_json::to_vec(&incoming).unwrap_or_else(|_| body_bytes.to_vec()));
    let mut requested_model = incoming
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut is_catalog_subagent = false;
    let mut route_decision: Option<crate::gateway::RouteDecision> = None;
    let mut route_plan: Option<crate::gateway::RouteExecutionPlan> = None;
    let mut attempt_index: i64 = 0;
    if gateway_catalog_enabled(&state) {
        let force_subagent = is_claude_code_subagent_request(&headers, &requested_model);
        match select_gateway_runtime_provider_with(&state, &requested_model, force_subagent, &incoming, uri.path()) {
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
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "无法读取模型目录");
            }
        }
    }
    let prepared = match prepare_upstream_request(&state, &mut provider, &method, &headers, &incoming, &body_bytes, incoming_stream) {
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
    let mut upstream_resp = match apply_catalog_subagent_signal(prepared.builder, is_catalog_subagent)
        .body(prepared.outgoing_body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            record_provider_failure(&state, &provider.id);
            failover_trace.push(format!("{}({}) 网络错误", provider.name, provider.id));
            let mut excluded = vec![provider.id.clone()];
            let mut last_error = e.to_string();
            let mut recovered = None::<reqwest::Response>;
            for _ in 0..FAILOVER_MAX_HOPS {
                let Some(mut fallback) =
                    next_failover_provider(&state, &excluded, &requested_model)
                        .ok()
                        .flatten()
                else {
                    break;
                };
                excluded.push(fallback.id.clone());
                log::warn!(
                    "供应商 {} 网络请求失败，尝试故障切换到 {}",
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
                ) else {
                    continue;
                };
                translated = fallback_prepared.translated;
                retry_without_stream_options =
                    compatible_stream_retry(&fallback, &fallback_prepared, incoming_stream);
                match apply_catalog_subagent_signal(
                    fallback_prepared.builder,
                    is_catalog_subagent,
                )
                .body(fallback_prepared.outgoing_body)
                .send()
                .await
                {
                    Ok(response) => {
                        failover_trace.push(format!("{}({}) 接管", fallback.name, fallback.id));
                        provider = fallback;
                        attempt_index = attempt_index.saturating_add(1);
                        recovered = Some(response);
                        break;
                    }
                    Err(fallback_error) => {
                        record_provider_failure(&state, &fallback.id);
                        failover_trace.push(format!("{}({}) 失败: {fallback_error}", fallback.name, fallback.id));
                        last_error = fallback_error.to_string();
                    }
                }
            }
            match recovered {
                Some(response) => response,
                None => {
                    let failover_diag = if !failover_trace.is_empty() {
                        Some(format!("故障降级失败: {}", failover_trace.join(" → ")))
                    } else {
                        None
                    };
                    log_request_with_diagnostic(
                        &state,
                        &provider,
                        Some(502),
                        started.elapsed().as_millis() as i64,
                        uri.path(),
                        incoming_stream,
                        Some("network"),
                        failover_diag.as_deref(),
                    );
                    if translated {
                        log::warn!("转发到 OpenAI 兼容上游失败: {last_error}");
                        return anthropic_error(
                            StatusCode::BAD_GATEWAY,
                            convert::openai_error_to_anthropic(502),
                        );
                    }
                    return json_error(
                        StatusCode::BAD_GATEWAY,
                        format!("转发到上游失败: {last_error}"),
                    );
                }
            }
        }
    };

    if is_retryable_upstream_status(&state, upstream_resp.status())
        && should_failover_upstream_status(&provider, upstream_resp.status())
    {
        // Ordered model fallback on the same provider before walking other vendors.
        // Only used before any client bytes are written.
        let plan_models: Vec<String> = route_plan
            .as_ref()
            .filter(|plan| plan.fallback_mode == "model_chain")
            .map(|plan| {
                plan.attempts
                    .iter()
                    .skip(1)
                    .map(|attempt| attempt.model.clone())
                    .collect()
            })
            .unwrap_or_default();
        let model_chain = if plan_models.is_empty() {
            provider.failover_models.clone()
        } else {
            plan_models
        };
        for next_model in model_chain {
            let next_model = next_model.trim().to_string();
            if next_model.is_empty() || next_model.eq_ignore_ascii_case(&requested_model) {
                continue;
            }
            if let Some(object) = incoming.as_object_mut() {
                object.insert("model".to_string(), Value::String(next_model.clone()));
            }
            body_bytes = Bytes::from(rewrite_json_model(&body_bytes, &next_model));
            requested_model = next_model.clone();
            let Ok(fallback_prepared) = prepare_upstream_request(
                &state,
                &mut provider,
                &method,
                &headers,
                &incoming,
                &body_bytes,
                incoming_stream,
            ) else {
                continue;
            };
            translated = fallback_prepared.translated;
            retry_without_stream_options =
                compatible_stream_retry(&provider, &fallback_prepared, incoming_stream);
            match apply_catalog_subagent_signal(fallback_prepared.builder, is_catalog_subagent)
                .body(fallback_prepared.outgoing_body)
                .send()
                .await
            {
                Ok(response) if !is_retryable_upstream_status(&state, response.status()) => {
                    failover_trace.push(format!("{} 模型 {} 接管", provider.name, next_model));
                    attempt_index = attempt_index.saturating_add(1);
                    upstream_resp = response;
                    break;
                }
                Ok(response) => {
                    failover_trace.push(format!(
                        "{} 模型 {} 状态码 {}",
                        provider.name,
                        next_model,
                        response.status()
                    ));
                    upstream_resp = response;
                }
                Err(error) => {
                    failover_trace.push(format!("{} 模型 {} 失败: {error}", provider.name, next_model));
                }
            }
        }
    }

    if is_retryable_upstream_status(&state, upstream_resp.status())
        && should_failover_upstream_status(&provider, upstream_resp.status())
    {
        record_provider_failure(&state, &provider.id);
        failover_trace.push(format!("{}({}) 状态码 {}", provider.name, provider.id, upstream_resp.status()));
        let mut excluded = vec![provider.id.clone()];
        for _ in 0..FAILOVER_MAX_HOPS {
            let Some(mut fallback) =
                next_failover_provider(&state, &excluded, &requested_model)
                    .ok()
                    .flatten()
            else {
                break;
            };
            excluded.push(fallback.id.clone());
            log::warn!(
                "供应商 {} 返回 {}，尝试故障切换到 {}",
                provider.id,
                upstream_resp.status(),
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
            ) else {
                continue;
            };
            let fallback_translated = fallback_prepared.translated;
            let fallback_retry =
                compatible_stream_retry(&fallback, &fallback_prepared, incoming_stream);
            match apply_catalog_subagent_signal(fallback_prepared.builder, is_catalog_subagent)
                .body(fallback_prepared.outgoing_body)
                .send()
                .await
            {
                Ok(response) => {
                    provider = fallback;
                    translated = fallback_translated;
                    retry_without_stream_options = fallback_retry;
                    upstream_resp = response;
                    if !is_retryable_upstream_status(&state, upstream_resp.status()) {
                        failover_trace.push(format!("{}({}) 接管成功", provider.name, provider.id));
                        break;
                    }
                    record_provider_failure(&state, &provider.id);
                    failover_trace.push(format!("{}({}) 状态码 {}", provider.name, provider.id, upstream_resp.status()));
                }
                Err(error) => {
                    record_provider_failure(&state, &fallback.id);
                    failover_trace.push(format!("{}({}) 连接失败: {error}", fallback.name, fallback.id));
                    log::warn!("备用供应商 {} 连接失败: {error}", fallback.id);
                }
            }
        }
    }

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
                upstream_resp = match retry_request.send().await {
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
                return anthropic_error(
                    rejected_status,
                    convert::openai_error_to_anthropic(rejected_status.as_u16()),
                );
            }
        }
    }

    let status = upstream_resp.status();
    if status.is_success() {
        record_provider_success(&state, &provider.id);
    }
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
    if let (Some(id), Some(decision)) = (log_id.as_deref(), route_decision.as_ref()) {
        patch_route_log(&state, id, decision, attempt_index);
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
            let converter = convert::OpenAiSseConverter::new(protocol, provider.model.trim());
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
                                for item in decoder.push(&bytes) {
                                    match item {
                                        UpstreamSseItem::Json(event) => {
                                            output.extend(converter.push_event(&event))
                                        }
                                        UpstreamSseItem::Done => {
                                            output.extend(converter.finish_stream())
                                        }
                                    }
                                }
                                (output, false)
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
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header("x-accel-buffering", "no")
                .body(Body::from_stream(stream))
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
                record_provider_failure(&state, &provider.id);
                update_log_diagnostic(
                    &state,
                    log_id.as_deref(),
                    "upstream",
                    failed
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Responses status=failed"),
                );
                let mut excluded = vec![provider.id.clone()];
                for _ in 0..FAILOVER_MAX_HOPS {
                    let Some(mut fallback) =
                        next_failover_provider(&state, &excluded, &requested_model)
                            .ok()
                            .flatten()
                    else {
                        break;
                    };
                    excluded.push(fallback.id.clone());
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
                    ) else {
                        continue;
                    };
                    match fallback_prepared
                        .builder
                        .body(fallback_prepared.outgoing_body)
                        .send()
                        .await
                    {
                        Ok(response) if response.status().is_success() => {
                            let fallback_bytes = match response.bytes().await {
                                Ok(bytes) => bytes,
                                Err(_) => {
                                    record_provider_failure(&state, &fallback.id);
                                    continue;
                                }
                            };
                            let fallback_json: Value = match serde_json::from_slice(&fallback_bytes)
                            {
                                Ok(value) => value,
                                Err(_) => {
                                    record_provider_failure(&state, &fallback.id);
                                    continue;
                                }
                            };
                            if fallback.protocol_type == ProtocolType::OpenAiResponses
                                && convert::responses_failed_anthropic_error(&fallback_json)
                                    .is_some()
                            {
                                record_provider_failure(&state, &fallback.id);
                                continue;
                            }
                            provider = fallback;
                            let anthropic = match provider.protocol_type {
                                ProtocolType::OpenAiResponses => {
                                    convert::openai_responses_to_anthropic(
                                        &fallback_json,
                                        provider.model.trim(),
                                    )
                                }
                                _ => convert::openai_chat_to_anthropic(
                                    &fallback_json,
                                    provider.model.trim(),
                                ),
                            };
                            record_provider_success(&state, &provider.id);
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
                            record_provider_failure(&state, &fallback.id);
                        }
                    }
                }
                return anthropic_error(StatusCode::BAD_GATEWAY, failed);
            }
        }
        let anthropic = match provider.protocol_type {
            ProtocolType::OpenAiResponses => convert::openai_responses_to_anthropic(&upstream, provider.model.trim()),
            _ => convert::openai_chat_to_anthropic(&upstream, provider.model.trim()),
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
    let stream = futures_util::stream::unfold(
        (upstream_stream, sse_buffer, false),
        move |(mut upstream_stream, mut sse_buffer, done)| {
            let db = Arc::clone(&db);
            let target_app = target_app.clone();
            let provider_id = provider_id.clone();
            let stream_log_id = stream_log_id.clone();
            async move {
                if done {
                    return None;
                }
                match tokio::time::timeout(idle, upstream_stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        if let Some(id) = stream_log_id.as_deref() {
                            sse_buffer.extend_from_slice(&bytes);
                            while let Some(end) =
                                sse_buffer.windows(2).position(|window| window == b"\n\n")
                            {
                                let event = sse_buffer.drain(..end + 2).collect::<Vec<_>>();
                                if let Some(usage) = extract_usage_from_sse(&event) {
                                    if let Err(e) = db.with_conn(|conn| {
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
                                        log::error!("更新代理请求 Token 用量失败: {e}");
                                    } else {
                                        crate::usage_events::notify_log_recorded();
                                    }
                                }
                            }
                        }
                        Some((
                            Ok::<Bytes, reqwest::Error>(bytes),
                            (upstream_stream, sse_buffer, false),
                        ))
                    }
                    Ok(Some(Err(error))) => Some((Err(error), (upstream_stream, sse_buffer, true))),
                    Ok(None) => None,
                    Err(_) => {
                        let message = convert::OpenAiSseConverter::new(
                            convert::OpenAiStreamProtocol::Chat,
                            "proxy",
                        )
                        .error_event("流式响应空闲超时");
                        Some((
                            Ok(Bytes::from(message)),
                            (upstream_stream, sse_buffer, true),
                        ))
                    }
                }
            }
        },
    );
    let body = Body::from_stream(stream);

    resp_builder
        .body(body)
        .unwrap_or_else(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("构造响应失败: {e}")))
}
