//! L1 system scenarios. Run with:
//! `cargo test --lib system_test -- --test-threads=1 --nocapture`

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;

use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc::unbounded_channel;

use crate::config::paths::{self, IsolatedHomeGuard};
use crate::database::dao;
use crate::database::dao::gateway::{
    binding_for_target, ensure_profile_for_target, is_gateway_connection, patch_route_mode,
    replace_upstream_models, set_binding_profile, upsert_binding, upsert_upstream, RouteModePatch,
    SHARED_PROFILE_ID,
};
use crate::database::dao::proxy_logs::{
    insert_proxy_log, update_proxy_log_hop, EFFECTIVE_USAGE_FILTER,
};
use crate::database::Database;
use crate::error::AppResult;
use crate::gateway::simulate::{simulate, SimulateRouteInput};
use crate::provider::{
    ClaudeModelMapping, ProtocolType, ProviderInput, ProviderKind, ProviderTarget,
};
use crate::proxy::ProxyManager;
use crate::store::AppState;

pub struct Harness {
    _home: tempfile::TempDir,
    _isolated: IsolatedHomeGuard,
    pub state: AppState,
}

impl Harness {
    pub fn new() -> AppResult<Self> {
        let home = tempfile::tempdir().map_err(|error| {
            crate::error::AppError::Config(format!("temp home: {error}"))
        })?;
        let isolated = paths::enter_isolated_home(home.path());
        fs::create_dir_all(paths::get_claude_config_dir())?;
        fs::create_dir_all(paths::get_legacy_app_config_dir())?;
        let db = Arc::new(Database::memory()?);
        let (lifecycle_tx, _lifecycle_rx) = unbounded_channel();
        let state = AppState {
            db: Arc::clone(&db),
            proxy: tokio::sync::Mutex::new(ProxyManager::new(db, lifecycle_tx)),
            proxy_status: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        };
        Ok(Self {
            _home: home,
            _isolated: isolated,
            state,
        })
    }
}

pub fn provider_input(
    id: Option<&str>,
    target: ProviderTarget,
    kind: ProviderKind,
    protocol: ProtocolType,
    base_url: &str,
    model: &str,
    name: &str,
) -> ProviderInput {
    ProviderInput {
        id: id.map(str::to_string),
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key: "sk-test".to_string(),
        clear_api_key: false,
        model: model.to_string(),
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: ClaudeModelMapping::default(),
        protocol_type: protocol,
        provider_kind: kind,
        auth_binding: String::new(),
        target_app: target,
        notes: String::new(),
        failover_group: 0,
        failover_models: Vec::new(),
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    }
}

pub fn read_code_env() -> Value {
    let path = paths::get_claude_settings_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return json!({});
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|root| root.get("env").cloned())
        .unwrap_or(json!({}))
}

pub fn code_base_url() -> String {
    read_code_env()
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub fn code_discovery_enabled() -> bool {
    read_code_env()
        .get("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY")
        .and_then(Value::as_str)
        == Some("1")
}

pub async fn sg_regress_independent_not_stolen(h: &Harness) -> AppResult<()> {
    let independent = h.state.db.with_conn(|conn| {
        dao::upsert_provider(
            conn,
            &provider_input(
                Some("p_code_indep"),
                ProviderTarget::ClaudeCode,
                ProviderKind::Standard,
                ProtocolType::Anthropic,
                "https://8tou.example.test",
                "gpt-5.6-terra",
                "sub2api",
            ),
        )
    })?;
    crate::commands::providers::switch_provider_for_target(
        &independent.id,
        ProviderTarget::ClaudeCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await?;
    let before = code_base_url();
    assert!(
        !before.contains(":15828"),
        "independent current must not write 15828, got {before}"
    );
    crate::commands::providers::push_bound_gateway_catalogs(&h.state).await?;
    h.state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        patch_route_mode(
            conn,
            "default",
            &RouteModePatch {
                enabled: Some(true),
                ..RouteModePatch::default()
            },
            Some(SHARED_PROFILE_ID),
        )?;
        Ok(())
    })?;
    crate::commands::providers::push_bound_gateway_catalogs(&h.state).await?;
    let after = code_base_url();
    assert_eq!(before, after, "editing gateway modes must not rewrite live Code env");
    assert!(!after.contains(":15828"));
    assert!(!code_discovery_enabled());
    Ok(())
}

