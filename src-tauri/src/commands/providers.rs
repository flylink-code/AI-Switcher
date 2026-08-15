//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::config::{claude_code, claude_desktop, codex, codex_provider_sync, opencode};
use crate::config::codex_provider_sync::CodexProviderSyncResult;
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, catalog_models_from_provider, copied_provider_input, normalize_base_url, protocol_endpoint_path, ClaudeModelMapping,
    ConnectionTestResult,
    EndpointSpeedtestResult, LiveProviderInfo, ModelDiscoveryResult, Provider, ProviderExportBundle,
    ProviderExportEntry, normalized_model_mapping, normalized_auto_review_model_override,
    validate_target_protocol, ProviderImportResult, ProviderInput, ProviderKind, ProviderTarget,
    ProtocolType,
};
use crate::store::AppState;
use chrono::Utc;
use reqwest::header;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tauri::Emitter;

const MODEL_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_DISCOVERED_MODELS: usize = 1_000;
const MAX_MODEL_NAME_CHARS: usize = 256;
const CODEX_OWNERSHIP_KEY: &str = "v040.codex_managed";
const OPENCODE_OWNERSHIP_KEY: &str = "v131.opencode_managed";
const PI_OWNERSHIP_KEY: &str = "v136.pi_managed";

/// A Codex switch updates config.toml, auth.json, and the model catalog as one
/// logical operation. Startup repair and a user click must not interleave their
/// snapshots, writes, or rollback decisions after an updater relaunch.
fn codex_switch_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

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
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    sync_catalog_target(&state, provider.target_app)?;
    Ok(provider)
}

