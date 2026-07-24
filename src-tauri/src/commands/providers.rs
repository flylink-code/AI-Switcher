//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::config::{claude_code, claude_desktop};
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    ConnectionTestResult, LiveProviderInfo, ModelDiscoveryResult, Provider,
    ProviderExportBundle, ProviderExportEntry, ProviderImportResult, ProviderInput,
    ProviderTarget, ProtocolType,
};
use crate::store::AppState;
use chrono::Utc;
use reqwest::header;
use serde_json::Value;
use std::collections::BTreeMap;


#[tauri::command]
pub fn list_providers(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Vec<Provider>> {
    state.db.with_conn(|conn| dao::list_providers(conn, target))
}

#[tauri::command]
pub fn get_current_provider(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<Option<Provider>> {
    state.db.with_conn(|conn| dao::get_current_provider(conn, target))
}

#[tauri::command]
pub fn create_provider(input: ProviderInput, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    state.db.with_conn(|conn| dao::upsert_provider(conn, &input))
}

#[tauri::command]
pub async fn update_provider(input: ProviderInput, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    if input.id.is_none() {
        return Err(AppError::Config("更新供应商时缺少 id".to_string()));
    }
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if provider.is_current {
        apply_target_provider(&provider, &state).await?;
    }
    Ok(provider)
}

#[tauri::command]
pub fn delete_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::delete_provider(conn, &id))
}

/// Activate a provider only for the application that owns it.
#[tauri::command]
pub async fn switch_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<Provider> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let preflight = test_provider_impl(&provider, &state).await?;
    if !preflight.ok {
        return Err(AppError::Config(format!("连接验证失败（{}）：{}", preflight.category, preflight.message)));
    }
    apply_target_provider(&provider, &state).await?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &id))?;
    Ok(provider)
}

/// Test an already-stored provider without exposing its credential to the UI.
#[tauri::command]
pub async fn test_provider_connection(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    test_provider_impl(&provider, &state).await
}

/// Try the standard model-list endpoint. Failure is non-fatal: providers that
/// do not expose it can still use a manually entered model name.
#[tauri::command]
pub async fn discover_provider_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let key = state.db.with_conn(|conn| {
        dao::resolve_api_key(conn, &provider.id)?.ok_or_else(|| AppError::Config("供应商未配置 API Key".to_string()))
    })?;
    let checked_at = Utc::now().timestamp_millis();
    let url = endpoint_url(&provider.base_url, "/v1/models");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Other(format!("创建连接测试客户端失败: {e}")))?;
    let response = client
        .get(url)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header("x-api-key", key)
        .send()
        .await;
    let (models, message) = match response {
        Ok(response) if response.status().is_success() => {
            let bytes = response.bytes().await.unwrap_or_default();
            let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let models = value
                .get("data")
                .or_else(|| value.get("models"))
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| {
                    item.get("id").or_else(|| item.get("name")).and_then(Value::as_str)
                }).map(str::to_string).collect())
                .unwrap_or_default();
            (models, "模型列表已更新".to_string())
        }
        Ok(response) => (Vec::new(), format!("供应商不支持模型发现（HTTP {}）", response.status().as_u16())),
        Err(_) => (Vec::new(), "无法连接模型发现端点".to_string()),
    };
    let models_json = serde_json::to_string(&models)?;
    state.db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO provider_models (provider_id, models_json, checked_at) VALUES (?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET models_json = excluded.models_json, checked_at = excluded.checked_at",
            rusqlite::params![provider.id, models_json, checked_at],
        )?;
        Ok(())
    })?;
    Ok(ModelDiscoveryResult { models, message, checked_at })
}

#[tauri::command]
pub fn switch_to_official(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    match target {
        ProviderTarget::ClaudeCode => restore_code_ownership(&state)?,
        ProviderTarget::ClaudeDesktop => restore_desktop_ownership(&state)?,
    }
    state.db.with_conn(|conn| dao::clear_current_provider(conn, target))
}

