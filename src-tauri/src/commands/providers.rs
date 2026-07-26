//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::config::{claude_code, claude_desktop};
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, normalize_base_url, protocol_endpoint_path, ConnectionTestResult,
    LiveProviderInfo, ModelDiscoveryResult, Provider, ProviderExportBundle, ProviderExportEntry,
    ProviderImportResult, ProviderInput, ProviderTarget, ProtocolType,
};
use crate::store::AppState;
use chrono::Utc;
use reqwest::header;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tauri::Emitter;

const MODEL_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_DISCOVERED_MODELS: usize = 1_000;
const MAX_MODEL_NAME_CHARS: usize = 256;

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
pub async fn switch_provider(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Provider> {
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let provider = switch_provider_for_target(&id, target, &state).await?;
    schedule_provider_health_check(app, provider.clone(), Arc::clone(&state.db));
    Ok(provider)
}

/// Shared provider switching service used by both IPC and tray actions.
/// Live configuration is switched locally first. Connectivity is checked in the
/// background so network latency never blocks the user's explicit selection.
pub async fn switch_provider_for_target(
    id: &str,
    target: ProviderTarget,
    state: &AppState,
) -> AppResult<Provider> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    if provider.target_app != target {
        return Err(AppError::Config("供应商不属于此应用".to_string()));
    }
    let started = Instant::now();
    let snapshot = apply_target_provider(&provider, state).await?;
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
    Ok(provider)
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
    let url = api_endpoint_url(&provider.base_url, "/v1/models")?;
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
pub async fn switch_to_official(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<()> {
    switch_to_official_for_target(target, &state).await
}

/// Shared official-login restoration used by IPC and tray actions.
pub async fn switch_to_official_for_target(target: ProviderTarget, state: &AppState) -> AppResult<()> {
    match target {
        ProviderTarget::ClaudeCode => restore_code_ownership(state)?,
        ProviderTarget::ClaudeDesktop => restore_desktop_ownership(state)?,
    }
    state.proxy.lock().await.stop_target(target);
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
            model_mapping: provider.model_mapping,
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
        let normalized_base_url = normalize_base_url(&entry.base_url)?;
        let existing = state.db.with_conn(|conn| dao::list_providers(conn, entry.target_app))?;
        if existing.iter().any(|provider| {
            provider.name == entry.name && provider.base_url == normalized_base_url
        }) {
            skipped += 1;
            continue;
        }
        state.db.with_conn(|conn| dao::upsert_provider(conn, &ProviderInput {
            id: None,
            name: entry.name,
            base_url: normalized_base_url,
            api_key: String::new(),
            clear_api_key: false,
            model: entry.model,
            model_mapping: entry.model_mapping,
            protocol_type: entry.protocol_type,
            target_app: entry.target_app,
            notes: entry.notes,
        }))?;
        imported += 1;
    }
    Ok(ProviderImportResult { imported, skipped })
}

async fn apply_target_provider(provider: &Provider, state: &AppState) -> AppResult<SwitchSnapshot> {
    // Provider rows carry only a keyring reference. Hydrate a short-lived clone
    // for config writing; it is never serialized or persisted.
    let mut runtime_provider = provider.clone();
    runtime_provider.api_key = state.db.with_conn(|conn| {
        dao::resolve_api_key(conn, &provider.id)?.ok_or_else(|| {
            AppError::Config("供应商未配置 API Key，无法切换".to_string())
        })
    })?;
    let proxy_port = get_saved_proxy_port(state, runtime_provider.target_app);
    let mut snapshot = SwitchSnapshot::capture(state, runtime_provider.target_app).await?;
    if runtime_provider.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空，请先编辑供应商配置".to_string()));
    }
    let uses_proxy = runtime_provider.requires_local_proxy();
    let result: AppResult<()> = async {
        match runtime_provider.target_app {
            ProviderTarget::ClaudeCode => {
                let ownership = prepare_code_ownership(
                    &runtime_provider,
                    state,
                    uses_proxy,
                    proxy_port,
                )?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeCode).await?;
                    claude_code::apply_provider_to_settings_via_proxy(&runtime_provider, proxy_port)?;
                } else {
                    claude_code::apply_provider_to_settings(&runtime_provider)?;
                }
                commit_code_ownership(state, ownership)?;
            }
            ProviderTarget::ClaudeDesktop => {
                let original_applied_id = prepare_desktop_ownership(state)?;
                if uses_proxy {
                    state.proxy.lock().await.start(proxy_port, ProviderTarget::ClaudeDesktop).await?;
                }
                claude_desktop::apply_provider(&runtime_provider, proxy_port)?;
                commit_desktop_ownership(state, original_applied_id)?;
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result {
        if let Err(mark_error) = snapshot.capture_last_written_files() {
            return rollback_switch(snapshot, state, AppError::Config(format!("{error}；无法安全确认配置写入状态：{mark_error}"))).await;
        }
        return rollback_switch(snapshot, state, error).await;
    }
    if let Err(error) = snapshot.capture_last_written_files() {
        return rollback_switch(snapshot, state, error).await;
    }
    if !uses_proxy {
        state.proxy.lock().await.stop_target(runtime_provider.target_app);
    }
    Ok(snapshot)
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
        };
        let files = paths.into_iter().map(FileSnapshot::capture).collect::<AppResult<Vec<_>>>()?;
        let ownership_key = match target {
            ProviderTarget::ClaudeCode => CODE_OWNERSHIP_KEY,
            ProviderTarget::ClaudeDesktop => DESKTOP_OWNERSHIP_KEY,
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
        Ok(()) => Err(error),
        Err(rollback_error) => Err(AppError::Config(format!("{error}；已尝试回滚，但部分恢复失败：{rollback_error}"))),
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
        (key.to_string(), env.and_then(|map| map.get(key)).cloned())
    }).collect())
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
    let raw = state.db.with_conn(|conn| get_setting(conn, CODE_OWNERSHIP_KEY))?;
    if let Some(raw) = raw.filter(|value| !value.is_empty()) {
        let mut ownership: CodeOwnership = serde_json::from_str(&raw)
            .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全切换".to_string()))?;
        upgrade_code_ownership_fields(&mut ownership, &current);
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
    let mut ownership: CodeOwnership = serde_json::from_str(&raw)
        .map_err(|_| AppError::Config("配置所有权记录已损坏，无法安全恢复".to_string()))?;
    let current = code_managed_fields()?;
    upgrade_code_ownership_fields(&mut ownership, &current);
    if current != ownership.written {
        return Err(AppError::Config("检测到 Claude Code 配置已被外部修改，已拒绝覆盖".to_string()));
    }
    claude_code::restore_managed_fields(&ownership.before)?;
    state.db.with_conn(|conn| set_setting(conn, CODE_OWNERSHIP_KEY, ""))
}