/// Copy a configured provider (including API key) onto another Agent, adapting
/// protocol / Base URL / Claude role mapping for the destination.
#[tauri::command]
pub fn copy_provider_to_target(
    id: String,
    target: ProviderTarget,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let (source, api_key, existing_names) = state.db.with_conn(|conn| {
        let source = dao::get_provider(conn, &id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
        let api_key = dao::resolve_api_key(conn, &id)?.unwrap_or_default();
        let existing_names = dao::list_providers(conn, target)?
            .into_iter()
            .map(|provider| provider.name)
            .collect::<Vec<_>>();
        Ok((source, api_key, existing_names))
    })?;
    let input = copied_provider_input(&source, target, &existing_names, api_key)?;
    let cache = state
        .db
        .with_conn(|conn| dao::get_provider_model_cache(conn, &id))?;
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if let Some(cache) = cache {
        if !cache.models.is_empty() {
            state.db.with_conn(|conn| {
                dao::save_provider_model_cache(conn, &provider.id, &cache.models, cache.checked_at)
            })?;
        }
    }
    sync_catalog_target(&state, provider.target_app)?;
    Ok(provider)
}

#[tauri::command]
pub async fn update_provider(
    input: ProviderInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    if input.id.is_none() {
        return Err(AppError::Config("更新供应商时缺少 id".to_string()));
    }
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    if matches!(provider.target_app, ProviderTarget::OpenCode | ProviderTarget::Pi | ProviderTarget::Dsh) {
        sync_catalog_target(&state, provider.target_app)?;
    } else if provider.is_current {
        let _ = apply_target_provider(&provider, Some(&app), &state).await?;
    }
    Ok(provider)
}

#[tauri::command]
pub fn delete_provider(id: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?
            .map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    state.db.with_conn(|conn| dao::delete_provider(conn, &id))?;
    sync_catalog_target(&state, target)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchProviderResult {
    pub provider: Provider,
    pub session_sync: Option<CodexProviderSyncResult>,
    /// Codex-only hint for the frontend: `preserved_official_login` when the
    /// vendor key went into config.toml and auth.json kept its ChatGPT login,
    /// `official_login_required` when no official login exists and the Codex
    /// desktop app will hide custom models until the user logs in once.
    pub codex_notice: Option<&'static str>,
}

/// Activate a provider only for the application that owns it.
#[tauri::command]
pub async fn switch_provider(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<SwitchProviderResult> {
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let result = switch_provider_for_target(&id, target, Some(&app), &state).await?;
    schedule_provider_health_check(app, result.provider.clone(), Arc::clone(&state.db));
    Ok(result)
}

/// Shared provider switching service used by both IPC and tray actions.
/// Live configuration is switched locally first. Connectivity is checked in the
/// background so network latency never blocks the user's explicit selection.
pub async fn switch_provider_for_target<R: tauri::Runtime>(
    id: &str,
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<SwitchProviderResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    if provider.target_app != target {
        return Err(AppError::Config("供应商不属于此应用".to_string()));
    }
    if provider.is_current {
        let needs_proxy = provider.is_codex_oauth() || provider.requires_local_proxy();
        let proxy_running = state.proxy.lock().await.status_for(target).running;
        if needs_proxy == proxy_running {
            log::debug!(
                "跳过重复供应商切换: target={} provider={}",
                target.as_str(),
                id
            );
            return Ok(SwitchProviderResult {
                provider,
                session_sync: None,
                codex_notice: None,
            });
        }
        log::info!(
            "当前供应商代理状态不一致，执行修复式重应用: target={} provider={} needs_proxy={} running={}",
            target.as_str(),
            id,
            needs_proxy,
            proxy_running
        );
        let (mut snapshot, session_sync, codex_notice) =
            apply_target_provider(&provider, app, state).await?;
        let _ = snapshot.capture_last_written_files();
        return Ok(SwitchProviderResult {
            provider,
            session_sync,
            codex_notice,
        });
    }
    let started = Instant::now();
    let (snapshot, session_sync, codex_notice) = apply_target_provider(&provider, app, state).await?;
    let applied_ms = started.elapsed().as_millis();
    if let Err(error) = state.db.with_conn(|conn| dao::set_current_provider(conn, id)) {
        return rollback_switch(snapshot, state, error).await;
    }
    log::info!(
        "供应商快速切换完成: target={} provider={} apply={}ms total={}ms",
        target.as_str(),
        id,
        applied_ms,
        started.elapsed().as_millis()
    );
    Ok(SwitchProviderResult {
        provider,
        session_sync,
        codex_notice,
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderHealthUpdated {
    provider_id: String,
    target_app: ProviderTarget,
    ok: bool,
    category: String,
    message: String,
    checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
}

pub fn schedule_provider_health_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    provider: Provider,
    db: Arc<crate::database::Database>,
) {
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let key = db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        let result = test_provider_with_key(&provider, key, db.as_ref(), true).await;
        let result = match result {
            Ok(result) => result,
            Err(error) => ConnectionTestResult {
                ok: false,
                category: "internal".to_string(),
                message: error.to_string(),
                checked_at: Utc::now().timestamp_millis(),
                latency_ms: None,
            },
        };
        log::info!(
            "供应商后台验证完成: target={} provider={} ok={} duration={}ms",
            provider.target_app.as_str(),
            provider.id,
            result.ok,
            started.elapsed().as_millis()
        );
        let _ = app.emit(
            "provider-health-updated",
            ProviderHealthUpdated {
                provider_id: provider.id,
                target_app: provider.target_app,
                ok: result.ok,
                category: result.category,
                message: result.message,
                checked_at: result.checked_at,
                latency_ms: result.latency_ms,
            },
        );
    });
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

/// Test values currently entered in the form.  The supplied API key is kept
/// only in this request and is never written to SQLite, the keyring or logs.
#[tauri::command]
pub async fn test_provider_input(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = temporary_provider(&input, &state)?;
    test_provider_with_key(&provider, provider.api_key.clone(), state.db.as_ref(), false).await
}

/// Measure RTT to the provider Base URL with a lightweight HTTP request (no API key).
#[tauri::command]
pub async fn speedtest_provider_endpoint(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<EndpointSpeedtestResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    speedtest_base_url(&provider.base_url).await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorReport {
    pub provider_id: String,
    pub provider_name: String,
    pub target_app: ProviderTarget,
    pub ok: bool,
    pub category: String,
    pub message: String,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub quarantined: bool,
}

#[tauri::command]
pub async fn batch_diagnose_providers(
    target: Option<ProviderTarget>,
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ProviderDoctorReport>> {
    let targets = match target {
        Some(t) => vec![t],
        None => vec![
            ProviderTarget::ClaudeCode,
            ProviderTarget::Codex,
            ProviderTarget::OpenCode,
            ProviderTarget::Pi,
        ],
    };

    let mut providers = Vec::new();
    state.db.with_conn(|conn| {
        for t in targets {
            let mut list = dao::list_providers(conn, t)?;
            providers.append(&mut list);
        }
        Ok::<(), AppError>(())
    })?;

    let mut reports = Vec::with_capacity(providers.len());
    for provider in providers {
        let test_res = test_provider_impl(&provider, &state).await;
        let (ok, category, message, latency_ms, status_code) = match test_res {
            Ok(res) => {
                let code = if res.category == "authentication" {
                    Some(401u16)
                } else if res.ok {
                    Some(200u16)
                } else {
                    None
                };
                (res.ok, res.category, res.message, res.latency_ms, code)
            }
            Err(e) => (false, "system".to_string(), e.to_string(), None, None),
        };
        let quarantined = !ok
            && (category == "authentication"
                || status_code == Some(401)
                || status_code == Some(403)
                || provider.failover_group == 0);
        reports.push(ProviderDoctorReport {
            provider_id: provider.id,
            provider_name: provider.name,
            target_app: provider.target_app,
            ok,
            category,
            message,
            latency_ms,
            status_code,
            quarantined,
        });
    }

    Ok(reports)
}

#[tauri::command]
pub async fn quarantine_failed_providers(
    provider_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<usize> {
    let mut count = 0;
    for id in provider_ids {
        let res = state.db.with_conn(|conn| {
            if let Some(mut provider) = dao::get_provider(conn, &id)? {
                provider.failover_group = 0;
                if !provider.notes.contains("[已隔离]") {
                    if !provider.notes.is_empty() {
                        provider.notes.push_str(" ");
                    }
                    provider.notes.push_str("[已隔离: 401/403 鉴权异常]");
                }
                let input = ProviderInput {
                    id: Some(provider.id.clone()),
                    name: provider.name,
                    base_url: provider.base_url,
                    api_key: String::new(),
                    clear_api_key: false,
                    model: provider.model,
                    model_context_window: provider.model_context_window,
                    auto_review_model_override: provider.auto_review_model_override,
                    web_search_enabled: provider.web_search_enabled,
                    model_mapping: provider.model_mapping,
                    protocol_type: provider.protocol_type,
                    provider_kind: provider.provider_kind,
                    auth_binding: provider.auth_binding,
                    target_app: provider.target_app,
                    notes: provider.notes,
                    failover_group: 0,
                    failover_models: provider.failover_models,
                };
                let _ = dao::upsert_provider(conn, &input)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        if res {
            count += 1;
        }
    }
    Ok(count)
}

async fn speedtest_base_url(base_url: &str) -> AppResult<EndpointSpeedtestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let url = normalize_base_url(base_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|error| AppError::Other(format!("创建测速客户端失败: {error}")))?;
    let started = Instant::now();
    let response = client.get(&url).send().await;
    let latency_ms = Some(started.elapsed().as_millis() as u64);
    match response {
        Ok(response) => {
            let status = response.status();
            Ok(EndpointSpeedtestResult {
                ok: true,
                latency_ms,
                message: format!("RTT {} · HTTP {}", format_latency(latency_ms), status.as_u16()),
                checked_at,
                url,
            })
        }
        Err(error) => Ok(EndpointSpeedtestResult {
            ok: false,
            latency_ms,
            message: format!("RTT {} · {}", format_latency(latency_ms), sanitize_network_error(&error)),
            checked_at,
            url,
        }),
    }
}

fn format_latency(latency_ms: Option<u64>) -> String {
    match latency_ms {
        Some(ms) => format!("{ms} ms"),
        None => "—".to_string(),
    }
}

fn sanitize_network_error(error: &reqwest::Error) -> String {
    let text = error.to_string();
    if text.len() > 180 {
        format!("{}…", &text[..180])
    } else {
        text
    }
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
    let key = state.db.with_conn(|conn| dao::resolve_api_key(conn, &provider.id))?;
    let Some(key) = key else {
        if uses_antigravity_model_catalog(&provider) {
            return discover_provider_models_with_key(&provider, String::new(), &state, true).await;
        }
        return cached_or_empty_model_result(
            &provider.id,
            "供应商未配置 API Key",
            &state,
        );
    };
    discover_provider_models_with_key(&provider, key, &state, true).await
}

#[tauri::command]
pub fn get_cached_provider_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    state.db.with_conn(|conn| {
        if dao::get_provider(conn, &id)?.is_none() {
            return Err(AppError::Config(format!("供应商不存在: {id}")));
        }
        Ok(model_result_from_cache(
            dao::get_provider_model_cache(conn, &id)?,
            "已加载保存的模型列表",
            None,
        ))
    })
}

/// Discover models from an unsaved form without persisting its endpoint,
/// model, notes or newly entered credential.
#[tauri::command]
pub async fn discover_provider_models_input(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    let provider = temporary_provider(&input, &state)?;
    discover_provider_models_with_key(&provider, provider.api_key.clone(), &state, false).await
}

async fn discover_provider_models_with_key(
    provider: &Provider,
    key: String,
    state: &AppState,
    cache_result: bool,
) -> AppResult<ModelDiscoveryResult> {
    let checked_at = Utc::now().timestamp_millis();

    // Built-in / local Antigravity gateway: serve the live Cloud Code catalog.
    // Avoid HTTP /v1/models against loopback — system proxies (Clash etc.) often
    // return HTTP 502 for 127.0.0.1, and the gateway may not be running yet while
    // the user is still filling the provider form.
    if uses_antigravity_model_catalog(provider) {
        let models = antigravity_catalog_model_ids();
        if models.is_empty() {
            let error = "Antigravity 暂无可用模型，请先在网关页登录账号并刷新额度".to_string();
            return if cache_result {
                cached_or_empty_model_result(&provider.id, &error, state)
            } else {
                Ok(ModelDiscoveryResult {
                    models: Vec::new(),
                    message: error.clone(),
                    checked_at,
                    source: "none".to_string(),
                    stale: false,
                    expires_at: None,
                    error: Some(error),
                })
            };
        }
        if cache_result {
            state.db.with_conn(|conn| {
                dao::save_provider_model_cache(conn, &provider.id, &models, checked_at)
            })?;
        }
        return Ok(ModelDiscoveryResult {
            models,
            message: "已从 Antigravity Cloud Code 目录加载模型".to_string(),
            checked_at,
            source: "antigravity".to_string(),
            stale: false,
            expires_at: cache_result.then_some(checked_at + MODEL_CACHE_TTL_MS),
            error: None,
        });
    }

    let url = api_endpoint_url(&provider.base_url, "/v1/models")?;
    let client = discovery_http_client(&url)?;
    let response = client
        .get(url)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header("x-api-key", key)
        .send()
        .await;
    let discovered = match response {
        Ok(response) if response.status().is_success() => {
            match response.bytes().await {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(value) => {
                        let models = extract_model_ids(&value);
                        if models.is_empty() {
                            Err("供应商没有返回可用的模型".to_string())
                        } else {
                            Ok(models)
                        }
                    }
                    Err(_) => Err("模型发现响应不是有效 JSON".to_string()),
                },
                Err(_) => Err("读取模型发现响应失败".to_string()),
            }
        }
        Ok(response) => Err(format!(
            "供应商不支持模型发现（HTTP {}）",
            response.status().as_u16()
        )),
        Err(_) => Err("无法连接模型发现端点".to_string()),
    };
    match discovered {
        Ok(models) => {
            if cache_result {
                state.db.with_conn(|conn| {
                    dao::save_provider_model_cache(conn, &provider.id, &models, checked_at)
                })?;
            }
            Ok(ModelDiscoveryResult {
                models,
                message: "模型列表已更新".to_string(),
                checked_at,
                source: "network".to_string(),
                stale: false,
                expires_at: cache_result.then_some(checked_at + MODEL_CACHE_TTL_MS),
                error: None,
            })
        }
        Err(error) if cache_result => cached_or_empty_model_result(&provider.id, &error, state),
        Err(error) => Ok(ModelDiscoveryResult {
            models: Vec::new(),
            message: error.clone(),
            checked_at,
            source: "none".to_string(),
            stale: false,
            expires_at: None,
            error: Some(error),
        }),
    }
}

fn uses_antigravity_model_catalog(provider: &Provider) -> bool {
    provider.is_antigravity() || is_antigravity_gateway_base_url(&provider.base_url)
}

fn is_antigravity_gateway_base_url(base_url: &str) -> bool {
    let Ok(normalized) = normalize_base_url(base_url) else {
        return false;
    };
    let lower = normalized.to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower.as_str());
    let host_port = without_scheme.split('/').next().unwrap_or("");
    matches!(
        host_port,
        "127.0.0.1:15830"
            | "localhost:15830"
            | "[::1]:15830"
            | "127.0.0.1:8045"
            | "localhost:8045"
            | "[::1]:8045"
    )
}

fn antigravity_catalog_model_ids() -> Vec<String> {
    // Seed from persisted account quotas when the in-memory catalog is cold.
    let _ = crate::antigravity::list_accounts();
    crate::antigravity::list_model_ids()
}

fn discovery_http_client(url: &str) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    if url_targets_loopback(url) {
        // Clash/system proxy often 502s loopback API probes.
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|e| AppError::Other(format!("创建连接测试客户端失败: {e}")))
}

fn url_targets_loopback(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.contains("://127.0.0.1")
        || lower.contains("://localhost")
        || lower.contains("://[::1]")
}

fn cached_or_empty_model_result(
    provider_id: &str,
    error: &str,
    state: &AppState,
) -> AppResult<ModelDiscoveryResult> {
    state.db.with_conn(|conn| {
        Ok(model_result_from_cache(
            dao::get_provider_model_cache(conn, provider_id)?,
            "刷新失败，继续使用已保存的模型列表",
            Some(error.to_string()),
        ))
    })
}

fn model_result_from_cache(
    cache: Option<dao::providers::ProviderModelCache>,
    cached_message: &str,
    error: Option<String>,
) -> ModelDiscoveryResult {
    let now = Utc::now().timestamp_millis();
    match cache {
        Some(cache) if !cache.models.is_empty() => {
            let expires_at = cache.checked_at + MODEL_CACHE_TTL_MS;
            ModelDiscoveryResult {
                models: cache.models,
                message: cached_message.to_string(),
                checked_at: cache.checked_at,
                source: "cache".to_string(),
                stale: now >= expires_at,
                expires_at: Some(expires_at),
                error,
            }
        }
        _ => {
            let message = error
                .clone()
                .unwrap_or_else(|| "尚未保存模型列表".to_string());
            ModelDiscoveryResult {
                models: Vec::new(),
                message,
                checked_at: now,
                source: "none".to_string(),
                stale: false,
                expires_at: None,
                error,
            }
        }
    }
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let mut models = BTreeSet::new();
    if let Some(items) = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
    {
        for item in items {
            let value = item.as_str().or_else(|| {
                item.get("id")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
            });
            let Some(model) = value.map(str::trim) else {
                continue;
            };
            if !model.is_empty() && model.chars().count() <= MAX_MODEL_NAME_CHARS {
                models.insert(model.to_string());
                if models.len() >= MAX_DISCOVERED_MODELS {
                    break;
                }
            }
        }
    }
    models.into_iter().collect()
}

#[tauri::command]
pub async fn switch_to_official(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    switch_to_official_for_target(target, Some(&app), &state).await
}

/// Shared official-login restoration used by IPC and tray actions.
pub async fn switch_to_official_for_target<R: tauri::Runtime>(
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<()> {
    let mut snapshot = SwitchSnapshot::capture(state, target).await?;
    let result: AppResult<()> = async {
        match target {
            ProviderTarget::ClaudeCode => restore_code_ownership(state)?,
            ProviderTarget::ClaudeDesktop => restore_desktop_ownership(state)?,
            ProviderTarget::Codex => {
                tauri::async_runtime::spawn_blocking(codex::restore_official)
                    .await
                    .map_err(|error| {
                        AppError::Tauri(format!("Codex 官方配置恢复任务失败: {error}"))
                    })??;
            }
            ProviderTarget::OpenCode => {
                tauri::async_runtime::spawn_blocking(opencode::clear_provider)
                    .await
                    .map_err(|error| {
                        AppError::Tauri(format!("OpenCode 托管配置移除任务失败: {error}"))
                    })??;
            }
            // Pi / Dsh 无独立「官方配置」文件；清除当前供应商 + 停代理即可。
            ProviderTarget::Pi | ProviderTarget::Dsh => {}
        }
        state.proxy.lock().await.stop_target(target);
        if let Some(app) = app {
            crate::commands::proxy::publish_target_stopped(app, state, target).await;
        }
        state.db.with_conn(|conn| dao::clear_current_provider(conn, target))?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            let _ = snapshot.capture_last_written_files();
            Ok(())
        }
        Err(error) => {
            if let Err(mark_error) = snapshot.capture_last_written_files() {
                return rollback_switch(
                    snapshot,
                    state,
                    AppError::Config(format!(
                        "{error}；无法安全确认配置写入状态：{mark_error}"
                    )),
                )
                .await;
            }
            rollback_switch(snapshot, state, error).await
        }
    }
}

#[tauri::command]
pub fn reorder_providers(ordered_ids: Vec<String>, target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| dao::reorder_providers(conn, &ordered_ids, target))
}

/// Import a live third-party configuration into its matching application list.
#[tauri::command]
pub fn import_live_config(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    if target == ProviderTarget::OpenCode {
        return import_opencode_live_providers(&state);
    }
    if target == ProviderTarget::Pi {
        return sync_pi_providers_to_live(&state);
    }
    if target == ProviderTarget::Dsh {
        return sync_dsh_providers_to_live(&state);
    }
    let live = match target {
        ProviderTarget::ClaudeCode => claude_code::read_current_live_provider()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::read_current_live_provider()?,
        ProviderTarget::Codex => codex::read_current_live_provider()?,
        ProviderTarget::OpenCode | ProviderTarget::Pi | ProviderTarget::Dsh => unreachable!(),
    };
    let Some(live) = live else {
        return Ok(());
    };
    import_live_provider(live, target, &state)
}

/// OpenCode 配置可携带多个自有供应商（provider 段 + 顶层 model 引用当前项）。
/// 全部同步：base_url 已存在则更新名称/模型/密钥/协议；否则新建。
/// OpenCode 无激活切换，导入不标记 `is_current`。
fn import_opencode_live_providers(state: &AppState) -> AppResult<()> {
    let live_providers = opencode::read_live_providers()?;
    if live_providers.is_empty() {
        let config_path = crate::config::get_opencode_config_path();
        return Err(AppError::Config(format!(
            "未在 OpenCode 配置中找到可导入的供应商（{}）。请确认 provider 段含 baseURL 或 baseUrl；托管项 aisw-* / ai-switcher 不会导入。若同时存在 opencode.json 与 opencode.jsonc，将优先读取含 provider 的文件。也可检查旧版 config.json 与 auth.json。",
            config_path.display()
        )));
    }
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, ProviderTarget::OpenCode))?;
    for live in &live_providers {
        let normalized_base_url = normalize_base_url(&live.base_url)?;
        let default_model = live
            .current_model
            .clone()
            .or_else(|| live.models.first().cloned())
            .unwrap_or_default();
        if default_model.trim().is_empty() {
            continue;
        }
        let failover_models: Vec<String> = live
            .models
            .iter()
            .filter(|model| model.as_str() != default_model)
            .cloned()
            .collect();
        let matched = existing.iter().find(|p| p.base_url == normalized_base_url);
        state.db.with_conn(|conn| {
            dao::upsert_provider(
                conn,
                &ProviderInput {
                    id: matched.map(|p| p.id.clone()),
                    name: live.name.clone(),
                    base_url: normalized_base_url,
                    api_key: live.auth_token.clone(),
                    clear_api_key: false,
                    model: default_model,
                    model_context_window: matched.and_then(|p| p.model_context_window),
                    auto_review_model_override: None,
                    web_search_enabled: matched.and_then(|p| p.web_search_enabled),
                    model_mapping: ClaudeModelMapping::default(),
                    protocol_type: live.protocol_type,
                    provider_kind: ProviderKind::Standard,
                    auth_binding: String::new(),
                    target_app: ProviderTarget::OpenCode,
                    notes: matched
                        .map(|p| p.notes.clone())
                        .filter(|notes| !notes.trim().is_empty())
                        .unwrap_or_else(|| {
                            format!("从 OpenCode 配置同步（provider: {}）", live.id)
                        }),
                    failover_group: matched.map(|p| p.failover_group).unwrap_or(0),
                    failover_models,
                },
            )
        })?;
    }
    // 导入只更新 DB；OpenCode 侧用户自有项已存在，再把 AI-Switcher 托管项同步出去。
    sync_opencode_providers_to_live(state)?;
    Ok(())
}

/// 把 DB 中全部 OpenCode 供应商写入 `opencode.json`（多供应商并存，无需切换）。
pub(crate) fn sync_opencode_providers_to_live(state: &AppState) -> AppResult<()> {
    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::OpenCode))?;
    let mut entries: Vec<(Provider, Vec<String>)> = Vec::with_capacity(providers.len());
    for provider in providers {
        let mut runtime = provider.clone();
        runtime.api_key = state
            .db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut extra_models = state
            .db
            .with_conn(|conn| {
                Ok(dao::get_provider_model_cache(conn, &provider.id)?
                    .map(|cache| cache.models)
                    .unwrap_or_default())
            })
            .unwrap_or_default();
        extra_models = extra_models_for_ag_catalog_apply(&provider, extra_models);
        entries.push((runtime, extra_models));
    }
    opencode::apply_all_providers(&entries)
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
            model_context_window: provider.model_context_window,
            web_search_enabled: provider.web_search_enabled,
            model_mapping: provider.model_mapping,
            protocol_type: provider.protocol_type,
            target_app: provider.target_app,
            notes: provider.notes,
            failover_group: provider.failover_group,
            failover_models: provider.failover_models,
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
    let mut touched_opencode = false;
    let mut touched_pi = false;
    let mut touched_dsh = false;
    for entry in bundle.providers {
        let normalized_base_url = normalize_base_url(&entry.base_url)?;
        let existing = state.db.with_conn(|conn| dao::list_providers(conn, entry.target_app))?;
        if existing.iter().any(|provider| {
            provider.name == entry.name && provider.base_url == normalized_base_url
        }) {
            skipped += 1;
            continue;
        }
        let target_app = entry.target_app;
        state.db.with_conn(|conn| dao::upsert_provider(conn, &ProviderInput {
            id: None,
            name: entry.name,
            base_url: normalized_base_url,
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
            target_app,
            notes: entry.notes,
            failover_group: entry.failover_group,
            failover_models: entry.failover_models,
        }))?;
        if target_app == ProviderTarget::OpenCode {
            touched_opencode = true;
        }
        if target_app == ProviderTarget::Pi {
            touched_pi = true;
        }
        if target_app == ProviderTarget::Dsh {
            touched_dsh = true;
        }
        imported += 1;
    }
    if touched_opencode {
        sync_opencode_providers_to_live(&state)?;
    }
    if touched_pi {
        sync_pi_providers_to_live(&state)?;
    }
    if touched_dsh {
        sync_dsh_providers_to_live(&state)?;
    }
    Ok(ProviderImportResult { imported, skipped })
}

