//! 出站尝试的健康门禁和结果记录；仅在响应交给客户端之前执行。

use super::*;
use crate::gateway::health::{self, FailureKind};
use crate::gateway::upstream_limits::Permit;
use super::first_output::{FirstOutputError, FirstOutputProbe};

#[derive(Debug, Clone)]
struct LocalAttemptFailure {
    category: String,
    message: String,
}

fn local_failure(status: StatusCode, category: &str, message: &str, retry_after: Option<u64>) -> reqwest::Response {
    let mut builder = http::Response::builder().status(status)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(seconds) = retry_after {
        builder = builder.header(header::RETRY_AFTER, seconds.max(1).to_string());
    }
    let mut response: reqwest::Response = builder.body(reqwest::Body::from(
        serde_json::json!({"error":{"type":category,"message":message}}).to_string(),
    )).expect("固定的本地错误响应有效").into();
    response.extensions_mut().insert(LocalAttemptFailure { category: category.into(), message: message.into() });
    response
}

fn first_output_failure(provider: &Provider, error: FirstOutputError) -> reqwest::Response {
    if !provider.is_antigravity() && !provider.is_kiro() {
        health::record_failure(&provider.id);
    }
    let status = if error == FirstOutputError::Timeout { StatusCode::GATEWAY_TIMEOUT } else { StatusCode::BAD_GATEWAY };
    local_failure(status, error.category(), error.message(), None)
}

async fn prefetch_first_output(response: &mut reqwest::Response) -> Result<Vec<Bytes>, FirstOutputError> {
    let mut probe = FirstOutputProbe::default();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let take = chunk.len().min(probe.remaining_capacity());
                if probe.push(&chunk[..take])? {
                    // 网络 chunk 可大于预读上限；只解析上限内字节，剩余部分零拷贝回放。
                    return Ok(vec![probe.into_bytes(), chunk.slice(take..)]);
                }
                if take < chunk.len() { return Err(FirstOutputError::BufferLimit); }
            }
            Ok(None) | Err(_) => return Err(FirstOutputError::EarlyEof),
        }
    }
}

// 许可放在真正的正文流中，而非会在 bytes_stream() 时被丢弃的 extensions。
fn hold_upstream_response(response: reqwest::Response, permit: Option<Permit>, prefix: Vec<Bytes>) -> reqwest::Response {
    let mut builder = http::Response::builder().status(response.status()).version(response.version());
    *builder.headers_mut().expect("响应头存在") = response.headers().clone();
    let stream = response.bytes_stream();
    let stream = futures_util::stream::unfold((Box::pin(stream), permit, prefix.into_iter()),
        |(mut stream, permit, mut prefix)| async move {
            for bytes in prefix.by_ref() {
                if !bytes.is_empty() {
                    return Some((Ok(bytes), (stream, permit, prefix)));
                }
            }
            stream.next().await.map(|chunk| (chunk, (stream, permit, prefix)))
        });
    builder.body(reqwest::Body::wrap_stream(stream)).expect("上游响应有效").into()
}

#[derive(Debug, Clone, Copy)]
struct ObservedFailure(FailureKind);

pub(crate) fn should_try_explicit_response(provider: &Provider, response: &reqwest::Response) -> bool {
    should_try_explicit_fallback(provider, response.status())
        || response.extensions().get::<ObservedFailure>()
            .is_some_and(|failure| failure.0 == FailureKind::ModelUnsupported)
}

pub(crate) async fn send_observed_upstream_with_timing(
    builder: reqwest::RequestBuilder,
    provider: &Provider,
    state: Option<&ProxyState>,
    streaming: bool,
) -> (Result<reqwest::Response, reqwest::Error>, Option<i64>) {
    let mut queue_wait_ms = None;
    let result = send_observed_upstream_inner(builder, provider, state, streaming, &mut queue_wait_ms).await;
    (result, queue_wait_ms)
}

