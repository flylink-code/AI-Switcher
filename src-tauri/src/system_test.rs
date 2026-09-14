//! L1 system scenarios. Run with:
//! `cargo test --lib system_test -- --test-threads=1 --nocapture`

use std::fs;
use std::sync::Arc;

use serde_json::{json, Value};
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