fn sync_catalog_target(state: &AppState, target: ProviderTarget) -> AppResult<()> {
    match target {
        ProviderTarget::OpenCode => sync_opencode_providers_to_live(state),
        ProviderTarget::Pi => sync_pi_providers_to_live(state),
        ProviderTarget::Dsh => sync_dsh_providers_to_live(state),
        _ => Ok(()),
    }
}

/// 把 DB 中全部 DeepSeek Harness 供应商写入 `settings.yaml` / `.credentials.yaml`（多供应商��存，无需切换）。
pub(crate) fn sync_dsh_providers_to_live(state: &AppState) -> AppResult<()> {
    use crate::config::dsh::sync_managed_dsh_providers;

    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::Dsh))?;
    let mut entries: Vec<(Provider, Vec<String>)> = Vec::with_capacity(providers.len());
    for provider in providers {
        let mut runtime = provider.clone();
        runtime.api_key = state
            .db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        if runtime.is_antigravity() && runtime.api_key.trim().is_empty() {
            runtime.api_key = crate::antigravity::gateway::builtin_api_key();
        }
        let mut extra_models = state
            .db
            .with_conn(|conn| {
                Ok(dao::get_provider_model_cache(conn, &provider.id)?
                    .map(|cache| cache.models)
                    .unwrap_or_default())
            })
            .unwrap_or_default();
        extra_models = extra_models_for_ag_catalog_apply(&provider, extra_models);
        entries.push((runtime, extra_models));
    }
    sync_managed_dsh_providers(&entries)
}

