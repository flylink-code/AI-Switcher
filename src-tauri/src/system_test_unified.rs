// 统一上游连接的隔离回归，不向真实供应商发请求。

#[test]
fn sg_mig_v34_preserves_live_env() {
    let home = tempfile::tempdir().unwrap();
    paths::with_isolated_codex_home(home.path(), || {
        let h = Harness::new().unwrap();
        let code_path = paths::get_claude_settings_path();
        let codex_path = paths::get_codex_config_path();
        fs::create_dir_all(codex_path.parent().unwrap()).unwrap();
        let code = br#"{"env":{"ANTHROPIC_BASE_URL":"https://legacy.example.test","ANTHROPIC_MODEL":"custom"},"unknown":true}"#;
        let codex = b"model = \"legacy-model\"\nmodel_provider = \"legacy\"\n[model_providers.legacy]\nbase_url = \"https://legacy-codex.example.test/v1\"\n";
        fs::write(&code_path, code).unwrap();
        fs::write(&codex_path, codex).unwrap();
        h.state.db.with_conn(|conn| {
            conn.execute("DELETE FROM upstream_migration_v34;", [])?;
            conn.execute("DELETE FROM gateway_bindings;", [])?;
            let old = dao::upsert_provider(conn, &provider_input(
                Some("legacy_code"), ProviderTarget::ClaudeCode,
                ProviderKind::Standard, ProtocolType::Anthropic,
                "https://legacy.example.test", "custom", "旧卡",
            ))?;
            dao::set_current_provider(conn, &old.id)?;
            crate::database::dao::gateway::migrate_v33_to_v34(conn)?;
            let binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(binding.mode, "direct");
            Ok(())
        }).unwrap();
        assert_eq!(fs::read(code_path).unwrap(), code);
        assert_eq!(fs::read(codex_path).unwrap(), codex);
    });
}

