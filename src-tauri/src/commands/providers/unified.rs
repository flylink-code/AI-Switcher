/// 显式切换连接模式时串行化绑定和配置写出，避免两个点击互相覆盖快照。
pub(crate) fn agent_connection_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) fn validate_direct_provider_for_target(
    target: ProviderTarget,
    provider: &Provider,
) -> AppResult<()> {
    validate_target_protocol(target, provider.protocol_type)?;
    if provider.is_codex_oauth()
        || (target == ProviderTarget::ClaudeCode
            && provider.protocol_type != ProtocolType::Anthropic)
        || (target == ProviderTarget::Codex && provider.protocol_type == ProtocolType::Anthropic)
    {
        return Err(AppError::Config(
            "此上游需要协议转换或 OAuth 认证，请通过智能网关使用；直连不启动本地代理".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_upstream_update_for_direct_bindings(
    conn: &rusqlite::Connection,
    upstream_id: &str,
    new_kind: ProviderKind,
    new_protocol: ProtocolType,
) -> AppResult<()> {
    let bindings = crate::database::dao::gateway::list_bindings(conn)?;
    for binding in bindings {
        if binding.mode == "direct" && binding.direct_upstream_id == upstream_id {
            validate_target_protocol(binding.target_app, new_protocol)?;
            if new_kind == ProviderKind::CodexOauth
                || (binding.target_app == ProviderTarget::ClaudeCode
                    && new_protocol != ProtocolType::Anthropic)
                || (binding.target_app == ProviderTarget::Codex
                    && new_protocol == ProtocolType::Anthropic)
            {
                return Err(AppError::Config(format!(
                    "该上游正被 {} 直连使用，修改后的协议/认证类型需要协议转换或反向代理，请通过智能网关使用，或先将该 Agent 切换为其它直连上游后再编辑",
                    binding.target_app.as_str()
                )));
            }
        }
    }
    Ok(())
}

struct AgentConnectionSnapshot {
    binding: Option<crate::database::dao::gateway::GatewayBinding>,
    current_id: Option<String>,
}

impl AgentConnectionSnapshot {
    fn capture(state: &AppState, target: ProviderTarget) -> AppResult<Self> {
        state.db.with_conn(|conn| {
            Ok(Self {
                binding: crate::database::dao::gateway::binding_for_target(conn, target)?,
                current_id: dao::get_current_provider(conn, target)?.map(|provider| provider.id),
            })
        })
    }

    fn restore(self, state: &AppState, target: ProviderTarget) -> AppResult<()> {
        state.db.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM gateway_bindings WHERE target_app=?1", [target.as_str()])?;
            if let Some(binding) = self.binding {
                tx.execute(
                    "INSERT INTO gateway_bindings (target_app,entry_token,provider_id,created_at,profile_id,mode,direct_upstream_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![target.as_str(), binding.entry_token, binding.provider_id, binding.created_at, binding.profile_id, binding.mode, binding.direct_upstream_id],
                )?;
            }
            dao::clear_current_provider(&tx, target)?;
            if let Some(id) = self.current_id {
                dao::set_current_provider(&tx, &id)?;
            }
            tx.commit()?;
            Ok(())
        })
    }
}

fn direct_connection_upstream(
    state: &AppState,
    target: ProviderTarget,
) -> AppResult<Option<String>> {
    state.db.with_read_conn(|conn| {
        Ok(
            crate::database::dao::gateway::binding_for_target(conn, target)?
                .filter(|binding| binding.mode == "direct")
                .map(|binding| binding.direct_upstream_id),
        )
    })
}

/// 从全局上游选择直连；只改变连接和 live 配置，不创建按 Agent 复制的供应商。
#[tauri::command]
pub async fn set_agent_direct(
    target: ProviderTarget,
    upstream_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    set_agent_direct_for_target(target, &upstream_id, Some(&app), &state).await
}

pub(crate) async fn set_agent_direct_for_target<R: tauri::Runtime>(
    target: ProviderTarget,
    upstream_id: &str,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<Provider> {
    let _guard = agent_connection_lock().lock().await;
    let provider = state.db.with_conn(|conn| {
        crate::database::dao::gateway::provider_from_upstream(conn, upstream_id.trim(), target)
    })?;
    validate_direct_provider_for_target(target, &provider)?;
    let old = AgentConnectionSnapshot::capture(state, target)?;
    let result = async {
        state.db.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            crate::database::dao::gateway::set_direct_binding(&tx, target, &provider.id)?;
            dao::clear_current_provider(&tx, target)?;
            tx.commit()?;
            Ok(())
        })?;
        apply_runtime_provider(&provider, app, state).await
    }
    .await;
    if let Err(error) = result {
        return restore_connection_failure(old, state, target, error).await;
    }
    crate::catalog::invalidate_view_cache();
    Ok(provider)
}

pub(crate) async fn set_agent_gateway_for_target<R: tauri::Runtime>(
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<Provider> {
    let _guard = agent_connection_lock().lock().await;
    let enabled = state
        .db
        .with_conn(|conn| crate::database::dao::gateway::list_upstream_providers(conn, false))?;
    if enabled.is_empty() {
        return Err(AppError::Config("请先添加至少一个供应商".into()));
    }
    ensure_reverse_gateways_for_pool(state).await?;
    crate::gateway::service::mark_enabled(state.db.as_ref())?;
    if !crate::gateway::service::current_status().running {
        crate::gateway::service::start_via_state(state, None).await?;
    }
    let old = AgentConnectionSnapshot::capture(state, target)?;
    let result = async {
        let id = crate::gateway::smart_gateway_provider_id(target);
        state
            .db
            .with_conn(|conn| crate::database::dao::gateway::upsert_binding(conn, target, &id))?;
        let provider = ensure_smart_gateway_provider_row(state, target)?;
        if target.is_catalog_target() {
            state
                .db
                .with_conn(|conn| dao::clear_current_provider(conn, target))?;
        } else {
            state
                .db
                .with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        }
        apply_runtime_provider(&provider, app, state).await?;
        Ok(provider)
    }
    .await;
    match result {
        Ok(provider) => {
            crate::catalog::invalidate_view_cache();
            Ok(provider)
        }
        Err(error) => restore_connection_failure(old, state, target, error).await,
    }
}

async fn restore_connection_failure<T>(
    old: AgentConnectionSnapshot,
    state: &AppState,
    target: ProviderTarget,
    error: AppError,
) -> AppResult<T> {
    // 文件写出函数自身负责恢复文件；此处恢复此前的数据库连接状态。
    let restored = old.restore(state, target);
    crate::catalog::invalidate_view_cache();
    match restored {
        Ok(()) => Err(error),
        Err(restore_error) => Err(AppError::Config(format!(
            "{error}；连接状态回滚失败：{restore_error}"
        ))),
    }
}

pub(crate) async fn apply_runtime_provider<R: tauri::Runtime>(
    provider: &Provider,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<()> {
    let _ = apply_target_provider(provider, app, state).await?;
    Ok(())
}

pub(crate) async fn refresh_direct_upstream_locked<R: tauri::Runtime>(
    upstream_id: &str,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<()> {
    let bindings = state
        .db
        .with_read_conn(crate::database::dao::gateway::list_bindings)?;
    for binding in bindings {
        if binding.mode == "direct" && binding.direct_upstream_id == upstream_id {
            let provider = state.db.with_read_conn(|conn| {
                crate::database::dao::gateway::provider_from_upstream(
                    conn,
                    upstream_id,
                    binding.target_app,
                )
            })?;
            validate_direct_provider_for_target(binding.target_app, &provider)?;
            apply_runtime_provider(&provider, app, state).await?;
        }
    }
    Ok(())
}

pub(crate) async fn ensure_reverse_gateways_for_pool(state: &AppState) -> AppResult<()> {
    let providers = state.db.with_read_conn(|conn| {
        crate::database::dao::gateway::list_upstream_providers(conn, false)
    })?;
    for provider in providers {
        if provider.is_antigravity() || is_loopback_endpoint_with_port(&provider.base_url, 15830) {
            let mut p = provider.clone();
            p.provider_kind = ProviderKind::Antigravity;
            crate::commands::antigravity::ensure_gateway_running_for_provider(&p).await?;
        }
        if provider.is_kiro() || is_loopback_endpoint_with_port(&provider.base_url, 15831) {
            let mut p = provider.clone();
            p.provider_kind = ProviderKind::Kiro;
            crate::commands::kiro::ensure_gateway_running_for_provider(&p).await?;
        }
    }
    Ok(())
}

fn is_loopback_endpoint_with_port(base_url: &str, target_port: u16) -> bool {
    if let Ok(url) = url::Url::parse(base_url) {
        if is_live_upstream_loopback(&url) && url.port_or_known_default() == Some(target_port) {
            return true;
        }
    }
    false
}

#[tauri::command]
pub fn get_agent_connection_mode(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    state.db.with_read_conn(|conn| {
        let binding = crate::database::dao::gateway::binding_for_target(conn, target)?;
        Ok(match binding {
            Some(binding) if binding.mode == "direct" => "direct".to_string(),
            Some(_) if crate::database::dao::gateway::is_gateway_connection(conn, target) => {
                "gateway".to_string()
            }
            _ => "external".to_string(),
        })
    })
}

#[tauri::command]
pub async fn test_upstream_connection(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = state.db.with_read_conn(|conn| {
        crate::database::dao::gateway::get_upstream_provider(conn, &id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    if provider.is_codex_oauth() {
        return Ok(ConnectionTestResult {
            ok: false,
            category: "unsupported".into(),
            message: "OAuth 上游请通过智能网关发送对话验证；此 API Key 探测不适用".into(),
            checked_at: Utc::now().timestamp_millis(),
            latency_ms: None,
        });
    }
    let key = state
        .db
        .with_read_conn(|conn| dao::resolve_api_key(conn, &id))?
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_else(|| {
            if provider.is_antigravity() {
                crate::antigravity::gateway::builtin_api_key()
            } else if provider.is_kiro() {
                crate::kiro::gateway::builtin_api_key()
            } else {
                String::new()
            }
        });
    let result = test_provider_with_key(&provider, key, state.db.as_ref(), true).await?;
    if result.ok {
        crate::gateway::health::record_model_success(&provider.id, &provider.model, result.latency_ms.map(|ms| ms.min(i64::MAX as u64) as i64));
    }
    Ok(result)
}

fn sanitize_export_custom_headers(
    headers: Option<std::collections::HashMap<String, String>>,
) -> Option<std::collections::HashMap<String, String>> {
    let headers = headers?;
    let filtered: std::collections::HashMap<String, String> = headers
        .into_iter()
        .filter(|(key, _)| {
            let lower = key.trim().to_ascii_lowercase();
            !lower.contains("auth")
                && !lower.contains("key")
                && !lower.contains("token")
                && !lower.contains("secret")
                && !lower.contains("cookie")
                && !lower.contains("password")
                && !lower.contains("credential")
                && !lower.contains("bearer")
        })
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

/// 导出仅包含可移植元数据，不包含 Key、keyring 引用或 OAuth 账号标识，敏感 custom headers 会被过滤。
#[tauri::command]
pub fn export_gateway_upstreams(state: tauri::State<'_, AppState>) -> AppResult<String> {
    let providers = state.db.with_read_conn(|conn| {
        crate::database::dao::gateway::list_upstream_providers(conn, true)
    })?;
    let bundle = ProviderExportBundle {
        version: 1,
        providers: providers
            .into_iter()
            .map(|provider| ProviderExportEntry {
                name: provider.name,
                base_url: provider.base_url,
                model: provider.model,
                model_context_window: provider.model_context_window,
                web_search_enabled: provider.web_search_enabled,
                model_mapping: provider.model_mapping,
                protocol_type: provider.protocol_type,
                target_app: provider.target_app,
                notes: provider.notes,
                failover_group: provider.failover_group,
                failover_models: provider.failover_models,
                hidden_models: provider.hidden_models,
                thinking_config: provider.thinking_config,
                custom_headers: sanitize_export_custom_headers(provider.custom_headers),
            })
            .collect(),
    };
    Ok(serde_json::to_string_pretty(&bundle)?)
}

#[tauri::command]
pub async fn import_live_config_as_upstreams(
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProviderImportResult> {
    let mut inputs = Vec::new();
    if target == ProviderTarget::OpenCode {
        for live in opencode::read_live_providers()? {
            let model = live
                .current_model
                .or_else(|| live.models.first().cloned())
                .unwrap_or_default();
            inputs.push(live_upstream_input(
                target,
                live.name,
                live.base_url,
                live.auth_token,
                model,
                live.protocol_type,
                live.models,
                ClaudeModelMapping::default(),
            ));
        }
    } else {
        let live = match target {
            ProviderTarget::ClaudeCode => claude_code::read_current_live_provider()?,
            ProviderTarget::Codex => codex::read_current_upstream_for_import()?,
            _ => {
                return Err(AppError::Config(
                    "此 Agent 暂不支持读取 live 配置，请使用 JSON 导入".into(),
                ))
            }
        };
        if let Some(live) = live {
            let model_mapping = if target == ProviderTarget::ClaudeCode {
                live.model_mapping
            } else {
                ClaudeModelMapping::default()
            };
            inputs.push(live_upstream_input(
                target,
                format!("{}（导入）", target.as_str()),
                live.base_url,
                live.auth_token,
                live.model,
                live.protocol_type,
                Vec::new(),
                model_mapping,
            ));
        }
    }
    let mut imported = 0;
    let mut skipped = 0;
    for input in inputs {
        let url = url::Url::parse(&input.base_url)
            .map_err(|_| AppError::Config("配置中的供应商地址无效".into()))?;
        if is_live_upstream_loopback(&url)
            && url.port_or_known_default().is_some_and(|port| {
                (15821..=15828).contains(&port) || port == saved_smart_gateway_port(&state)
            })
        {
            skipped += 1;
            continue;
        }
        state.db.with_conn(|conn| {
            let existing = crate::database::dao::gateway::list_upstream_providers(conn, true)?;
            for provider in existing {
                if provider.protocol_type == input.protocol_type
                    && normalize_base_url(&provider.base_url)?
                        == normalize_base_url(&input.base_url)?
                {
                    let key = dao::resolve_api_key(conn, &provider.id)?.unwrap_or_default();
                    if key == input.api_key {
                        skipped += 1;
                        return Ok(());
                    }
                }
            }
            crate::database::dao::gateway::upsert_upstream(conn, &input)?;
            imported += 1;
            Ok(())
        })?;
    }
    crate::catalog::invalidate_view_cache();
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(ProviderImportResult { imported, skipped })
}

fn is_live_upstream_loopback(url: &url::Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    )
}

fn live_upstream_input(
    target: ProviderTarget,
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    protocol_type: ProtocolType,
    failover_models: Vec<String>,
    model_mapping: ClaudeModelMapping,
) -> ProviderInput {
    ProviderInput {
        id: None,
        name,
        base_url,
        api_key,
        clear_api_key: false,
        model,
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping,
        protocol_type,
        provider_kind: ProviderKind::Standard,
        auth_binding: String::new(),
        target_app: target,
        notes: "从 live 配置导入；未改写原配置".into(),
        failover_group: 0,
        failover_models,
        hidden_models: Vec::new(),
        thinking_config: None,
        custom_headers: None,
    }
}

#[tauri::command]
pub async fn import_gateway_upstreams_json(
    json: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProviderImportResult> {
    let bundle: ProviderExportBundle =
        serde_json::from_str(&json).map_err(|_| AppError::Config("供应商导入文件无效".into()))?;
    if bundle.version != 1 {
        return Err(AppError::Config(format!(
            "不支持的供应商导入版本: {}",
            bundle.version
        )));
    }
    let mut imported = 0;
    let mut skipped = 0;
    for entry in bundle.providers {
        let base_url = normalize_base_url(&entry.base_url)?;
        state.db.with_conn(|conn| {
            let existing = crate::database::dao::gateway::list_upstream_providers(conn, true)?;
            if existing
                .iter()
                .any(|provider| provider.name == entry.name && provider.base_url == base_url)
            {
                skipped += 1;
                return Ok(());
            }
            crate::database::dao::gateway::upsert_upstream(
                conn,
                &ProviderInput {
                    id: None,
                    name: entry.name,
                    base_url,
                    api_key: String::new(),
                    clear_api_key: false,
                    model: entry.model,
                    model_context_window: entry.model_context_window,
                    auto_review_model_override: None,
                    web_search_enabled: entry.web_search_enabled,
                    model_mapping: entry.model_mapping,
                    protocol_type: entry.protocol_type,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: entry.target_app,
                    notes: entry.notes,
                    failover_group: entry.failover_group,
                    failover_models: entry.failover_models,
                    hidden_models: entry.hidden_models,
                    thinking_config: entry.thinking_config,
                    custom_headers: entry.custom_headers,
                },
            )?;
            imported += 1;
            Ok(())
        })?;
    }
    crate::catalog::invalidate_view_cache();
    let _ = crate::commands::providers::push_bound_gateway_catalogs(&state).await;
    Ok(ProviderImportResult { imported, skipped })
}

#[cfg(test)]
mod unified_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_sanitize_export_custom_headers_strips_sensitive_keys() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret-token".to_string());
        headers.insert("X-Api-Key".to_string(), "sk-123456".to_string());
        headers.insert("Cookie".to_string(), "session=abc".to_string());
        headers.insert("Proxy-Authorization".to_string(), "Basic xyz".to_string());
        headers.insert("x-custom-env".to_string(), "production".to_string());
        headers.insert("User-Agent".to_string(), "CustomAgent/1.0".to_string());

        let sanitized = sanitize_export_custom_headers(Some(headers)).expect("should retain non-sensitive");
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized.get("x-custom-env").map(String::as_str), Some("production"));
        assert_eq!(sanitized.get("User-Agent").map(String::as_str), Some("CustomAgent/1.0"));
        assert!(!sanitized.contains_key("Authorization"));
        assert!(!sanitized.contains_key("X-Api-Key"));
        assert!(!sanitized.contains_key("Cookie"));
        assert!(!sanitized.contains_key("Proxy-Authorization"));
    }

    #[test]
    fn test_sanitize_export_custom_headers_none_when_all_sensitive() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret".to_string());
        headers.insert("secret-token".to_string(), "123".to_string());

        let sanitized = sanitize_export_custom_headers(Some(headers));
        assert!(sanitized.is_none());
    }

    fn sample_test_provider(name: &str, protocol: ProtocolType, kind: ProviderKind) -> Provider {
        Provider {
            id: "test_id".to_string(),
            name: name.to_string(),
            base_url: "https://example.test".to_string(),
            api_key: String::new(),
            api_key_set: false,
            model: "default-model".to_string(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: protocol,
            provider_kind: kind,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::ClaudeCode,
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
        }
    }

    #[test]
    fn test_validate_direct_provider_claude_code() {
        // Claude Code 使用 Anthropic 原生协议直连：放行
        let ok_provider = sample_test_provider("Anthropic 上游", ProtocolType::Anthropic, ProviderKind::Standard);
        assert!(validate_direct_provider_for_target(ProviderTarget::ClaudeCode, &ok_provider).is_ok());

        // Claude Code 尝试使用 OpenAI Chat 协议直连：跨协议直连应被拒绝
        let openai_provider = sample_test_provider("OpenAI 上游", ProtocolType::OpenAiChat, ProviderKind::Standard);
        let err = validate_direct_provider_for_target(ProviderTarget::ClaudeCode, &openai_provider).unwrap_err();
        assert!(err.to_string().contains("协议转换或 OAuth 认证"));

        // Claude Code 尝试使用 Codex OAuth 直连：OAuth 直连应被拒绝
        let oauth_provider = sample_test_provider("OAuth 上游", ProtocolType::Anthropic, ProviderKind::CodexOauth);
        let err = validate_direct_provider_for_target(ProviderTarget::ClaudeCode, &oauth_provider).unwrap_err();
        assert!(err.to_string().contains("协议转换或 OAuth 认证"));
    }

    #[test]
    fn test_validate_direct_provider_codex() {
        // Codex 使用 OpenAI Responses 原生协议直连：放行
        let ok_provider = sample_test_provider("Codex 上游", ProtocolType::OpenAiResponses, ProviderKind::Standard);
        assert!(validate_direct_provider_for_target(ProviderTarget::Codex, &ok_provider).is_ok());

        // Codex 尝试使用 Anthropic 协议直连：跨协议直连应被拒绝
        let anthropic_provider = sample_test_provider("Anthropic 上游", ProtocolType::Anthropic, ProviderKind::Standard);
        let err = validate_direct_provider_for_target(ProviderTarget::Codex, &anthropic_provider).unwrap_err();
        assert!(err.to_string().contains("协议转换或 OAuth 认证"));

        // Codex 尝试使用 Codex OAuth 直连：OAuth 直连应被拒绝
        let oauth_provider = sample_test_provider("OAuth 上游", ProtocolType::OpenAiResponses, ProviderKind::CodexOauth);
        let err = validate_direct_provider_for_target(ProviderTarget::Codex, &oauth_provider).unwrap_err();
        assert!(err.to_string().contains("协议转换或 OAuth 认证"));
    }

    #[test]
    fn test_validate_upstream_update_for_direct_bindings() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE gateway_bindings (
                target_app TEXT PRIMARY KEY,
                entry_token TEXT NOT NULL DEFAULT '',
                provider_id TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT 0,
                profile_id TEXT NOT NULL DEFAULT 'gprof_shared',
                mode TEXT NOT NULL DEFAULT 'gateway',
                direct_upstream_id TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();

        // 1. claude_code 处于直连模式，指向 'up_code'
        conn.execute(
            "INSERT INTO gateway_bindings (target_app, mode, direct_upstream_id) VALUES ('claude_code', 'direct', 'up_code');",
            [],
        )
        .unwrap();

        // 将 'up_code' 修改为 Anthropic 协议：允许
        assert!(validate_upstream_update_for_direct_bindings(
            &conn,
            "up_code",
            ProviderKind::Standard,
            ProtocolType::Anthropic,
        )
        .is_ok());

        // 将 'up_code' 修改为 OpenAI Chat 协议：claude_code 无法跨协议直连，应拒绝
        let err = validate_upstream_update_for_direct_bindings(
            &conn,
            "up_code",
            ProviderKind::Standard,
            ProtocolType::OpenAiChat,
        )
        .unwrap_err();
        assert!(err.to_string().contains("claude_code 直连使用"));

        // 将 'up_code' 修改为 CodexOauth：无法直连 OAuth，应拒绝
        let err = validate_upstream_update_for_direct_bindings(
            &conn,
            "up_code",
            ProviderKind::CodexOauth,
            ProtocolType::Anthropic,
        )
        .unwrap_err();
        assert!(err.to_string().contains("claude_code 直连使用"));

        // 2. codex 处于直连模式，指向 'up_codex'
        conn.execute(
            "INSERT INTO gateway_bindings (target_app, mode, direct_upstream_id) VALUES ('codex', 'direct', 'up_codex');",
            [],
        )
        .unwrap();

        // 将 'up_codex' 修改为 OpenAI Responses 协议：允许
        assert!(validate_upstream_update_for_direct_bindings(
            &conn,
            "up_codex",
            ProviderKind::Standard,
            ProtocolType::OpenAiResponses,
        )
        .is_ok());

        // 将 'up_codex' 修改为 Anthropic 协议：codex 无法直连 Anthropic，应拒绝
        let err = validate_upstream_update_for_direct_bindings(
            &conn,
            "up_codex",
            ProviderKind::Standard,
            ProtocolType::Anthropic,
        )
        .unwrap_err();
        assert!(err.to_string().contains("codex 直连使用"));

        // 3. 更新未被任何 direct 引用的上游 'up_unbound'：允许更新
        assert!(validate_upstream_update_for_direct_bindings(
            &conn,
            "up_unbound",
            ProviderKind::CodexOauth,
            ProtocolType::OpenAiChat,
        )
        .is_ok());
    }
}