pub async fn sg_regress_no_autobind(h: &Harness) -> AppResult<()> {
    h.state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        let cloned = crate::database::dao::gateway::create_profile(conn, "gpt", Some(SHARED_PROFILE_ID))?;
        assert!(set_binding_profile(conn, ProviderTarget::OpenCode, &cloned.id).is_err());
        assert!(binding_for_target(conn, ProviderTarget::OpenCode)?.is_none());
        let rows: i64 = conn.query_row(
            "SELECT count(*) FROM gateway_bindings WHERE target_app = ?;",
            [ProviderTarget::OpenCode.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(rows, 0);
        Ok(())
    })
}

pub async fn sg_regress_auto_current_writes_gateway(h: &Harness) -> AppResult<()> {
    let independent = h.state.db.with_conn(|conn| {
        dao::upsert_provider(
            conn,
            &provider_input(
                Some("p_code_indep2"),
                ProviderTarget::ClaudeCode,
                ProviderKind::Standard,
                ProtocolType::Anthropic,
                "https://8tou.example.test",
                "gpt-5.6-terra",
                "sub2api",
            ),
        )
    })?;
    crate::commands::providers::switch_provider_for_target(
        &independent.id,
        ProviderTarget::ClaudeCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await?;
    h.state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        upsert_upstream(
            conn,
            &provider_input(
                Some("up_pool"),
                ProviderTarget::ClaudeCode,
                ProviderKind::Standard,
                ProtocolType::Anthropic,
                "https://pool.example.test",
                "claude-sonnet-custom",
                "pool",
            ),
        )?;
        upsert_binding(conn, ProviderTarget::ClaudeCode, "sgw_claude_code")?;
        Ok(())
    })?;
    let auto = crate::commands::providers::ensure_smart_gateway_provider_row(
        &h.state,
        ProviderTarget::ClaudeCode,
    )?;
    crate::commands::providers::switch_provider_for_target(
        &auto.id,
        ProviderTarget::ClaudeCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await?;
    h.state.db.with_conn(|conn| {
        assert!(is_gateway_connection(conn, ProviderTarget::ClaudeCode));
        Ok(())
    })?;
    assert!(
        code_base_url().contains(":15828"),
        "Auto current should write 15828, got {}",
        code_base_url()
    );
    assert!(code_discovery_enabled());

    crate::commands::providers::switch_provider_for_target(
        &independent.id,
        ProviderTarget::ClaudeCode,
        None::<&tauri::AppHandle>,
        &h.state,
    )
    .await?;
    crate::commands::providers::repair_current_code_model_fields(&h.state).await?;
    assert!(
        !code_base_url().contains(":15828"),
        "switching off Auto must clear 15828, got {}",
        code_base_url()
    );
    assert!(!code_discovery_enabled());
    Ok(())
}

