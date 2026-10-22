
#[tauri::command]
pub fn ensure_smart_gateway_provider(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    ensure_smart_gateway_provider_row(&state, target)
}

pub(crate) fn get_saved_proxy_port(state: &AppState, target: ProviderTarget) -> u16 {
    let key = match target {
        ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
        ProviderTarget::Codex => "proxy_port_codex",
        // OpenCode / Pi / Dsh 直连写入，不使用本地代理；仅为穷尽匹配保留键名。
        ProviderTarget::OpenCode => "proxy_port_opencode",
        ProviderTarget::Pi => "proxy_port_pi",
        ProviderTarget::Dsh => "proxy_port_dsh",
        ProviderTarget::Cline => "proxy_port_cline",
        ProviderTarget::VscodeCopilot => "proxy_port_vscode_copilot",
    };
    state.db.with_conn(|conn| get_setting(conn, key))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .or_else(|| state.db.with_conn(|conn| get_setting(conn, "proxy_port")).ok().flatten().and_then(|value| value.parse::<u16>().ok()))
        .unwrap_or(match target {
            ProviderTarget::ClaudeCode => 15821,
            ProviderTarget::ClaudeDesktop => 15822,
            ProviderTarget::Codex => 15823,
            ProviderTarget::OpenCode => 15824,
            ProviderTarget::Pi => 15825,
            ProviderTarget::Dsh => 15826,
            ProviderTarget::Cline => 15827,
            ProviderTarget::VscodeCopilot => 15829,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_follows_the_first_catalog_id() {
        let catalog = vec![
            "claude.gemini-3.8-flash".to_string(),
            "claude.gpt-6-astra".to_string(),
        ];
        assert_eq!(
            smart_gateway_default_model(Some("claude.sub2api.gpt-6-astra"), &catalog),
            "claude.gemini-3.8-flash"
        );
        assert_eq!(
            smart_gateway_default_model(Some("auto"), &catalog),
            "claude.gemini-3.8-flash"
        );
    }

    #[test]
    fn default_model_keeps_a_concrete_id_when_the_catalog_is_empty() {
        assert_eq!(
            smart_gateway_default_model(Some("claude.gemini-3.8-flash"), &[]),
            "claude.gemini-3.8-flash"
        );
        assert_eq!(smart_gateway_default_model(Some("auto"), &[]), "");
        assert_eq!(smart_gateway_default_model(None, &[]), "");
    }

    #[tokio::test]
    async fn discovery_fetches_sub2api_manifest_and_falls_back_when_unsupported() {
        use axum::{extract::Query, routing::get, Json, Router};
        for supported in [true, false] {
            let app = Router::new().route("/v1/models", get(move |Query(query): Query<std::collections::HashMap<String, String>>| async move {
                if query.contains_key("client_version") {
                    if supported {
                        return Ok(Json(serde_json::json!({"models": [
                            {"slug": "gpt-fixture", "context_window": 400000, "max_context_window": 1050000}
                        ]})));
                    }
                    return Err(axum::http::StatusCode::BAD_REQUEST);
                }
                Ok(Json(serde_json::json!({"data": [{"id": "gpt-fixture"}, {"id": "other"}]})))
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/v1/models", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let response = fetch_discovered_catalog(&client, &[url], "fixture", None, ProtocolType::OpenAiResponses).await;
            server.abort();
            let value = response.unwrap();
            let models = extract_model_ids(&value);
            assert_eq!(models, vec!["gpt-fixture", "other"]);
            let metadata = extract_model_metadata(&value, &models);
            if supported {
                assert_eq!(metadata["gpt-fixture"].context_window, Some(400000));
            } else {
                assert!(metadata.is_empty());
            }
        }
    }

    #[test]
    fn fast_mode_discovery_merges_capability_without_losing_windows() {
        let value = serde_json::json!({
            "data": [{"id": "known", "service_tiers": ["priority"]}, {"id": "unknown"}, {"id": "denied", "service_tiers": []}],
            "codex_models": [
                {"slug": "known", "context_window": 272000},
                {"slug": "denied", "service_tiers": ["priority"]},
                {"slug": "not-listed", "service_tiers": ["priority"]}
            ]
        });
        let models = extract_model_ids(&value);
        let metadata = extract_model_metadata(&value, &models);
        assert_eq!(metadata["known"].priority_supported, Some(true));
        assert_eq!(metadata["known"].context_window, Some(272000));
        assert_eq!(metadata["denied"].priority_supported, Some(false));
        assert!(!metadata.contains_key("unknown"));
        assert!(!metadata.contains_key("not-listed"));
        let manifest = serde_json::json!({"data": [{"id": "known", "context_window": 400000}],
            "codex_models": [{"slug": "known", "service_tiers": [{"id": "priority"}]}]});
        let metadata = extract_model_metadata(&manifest, &extract_model_ids(&manifest));
        assert_eq!(metadata["known"].priority_supported, Some(true));
        assert_eq!(metadata["known"].context_window, Some(400000));
    }

    #[test]
    fn discovery_preserves_manifest_windows_without_changing_model_ids() {
        let value = serde_json::json!({
            "data": [{"id": "gpt-fixture"}, {"id": "other"}],
            "codex_models": [
                {"slug": "gpt-fixture", "context_window": 272000,
                 "max_context_window": 872000, "effective_context_window_percent": 95},
                {"slug": "not-in-list", "context_window": 999999}
            ]
        });
        let models = extract_model_ids(&value);
        let metadata = extract_model_metadata(&value, &models);
        assert_eq!(models, vec!["gpt-fixture", "other"]);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata["gpt-fixture"].context_window, Some(272000));
        assert_eq!(metadata["gpt-fixture"].max_context_window, Some(872000));
        assert_eq!(metadata["gpt-fixture"].effective_context_window_percent, Some(95));
        let manifest = serde_json::json!({"models": [{"slug": "gpt-fixture", "context_window": 400000}]});
        assert_eq!(extract_model_ids(&manifest), vec!["gpt-fixture"]);
        assert_eq!(extract_model_metadata(&manifest, &models)["gpt-fixture"].context_window, Some(400000));
    }

    #[test]
    fn discovery_prefers_api_metadata_and_bounds_input_to_total_context() {
        let value = serde_json::json!({
            "data": [{"id": "gpt-fixture", "limit": {"context": 1050000, "input": 922000, "output": 128000}}],
            "codex_models": [{"slug": "gpt-fixture", "context_window": 272000}]
        });
        let metadata = extract_model_metadata(&value, &extract_model_ids(&value));
        assert_eq!(metadata["gpt-fixture"].context_window, Some(922000));
        assert_eq!(metadata["gpt-fixture"].max_output_tokens, Some(128000));
        let bounded = crate::gateway::metadata::DiscoveredModelMeta::from_model(&serde_json::json!({
            "context_length": 128000, "max_input_tokens": 200000, "max_output_tokens": 0
        }));
        assert_eq!(bounded.context_window, Some(128000));
        assert_eq!(bounded.max_output_tokens, None);
    }

    #[test]
    fn discovered_models_are_trimmed_deduplicated_and_sorted() {
        let value = serde_json::json!({
            "data": [
                {"id": " model-z "},
                {"name": "model-a"},
                {"id": "model-a"},
                "model-m",
                {"id": ""}
            ]
        });
        assert_eq!(
            extract_model_ids(&value),
            vec![
                "model-a".to_string(),
                "model-m".to_string(),
                "model-z".to_string()
            ]
        );
    }

    #[test]
    fn antigravity_gateway_urls_use_local_catalog() {
        assert!(is_antigravity_gateway_base_url("http://127.0.0.1:15830"));
        assert!(is_antigravity_gateway_base_url("http://127.0.0.1:15830/"));
        assert!(is_antigravity_gateway_base_url("http://localhost:15830/v1"));
        assert!(is_antigravity_gateway_base_url("http://127.0.0.1:8045"));
        assert!(!is_antigravity_gateway_base_url("https://api.anthropic.com"));
        assert!(is_kiro_gateway_base_url("http://127.0.0.1:15831"));
        assert!(is_kiro_gateway_base_url("http://127.0.0.1:15831/"));
        assert!(is_kiro_gateway_base_url("http://localhost:15831/v1"));
        assert!(!is_kiro_gateway_base_url("http://127.0.0.1:15830"));
        assert!(!is_antigravity_gateway_base_url("http://127.0.0.1:15831"));
        assert!(url_targets_loopback("http://127.0.0.1:15830/v1/models"));
        assert!(!url_targets_loopback("https://api.deepseek.com/v1/models"));
    }

    #[test]
    fn deepseek_anthropic_discovery_falls_back_to_host_models() {
        assert_eq!(
            model_discovery_urls("https://api.deepseek.com/anthropic").unwrap(),
            vec![
                "https://api.deepseek.com/anthropic/v1/models".to_string(),
                "https://api.deepseek.com/anthropic/models".to_string(),
                "https://api.deepseek.com/v1/models".to_string(),
                "https://api.deepseek.com/models".to_string(),
            ]
        );
    }

    #[test]
    fn deepseek_openai_discovery_includes_unversioned_models() {
        assert_eq!(
            model_discovery_urls("https://api.deepseek.com").unwrap(),
            vec![
                "https://api.deepseek.com/v1/models".to_string(),
                "https://api.deepseek.com/models".to_string(),
            ]
        );
        assert_eq!(
            model_discovery_urls("https://api.deepseek.com/v1").unwrap(),
            vec![
                "https://api.deepseek.com/v1/models".to_string(),
                "https://api.deepseek.com/models".to_string(),
            ]
        );
    }

    #[test]
    fn versioned_openai_discovery_does_not_duplicate_v1_models() {
        assert_eq!(
            model_discovery_urls("https://api.moonshot.cn/v1").unwrap(),
            vec![
                "https://api.moonshot.cn/v1/models".to_string(),
                "https://api.moonshot.cn/models".to_string(),
            ]
        );
    }

    #[test]
    fn pi_anthropic_base_url_strips_v1_so_sdk_does_not_double_path() {
        use crate::provider::ProtocolType;
        assert_eq!(
            normalize_pi_base_url("http://127.0.0.1:15830/v1", ProtocolType::Anthropic),
            "http://127.0.0.1:15830"
        );
        assert_eq!(
            normalize_pi_base_url("http://127.0.0.1:15830/", ProtocolType::Anthropic),
            "http://127.0.0.1:15830"
        );
        assert_eq!(
            pi_proxy_base_url(15825, ProtocolType::Anthropic),
            "http://127.0.0.1:15825"
        );
        assert_eq!(
            pi_proxy_base_url(15825, ProtocolType::OpenAiChat),
            "http://127.0.0.1:15825/v1"
        );
        assert_eq!(
            normalize_pi_base_url("https://api.deepseek.com", ProtocolType::OpenAiChat),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_pi_base_url(
                "https://open.bigmodel.cn/api/paas/v4",
                ProtocolType::OpenAiChat
            ),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            normalize_pi_base_url("https://api.deepseek.com/anthropic", ProtocolType::Anthropic),
            "https://api.deepseek.com/anthropic"
        );
    }

    fn pi_test_provider(protocol: ProtocolType, model: &str) -> Provider {
        Provider {
            id: "p1".into(),
            name: "Pi Gateway".into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: "sk-test".into(),
            api_key_set: true,
            model: model.into(),
            model_context_window: Some(128_000),
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: protocol,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::Pi,
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
            enabled: true,
        }
    }

    #[test]
    fn pi_models_always_declare_reasoning_and_image_input() {
        let provider = pi_test_provider(ProtocolType::OpenAiChat, "gemini-3.6-flash");
        let entries = build_pi_model_entries(&provider, &["qwen3.6-plus".into(), "custom-id".into()]);
        let by_id = |id: &str| {
            entries
                .iter()
                .find(|entry| entry["id"] == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        for id in ["gemini-3.6-flash", "qwen3.6-plus", "custom-id"] {
            let entry = by_id(id);
            assert_eq!(entry["reasoning"], true, "{id} must declare reasoning");
            assert_eq!(entry["input"], serde_json::json!(["text", "image"]));
            assert_eq!(entry["thinkingLevelMap"]["high"], "high");
            assert_eq!(entry["contextWindow"], 128_000);
        }
    }

    #[test]
    fn pi_anthropic_models_skip_openai_thinking_level_map() {
        let provider = pi_test_provider(ProtocolType::Anthropic, "claude-sonnet-4-6");
        let entries = build_pi_model_entries(&provider, &[]);
        assert_eq!(entries[0]["reasoning"], true);
        assert!(entries[0].get("thinkingLevelMap").is_none());
    }

    #[test]
    fn old_model_cache_remains_available_but_is_stale() {
        let result = model_result_from_cache(
            Some(dao::providers::ProviderModelCache {
                models: vec!["cached-model".to_string()],
                metadata: Default::default(),
                checked_at: Utc::now().timestamp_millis() - MODEL_CACHE_TTL_MS - 1,
            }),
            "cached",
            None,
        );
        assert_eq!(result.models, vec!["cached-model".to_string()]);
        assert!(result.stale);
        assert_eq!(result.source, "cache");
    }

    #[test]
    fn legacy_ownership_adopts_new_role_fields_as_newly_managed() {
        let mut ownership = CodeOwnership {
            before: BTreeMap::from([(
                "ANTHROPIC_MODEL".to_string(),
                Some(Value::String("original-default".to_string())),
            )]),
            written: BTreeMap::from([(
                "ANTHROPIC_MODEL".to_string(),
                Some(Value::String("managed-default".to_string())),
            )]),
        };
        let current = BTreeMap::from([
            (
                "ANTHROPIC_MODEL".to_string(),
                Some(Value::String("managed-default".to_string())),
            ),
            (
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                Some(Value::String("user-sonnet".to_string())),
            ),
        ]);

        upgrade_code_ownership_fields(&mut ownership, &current);

        assert_eq!(ownership.before["ANTHROPIC_DEFAULT_SONNET_MODEL"], None);
        assert_eq!(
            ownership.written["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            Some(Value::String("user-sonnet".to_string()))
        );
    }

    #[test]
    fn ownership_adopts_absent_api_key_drift() {
        let mut ownership = CodeOwnership {
            before: BTreeMap::from([("ANTHROPIC_AUTH_TOKEN".into(), None)]),
            written: BTreeMap::from([
                ("ANTHROPIC_AUTH_TOKEN".into(), Some(Value::String("tok".into()))),
                ("ANTHROPIC_API_KEY".into(), None),
            ]),
        };
        let current = BTreeMap::from([
            ("ANTHROPIC_AUTH_TOKEN".into(), Some(Value::String("tok".into()))),
            ("ANTHROPIC_API_KEY".into(), Some(Value::String("tok".into()))),
        ]);
        adopt_absent_key_drift(&mut ownership, &current);
        assert!(managed_fields_match(&ownership.written, &current));
        assert_eq!(
            ownership.before.get("ANTHROPIC_API_KEY"),
            Some(&Some(Value::String("tok".into())))
        );
    }

    #[test]
    fn ownership_adopts_auth_token_migrated_to_api_key() {
        let mut ownership = CodeOwnership {
            before: claude_code::MANAGED_ENV_KEYS
                .into_iter()
                .map(|key| (key.to_string(), None))
                .collect(),
            written: {
                let mut fields = claude_code::MANAGED_ENV_KEYS
                    .into_iter()
                    .map(|key| (key.to_string(), None))
                    .collect::<BTreeMap<_, _>>();
                fields.insert(
                    "ANTHROPIC_AUTH_TOKEN".into(),
                    Some(Value::String("tok".into())),
                );
                fields
            },
        };
        let mut current = claude_code::MANAGED_ENV_KEYS
            .into_iter()
            .map(|key| (key.to_string(), None))
            .collect::<BTreeMap<_, _>>();
        current.insert(
            "ANTHROPIC_API_KEY".into(),
            Some(Value::String("tok".into())),
        );
        reconcile_code_ownership(&mut ownership, &current);
        assert!(managed_fields_match(&ownership.written, &current));
    }

    #[test]
    fn ownership_normalizes_base_url_trailing_slash() {
        let left = BTreeMap::from([(
            "ANTHROPIC_BASE_URL".into(),
            Some(Value::String("https://api.example.com/".into())),
        )]);
        let right = BTreeMap::from([(
            "ANTHROPIC_BASE_URL".into(),
            Some(Value::String("https://api.example.com".into())),
        )]);
        assert!(managed_fields_match(&left, &right));
    }

    #[test]
    fn ownership_normalizes_blank_strings_as_absent() {
        let left = BTreeMap::from([("ANTHROPIC_API_KEY".into(), Some(Value::String("  ".into())))]);
        let right = BTreeMap::from([("ANTHROPIC_API_KEY".into(), None)]);
        assert!(managed_fields_match(&left, &right));
    }

    #[test]
    fn desktop_switch_keeps_stored_original_when_still_on_managed_profile() {
        let stored = serde_json::to_string(&Some("user-profile")).unwrap();
        let original = desktop_switch_original_applied_id(
            Some(&stored),
            Some(claude_desktop::PROFILE_ID),
        )
        .unwrap();
        assert_eq!(original.as_deref(), Some("user-profile"));
    }

    #[test]
    fn desktop_switch_rebases_when_applied_id_drifted() {
        let stored = serde_json::to_string(&Some("old-profile")).unwrap();
        let original =
            desktop_switch_original_applied_id(Some(&stored), Some("desktop-user-profile"))
                .unwrap();
        assert_eq!(original.as_deref(), Some("desktop-user-profile"));
    }

    #[test]
    fn desktop_switch_first_apply_captures_unmanaged_applied_id() {
        let original =
            desktop_switch_original_applied_id(None, Some("desktop-user-profile")).unwrap();
        assert_eq!(original.as_deref(), Some("desktop-user-profile"));
        assert_eq!(
            desktop_switch_original_applied_id(None, Some(claude_desktop::PROFILE_ID)).unwrap(),
            None
        );
    }

    #[test]
    fn ag_opencode_extra_models_keep_gemini_from_failover_and_suggestions() {
        let provider = Provider {
            id: "p-ag".into(),
            name: "Antigravity (Built-in)".into(),
            base_url: "http://127.0.0.1:15830/v1".into(),
            api_key: "local".into(),
            api_key_set: true,
            model: "claude-sonnet-4-6".into(),
            model_context_window: Some(200_000),
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Antigravity,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::OpenCode,
            sort_index: 0,
            failover_group: 0,
            failover_models: vec!["gemini-3.8-flash".into()],
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
            enabled: true,
        };
        let extra = extra_models_for_ag_catalog_apply(
            &provider,
            vec!["claude-sonnet-5-5-medium".into(), "chat_20706".into()],
        );
        assert!(extra.iter().any(|id| id == "claude-sonnet-5-5-medium"));
        assert!(extra.iter().any(|id| id.starts_with("gemini-")));
        assert!(!extra.iter().any(|id| id == "chat_20706"));
    }

    #[test]
    fn extra_models_omit_hidden_but_keep_default() {
        let mut provider = pi_test_provider(ProtocolType::OpenAiChat, "keep-me");
        provider.failover_models = vec!["hide-me".into(), "show-me".into()];
        provider.hidden_models = vec!["hide-me".into(), "keep-me".into()];
        let extra = extra_models_for_ag_catalog_apply(
            &provider,
            vec!["keep-me".into(), "hide-me".into(), "cached".into()],
        );
        assert!(extra.iter().any(|id| id == "keep-me"));
        assert!(extra.iter().any(|id| id == "show-me"));
        assert!(extra.iter().any(|id| id == "cached"));
        assert!(!extra.iter().any(|id| id == "hide-me"));
    }

    #[test]
    fn smart_gateway_catalog_starts_desktop_proxy_not_code_or_codex() {
        let mut provider = pi_test_provider(ProtocolType::Anthropic, "claude.auto");
        provider.provider_kind = ProviderKind::SmartGateway;
        provider.target_app = ProviderTarget::ClaudeCode;
        assert!(!target_starts_agent_proxy(
            ProviderTarget::ClaudeCode,
            true,
            &provider
        ));
        provider.target_app = ProviderTarget::Codex;
        assert!(!target_starts_agent_proxy(
            ProviderTarget::Codex,
            true,
            &provider
        ));
        provider.target_app = ProviderTarget::ClaudeDesktop;
        assert!(target_starts_agent_proxy(
            ProviderTarget::ClaudeDesktop,
            true,
            &provider
        ));
        provider.target_app = ProviderTarget::OpenCode;
        assert!(!target_starts_agent_proxy(
            ProviderTarget::OpenCode,
            true,
            &provider
        ));
        provider.target_app = ProviderTarget::Cline;
        assert!(!target_starts_agent_proxy(
            ProviderTarget::Cline,
            true,
            &provider
        ));
        provider.provider_kind = ProviderKind::Standard;
        assert!(target_starts_agent_proxy(
            ProviderTarget::Cline,
            false,
            &provider
        ));
    }
}