#[tokio::test]
async fn sg_direct_switch_writes_upstream_and_rules_do_not_steal() {
    let h = Harness::new().unwrap();
    let upstream = h
        .state
        .db
        .with_conn(|conn| {
            upsert_upstream(
                conn,
                &provider_input(
                    Some("up_direct_code"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://direct.example.test",
                    "claude-custom",
                    "直连测试",
                ),
            )
        })
        .unwrap();
    crate::commands::providers::set_agent_direct_for_target(
        ProviderTarget::ClaudeCode,
        &upstream.id,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await
    .unwrap();
    assert_eq!(code_base_url(), "https://direct.example.test");
    let report = crate::commands::providers::config_drift_report(&h.state, ProviderTarget::ClaudeCode).unwrap();
    assert_eq!(report.status, "in_sync");
    let settings_path = paths::get_claude_settings_path();
    let original_settings = fs::read(&settings_path).unwrap();
    let mut external: serde_json::Value = serde_json::from_slice(&original_settings).unwrap();
    external["env"]["ANTHROPIC_MODEL"] = serde_json::json!("external-model");
    fs::write(&settings_path, serde_json::to_vec(&external).unwrap()).unwrap();
    let drift = crate::commands::providers::config_drift_report(&h.state, ProviderTarget::ClaudeCode).unwrap();
    assert_eq!(drift.status, "drifted");
    assert_ne!(drift.revision, report.revision);
    assert!(drift.fields.iter().any(|field| field.field == "ANTHROPIC_MODEL"));
    assert!(!serde_json::to_string(&drift).unwrap().contains("external-model"));
    // 检测只读，不能悄悄修复外部修改。
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&settings_path).unwrap()).unwrap(), external);
    fs::write(&settings_path, original_settings).unwrap();
    let before = read_code_env();
    h.state
        .db
        .with_conn(|conn| {
            let binding = binding_for_target(conn, ProviderTarget::ClaudeCode)?.unwrap();
            assert_eq!(binding.mode, "direct");
            assert_eq!(binding.direct_upstream_id, upstream.id);
            assert!(!is_gateway_connection(conn, ProviderTarget::ClaudeCode));
            assert!(
                crate::database::dao::gateway::binding_by_token(conn, &binding.entry_token)?
                    .is_none()
            );
            assert!(dao::get_current_provider(conn, ProviderTarget::ClaudeCode)?.is_none());
            assert!(dao::list_providers(conn, ProviderTarget::ClaudeCode)?.is_empty());
            assert!(
                set_binding_profile(conn, ProviderTarget::ClaudeCode, SHARED_PROFILE_ID).is_err()
            );
            Ok(())
        })
        .unwrap();
    crate::commands::providers::push_bound_gateway_catalogs(&h.state)
        .await
        .unwrap();
    assert_eq!(read_code_env(), before);
    assert!(
        !h.state
            .proxy
            .lock()
            .await
            .status_for(ProviderTarget::ClaudeCode)
            .running
    );
    crate::commands::providers::switch_to_official_for_target(
        ProviderTarget::ClaudeCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await
    .unwrap();
    assert!(!code_base_url().contains("direct.example.test"));
    assert!(h
        .state
        .db
        .with_read_conn(|conn| binding_for_target(conn, ProviderTarget::ClaudeCode))
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sg_direct_invalid_model_restores_previous_connection() {
    let h = Harness::new().unwrap();
    let (good, bad) = h
        .state
        .db
        .with_conn(|conn| {
            let good = upsert_upstream(
                conn,
                &provider_input(
                    Some("up_good"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://good.example.test",
                    "claude-custom",
                    "正常",
                ),
            )?;
            let bad = upsert_upstream(
                conn,
                &provider_input(
                    Some("up_bad"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::Anthropic,
                    "https://bad.example.test",
                    "placeholder-model",
                    "无模型",
                ),
            )?;
            conn.execute("UPDATE upstreams SET model = '' WHERE id = 'up_bad';", [])?;
            Ok((good, bad))
        })
        .unwrap();
    crate::commands::providers::set_agent_direct_for_target(
        ProviderTarget::ClaudeCode,
        &good.id,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await
    .unwrap();
    let before = read_code_env();
    let result = crate::commands::providers::set_agent_direct_for_target(
        ProviderTarget::ClaudeCode,
        &bad.id,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(read_code_env(), before);
    assert_eq!(
        h.state
            .db
            .with_read_conn(|conn| binding_for_target(conn, ProviderTarget::ClaudeCode))
            .unwrap()
            .unwrap()
            .direct_upstream_id,
        good.id
    );
}

#[tokio::test]
async fn sg_direct_catalog_writes_one_global_upstream() {
    let h = Harness::new().unwrap();
    let upstream = h
        .state
        .db
        .with_conn(|conn| {
            upsert_upstream(
                conn,
                &provider_input(
                    Some("up_oc_direct"),
                    ProviderTarget::ClaudeCode,
                    ProviderKind::Standard,
                    ProtocolType::OpenAiChat,
                    "https://openai.example.test/v1",
                    "gpt-custom",
                    "全局上游",
                ),
            )
        })
        .unwrap();
    crate::commands::providers::set_agent_direct_for_target(
        ProviderTarget::OpenCode,
        &upstream.id,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await
    .unwrap();
    let text = fs::read_to_string(paths::get_opencode_config_path()).unwrap();
    assert!(text.contains("openai.example.test"));
    assert!(!text.contains("15828"));
    assert!(h
        .state
        .db
        .with_read_conn(|conn| dao::list_providers(conn, ProviderTarget::OpenCode))
        .unwrap()
        .is_empty());
    crate::commands::providers::switch_to_official_for_target(
        ProviderTarget::OpenCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await
    .unwrap();
    let text = fs::read_to_string(paths::get_opencode_config_path()).unwrap();
    assert!(!text.contains("openai.example.test"));
}

#[test]
fn sg_direct_codex_uses_isolated_config_without_local_proxy() {
    let home = tempfile::tempdir().unwrap();
    paths::with_isolated_codex_home(home.path(), || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let h = Harness::new().unwrap();
            let upstream = h
                .state
                .db
                .with_conn(|conn| {
                    upsert_upstream(
                        conn,
                        &provider_input(
                            Some("up_codex_direct"),
                            ProviderTarget::Codex,
                            ProviderKind::Standard,
                            ProtocolType::OpenAiResponses,
                            "https://codex.example.test/v1",
                            "gpt-custom",
                            "Codex 直连",
                        ),
                    )
                })
                .unwrap();
            crate::commands::providers::set_agent_direct_for_target(
                ProviderTarget::Codex,
                &upstream.id,
                None::<&tauri::AppHandle>,
                &h.state,
            )
            .await
            .unwrap();
            let text = fs::read_to_string(paths::get_codex_config_path()).unwrap();
            assert!(text.contains("codex.example.test/v1"));
            assert!(!text.contains("15823"));
            assert!(!text.contains("15828"));
            assert!(
                !h.state
                    .proxy
                    .lock()
                    .await
                    .status_for(ProviderTarget::Codex)
                    .running
            );
        });
    });
}