pub async fn sg_p0_simulate_default_mode(h: &Harness) -> AppResult<()> {
    h.state.db.with_conn(|conn| {
        ensure_profile_for_target(conn, ProviderTarget::ClaudeCode)?;
        let upstream = upsert_upstream(
            conn,
            &provider_input(
                Some("up_sim"),
                ProviderTarget::ClaudeCode,
                ProviderKind::Standard,
                ProtocolType::Anthropic,
                "https://sim.example.test",
                "claude-sonnet-custom",
                "sim",
            ),
        )?;
        replace_upstream_models(conn, &upstream.id, &["claude-sonnet-custom".into()])?;
        patch_route_mode(
            conn,
            "default",
            &RouteModePatch {
                enabled: Some(true),
                model: Some("claude-sonnet-custom".into()),
                ..RouteModePatch::default()
            },
            Some(SHARED_PROFILE_ID),
        )?;
        Ok(())
    })?;
    let result = simulate(
        h.state.db.as_ref(),
        SimulateRouteInput {
            requested_model: Some("claude.auto".into()),
            body_json: None,
            token_count: Some(32),
            has_web_search: Some(false),
            has_vision: Some(false),
            has_thinking: Some(false),
            is_subagent: Some(false),
            is_image_gen: Some(false),
            tool_names: Some(Vec::new()),
            recent_write_tool: None,
            path: Some("/v1/messages".into()),
            target: Some(ProviderTarget::ClaudeCode),
            profile_id: Some(SHARED_PROFILE_ID.into()),
        },
    )?;
    let decision = result.decision.expect("route decision");
    assert_ne!(
        decision.normalized_model, "gpt-6-astra",
        "non-empty modes must not fall through to a catalog first item"
    );
    assert!(
        decision.mode_id.as_deref() == Some("default")
            || decision.normalized_model.contains("claude-sonnet-custom")
            || result.upstream_model.as_deref().is_some_and(|model| model.contains("claude-sonnet-custom")),
        "expected default-mode routing, got mode={:?} model={} upstream={:?}",
        decision.mode_id,
        decision.normalized_model,
        result.upstream_model
    );
    Ok(())
}

pub async fn sg_p0_usage_inner_hop(h: &Harness) -> AppResult<()> {
    h.state.db.with_conn(|conn| {
        let outer = insert_proxy_log(
            conn,
            Some("p_proxy"),
            Some("sub2api"),
            Some("gpt-5.6-terra"),
            Some(200),
            10,
            Some("claude_code"),
            Some("anthropic"),
            Some("/v1/messages"),
            false,
            None,
            None,
        )?;
        update_proxy_log_hop(conn, &outer, Some("corr_sys"), Some("agent_proxy"))?;
        let inner = insert_proxy_log(
            conn,
            Some("p_gw"),
            Some("智能网关"),
            Some("claude-sonnet-custom"),
            Some(200),
            20,
            Some("claude_code"),
            Some("anthropic"),
            Some("/v1/messages"),
            false,
            None,
            None,
        )?;
        update_proxy_log_hop(conn, &inner, Some("corr_sys"), Some("smart_gateway"))?;
        let hop: String = conn.query_row(
            &format!(
                "SELECT hop FROM proxy_request_logs l WHERE 1=1 {EFFECTIVE_USAGE_FILTER}"
            ),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hop, "smart_gateway");
        let counted: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM proxy_request_logs l WHERE 1=1 {EFFECTIVE_USAGE_FILTER}"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(counted, 1);
        Ok(())
    })
}

pub async fn sg_p0_catalog_bind_appends_auto(h: &Harness) -> AppResult<()> {
    h.state.db.with_conn(|conn| {
        dao::upsert_provider(
            conn,
            &provider_input(
                Some("p_oc_direct"),
                ProviderTarget::OpenCode,
                ProviderKind::Standard,
                ProtocolType::OpenAiChat,
                "https://direct.example.test/v1",
                "gpt-5.6-terra",
                "direct",
            ),
        )?;
        ensure_profile_for_target(conn, ProviderTarget::OpenCode)?;
        upsert_upstream(
            conn,
            &provider_input(
                Some("up_oc"),
                ProviderTarget::ClaudeCode,
                ProviderKind::Standard,
                ProtocolType::Anthropic,
                "https://pool.example.test",
                "claude-sonnet-custom",
                "pool",
            ),
        )?;
        upsert_binding(conn, ProviderTarget::OpenCode, "sgw_opencode")?;
        Ok(())
    })?;
    crate::commands::providers::push_bound_gateway_catalogs(&h.state).await?;
    let path = paths::get_opencode_config_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        text.contains("direct.example.test") || text.contains("direct"),
        "independent OpenCode provider must remain in live config: {path:?}"
    );
    assert!(
        text.contains("ai-switcher") || text.contains("15828"),
        "binding must append the Auto entry: {path:?} {text}"
    );
    Ok(())
}