/// 把 DB 中全部 Pi 供应商写入 `models.json` / `auth.json`（多供应商并存，无需切换）。
pub(crate) fn sync_pi_providers_to_live(state: &AppState) -> AppResult<()> {
    use crate::coding::pi::config::{
        read_pi_settings, sync_managed_pi_auth, sync_managed_pi_providers, update_pi_settings,
    };

    let providers = state
        .db
        .with_conn(|conn| dao::list_providers(conn, ProviderTarget::Pi))?;
    let mut model_entries: Vec<(String, serde_json::Value)> = Vec::with_capacity(providers.len());
    let mut auth_entries: Vec<(String, String)> = Vec::with_capacity(providers.len());
    for provider in &providers {
        let mut runtime = provider.clone();
        runtime.api_key = state
            .db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        if runtime.is_antigravity() && runtime.api_key.trim().is_empty() {
            runtime.api_key = crate::antigravity::gateway::builtin_api_key();
        }
        let extra_models = state
            .db
            .with_conn(|conn| extra_models_for_pi_apply(conn, &runtime))
            .unwrap_or_default();
        let provider_id = pi_provider_id(&runtime);
        model_entries.push((
            provider_id.clone(),
            pi_provider_config(&runtime, &extra_models)?,
        ));
        if !runtime.api_key.trim().is_empty() {
            auth_entries.push((provider_id, runtime.api_key));
        }
    }
    let retired = sync_managed_pi_providers(&model_entries)?;
    sync_managed_pi_auth(&auth_entries, &retired)?;

    let settings = read_pi_settings().unwrap_or_else(|_| serde_json::json!({}));
    let default_provider = settings
        .get("defaultProvider")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let default_missing = default_provider.is_empty()
        || retired.iter().any(|id| id == default_provider);
    if default_missing {
        if let (Some((id, _)), Some(provider)) = (model_entries.first(), providers.first()) {
            let _ = update_pi_settings(Some(id.clone()), Some(provider.model.clone()), None, None);
        }
    }
    Ok(())
}

fn pi_provider_config(provider: &Provider, extra_models: &[String]) -> AppResult<serde_json::Value> {
    use crate::provider::ProtocolType;
    use serde_json::json;

    let model_id = provider.model.trim();
    if model_id.is_empty() {
        return Err(AppError::Config("Pi 默认模型不能为空".to_string()));
    }
    let mut provider_cfg = json!({
        "baseUrl": normalize_pi_base_url(&provider.base_url, provider.protocol_type),
        "api": pi_api_for_protocol(provider.protocol_type),
        "models": build_pi_model_entries(provider, extra_models),
    });
    if matches!(provider.protocol_type, ProtocolType::Anthropic) {
        provider_cfg["compat"] = json!({
            "supportsEagerToolInputStreaming": false,
        });
    }
    Ok(provider_cfg)
}

fn pi_provider_id(provider: &Provider) -> String {
    if provider.is_antigravity() {
        return "antigravity".to_string();
    }
    let slug: String = provider
        .name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "custom".to_string()
    } else {
        slug
    }
}

fn pi_api_for_protocol(protocol: crate::provider::ProtocolType) -> &'static str {
    use crate::provider::ProtocolType;
    match protocol {
        ProtocolType::Anthropic => "anthropic-messages",
        ProtocolType::OpenAiResponses => "openai-responses",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => "openai-completions",
    }
}

fn pi_proxy_base_url(port: u16, protocol: crate::provider::ProtocolType) -> String {
    use crate::provider::ProtocolType;
    match protocol {
        // Anthropic SDK posts `/v1/messages` onto baseURL.
        ProtocolType::Anthropic => format!("http://127.0.0.1:{port}"),
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy => {
            format!("http://127.0.0.1:{port}/v1")
        }
    }
}

fn normalize_pi_base_url(base: &str, protocol: crate::provider::ProtocolType) -> String {
    use crate::provider::{ensure_openai_v1_suffix, ProtocolType};
    let mut url = base.trim().trim_end_matches('/').to_string();
    match protocol {
        ProtocolType::Anthropic => {
            // Official Anthropic base is `https://api.anthropic.com` (no `/v1`).
            // A trailing `/v1` becomes `/v1/v1/messages` → 404.
            if url.ends_with("/v1") {
                url.truncate(url.len() - 3);
                url = url.trim_end_matches('/').to_string();
            }
            url
        }
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy => {
            // Only append `/v1` when the path is empty (host root). Keep `/v4`,
            // `/compatible-mode/v1`, etc. as the vendor published them.
            ensure_openai_v1_suffix(&url).unwrap_or(url)
        }
    }
}