#[tauri::command]
pub fn reorder_providers(ordered_ids: Vec<String>, target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::reorder_providers(conn, &ordered_ids, target))
}

/// Import a live third-party configuration into its matching application list.
#[tauri::command]
pub fn import_live_config(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let live = match target {
        ProviderTarget::ClaudeCode => claude_code::read_current_live_provider()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::read_current_live_provider()?,
    };
    let Some(live) = live else {
        return Ok(());
    };
    import_live_provider(live, target, &state)
}

/// Export provider metadata only. API keys and keyring references are never
/// included in this payload.
#[tauri::command]
pub fn export_providers(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<String> {
    let providers = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    let bundle = ProviderExportBundle {
        version: 1,
        providers: providers.into_iter().map(|provider| ProviderExportEntry {
            name: provider.name,
            base_url: provider.base_url,
            model: provider.model,
            protocol_type: provider.protocol_type,
            target_app: provider.target_app,
            notes: provider.notes,
        }).collect(),
    };
    Ok(serde_json::to_string_pretty(&bundle)?)
}

/// Import an exported bundle non-destructively. Existing matching providers are
/// skipped and no imported record contains a credential.
#[tauri::command]
pub fn import_providers_json(json: String, state: tauri::State<'_, AppState>) -> AppResult<ProviderImportResult> {
    let bundle: ProviderExportBundle = serde_json::from_str(&json)
        .map_err(|_| AppError::Config("供应商导入文件无效".to_string()))?;
    if bundle.version != 1 {
        return Err(AppError::Config(format!("不支持的供应商导入版本: {}", bundle.version)));
    }
    let mut imported = 0;
    let mut skipped = 0;
    for entry in bundle.providers {
        let existing = state.db.with_conn(|conn| dao::list_providers(conn, entry.target_app))?;
        if existing.iter().any(|provider| {
            provider.name == entry.name && provider.base_url == entry.base_url
        }) {
            skipped += 1;
            continue;
        }
        state.db.with_conn(|conn| dao::upsert_provider(conn, &ProviderInput {
            id: None,
            name: entry.name,
            base_url: entry.base_url,
            api_key: String::new(),
            clear_api_key: false,
            model: entry.model,
            protocol_type: entry.protocol_type,
            target_app: entry.target_app,
            notes: entry.notes,
        }))?;
        imported += 1;
    }
    Ok(ProviderImportResult { imported, skipped })
}

async fn apply_target_provider(provider: &Provider, state: &AppState) -> AppResult<()> {
    // Provider rows carry only a keyring reference. Hydrate a short-lived clone
    // for config writing; it is never serialized or persisted.
    let mut runtime_provider = provider.clone();
    runtime_provider.api_key = state.db.with_conn(|conn| {
        dao::resolve_api_key(conn, &provider.id)?.ok_or_else(|| {
            AppError::Config("供应商未配置 API Key，无法切换".to_string())
        })
    })?;
    let proxy_port = get_saved_proxy_port(state);
    match runtime_provider.target_app {
        ProviderTarget::ClaudeCode => {
            let ownership = prepare_code_ownership(
                &runtime_provider,
                state,
                runtime_provider.protocol_type == ProtocolType::Proxy,
                proxy_port,
            )?;
            if runtime_provider.protocol_type == ProtocolType::Proxy {
                state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeCode).await?;
                claude_code::apply_provider_to_settings_via_proxy(&runtime_provider, proxy_port)?;
            } else {
                claude_code::apply_provider_to_settings(&runtime_provider)?;
            }
            commit_code_ownership(state, ownership)
        }
        ProviderTarget::ClaudeDesktop => {
            let original_applied_id = prepare_desktop_ownership(state)?;
            if runtime_provider.protocol_type == ProtocolType::Proxy {
                state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeDesktop).await?;
            }
            claude_desktop::apply_provider(&runtime_provider, proxy_port)?;
            commit_desktop_ownership(state, original_applied_id)
        }
    }
}