async fn send_observed_upstream_inner(
    builder: reqwest::RequestBuilder,
    provider: &Provider,
    state: Option<&ProxyState>,
    streaming: bool,
    queue_wait_ms: &mut Option<i64>,
) -> Result<reqwest::Response, reqwest::Error> {
    // 短冷却在提交客户端响应前等待；长冷却直接交给备用候选或客户端。
    if !health::is_available(&provider.id, Some(&provider.model)) {
        if let Some(wait) = health::min_cooldown_remaining(&provider.id, Some(&provider.model)) {
            if wait <= Duration::from_secs(10) {
                tokio::time::sleep(wait).await;
            }
        }
    }
    let admission = if let Some(state) = state.filter(|state| state.listener_kind == ListenerKind::SmartGateway) {
        let queued_at = Instant::now();
        let result = state.db.gateway_upstream_limiter.acquire(&provider.id).await;
        *queue_wait_ms = Some(queued_at.elapsed().as_millis().min(i64::MAX as u128) as i64);
        match result {
            Ok(permit) => Some(permit),
            Err(error) => return Ok(local_failure(StatusCode::TOO_MANY_REQUESTS,
                error.category(), error.message(), Some(error.retry_after_secs()))),
        }
    } else { None };
    let mut admission = admission;
    let Some(_permit) = health::acquire_permit(&provider.id, Some(&provider.model)) else {
        let retry_after = health::min_cooldown_remaining_secs(&provider.id, Some(&provider.model))
            .unwrap_or(1).max(1);
        return Ok(http::Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::RETRY_AFTER, retry_after.to_string())
            .body(reqwest::Body::from(r#"{"error":{"type":"rate_limit_error","message":"上游正在冷却，请稍后重试"}}"#))
            .expect("固定的冷却响应有效")
            .into());
    };
    let started = Instant::now();
    if let Some(permit) = admission.as_mut() {
        permit.commit_outbound();
    }
    let timeout_ms = if streaming { admission.as_ref().map(Permit::first_output_timeout_ms).unwrap_or(0) } else { 0 };
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    let result = if timeout_ms > 0 {
        match tokio::time::timeout_at(deadline, builder.send()).await {
            Ok(result) => result,
            Err(_) => return Ok(first_output_failure(provider, FirstOutputError::Timeout)),
        }
    } else {
        builder.send().await
    };
    let mut response = match result {
        Ok(response) => response,
        Err(error) => {
            // 本机反代管理自己的账号池，网络连接失败不应给账号或整个反代冷却。
            if !provider.is_antigravity() && !provider.is_kiro() {
                health::record_failure(&provider.id);
            }
            return Err(error);
        }
    };
    if response.status().is_success() {
        let prefix = if timeout_ms > 0 {
            match tokio::time::timeout_at(deadline, prefetch_first_output(&mut response)).await {
                Ok(Ok(prefix)) => prefix,
                Ok(Err(error)) => return Ok(first_output_failure(provider, error)),
                Err(_) => return Ok(first_output_failure(provider, FirstOutputError::Timeout)),
            }
        } else { Vec::new() };
        health::record_model_success(&provider.id, &provider.model, Some(started.elapsed().as_millis() as i64));
        return Ok(if admission.is_some() || !prefix.is_empty() {
            hold_upstream_response(response, admission, prefix)
        } else { response });
    }
    let status = response.status();
    let headers = response.headers().clone();
    let version = response.version();
    let mut body = Vec::new();
    // 只缓冲有界错误正文；成功流不缓冲，也不重复消费客户端流。
    while let Some(chunk) = response.chunk().await? {
        let remaining = MAX_UPSTREAM_ERROR_BYTES.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if body.len() >= MAX_UPSTREAM_ERROR_BYTES { break; }
    }
    let text = String::from_utf8_lossy(&body);
    let kind = if provider.is_antigravity() || provider.is_kiro() {
        // 429/504/5xx 已在反代内部处理，不能在入口再封掉整个账号池。
        let classified = health::classify_failure(status.as_u16(), Some(&headers), Some(&text));
        if matches!(classified.kind, FailureKind::Auth | FailureKind::ModelUnsupported) {
            health::record_http_failure(&provider.id, Some(&provider.model), status.as_u16(), Some(&headers), Some(&text));
        }
        classified.kind
    } else {
        health::record_http_failure(&provider.id, Some(&provider.model), status.as_u16(), Some(&headers), Some(&text))
    };
    let mut rebuilt = http::Response::builder().status(status).version(version);
    *rebuilt.headers_mut().expect("响应头存在") = headers;
    rebuilt.headers_mut().expect("响应头存在").remove(header::CONTENT_LENGTH);
    rebuilt.headers_mut().expect("响应头存在").remove(header::TRANSFER_ENCODING);
    let mut response: reqwest::Response = rebuilt.body(reqwest::Body::from(body))
        .expect("上游响应状态与头有效").into();
    response.extensions_mut().insert(ObservedFailure(kind));
    Ok(response)
}

pub(crate) fn build_request_attempt(
    attempt_index: usize,
    provider: &Provider,
    model: &str,
    duration_ms: i64,
    result: &Result<reqwest::Response, reqwest::Error>,
) -> crate::database::dao::proxy_logs::ProxyRequestAttempt {
    match result {
        Ok(response) => {
            let status_code = Some(response.status().as_u16());
            let success = response.status().is_success();
            let (error_category, diagnostic) = if let Some(local) = response.extensions().get::<LocalAttemptFailure>() {
                (Some(local.category.clone()), Some(local.message.clone()))
            } else if success {
                (None, None)
            } else {
                let failure_kind = response.extensions().get::<ObservedFailure>().map(|f| f.0);
                let (cat, diag) = match failure_kind {
                    Some(FailureKind::Auth) => (Some("auth"), Some("身份验证失败 (HTTP 401/403)")),
                    Some(FailureKind::Quota) => (Some("quota"), Some("额度不足或已用尽")),
                    Some(FailureKind::RateLimit) => (Some("rate_limit"), Some("请求频次超限 (HTTP 429)")),
                    Some(FailureKind::ModelUnsupported) => (Some("model_unsupported"), Some("模型不支持或不存在")),
                    Some(FailureKind::Transient) => (Some("transient"), Some("上游服务暂时不可用 (5xx)")),
                    Some(FailureKind::ClientErrorIgnored) => (Some("client_error"), Some("客户端请求错误")),
                    None => {
                        let cat = if response.status().is_server_error() {
                            Some("transient")
                        } else if response.status() == StatusCode::TOO_MANY_REQUESTS {
                            Some("rate_limit")
                        } else if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
                            Some("auth")
                        } else {
                            Some("upstream")
                        };
                        (cat, None)
                    }
                };
                (cat.map(String::from), diag.map(String::from))
            };
            crate::database::dao::proxy_logs::ProxyRequestAttempt::new(
                attempt_index,
                Some(provider.id.clone()),
                Some(provider.name.clone()),
                model.to_string(),
                status_code,
                duration_ms,
                error_category,
                diagnostic,
                success,
            )
        }
        Err(error) => {
            let diag = if error.is_timeout() {
                "网络请求超时"
            } else {
                "上游网络连接失败"
            };
            crate::database::dao::proxy_logs::ProxyRequestAttempt::new(
                attempt_index,
                Some(provider.id.clone()),
                Some(provider.name.clone()),
                model.to_string(),
                None,
                duration_ms,
                Some("network".to_string()),
                Some(diag.to_string()),
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider(id: &str, name: &str, model: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: "https://api.test".into(),
            api_key: "".into(),
            api_key_set: false,
            model: model.into(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: crate::provider::ProviderKind::Standard,
            auth_binding: "".into(),
            target_app: ProviderTarget::ClaudeCode,
            notes: "".into(),
            sort_index: 0,
            failover_group: 0,
            failover_models: vec![],
            hidden_models: vec![],
            thinking_config: None,
            custom_headers: None,
            model_mapping: Default::default(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    #[tokio::test]
    async fn admission_wait_is_preserved_on_local_rejection_and_network_error() {
        let db = Arc::new(Database::memory().unwrap());
        let provider = test_provider("queue-timing-isolated", "timing", "model");
        let state = ProxyState {
            db: Arc::clone(&db),
            client: Client::builder().no_proxy().build().unwrap(),
            codex_history: Arc::new(super::super::codex_history::CodexHistoryStore::default()),
            target: ProviderTarget::ClaudeCode,
            listener_kind: ListenerKind::SmartGateway,
            port: 0,
            started_at: Instant::now(),
            correlation: None,
            request_log: None,
            request_path: String::new(),
        };
        db.gateway_upstream_limiter.apply(&provider.id, crate::gateway::upstream_limits::UpstreamLimitPolicy {
            max_concurrency: 1,
            queue_timeout_ms: 30,
            ..Default::default()
        });
        let held = db.gateway_upstream_limiter.acquire(&provider.id).await.unwrap();
        let (result, wait) = send_observed_upstream_with_timing(
            state.client.post("http://127.0.0.1:1/"), &provider, Some(&state), false,
        ).await;
        assert_eq!(result.as_ref().unwrap().status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(wait.unwrap() >= 20);
        let attempt = build_request_attempt(0, &provider, "model", 30, &result).with_queue_wait_ms(wait);
        assert_eq!(attempt.error_category.as_deref(), Some("local_queue_timeout"));
        assert_eq!(attempt.queue_wait_ms, wait);
        drop(held);
        let (result, wait) = send_observed_upstream_with_timing(
            state.client.post("http://127.0.0.1:1/"), &provider, Some(&state), false,
        ).await;
        assert!(result.is_err());
        assert!(wait.is_some());
        let attempt = build_request_attempt(1, &provider, "model", 1, &result).with_queue_wait_ms(wait);
        assert_eq!(attempt.queue_wait_ms, wait);
        health::record_success(&provider.id, None);
    }

    #[test]
    fn legacy_attempt_without_queue_wait_remains_readable() {
        let attempt: crate::database::dao::proxy_logs::ProxyRequestAttempt = serde_json::from_value(
            serde_json::json!({"attemptIndex":0,"upstreamId":null,"providerName":null,
                "model":"old","statusCode":200,"durationMs":1,"errorCategory":null,
                "diagnostic":null,"success":true}),
        ).unwrap();
        assert_eq!(attempt.queue_wait_ms, None);
    }

    #[tokio::test]
    async fn first_output_loopback_replays_three_protocols_and_holds_permit() {
        use axum::{routing::post, Router};
        for (index, payload) in [
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"thinking\":\"想\"}}\r\n\r\ndata: {\"type\":\"message_stop\"}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"name\":\"read\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"好\"}\n\ndata: {\"type\":\"response.completed\"}\n\n",
        ].iter().enumerate() {
            let payload = Bytes::from_static(payload.as_bytes());
            let expected = payload.clone();
            let app = Router::new().route("/", post(move || {
                let payload = payload.clone();
                async move {
                    let stream = futures_util::stream::iter(payload.chunks(7)
                        .map(Bytes::copy_from_slice).map(Ok::<_, Infallible>).collect::<Vec<_>>());
                    Response::builder().header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from_stream(stream)).unwrap()
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            let task_guard = server.abort_handle();
            let mut response = Client::builder().no_proxy().build().unwrap()
                .post(format!("http://{addr}/")).send().await.unwrap();
            let prefix = prefetch_first_output(&mut response).await.unwrap();
            let limiter = crate::gateway::upstream_limits::UpstreamLimiter::new();
            let id = format!("replay_{index}");
            let mut permit = limiter.acquire(&id).await.unwrap();
            permit.commit_outbound();
            let response = hold_upstream_response(response, Some(permit), prefix);
            assert_eq!(limiter.snapshot(&id).unwrap().active, 1);
            assert_eq!(response.bytes().await.unwrap(), expected);
            assert_eq!(limiter.snapshot(&id).unwrap().active, 0);
            task_guard.abort();
        }
    }

    #[tokio::test]
    async fn first_output_ignores_heartbeats_and_rejects_early_eof() {
        let stream = futures_util::stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b": ping\n\ndata: {\"type\":\"message_start\"}\n\n"))
        }).chain(futures_util::stream::pending());
        let mut response: reqwest::Response = http::Response::new(reqwest::Body::wrap_stream(stream)).into();
        assert!(tokio::time::timeout(Duration::from_millis(30), prefetch_first_output(&mut response)).await.is_err());
        let mut response: reqwest::Response = http::Response::new(reqwest::Body::from(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"\"}}\n\n",
        )).into();
        assert_eq!(prefetch_first_output(&mut response).await, Err(FirstOutputError::EarlyEof));
    }

    #[tokio::test]
    async fn first_output_large_chunk_replays_tail_without_counting_it_as_prefetch() {
        let mut bytes = b"data: {\"type\":\"message_stop\"}\n\n".to_vec();
        bytes.extend(vec![b'x'; super::super::first_output::MAX_PREFETCH_BYTES]);
        let mut response: reqwest::Response = http::Response::new(reqwest::Body::from(bytes.clone())).into();
        let prefix = prefetch_first_output(&mut response).await.unwrap();
        assert_eq!(hold_upstream_response(response, None, prefix).bytes().await.unwrap().as_ref(), bytes);
    }

    #[test]
    fn build_request_attempt_records_success() {
        let provider = test_provider("up_1", "Test Upstream", "claude-3-7-sonnet");
        let http_resp = http::Response::builder()
            .status(StatusCode::OK)
            .body(reqwest::Body::from("ok"))
            .unwrap();
        let resp: reqwest::Response = http_resp.into();
        let result: Result<reqwest::Response, reqwest::Error> = Ok(resp);
        let attempt = build_request_attempt(0, &provider, "claude-3-7-sonnet", 123, &result);

        assert_eq!(attempt.attempt_index, 0);
        assert_eq!(attempt.upstream_id.as_deref(), Some("up_1"));
        assert_eq!(attempt.provider_name.as_deref(), Some("Test Upstream"));
        assert_eq!(attempt.model, "claude-3-7-sonnet");
        assert_eq!(attempt.status_code, Some(200));
        assert_eq!(attempt.duration_ms, 123);
        assert!(attempt.success);
        assert_eq!(attempt.error_category, None);
        assert_eq!(attempt.diagnostic, None);
    }

    #[test]
    fn build_request_attempt_records_observed_failure() {
        let provider = test_provider("up_2", "Rate Limited Provider", "claude-3-7-sonnet");
        let mut resp: reqwest::Response = http::Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .body(reqwest::Body::from("rate limited"))
            .unwrap()
            .into();
        resp.extensions_mut().insert(ObservedFailure(FailureKind::RateLimit));
        let result: Result<reqwest::Response, reqwest::Error> = Ok(resp);
        let attempt = build_request_attempt(1, &provider, "claude-3-7-sonnet", 85, &result);

        assert_eq!(attempt.attempt_index, 1);
        assert_eq!(attempt.upstream_id.as_deref(), Some("up_2"));
        assert_eq!(attempt.status_code, Some(429));
        assert!(!attempt.success);
        assert_eq!(attempt.error_category.as_deref(), Some("rate_limit"));
        assert_eq!(attempt.diagnostic.as_deref(), Some("请求频次超限 (HTTP 429)"));
    }

    #[tokio::test]
    async fn loopback_failover_records_attempts_sequence() {
        use axum::routing::post;
        use axum::Router;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        let app = Router::new().route(
            "/v1/messages",
            post(move || {
                let count = count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    if count == 0 {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [("content-type", "application/json")],
                            r#"{"error":{"type":"rate_limit_error","message":"rate limited"}}"#,
                        )
                    } else {
                        (
                            StatusCode::OK,
                            [("content-type", "application/json")],
                            r#"{"id":"msg_1","type":"message","content":[{"type":"text","text":"hello"}]}"#,
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let client = Client::builder().no_proxy().build().unwrap();
        let provider1 = test_provider("prov_1", "Prov 1", "claude-3-7-sonnet");
        let provider2 = test_provider("prov_2", "Prov 2", "claude-3-7-sonnet");

        let mut attempts = Vec::new();

        // Attempt 0 -> hitting loopback, returns 429
        let req1 = client.post(format!("http://{addr}/v1/messages"));
        let start1 = Instant::now();
        let res1 = send_observed_upstream_with_timing(req1, &provider1, None, false).await.0;
        attempts.push(build_request_attempt(
            0,
            &provider1,
            "claude-3-7-sonnet",
            start1.elapsed().as_millis() as i64,
            &res1,
        ));

        // Attempt 1 -> hitting loopback, returns 200
        let req2 = client.post(format!("http://{addr}/v1/messages"));
        let start2 = Instant::now();
        let res2 = send_observed_upstream_with_timing(req2, &provider2, None, false).await.0;
        attempts.push(build_request_attempt(
            1,
            &provider2,
            "claude-3-7-sonnet",
            start2.elapsed().as_millis() as i64,
            &res2,
        ));

        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt_index, 0);
        assert_eq!(attempts[0].status_code, Some(429));
        assert_eq!(attempts[0].error_category.as_deref(), Some("rate_limit"));
        assert!(!attempts[0].success);

        assert_eq!(attempts[1].attempt_index, 1);
        assert_eq!(attempts[1].status_code, Some(200));
        assert!(attempts[1].success);

        // Serialize and verify JSON
        let json_str = serde_json::to_string(&attempts).unwrap();
        assert!(json_str.contains("\"attemptIndex\":0"));
        assert!(json_str.contains("\"attemptIndex\":1"));
        assert!(json_str.contains("\"statusCode\":429"));
        assert!(json_str.contains("\"statusCode\":200"));
        assert!(json_str.contains("\"errorCategory\":\"rate_limit\""));

        // Verify storage in isolated database
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let log_id = crate::database::dao::proxy_logs::insert_proxy_log(
                conn,
                Some("prov_2"),
                Some("Prov 2"),
                Some("claude-3-7-sonnet"),
                Some(200),
                start1.elapsed().as_millis() as i64,
                Some("claude_code"),
                Some("anthropic"),
                Some("/v1/messages"),
                false,
                None,
                None,
            )?;

            crate::database::dao::proxy_logs::update_proxy_log_attempts(conn, &log_id, &json_str)?;

            let logs = crate::database::dao::proxy_logs::list_proxy_request_logs(
                conn,
                &crate::database::dao::proxy_logs::ProxyLogFilters::default(),
                0,
                10,
            )?;
            assert_eq!(logs.data.len(), 1);
            let parsed_attempts = logs.data[0].parse_attempts();
            assert_eq!(parsed_attempts.len(), 2);
            assert_eq!(parsed_attempts[0].status_code, Some(429));
            assert_eq!(parsed_attempts[1].status_code, Some(200));
            Ok(())
        })
        .unwrap();
    }
}