fn build_pi_model_entries(provider: &Provider, extra_models: &[String]) -> Vec<serde_json::Value> {
    use serde_json::json;

    let mut ids = catalog_models_from_provider(provider);
    for model in extra_models {
        let model = model.trim();
        if !model.is_empty() && !ids.iter().any(|id| id == model) {
            ids.push(model.to_string());
        }
    }
    if provider.is_antigravity() {
        for id in crate::antigravity::model_catalog::provider_suggestion_ids(24) {
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
    }

    let context_window = provider.model_context_window.unwrap_or(200_000);
    let openai_compatible = matches!(
        provider.protocol_type,
        crate::provider::ProtocolType::OpenAiChat
            | crate::provider::ProtocolType::OpenAiResponses
            | crate::provider::ProtocolType::Proxy
    );
    ids.into_iter()
        .map(|id| {
            let mut entry = json!({
                "id": id,
                "reasoning": true,
                "input": ["text", "image"],
                "contextWindow": context_window,
            });
            if openai_compatible {
                entry["thinkingLevelMap"] = json!({
                    "off": "off",
                    "minimal": "minimal",
                    "low": "low",
                    "medium": "medium",
                    "high": "high",
                    "xhigh": "xhigh",
                    "max": "max"
                });
            }
            entry
        })
        .collect()
}

fn extra_models_for_ag_catalog_apply(provider: &Provider, mut extra_models: Vec<String>) -> Vec<String> {
    extend_unique_models(&mut extra_models, provider.failover_models.clone());
    if !uses_antigravity_model_catalog(provider) {
        return extra_models;
    }
    extend_unique_models(&mut extra_models, crate::antigravity::list_model_ids());
    extend_unique_models(
        &mut extra_models,
        crate::antigravity::model_catalog::provider_suggestion_ids(24),
    );
    extra_models.retain(|id| {
        let trimmed = id.trim();
        crate::antigravity::model_catalog::is_agent_facing_model(trimmed)
            && !crate::antigravity::model_catalog::is_retired_model(trimmed)
            && !crate::antigravity::model_catalog::should_remap_legacy_gemini(trimmed)
    });
    extra_models
}

fn extra_models_for_pi_apply(
    conn: &rusqlite::Connection,
    provider: &Provider,
) -> AppResult<Vec<String>> {
    let mut ids = Vec::new();
    if let Some(cache) = dao::get_provider_model_cache(conn, &provider.id)? {
        extend_unique_models(&mut ids, cache.models);
    }
    if ids.len() <= 1 {
        let source_url =
            normalize_base_url(&provider.base_url).unwrap_or_else(|_| provider.base_url.clone());
        for target in [
            ProviderTarget::ClaudeCode,
            ProviderTarget::ClaudeDesktop,
            ProviderTarget::Codex,
            ProviderTarget::OpenCode,
            ProviderTarget::Pi,
        ] {
            for sibling in dao::list_providers(conn, target)? {
                if sibling.id == provider.id {
                    continue;
                }
                let sibling_url = normalize_base_url(&sibling.base_url)
                    .unwrap_or_else(|_| sibling.base_url.clone());
                if sibling_url != source_url {
                    continue;
                }
                extend_unique_models(&mut ids, catalog_models_from_provider(&sibling));
                if let Some(cache) = dao::get_provider_model_cache(conn, &sibling.id)? {
                    extend_unique_models(&mut ids, cache.models);
                }
            }
        }
    }
    Ok(ids)
}

fn extend_unique_models(ids: &mut Vec<String>, extra: Vec<String>) {
    for model in extra {
        let model = model.trim();
        if !model.is_empty() && !ids.iter().any(|id| id == model) {
            ids.push(model.to_string());
        }
    }
}

async fn apply_target_provider<R: tauri::Runtime>(
    provider: &Provider,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<(SwitchSnapshot, Option<CodexProviderSyncResult>, Option<&'static str>)> {
    // Provider rows carry only a keyring reference. Hydrate a short-lived clone
    // for config writing; it is never serialized or persisted.
    let mut runtime_provider = provider.clone();
    if runtime_provider.is_antigravity() {
        crate::commands::antigravity::ensure_gateway_running_for_provider(&runtime_provider).await?;
        let gateway = crate::antigravity::gateway_status()?;
        if runtime_provider.api_key.trim().is_empty()
            || state
                .db
                .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
                .ok()
                .flatten()
                .is_none()
        {
            // Persist gateway key so subsequent resolves succeed.
            let _ = state.db.with_conn(|conn| {
                dao::upsert_provider(
                    conn,
                    &ProviderInput {
                        id: Some(provider.id.clone()),
                        name: provider.name.clone(),
                        base_url: gateway.base_url.clone(),
                        api_key: gateway.api_key.clone(),
                        clear_api_key: false,
                        model: provider.model.clone(),
                        model_context_window: provider.model_context_window,
                        auto_review_model_override: provider.auto_review_model_override.clone(),
                        web_search_enabled: provider.web_search_enabled,
                        model_mapping: provider.model_mapping.clone(),
                        protocol_type: provider.protocol_type,
                        provider_kind: provider.provider_kind,
                        auth_binding: provider.auth_binding.clone(),
                        target_app: provider.target_app,
                        notes: provider.notes.clone(),
                        failover_group: provider.failover_group,
                        failover_models: provider.failover_models.clone(),
                    },
                )
            });
            runtime_provider.base_url = gateway.base_url;
        }
    }
    runtime_provider.api_key = if provider.is_codex_oauth() {
        "PROXY_MANAGED".to_string()
    } else if provider.is_antigravity() {
        state
            .db
            .with_conn(|conn| dao::resolve_api_key(conn, &provider.id))
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| crate::antigravity::gateway::builtin_api_key())
    } else {
        state.db.with_conn(|conn| {
            dao::resolve_api_key(conn, &provider.id)?.ok_or_else(|| {
                AppError::Config("供应商未配置 API Key，无法切换".to_string())
            })
        })?
    };
    let proxy_port = get_saved_proxy_port(state, runtime_provider.target_app);
    let _codex_switch_guard = if runtime_provider.target_app == ProviderTarget::Codex {
        Some(codex_switch_lock().lock().await)
    } else {
        None
    };
    let mut snapshot = SwitchSnapshot::capture(state, runtime_provider.target_app).await?;
    if runtime_provider.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空，请先编辑供应商配置".to_string()));
    }
    let uses_proxy = runtime_provider.is_codex_oauth() || runtime_provider.requires_local_proxy();
    let result: AppResult<(Option<CodexProviderSyncResult>, Option<&'static str>)> = async {
        match runtime_provider.target_app {
            ProviderTarget::ClaudeCode => {
                let mut ownership = prepare_code_ownership(
                    &runtime_provider,
                    state,
                    uses_proxy,
                    proxy_port,
                )?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeCode).await?;
                }
                let provider = runtime_provider.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if uses_proxy {
                        claude_code::apply_provider_to_settings_via_proxy(&provider, proxy_port)
                    } else {
                        claude_code::apply_provider_to_settings(&provider)
                    }
                })
                .await
                .map_err(|error| AppError::Tauri(format!("Claude Code 配置写入任务失败: {error}")))??;
                // Persist the actual on-disk managed fields so later compares
                // match Claude Code / JSON normalization instead of our preview.
                ownership.written = code_managed_fields()?;
                commit_code_ownership(state, ownership)?;
                Ok((None, None))
            }
            ProviderTarget::ClaudeDesktop => {
                let original_applied_id = prepare_desktop_ownership(state)?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeDesktop).await?;
                }
                let provider = runtime_provider.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    claude_desktop::apply_provider(&provider, proxy_port)
                })
                .await
                .map_err(|error| AppError::Tauri(format!("Claude Desktop 配置写入任务失败: {error}")))??;
                commit_desktop_ownership(state, original_applied_id)?;
                Ok((None, None))
            }
            ProviderTarget::Codex => {
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::Codex).await?;
                }
                let extra_models = state
                    .db
                    .with_conn(|conn| {
                        Ok(dao::get_provider_model_cache(conn, &runtime_provider.id)?
                            .map(|cache| cache.models)
                            .unwrap_or_default())
                    })
                    .unwrap_or_default();
                let provider = runtime_provider.clone();
                let api_key = runtime_provider.api_key.clone();
                let apply_info = tauri::async_runtime::spawn_blocking(move || {
                    codex::apply_provider(
                        &provider,
                        &api_key,
                        if uses_proxy { Some(proxy_port) } else { None },
                        &extra_models,
                    )
                })
                .await
                .map_err(|error| AppError::Tauri(format!("Codex 配置写入任务失败: {error}")))??;
                let codex_notice = if apply_info.preserved_official_login {
                    Some("preserved_official_login")
                } else {
                    Some("official_login_required")
                };
                // Config write succeeded. Session rewrite failures must not roll
                // back the provider switch; surface them as a warning instead.
                let session_sync = match codex_provider_sync::sync_to_managed_provider() {
                    Ok(result) => {
                        log::info!(
                            "Codex 历史会话同步完成: changed={} sqlite={} status={}",
                            result.changed_session_files,
                            result.sqlite_rows_updated,
                            result.status
                        );
                        result
                    }
                    Err(error) => {
                        log::warn!("Codex 历史会话同步失败（配置已切换）: {error}");
                        CodexProviderSyncResult {
                            status: "warning".into(),
                            message: format!("供应商已切换，但历史会话同步失败：{error}"),
                            target_provider: codex::managed_provider_id().into(),
                            backup_dir: None,
                            changed_session_files: 0,
                            sqlite_rows_updated: 0,
                            skipped_locked_files: Vec::new(),
                        }
                    }
                };
                Ok((Some(session_sync), codex_notice))
            }
            ProviderTarget::OpenCode => {
                // OpenCode 多供应商并存：切换仅同步全部托管项，不改顶层 model。
                sync_opencode_providers_to_live(state)?;
                Ok((None, None))
            }
            ProviderTarget::Pi => {
                sync_pi_providers_to_live(state)?;
                let provider_id = pi_provider_id(&runtime_provider);
                crate::coding::pi::config::update_pi_settings(
                    Some(provider_id),
                    Some(runtime_provider.model.clone()),
                    None,
                    None,
                )?;
                Ok((None, None))
            }
            ProviderTarget::Dsh => {
                // Dsh resolves provider/model per session. Keep all managed routes available.
                sync_dsh_providers_to_live(state)?;
                Ok((None, None))
            }
        }
    }.await;
    let (session_sync, codex_notice) = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Err(mark_error) = snapshot.capture_last_written_files() {
                return rollback_switch(snapshot, state, AppError::Config(format!("{error}；无法安全确认配置写入状态：{mark_error}"))).await;
            }
            return rollback_switch(snapshot, state, error).await;
        }
    };
    if let Err(error) = snapshot.capture_last_written_files() {
        return rollback_switch(snapshot, state, error).await;
    }
    if !uses_proxy {
        state.proxy.lock().await.stop_target(runtime_provider.target_app);
    }
    if let Some(app) = app {
        crate::commands::proxy::publish_target_status(app, state, runtime_provider.target_app).await;
    }
    Ok((snapshot, session_sync, codex_notice))
}

struct SwitchSnapshot {
    target: ProviderTarget,
    files: Vec<FileSnapshot>,
    ownership_key: &'static str,
    ownership_value: Option<String>,
    proxy: ProxySnapshot,
}

struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
    last_written: Option<Option<Vec<u8>>>,
}

struct ProxySnapshot {
    running: bool,
    port: u16,
}

