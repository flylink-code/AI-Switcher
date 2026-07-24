//! Provider management commands scoped to Claude Code or Claude Desktop.

use crate::config::{claude_code, claude_desktop};
use crate::database::dao;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::provider::{
    api_endpoint_url, ConnectionTestResult, LiveProviderInfo, ModelDiscoveryResult, Provider,
    ProviderExportBundle, ProviderExportEntry, ProviderImportResult, ProviderInput,
    ProviderTarget, ProtocolType,
};
use crate::store::AppState;
use chrono::Utc;
use reqwest::header;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;


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
    let target = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.map(|provider| provider.target_app)
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    switch_provider_for_target(&id, target, &state).await
}

/// Shared provider switching service used by both IPC and tray actions.
/// It deliberately keeps the preflight before any live configuration change.
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
    let preflight = test_provider_impl(&provider, &state).await?;
    if !preflight.ok {
        return Err(AppError::Config(format!("连接验证失败（{}）：{}", preflight.category, preflight.message)));
    }
    let snapshot = apply_target_provider(&provider, state).await?;
    if let Err(error) = state.db.with_conn(|conn| dao::set_current_provider(conn, id)) {
        return rollback_switch(snapshot, state, error).await;
    }
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

/// Test values currently entered in the form.  The supplied API key is kept
/// only in this request and is never written to SQLite, the keyring or logs.
#[tauri::command]
pub async fn test_provider_input(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<ConnectionTestResult> {
    let provider = temporary_provider(&input, &state)?;
    test_provider_with_key(&provider, provider.api_key.clone(), &state, false).await
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
    discover_provider_models_with_key(&provider, key, &state, true).await
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
    let url = api_endpoint_url(&provider.base_url, "/v1/models");
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
    if cache_result {
        let models_json = serde_json::to_string(&models)?;
        state.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO provider_models (provider_id, models_json, checked_at) VALUES (?, ?, ?)
                 ON CONFLICT(provider_id) DO UPDATE SET models_json = excluded.models_json, checked_at = excluded.checked_at",
                rusqlite::params![provider.id, models_json, checked_at],
            )?;
            Ok(())
        })?;
    }
    Ok(ModelDiscoveryResult { models, message, checked_at })
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
    let uses_proxy = runtime_provider.protocol_type.uses_proxy();
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
                if let Some(config_library) = paths.config_library {
                    files.push(config_library.join(format!("{DESKTOP_PROFILE_ID}.json")));
                }
                if let Some(meta_path) = paths.meta_path {
                    files.push(meta_path);
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
            persist_provider_health(provider, &result, state)?;
            return Ok(result);
        }
    };
    test_provider_with_key(provider, key, state, true).await
}

async fn test_provider_with_key(
    provider: &Provider,
    key: String,
    state: &AppState,
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
                .post(api_endpoint_url(&provider.base_url, endpoint))
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
        persist_provider_health(provider, &result, state)?;
    }
    Ok(result)
}

fn persist_provider_health(provider: &Provider, result: &ConnectionTestResult, state: &AppState) -> AppResult<()> {
    state.db.with_conn(|conn| {
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
        name: input.name.clone(), base_url: input.base_url.clone(), api_key: key,
        api_key_set: !input.api_key.trim().is_empty(), model: input.model.clone(),
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
        ProtocolType::Anthropic => ("/v1/messages", serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiChat | ProtocolType::Proxy => ("/v1/chat/completions", serde_json::json!({
            "model": provider.model.trim(), "max_tokens": 1, "stream": false,
            "messages": [{"role": "user", "content": "ping"}]
        })),
        ProtocolType::OpenAiResponses => ("/v1/responses", serde_json::json!({
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