const CODE_OWNERSHIP_KEY: &str = "p7.code_config_ownership";
const CODE_MANAGED_KEYS: [&str; 4] = [
    "ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CodeOwnership {
    before: BTreeMap<String, Option<Value>>,
    written: BTreeMap<String, Option<Value>>,
}

fn code_managed_fields() -> AppResult<BTreeMap<String, Option<Value>>> {
    let path = crate::config::get_claude_settings_path();
    let settings = if path.exists() {
        serde_json::from_slice::<Value>(&std::fs::read(path)?)?
    } else {
        Value::Object(Default::default())
    };
    let env = settings.get("env").and_then(Value::as_object);
    Ok(CODE_MANAGED_KEYS.into_iter().map(|key| {
        (key.to_string(), env.and_then(|map| map.get(key)).cloned())
    }).collect())
}

fn expected_code_fields(provider: &Provider, proxy: bool, port: u16) -> BTreeMap<String, Option<Value>> {
    let mut values = BTreeMap::new();
    values.insert("ANTHROPIC_BASE_URL".to_string(), Some(Value::String(if proxy {
        format!("http://127.0.0.1:{port}")
    } else { provider.base_url.clone() })));
    values.insert("ANTHROPIC_AUTH_TOKEN".to_string(), Some(Value::String(if proxy {
        "local-proxy-code".to_string()
    } else { provider.api_key.clone() })));
    values.insert("ANTHROPIC_API_KEY".to_string(), None);
    values.insert("ANTHROPIC_MODEL".to_string(), if provider.model.is_empty() {
        None
    } else { Some(Value::String(provider.model.clone())) });
    values
}

fn prepare_code_ownership(provider: &Provider, state: &AppState, proxy: bool, port: u16) -> AppResult<CodeOwnership> {
    let current = code_managed_fields()?;
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        let mut ownership: CodeOwnership = serde_json::from_str(&raw)
            .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全切换".to_string()))?;
        if ownership.written != current {
            return Err(AppError::Config("检测到 Claude Code 配置已被外部修改，已拒绝覆盖".to_string()));
        }
        ownership.written = expected_code_fields(provider, proxy, port);
        Ok(ownership)
    } else {
        Ok(CodeOwnership { before: current, written: expected_code_fields(provider, proxy, port) })
    }
}

fn commit_code_ownership(state: &AppState, ownership: CodeOwnership) -> AppResult<()> {
    state.db.with_conn(|conn| {
        set_setting(conn, CODE_OWNERSHIP_KEY, &serde_json::to_string(&ownership)?)
    })
}

fn restore_code_ownership(state: &AppState) -> AppResult<()> {
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return claude_code::clear_provider_from_settings();
    };
    let ownership: CodeOwnership = serde_json::from_str(&raw)
        .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全恢复".to_string()))?;
    if code_managed_fields()? != ownership.written {
        return Err(AppError::Config("检测到 Claude Code 配置已被外部修改，已拒绝覆盖".to_string()));
    }
    claude_code::restore_managed_fields(&ownership.before)?;
    state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""))
}

const DESKTOP_OWNERSHIP_KEY: &str = "p7.desktop_original_applied_id";
const DESKTOP_PROFILE_ID: &str = "claude-switcher";

fn prepare_desktop_ownership(state: &AppState) -> AppResult<Option<String>> {
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        if claude_desktop::current_applied_id()?.as_deref() != Some(DESKTOP_PROFILE_ID) {
            return Err(AppError::Config("检测到 Claude Desktop 配置已被外部修改，已拒绝覆盖".to_string()));
        }
        Ok(serde_json::from_str(&raw)?)
    } else {
        claude_desktop::current_applied_id()
    }
}

fn commit_desktop_ownership(state: &AppState, original_applied_id: Option<String>) -> AppResult<()> {
    state.db.with_conn(|conn| {
        set_setting(conn, DESKTOP_OWNERSHIP_KEY, &serde_json::to_string(&original_applied_id)?)
    })
}