impl SwitchSnapshot {
    async fn capture(state: &AppState, target: ProviderTarget) -> AppResult<Self> {
        let paths = match target {
            ProviderTarget::ClaudeCode => vec![crate::config::get_claude_settings_path()],
            ProviderTarget::ClaudeDesktop => {
                let paths = claude_desktop::detect_claude_desktop();
                let mut files = Vec::new();
                if let Some(config_library) = &paths.config_library {
                    files.push(config_library.join(format!("{}.json", claude_desktop::PROFILE_ID)));
                    files.push(config_library.join(format!(
                        "{}.json",
                        claude_desktop::LEGACY_PROFILE_ID
                    )));
                }
                if let Some(meta_path) = &paths.meta_path {
                    files.push(meta_path.clone());
                }
                if let Some(normal_config_path) = &paths.normal_config_path {
                    files.push(normal_config_path.clone());
                }
                if let Some(threep_config_path) = &paths.threep_config_path {
                    files.push(threep_config_path.clone());
                }
                files
            }
            ProviderTarget::Codex => vec![
                crate::config::get_codex_config_path(),
                crate::config::get_codex_auth_path(),
                crate::config::get_codex_config_dir().join("ai-switcher-model-catalog.json"),
            ],
            ProviderTarget::OpenCode => vec![crate::config::get_opencode_config_path()],
            ProviderTarget::Pi => vec![
                crate::coding::pi::config::get_pi_settings_path(),
                crate::coding::pi::config::get_pi_auth_path(),
                crate::coding::pi::config::get_pi_models_path(),
            ],
            ProviderTarget::Dsh => vec![
                crate::config::get_dsh_settings_path(),
                crate::config::get_dsh_credentials_path(),
            ],
        };
        let files = paths.into_iter().map(FileSnapshot::capture).collect::<AppResult<Vec<_>>>()?;
        let ownership_key = match target {
            ProviderTarget::ClaudeCode => CODE_OWNERSHIP_KEY,
            ProviderTarget::ClaudeDesktop => DESKTOP_OWNERSHIP_KEY,
            ProviderTarget::Codex => CODEX_OWNERSHIP_KEY,
            ProviderTarget::OpenCode => OPENCODE_OWNERSHIP_KEY,
            ProviderTarget::Pi => PI_OWNERSHIP_KEY,
            ProviderTarget::Dsh => "v1310.dsh_managed",
        };
        let ownership_value = state.db.with_conn(|conn| get_setting(conn, ownership_key))?;
        let proxy = {
            let proxy = state.proxy.lock().await;
            let status = proxy.status_for(target);
            ProxySnapshot { running: status.running, port: status.port }
        };
        Ok(Self { target, files, ownership_key, ownership_value, proxy })
    }

    async fn restore(self, state: &AppState) -> AppResult<()> {
        let mut failures = Vec::new();
        for file in self.files {
            if let Err(error) = file.restore() {
                failures.push(format!("恢复配置文件失败: {error}"));
            }
        }
        if let Err(error) = state.db.with_conn(|conn| {
            set_setting(conn, self.ownership_key, self.ownership_value.as_deref().unwrap_or(""))
        }) {
            failures.push(format!("恢复配置所有权失败: {error}"));
        }
        let proxy_result = {
            let mut proxy = state.proxy.lock().await;
            if self.proxy.running {
                proxy.start(self.proxy.port, self.target).await
            } else {
                proxy.stop_target(self.target);
                Ok(())
            }
        };
        if let Err(error) = proxy_result {
            failures.push(format!("恢复本地代理失败: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Config(failures.join("；")))
        }
    }

    fn capture_last_written_files(&mut self) -> AppResult<()> {
        for file in &mut self.files {
            file.capture_last_written()?;
        }
        Ok(())
    }
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> AppResult<Self> {
        let contents = if path.exists() { Some(std::fs::read(&path)?) } else { None };
        Ok(Self { path, contents, last_written: None })
    }

    fn capture_last_written(&mut self) -> AppResult<()> {
        self.last_written = Some(Self::read_contents(&self.path)?);
        Ok(())
    }

    fn restore(self) -> AppResult<()> {
        let Some(last_written) = self.last_written else {
            return Ok(());
        };
        if Self::read_contents(&self.path)? != last_written {
            return Err(AppError::Config(format!(
                "检测到配置文件已被外部修改，已拒绝覆盖: {}",
                self.path.display()
            )));
        }
        match self.contents {
            Some(contents) => crate::config::atomic_write(&self.path, &contents),
            None if self.path.exists() => {
                std::fs::remove_file(&self.path)?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn read_contents(path: &std::path::Path) -> AppResult<Option<Vec<u8>>> {
        if path.exists() {
            Ok(Some(std::fs::read(path)?))
        } else {
            Ok(None)
        }
    }
}

async fn rollback_switch<T>(snapshot: SwitchSnapshot, state: &AppState, error: AppError) -> AppResult<T> {
    match snapshot.restore(state).await {
        Ok(()) => Err(AppError::Config(format!("{error}（已回滚到切换前配置）"))),
        Err(rollback_error) => Err(AppError::Config(format!(
            "{error}；已尝试回滚，但部分恢复失败：{rollback_error}"
        ))),
    }
}

const CODE_OWNERSHIP_KEY: &str = "p7.code_config_ownership";
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
    Ok(claude_code::MANAGED_ENV_KEYS.into_iter().map(|key| {
        (
            key.to_string(),
            normalize_managed_value(env.and_then(|map| map.get(key)).cloned()),
        )
    }).collect())
}

/// Treat missing / null / blank string as the same "absent" state for ownership.
fn normalize_managed_value(value: Option<Value>) -> Option<Value> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.trim().is_empty() => None,
        other => other,
    }
}

fn normalize_managed_value_for_key(key: &str, value: Option<Value>) -> Option<Value> {
    let value = normalize_managed_value(value);
    match (key, value) {
        ("ANTHROPIC_BASE_URL", Some(Value::String(url))) => {
            let trimmed = url.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(Value::String(trimmed))
            }
        }
        (_, other) => other,
    }
}

fn normalize_managed_fields(
    fields: &BTreeMap<String, Option<Value>>,
) -> BTreeMap<String, Option<Value>> {
    fields
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                normalize_managed_value_for_key(key, value.clone()),
            )
        })
        .collect()
}

/// Keys we expected to be absent (`None`) but which now exist (e.g. Claude Code
/// rewrote `ANTHROPIC_API_KEY`) are adopted so the next apply can clear them
/// without blocking the user. Real edits to values we previously wrote still
/// fail the ownership check.
fn adopt_absent_key_drift(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    for (key, current_value) in current {
        let written =
            normalize_managed_value_for_key(key, ownership.written.get(key).cloned().unwrap_or(None));
        let current_value = normalize_managed_value_for_key(key, current_value.clone());
        if written.is_none() && current_value.is_some() {
            ownership
                .before
                .entry(key.clone())
                .or_insert_with(|| current_value.clone());
            ownership.written.insert(key.clone(), current_value);
        }
    }
}

/// Claude Code sometimes relocates the same secret between `ANTHROPIC_AUTH_TOKEN`
/// and `ANTHROPIC_API_KEY`. Treat that migration as compatible ownership drift.
fn adopt_credential_key_migration(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    const AUTH: &str = "ANTHROPIC_AUTH_TOKEN";
    const API: &str = "ANTHROPIC_API_KEY";

    let written_auth =
        normalize_managed_value(ownership.written.get(AUTH).cloned().unwrap_or(None));
    let written_api = normalize_managed_value(ownership.written.get(API).cloned().unwrap_or(None));
    let current_auth = normalize_managed_value(current.get(AUTH).cloned().unwrap_or(None));
    let current_api = normalize_managed_value(current.get(API).cloned().unwrap_or(None));

    if written_auth.is_some()
        && current_auth.is_none()
        && current_api == written_auth
        && (written_api.is_none() || written_api == written_auth)
    {
        ownership.written.insert(AUTH.to_string(), None);
        ownership.written.insert(API.to_string(), current_api.clone());
        ownership
            .before
            .entry(API.to_string())
            .or_insert_with(|| current_api);
        return;
    }

    if written_api.is_some()
        && current_api.is_none()
        && current_auth == written_api
        && (written_auth.is_none() || written_auth == written_api)
    {
        ownership.written.insert(API.to_string(), None);
        ownership
            .written
            .insert(AUTH.to_string(), current_auth.clone());
        ownership
            .before
            .entry(AUTH.to_string())
            .or_insert_with(|| current_auth);
    }
}

fn reconcile_code_ownership(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    upgrade_code_ownership_fields(ownership, current);
    adopt_absent_key_drift(ownership, current);
    adopt_credential_key_migration(ownership, current);
}

fn managed_fields_match(
    left: &BTreeMap<String, Option<Value>>,
    right: &BTreeMap<String, Option<Value>>,
) -> bool {
    normalize_managed_fields(left) == normalize_managed_fields(right)
}

