// 真实 HTTP 处理器的回归，仅访问临时 loopback mock，不写 Agent live 配置。

async fn fallback_http_scenario(primary_status: u16, pinned: bool, target: ProviderTarget) {
    fallback_http_scenario_with_chain(primary_status, pinned, target, true).await;
}

async fn fallback_http_scenario_with_chain(primary_status: u16, pinned: bool, target: ProviderTarget, explicit_chain: bool) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;
    let h = Harness::new().unwrap();
    let primary_hits = Arc::new(AtomicUsize::new(0));
    let backup_hits = Arc::new(AtomicUsize::new(0));
    let primary_count = primary_hits.clone();
    let backup_count = backup_hits.clone();
    let primary_count2 = primary_hits.clone();
    let backup_count2 = backup_hits.clone();
    let app = Router::new()
        .route("/primary/v1/messages", post(move || {
            let count = primary_count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                (StatusCode::from_u16(primary_status).unwrap(), axum::Json(json!({"error":{"type":"api_error","message":"test failure"}})))
            }
        }))
        .route("/primary/v1/responses", post(move || {
            let count = primary_count2.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                (StatusCode::from_u16(primary_status).unwrap(), axum::Json(json!({"error":{"type":"api_error","message":"test failure"}})))
            }
        }))
        .route("/backup/v1/messages", post(move |axum::Json(body): axum::Json<Value>| {
            let count = backup_count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(body["model"], "backup-model");
                axum::Json(json!({"id":"msg_mock","type":"message","role":"assistant","model":"backup-model",
                    "content":[{"type":"text","text":"backup ok"}],"stop_reason":"end_turn","stop_sequence":null,
                    "usage":{"input_tokens":3,"output_tokens":2}}))
            }
        }))
        .route("/backup/v1/responses", post(move |axum::Json(body): axum::Json<Value>| {
            let count = backup_count2.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(body["model"], "backup-model");
                axum::Json(json!({"id":"resp_mock","model":"backup-model",
                    "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"backup ok"}]}],
                    "usage":{"input_tokens":3,"output_tokens":2}}))
            }
        }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let primary_id = format!("up_primary_{suffix}");
    let backup_id = format!("up_backup_{suffix}");
    let (primary_public, backup_public) = h.state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, target)?;
        // Handler 读取的初始卡不参与实际出网。
        let seed = dao::upsert_provider(conn, &provider_input(None, target, ProviderKind::Standard,
            ProtocolType::Anthropic, "http://127.0.0.1:1", "primary-model", "seed"))?;
        dao::set_current_provider(conn, &seed.id)?;
        let mut pairs = Vec::new();
        for (id, route, model) in [(&primary_id, "primary", "primary-model"), (&backup_id, "backup", "backup-model")] {
            let provider = upsert_upstream(conn, &provider_input(Some(id), target, ProviderKind::Standard,
                ProtocolType::Anthropic, &format!("http://127.0.0.1:{port}/{route}"), model, route))?;
            replace_upstream_models(conn, &provider.id, &[model.into()])?;
            pairs.push((provider, vec![model.into()]));
        }
        let entries = crate::catalog::build_catalog_with(crate::catalog::catalog_style_for(target), &pairs, false);
        let primary_public = entries.iter().find(|entry| entry.provider_id == primary_id).unwrap().public_id.clone();
        let backup_public = entries.iter().find(|entry| entry.provider_id == backup_id).unwrap().public_id.clone();
        let fallback_models = if explicit_chain {
            serde_json::to_string(&vec![backup_public.clone()]).unwrap_or_else(|_| "[]".into())
        } else {
            "[]".into()
        };
        conn.execute(
            "UPDATE gateway_profiles SET fallback_mode=?1, fallback_models_json=?2, explicit_fallback_enabled=0",
            rusqlite::params![if explicit_chain { "model_chain" } else { "retry" }, fallback_models],
        )?;
        Ok((primary_public, backup_public))
    }).unwrap();
    assert_ne!(primary_public, backup_public);
    let key = crate::gateway::service::ensure_api_key(&h.state.db);
    let router = crate::proxy::smart_gateway_router(h.state.db.clone(), 0);
    let model = if pinned { primary_public.as_str() } else if target == ProviderTarget::Codex { "auto" } else { "claude.auto" };
    let (path, body) = if target == ProviderTarget::Codex {
        ("/v1/responses", json!({"model":model,"input":"hello","stream":false}))
    } else {
        ("/v1/messages", json!({"model":model,"messages":[{"role":"user","content":"hello"}],"max_tokens":32,"stream":false}))
    };
    for _ in 0..if primary_status == 401 { 2 } else { 1 } {
        let request = http::Request::builder().method("POST").uri(path)
            .header("authorization", format!("Bearer {key}"))
            .header("x-ai-switcher-target", target.as_str())
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string())).unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        if primary_status == 500 && !pinned {
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            assert!(String::from_utf8_lossy(&body).contains("backup ok"));
        } else {
            assert!(!status.is_success());
        }
    }
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(backup_hits.load(Ordering::SeqCst), usize::from(primary_status == 500 && !pinned));
    server.abort();
}

#[tokio::test]
async fn sg_mode_fallback_without_model_chain() {
    fallback_http_scenario(500, false, ProviderTarget::ClaudeCode).await;
}

#[tokio::test]
async fn sg_explicit_pin_never_falls_back() {
    fallback_http_scenario(500, true, ProviderTarget::ClaudeCode).await;
}

#[tokio::test]
async fn sg_auth_failed_upstream_skipped() {
    fallback_http_scenario(401, true, ProviderTarget::ClaudeCode).await;
}

#[tokio::test]
async fn sg_codex_mode_fallback_without_model_chain() {
    fallback_http_scenario(500, false, ProviderTarget::Codex).await;
}

#[tokio::test]
async fn sg_codex_explicit_pin_never_falls_back() {
    fallback_http_scenario(500, true, ProviderTarget::Codex).await;
}

#[tokio::test]
async fn sg_codex_auth_failed_upstream_skipped() {
    fallback_http_scenario(401, true, ProviderTarget::Codex).await;
}

#[tokio::test]
async fn sg_retry_uses_unified_upstreams() {
    fallback_http_scenario_with_chain(500, false, ProviderTarget::ClaudeCode, false).await;
}

#[tokio::test]
async fn sg_codex_retry_uses_unified_upstreams() {
    fallback_http_scenario_with_chain(500, false, ProviderTarget::Codex, false).await;
}