fn restore_desktop_ownership(state: &AppState) -> AppResult<()> {
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return claude_desktop::clear_provider();
    };
    if claude_desktop::current_applied_id()?.as_deref() != Some(DESKTOP_PROFILE_ID) {
        return Err(AppError::Config("检测到 Claude Desktop 配置已被外部修改，已拒绝覆盖".to_string()));
    }
    let original: Option<String> = serde_json::from_str(&raw)?;
    claude_desktop::clear_provider_restoring_applied_id(original)?;
    state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""))
}

async fn test_provider_impl(provider: &Provider, state: &AppState) -> AppResult<ConnectionTestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let result = match state.db.with_conn(|conn| dao::resolve_api_key(conn, &provider.id)) {
        Ok(Some(key)) if !provider.model.trim().is_empty() => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| AppError::Other(format!("创建连接测试客户端失败: {e}")))?;
            let payload = serde_json::json!({
                "model": provider.model.trim(), "max_tokens": 1, "stream": false,
                "messages": [{"role": "user", "content": "ping"}]
            });
            let response = client
                .post(endpoint_url(&provider.base_url, "/v1/messages"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header("x-api-key", key)
                .body(serde_json::to_vec(&payload)?)
                .send()
                .await;
            classify_test_response(response, checked_at)
        }
        Ok(Some(_)) => ConnectionTestResult { ok: false, category: "model".to_string(), message: "请先填写模型名称".to_string(), checked_at },
        Ok(None) => ConnectionTestResult { ok: false, category: "authentication".to_string(), message: "供应商未配置 API Key".to_string(), checked_at },
        Err(_) => ConnectionTestResult { ok: false, category: "credential".to_string(), message: "无法读取系统凭据库中的 API Key".to_string(), checked_at },
    };
    state.db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO provider_health (provider_id, status, detail, checked_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET status = excluded.status, detail = excluded.detail, checked_at = excluded.checked_at",
            rusqlite::params![provider.id, if result.ok { "healthy" } else { "error" }, result.message, result.checked_at],
        )?;
        Ok(())
    })?;
    Ok(result)
}

fn classify_test_response(
    response: Result<reqwest::Response, reqwest::Error>, checked_at: i64,
) -> ConnectionTestResult {
    match response {
        Ok(response) if response.status().is_success() => ConnectionTestResult {
            ok: true, category: "ok".to_string(), message: "连接验证成功".to_string(), checked_at,
        },
        Ok(response) => {
            let status = response.status().as_u16();
            let (category, message) = match status {
                401 | 403 => ("authentication", "API Key 被拒绝"),
                404 | 405 => ("protocol", "供应商不支持 Anthropic /v1/messages 端点"),
                400 | 422 => ("model", "模型不可用或不兼容"),
                _ => ("upstream", "上游服务返回错误"),
            };
            ConnectionTestResult { ok: false, category: category.to_string(), message: message.to_string(), checked_at }
        }
        Err(error) if error.is_timeout() => ConnectionTestResult {
            ok: false, category: "network".to_string(), message: "连接测试超时".to_string(), checked_at,
        },
        Err(_) => ConnectionTestResult {
            ok: false, category: "network".to_string(), message: "无法连接供应商服务".to_string(), checked_at,
        },
    }
}

fn endpoint_url(base_url: &str, endpoint: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), endpoint)
}

fn import_live_provider(live: LiveProviderInfo, target: ProviderTarget, state: &AppState) -> AppResult<()> {
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    if let Some(provider) = existing.iter().find(|p| p.base_url == live.base_url) {
        state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        return Ok(());
    }
    let input = ProviderInput {
        id: None,
        name: "当前配置（已导入）".to_string(),
        base_url: live.base_url,
        api_key: live.auth_token,
        clear_api_key: false,
        model: live.model,
        protocol_type: ProtocolType::Anthropic,
        target_app: target,
        notes: "从当前 Claude Code 配置导入".to_string(),
    };
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))
}

fn get_saved_proxy_port(state: &AppState) -> u16 {
    state.db.with_conn(|conn| get_setting(conn, "proxy_port"))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(15821)
}