fn expected_code_fields(provider: &Provider, proxy: bool, port: u16) -> BTreeMap<String, Option<Value>> {
    use crate::provider::{
        ClaudeModelRole, CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID, CLAUDE_OPUS_ROLE_ID,
        CLAUDE_SONNET_ROLE_ID,
    };

    let mut values = claude_code::MANAGED_ENV_KEYS
        .into_iter()
        .map(|key| (key.to_string(), None))
        .collect::<BTreeMap<_, _>>();
    values.insert("ANTHROPIC_BASE_URL".to_string(), Some(Value::String(if proxy {
        format!("http://127.0.0.1:{port}")
    } else { provider.base_url.clone() })));
    values.insert("ANTHROPIC_AUTH_TOKEN".to_string(), Some(Value::String(if proxy {
        "local-proxy-code".to_string()
    } else { provider.api_key.clone() })));
    let roles = [
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            CLAUDE_SONNET_ROLE_ID,
            ClaudeModelRole::Sonnet,
        ),
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            CLAUDE_OPUS_ROLE_ID,
            ClaudeModelRole::Opus,
        ),
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            CLAUDE_HAIKU_ROLE_ID,
            ClaudeModelRole::Haiku,
        ),
        (
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            CLAUDE_FABLE_ROLE_ID,
            ClaudeModelRole::Fable,
        ),
    ];
    if !proxy && !provider.model.trim().is_empty() {
        values.insert(
            "ANTHROPIC_MODEL".to_string(),
            Some(Value::String(provider.model.trim().to_string())),
        );
    }
    for (model_key, name_key, stable_model, role) in roles {
        let upstream = provider
            .model_mapping
            .for_role(role, provider.model.trim())
            .to_string();
        values.insert(
            model_key.to_string(),
            Some(Value::String(if proxy {
                stable_model.to_string()
            } else {
                upstream.clone()
            })),
        );
        values.insert(name_key.to_string(), Some(Value::String(upstream)));
    }
    values.insert(
        "CLAUDE_CODE_SUBAGENT_MODEL".to_string(),
        Some(Value::String(
            provider
                .model_mapping
                .for_role(ClaudeModelRole::Subagent, provider.model.trim())
                .to_string(),
        )),
    );
    values
}

fn upgrade_code_ownership_fields(
    ownership: &mut CodeOwnership,
    current: &BTreeMap<String, Option<Value>>,
) {
    for key in claude_code::MANAGED_ENV_KEYS {
        if !ownership.written.contains_key(key) {
            let current_value = current.get(key).cloned().unwrap_or(None);
            ownership
                .before
                .entry(key.to_string())
                .or_insert(None);
            ownership
                .written
                .insert(key.to_string(), current_value);
        } else {
            ownership.before.entry(key.to_string()).or_insert(None);
        }
    }
}

fn prepare_code_ownership(provider: &Provider, state: &AppState, proxy: bool, port: u16) -> AppResult<CodeOwnership> {
    let current = code_managed_fields()?;
    let expected = expected_code_fields(provider, proxy, port);
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        let mut ownership: CodeOwnership = serde_json::from_str(&raw)
            .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全切换".to_string()))?;
        reconcile_code_ownership(&mut ownership, &current);
        if !managed_fields_match(&ownership.written, &current) {
            // Claude Code / user edits after our last write used to hard-block every
            // later switch. Rebaseline onto the live file so the user can continue.
            log::warn!(
                "Claude Code managed fields drifted from ownership record; rebasing onto current settings before switch"
            );
            return Ok(CodeOwnership {
                before: current,
                written: expected,
            });
        }
        ownership.written = expected;
        Ok(ownership)
    } else {
        Ok(CodeOwnership {
            before: current,
            written: expected,
        })
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
    let mut ownership: CodeOwnership = serde_json::from_str(&raw)
        .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全恢复".to_string()))?;
    let current = code_managed_fields()?;
    reconcile_code_ownership(&mut ownership, &current);
    if !managed_fields_match(&current, &ownership.written) {
        // Stale before-state is no longer trustworthy; clear managed keys for
        // official mode instead of refusing forever.
        log::warn!(
            "Claude Code managed fields drifted from ownership record; clearing managed fields for official restore"
        );
        claude_code::clear_provider_from_settings()?;
        return state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""));
    }
    claude_code::restore_managed_fields(&ownership.before)?;
    state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""))
}

const DESKTOP_OWNERSHIP_KEY: &str = "p7.desktop_original_applied_id";

fn prepare_desktop_ownership(state: &AppState) -> AppResult<Option<String>> {
    if !claude_desktop::is_supported_platform() {
        // Stale ownership from a Windows/macOS DB copy must not block Linux users.
        let _ = state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""));
        return Err(AppError::Config(
            "当前系统不支持 Claude Desktop 配置管理（仅 Windows / macOS）".to_string(),
        ));
    }
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        if !claude_desktop::current_applied_id()?
            .as_deref()
            .is_some_and(claude_desktop::is_managed_profile_id)
        {
            return Err(AppError::Config("检测到 Claude Desktop 配置已被外部修改，已拒绝覆盖".to_string()));
        }
        let original: Option<String> = serde_json::from_str(&raw)?;
        Ok(original.filter(|id| !claude_desktop::is_managed_profile_id(id)))
    } else {
        let applied = claude_desktop::current_applied_id()?;
        Ok(applied.filter(|id| !claude_desktop::is_managed_profile_id(id)))
    }
}

fn commit_desktop_ownership(state: &AppState, original_applied_id: Option<String>) -> AppResult<()> {
    state.db.with_conn(|conn| {
        set_setting(conn, DESKTOP_OWNERSHIP_KEY, &serde_json::to_string(&original_applied_id)?)
    })
}

fn restore_desktop_ownership(state: &AppState) -> AppResult<()> {
    if !claude_desktop::is_supported_platform() {
        let _ = state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""));
        return Err(AppError::Config(
            "当前系统不支持 Claude Desktop 配置管理（仅 Windows / macOS）".to_string(),
        ));
    }
    let raw = state.db.with_conn(|conn| get_setting(conn, DESKTOP_OWNERSHIP_KEY))?;
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return claude_desktop::clear_provider();
    };
    if !claude_desktop::current_applied_id()?
        .as_deref()
        .is_some_and(claude_desktop::is_managed_profile_id)
    {
        return Err(AppError::Config("检测到 Claude Desktop 配置已被外部修改，已拒绝覆盖".to_string()));
    }
    let original: Option<String> = serde_json::from_str(&raw)?;
    claude_desktop::clear_provider_restoring_applied_id(original)?;
    state.db.with_conn(|conn| set_setting(conn, DESKTOP_OWNERSHIP_KEY, ""))
}

/// Upgrade a legacy Desktop profile ID or model route list in the background
/// without a network preflight. The provider row and credential remain unchanged.
pub async fn repair_current_desktop_profile(state: &AppState) -> AppResult<()> {
    let applied_id = claude_desktop::current_applied_id()?;
    let legacy_profile = applied_id.as_deref() == Some(claude_desktop::LEGACY_PROFILE_ID);
    let managed_profile = applied_id.as_deref() == Some(claude_desktop::PROFILE_ID);
    if !legacy_profile && !managed_profile {
        return Ok(());
    }
    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::ClaudeDesktop))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let legacy_routes = managed_profile
        && provider.requires_local_proxy()
        && claude_desktop::current_profile_uses_legacy_role_routes()?;
    if !legacy_profile && !legacy_routes {
        return Ok(());
    }
    let _snapshot = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
    log::info!("Claude Desktop managed profile upgraded");
    Ok(())
}

/// Reapply the active Codex provider when the managed `ai_switcher` entry is
/// out of sync with the current routing mode:
/// - Anthropic / OAuth: must point at the local proxy
/// - OpenAI-compatible: must point at the real upstream (older builds pinned
///   these to the local proxy, which breaks chat when the desktop app is closed)
pub async fn repair_codex_managed_proxy_endpoint(state: &AppState) -> AppResult<()> {
    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::Codex))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let port = get_saved_proxy_port(state, ProviderTarget::Codex);
    let Some(current_base) = codex::managed_provider_base_url() else {
        // Missing managed entry — a full apply restores it.
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!("Codex managed provider entry missing; reapplied current provider");
        return Ok(());
    };
    let needs_proxy = provider.requires_local_proxy() || provider.is_codex_oauth();
    if needs_proxy {
        if is_local_proxy_base_url_for_port(&current_base, port) {
            return Ok(());
        }
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!(
            "Codex managed base_url was `{current_base}`; reapplied local proxy on port {port}"
        );
        return Ok(());
    }
    if is_loopback_v1_base_url(&current_base) {
        let _ = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
        log::info!(
            "Codex managed base_url was local proxy `{current_base}`; reapplied direct upstream"
        );
    }
    Ok(())
}

fn is_local_proxy_base_url_for_port(base_url: &str, expected_port: u16) -> bool {
    let trimmed = base_url.trim().trim_end_matches('/');
    let expected = format!("http://127.0.0.1:{expected_port}/v1");
    let expected_localhost = format!("http://localhost:{expected_port}/v1");
    trimmed.eq_ignore_ascii_case(&expected) || trimmed.eq_ignore_ascii_case(&expected_localhost)
}

fn is_loopback_v1_base_url(base_url: &str) -> bool {
    let trimmed = base_url.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    (lower.starts_with("http://127.0.0.1:") || lower.starts_with("http://localhost:"))
        && lower.ends_with("/v1")
}

/// Reapply an active Claude Code provider only when the live model fields use
/// the pre-display-name or pre-role-alias format.
pub async fn repair_current_code_model_fields(state: &AppState) -> AppResult<()> {
    let provider = state
        .db
        .with_conn(|conn| dao::get_current_provider(conn, ProviderTarget::ClaudeCode))?;
    let Some(provider) = provider else {
        return Ok(());
    };
    let proxy = provider.requires_local_proxy();
    let port = get_saved_proxy_port(state, ProviderTarget::ClaudeCode);
    let expected = expected_code_fields(&provider, proxy, port);
    let current = code_managed_fields()?;
    let model_keys = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "ANTHROPIC_REASONING_MODEL",
    ];
    if model_keys
        .iter()
        .all(|key| current.get(*key) == expected.get(*key))
    {
        return Ok(());
    }
    let _snapshot = apply_target_provider(&provider, None::<&tauri::AppHandle>, state).await?;
    log::info!("Claude Code live model fields upgraded");
    Ok(())
}