const DESKTOP_OWNERSHIP_KEY: &str = "p7.desktop_original_applied_id";

fn prepare_desktop_ownership(state: &AppState) -> AppResult<Option<String>> {
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
    let _snapshot = apply_target_provider(&provider, state).await?;
    log::info!("Claude Desktop managed profile upgraded");
    Ok(())
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
    let _snapshot = apply_target_provider(&provider, state).await?;
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
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| AppError::Other(format!("创建连接测试客户端失败: {e}")))?;
            let (endpoint, payload) = protocol_test_request(provider);
            let mut request = client
                .post(api_endpoint_url(&provider.base_url, endpoint)?)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header("x-api-key", key);
            if matches!(provider.protocol_type, ProtocolType::Anthropic) {
                request = request.header("anthropic-version", "2023-06-01");
            }
            let response = request.body(serde_json::to_vec(&payload)?).send().await;
            classify_test_response(response, checked_at, provider.protocol_type)
    } else if key.trim().is_empty() {
        ConnectionTestResult { ok: false, category: "authentication".to_string(), message: "供应商未配置 API Key".to_string(), checked_at }
    } else {
        ConnectionTestResult { ok: false, category: "model".to_string(), message: "请先填写模型名称".to_string(), checked_at }
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
            rusqlite::params![provider.id, if result.ok { "healthy" } else { "error" }, result.message, result.checked_at],
        )?;
        Ok(())
    })
}

fn temporary_provider(input: &ProviderInput, state: &AppState) -> AppResult<Provider> {
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
        model_mapping: input.model_mapping.clone(),
        protocol_type: input.protocol_type, notes: input.notes.clone(), target_app: input.target_app,
        sort_index: 0, is_current: false, created_at: 0,
        health_status: None, health_checked_at: None,
    })
}

fn classify_test_response(
    response: Result<reqwest::Response, reqwest::Error>, checked_at: i64, protocol: ProtocolType,
) -> ConnectionTestResult {
    match response {
        Ok(response) if response.status().is_success() => ConnectionTestResult {
            ok: true, category: "ok".to_string(), message: "连接验证成功".to_string(), checked_at,
        },
        Ok(response) => {
            let status = response.status().as_u16();
            let (category, message) = match status {
                401 | 403 => ("authentication", "API Key 被拒绝"),
                404 | 405 => ("protocol", protocol_endpoint_message(protocol)),
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
        model_mapping: live.model_mapping,
        protocol_type: ProtocolType::Anthropic,
        target_app: target,
        notes: "从当前 Claude Code 配置导入".to_string(),
    };
    let provider = state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
    state.db.with_conn(|conn| dao::set_current_provider(conn, &provider.id))
}

fn get_saved_proxy_port(state: &AppState, target: ProviderTarget) -> u16 {
    let key = match target {
        ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
    };
    state.db.with_conn(|conn| get_setting(conn, key))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .or_else(|| state.db.with_conn(|conn| get_setting(conn, "proxy_port")).ok().flatten().and_then(|value| value.parse::<u16>().ok()))
        .unwrap_or(match target { ProviderTarget::ClaudeCode => 15821, ProviderTarget::ClaudeDesktop => 15822 })
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
}