pub async fn sg_p0_protocol_responses_translation_roundtrip(_h: &Harness) -> AppResult<()> {
    let responses_body = json!({
        "model": "gpt-5.6-luna",
        "instructions": "You are a helpful coding assistant.",
        "input": [
            {
                "role": "user",
                "content": "Inspect the repository files."
            },
            {
                "role": "assistant",
                "content": "I will inspect the workspace files now."
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}"
            },
            {
                "type": "function_call",
                "call_id": "call_2",
                "name": "read_file",
                "arguments": "{\"path\":\"Cargo.toml\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "README contents"
            },
            {
                "type": "function_call_output",
                "call_id": "call_2",
                "output": "Cargo.toml contents"
            },
            {
                "role": "developer",
                "content": "Keep responses concise."
            },
            {
                "role": "user",
                "content": "What did you find?"
            }
        ],
        "tools": [
            {
                "type": "function",
                "name": "read_file",
                "description": "Read file contents",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        ]
    });

    let chat_body = crate::proxy::codex_chat::responses_to_chat_completions_body(&responses_body)
        .map_err(|error| {
        crate::error::AppError::Other(format!(
            "responses_to_chat_completions_body failed: {error}"
        ))
    })?;

    let recorded_body = Arc::new(tokio::sync::Mutex::new(None::<Value>));
    let payload_sink = Arc::clone(&recorded_body);

    let mock_app = Router::new()
        .route(
            "/v1/chat/completions",
            post(
                |State(sink): State<Arc<tokio::sync::Mutex<Option<Value>>>>,
                 axum::Json(body): axum::Json<Value>| async move {
                    let assistants: Vec<&Value> = body["messages"].as_array()
                        .into_iter().flatten()
                        .filter(|message| message["role"] == "assistant")
                        .collect();
                    let valid = assistants.len() == 1
                        && assistants[0]["content"] == "I will inspect the workspace files now."
                        && assistants[0]["tool_calls"].as_array().is_some_and(|calls| {
                            calls.len() == 2 && calls[0]["id"] == "call_1" && calls[1]["id"] == "call_2"
                        });
                    if !valid {
                        return (StatusCode::BAD_REQUEST, axum::Json(json!({
                            "error": "assistant commentary and tool_calls must share one turn"
                        })));
                    }
                    let mut guard = sink.lock().await;
                    *guard = Some(body);
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "id": "chatcmpl_mock_protocol",
                            "object": "chat.completion",
                            "created": 1700000000,
                            "model": "gpt-5.6-luna",
                            "choices": [{
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Mock completion response"
                                },
                                "finish_reason": "stop"
                            }],
                            "usage": {
                                "prompt_tokens": 20,
                                "completion_tokens": 8,
                                "total_tokens": 28
                            }
                        })),
                    )
                },
            ),
        )
        .with_state(payload_sink);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| crate::error::AppError::Io(format!("bind mock chat server: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| crate::error::AppError::Io(format!("read local addr: {e}")))?
        .port();

    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, mock_app).await;
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| crate::error::AppError::Other(format!("mock client: {error}")))?;
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let send_future = client.post(&url).json(&chat_body).send();
    let response = match tokio::time::timeout(Duration::from_secs(5), send_future).await {
        Ok(Ok(res)) => res,
        Ok(Err(e)) => {
            server_handle.abort();
            return Err(crate::error::AppError::Other(format!(
                "POST to local mock failed: {e}"
            )));
        }
        Err(_) => {
            server_handle.abort();
            return Err(crate::error::AppError::Other(
                "POST to local mock timed out after 5s".into(),
            ));
        }
    };

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "local mock chat completions must respond 200 OK"
    );

    server_handle.abort();

    let captured = recorded_body
        .lock()
        .await
        .clone()
        .expect("mock server must have captured chat completions payload");

    let messages = captured
        .get("messages")
        .and_then(Value::as_array)
        .expect("captured chat completions payload must have messages");

    let assistant_messages: Vec<&Value> = messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        .collect();

    assert!(
        !assistant_messages.is_empty(),
        "chat completions payload must have at least one assistant message"
    );

    let unified_assistant = assistant_messages.iter().find(|m| {
        let has_text = match m.get("content") {
            Some(Value::String(s)) => !s.trim().is_empty(),
            Some(Value::Array(arr)) => !arr.is_empty(),
            _ => false,
        };
        let call_count = m
            .get("tool_calls")
            .and_then(Value::as_array)
            .map_or(0, |calls| calls.len());
        has_text && call_count >= 2
    });

    assert!(
        unified_assistant.is_some(),
        "expected single assistant message containing commentary text and multiple tool_calls: {assistant_messages:?}"
    );
    assert_eq!(
        assistant_messages.len(),
        1,
        "must not split assistant commentary and tool_calls into separate assistant turns"
    );

    let gemini_parts =
        crate::antigravity::map::responses::responses_to_gemini_request(&responses_body, None)
            .map_err(|error| {
                crate::error::AppError::Other(format!(
                    "responses_to_gemini_request failed: {error}"
                ))
            })?;
    let gemini_req = &gemini_parts.request;

    let system_text = gemini_req
        .get("systemInstruction")
        .expect("gemini request must contain systemInstruction")
        .to_string();
    assert!(system_text.contains("You are a helpful coding assistant."));

    let gemini_contents = gemini_req
        .get("contents")
        .and_then(Value::as_array)
        .expect("gemini request must have contents array");

    let model_contents: Vec<&Value> = gemini_contents
        .iter()
        .filter(|content| content.get("role").and_then(Value::as_str) == Some("model"))
        .collect();
    assert_eq!(model_contents.len(), 1);
    let model_parts = model_contents[0]["parts"]
        .as_array()
        .expect("model content must have parts");
    assert!(model_parts.iter().any(|part| {
        part.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("inspect the workspace"))
    }));
    assert_eq!(
        model_parts
            .iter()
            .filter(|part| part.get("functionCall").is_some())
            .count(),
        2
    );

    let response_content_idx = gemini_contents
        .iter()
        .position(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("functionResponse").is_some())
                })
        })
        .expect("tool responses must be preserved");
    let reminder_content_idx = gemini_contents
        .iter()
        .position(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("text").and_then(Value::as_str).is_some_and(|text| {
                            text == "<system-reminder>\nKeep responses concise.\n</system-reminder>"
                        })
                    })
                })
        })
        .expect("mid-session developer message must become a reminder");
    assert!(response_content_idx <= reminder_content_idx);

    let follow_up_idx = gemini_contents
        .iter()
        .position(|content| content.to_string().contains("What did you find?"))
        .expect("follow-up user message must be preserved");
    assert!(response_content_idx < follow_up_idx);
    assert!(!system_text.contains("Keep responses concise."));

    Ok(())
}

fn fail(error: impl std::fmt::Display) -> ! {
    panic!("{error}");
}

#[tokio::test]
async fn sg_regress_independent_not_stolen_live_env() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_regress_independent_not_stolen(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_regress_picking_profile_does_not_bind() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_regress_no_autobind(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_regress_auto_current_writes_and_clears_15828() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_regress_auto_current_writes_gateway(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_p0_simulate_auto_uses_default_mode() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_p0_simulate_default_mode(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_p0_usage_counts_innermost_hop() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_p0_usage_inner_hop(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_p0_opencode_bind_appends_auto() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_p0_catalog_bind_appends_auto(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}

#[tokio::test]
async fn sg_p0_protocol_responses_roundtrip() {
    let harness = Harness::new().unwrap_or_else(|error| fail(error));
    sg_p0_protocol_responses_translation_roundtrip(&harness)
        .await
        .unwrap_or_else(|error| fail(error));
}