async fn test_provider_impl(provider: &Provider, state: &AppState) -> AppResult<ConnectionTestResult> {
    let key = state.db.with_conn(|conn| dao::resolve_api_key(conn, &provider.id));
    let key = match key {
        Ok(Some(key)) => key,
        Ok(None) => String::new(),
        Err(_) => {
            let result = ConnectionTestResult {
                ok: false,
                category: "credential".to_string(),
                message: "无法读取系统凭据库中的 API Key".to_string(),
                checked_at: Utc::now().timestamp_millis(),
                latency_ms: None,
            };
            persist_provider_health(provider, &result, state.db.as_ref())?;
            return Ok(result);
        }
    };
    test_provider_with_key(provider, key, state.db.as_ref(), true).await
}

async fn test_provider_with_key(
    provider: &Provider,
    key: String,
    db: &crate::database::Database,
    persist_health: bool,
) -> AppResult<ConnectionTestResult> {
    let checked_at = Utc::now().timestamp_millis();
    let result = if !key.trim().is_empty() && !provider.model.trim().is_empty() {
            let (endpoint, payload) = protocol_test_request(provider);
            let endpoint_url = api_endpoint_url(&provider.base_url, endpoint)?;
            let client = discovery_http_client(&endpoint_url)?;
            let mut request = client
                .post(endpoint_url)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header("x-api-key", key);
            if matches!(provider.protocol_type, ProtocolType::Anthropic) {
                request = request.header("anthropic-version", "2023-06-01");
            }
            let started = std::time::Instant::now();
            let response = request.body(serde_json::to_vec(&payload)?).send().await;
            let latency_ms = Some(started.elapsed().as_millis() as u64);
            classify_test_response(response, checked_at, provider.protocol_type, latency_ms).await
    } else if key.trim().is_empty() {
        ConnectionTestResult { ok: false, category: "authentication".to_string(), message: "供应商未配置 API Key".to_string(), checked_at, latency_ms: None }
    } else {
        ConnectionTestResult { ok: false, category: "model".to_string(), message: "请先填写模型名称".to_string(), checked_at, latency_ms: None }
    };
    if persist_health {
        persist_provider_health(provider, &result, db)?;
    }
    Ok(result)
}

fn persist_provider_health(
    provider: &Provider,
    result: &ConnectionTestResult,
    db: &crate::database::Database,
) -> AppResult<()> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO provider_health (provider_id, status, detail, checked_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET status = excluded.status, detail = excluded.detail, checked_at = excluded.checked_at",
            rusqlite::params![
                provider.id,
                if result.ok { "healthy" } else { "error" },
                match result.latency_ms {
                    Some(ms) => format!("{}|latency_ms={ms}", result.message),
                    None => result.message.clone(),
                },
                result.checked_at
            ],
        )?;
        Ok(())
    })
}

fn temporary_provider(input: &ProviderInput, state: &AppState) -> AppResult<Provider> {
    validate_target_protocol(input.target_app, input.protocol_type)?;
    let key = if !input.api_key.trim().is_empty() {
        input.api_key.clone()
    } else if let Some(id) = input.id.as_deref() {
        state.db.with_conn(|conn| dao::resolve_api_key(conn, id))?.unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Provider {
        id: input.id.clone().unwrap_or_else(|| "temporary-form-provider".to_string()),
        name: input.name.clone(), base_url: normalize_base_url(&input.base_url)?, api_key: key,
        api_key_set: !input.api_key.trim().is_empty(), model: input.model.clone(),
        model_context_window: input.model_context_window,
        auto_review_model_override: normalized_auto_review_model_override(
            input.target_app,
            input.auto_review_model_override.clone(),
        ),
        web_search_enabled: input.web_search_enabled,
        model_mapping: normalized_model_mapping(input.target_app, input.model_mapping.clone()),
        protocol_type: input.protocol_type, notes: input.notes.clone(), target_app: input.target_app,
        provider_kind: input.provider_kind, auth_binding: input.auth_binding.clone(),
        sort_index: 0, failover_group: input.failover_group,
        failover_models: input.failover_models.clone(),
        is_current: false, created_at: 0,
        health_status: None, health_checked_at: None, health_latency_ms: None,
    })
}

async fn classify_test_response(
    response: Result<reqwest::Response, reqwest::Error>,
    checked_at: i64,
    protocol: ProtocolType,
    latency_ms: Option<u64>,
) -> ConnectionTestResult {
    match response {
        Ok(response) if response.status().is_success() => ConnectionTestResult {
            ok: true,
            category: "ok".to_string(),
            message: match latency_ms {
                Some(ms) => format!("连接验证成功（{ms} ms）"),
                None => "连接验证成功".to_string(),
            },
            checked_at,
            latency_ms,
        },
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            let detail = extract_upstream_error_detail(&body);
            let (category, message) = match status {
                401 | 403 => ("authentication", "API Key 被拒绝".to_string()),
                404 | 405 => ("protocol", protocol_endpoint_message(protocol).to_string()),
                400 | 422 => ("model", "模型不可用或不兼容".to_string()),
                _ => (
                    "upstream",
                    if detail.is_empty() {
                        format!("上游服务返回错误（HTTP {status}）")
                    } else {
                        format!("上游服务返回错误（HTTP {status}）：{detail}")
                    },
                ),
            };
            ConnectionTestResult {
                ok: false,
                category: category.to_string(),
                message,
                checked_at,
                latency_ms,
            }
        }
        Err(error) if error.is_timeout() => ConnectionTestResult {
            ok: false,
            category: "network".to_string(),
            message: "连接测试超时".to_string(),
            checked_at,
            latency_ms,
        },
        Err(error) => ConnectionTestResult {
            ok: false,
            category: "network".to_string(),
            message: format!(
                "无法连接供应商服务（{}）",
                sanitize_network_error(&error)
            ),
            checked_at,
            latency_ms,
        },
    }
}

fn extract_upstream_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
        {
            let message = message.trim();
            if !message.is_empty() {
                return truncate_chars(message, 180);
            }
        }
    }
    truncate_chars(trimmed, 120)
}

fn truncate_chars(value: &str, max: usize) -> String {
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else {
        let truncated: String = value.chars().take(max).collect();
        format!("{truncated}…")
    }
}

fn protocol_test_request(provider: &Provider) -> (&'static str, Value) {
    match provider.protocol_type {
        ProtocolType::Anthropic => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiChat | ProtocolType::Proxy => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiResponses => (protocol_endpoint_path(provider.protocol_type), serde_json::json!({
            "model": provider.model.trim(), "max_output_tokens": 1, "stream": false,
            "input": "ping"
        })),
    }
}

fn protocol_endpoint_message(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "供应商不支持 Anthropic /v1/messages 端点",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => {
            "供应商不支持 OpenAI /v1/chat/completions 端点"
        }
        ProtocolType::OpenAiResponses => "供应商不支持 OpenAI /v1/responses 端点",
    }
}

fn import_live_provider(live: LiveProviderInfo, target: ProviderTarget, state: &AppState) -> AppResult<()> {
    let normalized_base_url = normalize_base_url(&live.base_url)?;
    let existing = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    if let Some(provider) = existing.iter().find(|p| p.base_url == normalized_base_url) {
        state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))?;
        return Ok(());
    }
    let input = ProviderInput {
        id: None,
        name: "当前配置（已导入）".to_string(),
        base_url: normalized_base_url,
        api_key: live.auth_token,
        clear_api_key: false,
        model: live.model,
        model_context_window: None,
        auto_review_model_override: None,
        web_search_enabled: None,
        model_mapping: live.model_mapping,
        protocol_type: live.protocol_type,
        provider_kind: ProviderKind::Standard,
        auth_binding: String::new(),
        target_app: target,
        notes: "从当前 Claude Code 配置导入".to_string(),
        failover_group: 0,
        failover_models: Vec::new(),
    };
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))
}

fn get_saved_proxy_port(state: &AppState, target: ProviderTarget) -> u16 {
    let key = match target {
        ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
        ProviderTarget::Codex => "proxy_port_codex",
        // OpenCode / Pi / Dsh 直连写入，不使用本地代理；仅为穷尽匹配保留键名。
        ProviderTarget::OpenCode => "proxy_port_opencode",
        ProviderTarget::Pi => "proxy_port_pi",
        ProviderTarget::Dsh => "proxy_port_dsh",
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
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(url_targets_loopback("http://127.0.0.1:15830/v1/models"));
        assert!(!url_targets_loopback("https://api.deepseek.com/v1/models"));
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
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
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
            failover_models: vec!["gemini-3.7-flash".into()],
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        };
        let extra = extra_models_for_ag_catalog_apply(
            &provider,
            vec!["claude-sonnet-4-6".into(), "chat_20706".into()],
        );
        assert!(extra.iter().any(|id| id == "claude-sonnet-4-6"));
        assert!(extra.iter().any(|id| id.starts_with("gemini-")));
        assert!(!extra.iter().any(|id| id == "chat_20706"));
    }
}
