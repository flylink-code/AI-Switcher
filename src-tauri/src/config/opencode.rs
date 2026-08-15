//! OpenCode 配置写入器（CLI 与 Desktop 应用共享 `opencode.json`）。
//!
//! OpenCode 原生支持多供应商并存、应用内选模型，因此本模块写入时会把
//! AI-Switcher 中全部 OpenCode 供应商同步到 `provider` 段（`aisw-<id>`），
//! **不**要求「切换当前供应商」。顶层 `model` 交由 OpenCode 自己选择，
//! 仅在悬空指向已删除的托管项时清理。

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Map, Value};

use crate::config::atomic::write_json_file;
use crate::config::paths::get_opencode_config_path;
use crate::error::{AppError, AppResult};
use crate::mcp::McpServer;
use crate::provider::{ClaudeModelMapping, LiveProviderInfo, ProtocolType, Provider};

/// 旧版单槽托管 provider ID（兼容清理）。
pub const MANAGED_PROVIDER_ID: &str = "ai-switcher";
/// 多供应商托管前缀：每个 DB 供应商写入 `aisw-<provider.id>`。
pub const MANAGED_PROVIDER_PREFIX: &str = "aisw-";

fn opencode_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_opencode_config() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    opencode_config_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("OpenCode 配置锁已中毒: {error}")))
}

pub fn managed_provider_key(provider_id: &str) -> String {
    format!("{MANAGED_PROVIDER_PREFIX}{provider_id}")
}

pub fn is_managed_provider_key(key: &str) -> bool {
    key == MANAGED_PROVIDER_ID || key.starts_with(MANAGED_PROVIDER_PREFIX)
}

/// 协议 → AI SDK 包名。Anthropic 直连用官方包，OpenAI 系走兼容包。
fn managed_npm_package(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "@ai-sdk/anthropic",
        _ => "@ai-sdk/openai-compatible",
    }
}

/// 读取并解析 OpenCode 配置（JSONC）。文件不存在时返回仅含 `$schema` 的空对象；
/// 根节点不是对象时报错——下游要对它做索引赋值，静默重建会丢用户自有配置。
pub fn read_opencode_config_at(path: &Path) -> AppResult<Value> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({ "$schema": "https://opencode.ai/config.json" }));
        }
        Err(err) => return Err(AppError::Io(format!("读取 OpenCode 配置失败: {err}"))),
    };
    let value: Value = json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "无法解析 OpenCode 配置: {}: {e}",
            path.display()
        ))
    })?;
    if !value.is_object() {
        return Err(AppError::Config(format!(
            "OpenCode 配置文件根节点必须是 JSON 对象: {}",
            path.display()
        )));
    }
    Ok(value)
}

pub fn read_opencode_config() -> AppResult<Value> {
    read_opencode_config_at(&get_opencode_config_path())
}

fn write_opencode_config_at(path: &Path, config: &Value) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        crate::config::ensure_dir_with_context(parent)?;
    }
    write_json_file(path, config)?;
    log::debug!("OpenCode 配置已写入 {path:?}");
    Ok(())
}

/// AI SDK 的 baseURL 必须以 `/v1` 结尾：anthropic 包请求 `{baseURL}/messages`，
/// openai-compatible 请求 `{baseURL}/chat/completions`；缺 `/v1` 时 AG 网关会 404。
fn normalize_opencode_sdk_base_url(protocol: ProtocolType, base_url: &str) -> AppResult<String> {
    let normalized = crate::provider::normalize_base_url(base_url)?;
    match protocol {
        ProtocolType::Anthropic | ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses => {
            crate::provider::ensure_openai_v1_suffix(&normalized)
        }
        ProtocolType::Proxy => Ok(normalized),
    }
}

fn opencode_sdk_base_url(provider: &Provider) -> String {
    normalize_opencode_sdk_base_url(provider.protocol_type, &provider.base_url).unwrap_or_else(
        |error| {
            log::warn!(
                "OpenCode baseURL 归一化失败（{}），使用原值: {error}",
                provider.base_url
            );
            provider.base_url.trim().to_string()
        },
    )
}

/// OpenCode 把缺失的 `limit.context` 当成 0；未配置时写 Pi/AG 同档的 200k。
const OPENCODE_DEFAULT_CONTEXT_WINDOW: u64 = 200_000;

fn opencode_model_context(provider: &Provider) -> u64 {
    provider
        .model_context_window
        .filter(|window| *window > 0)
        .unwrap_or(OPENCODE_DEFAULT_CONTEXT_WINDOW)
}

/// OpenCode 自定义模型缺字段时默认无图片、无推理档位。托管项必须显式声明。
fn opencode_model_entry(provider: &Provider, model_id: &str) -> Value {
    let mut entry = json!({
        "name": model_id,
        "attachment": true,
        "reasoning": true,
        "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
        },
        "limit": { "context": opencode_model_context(provider) }
    });
    if matches!(
        provider.protocol_type,
        ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy
    ) {
        entry["variants"] = json!({
            "low": { "reasoningEffort": "low" },
            "medium": { "reasoningEffort": "medium" },
            "high": { "reasoningEffort": "high" }
        });
    }
    entry
}

/// 构建托管 provider 段的 JSON。`extra_models` 来自模型缓存，与默认模型一起
/// 写入 `models` 映射。
fn managed_provider_value(provider: &Provider, extra_models: &[String]) -> Value {
    let mut models = Map::new();
    let default_model = provider.model.trim();
    if !default_model.is_empty() {
        models.insert(
            default_model.to_string(),
            opencode_model_entry(provider, default_model),
        );
    }
    for model in extra_models {
        let trimmed = model.trim();
        if !trimmed.is_empty() && trimmed != default_model {
            models.insert(trimmed.to_string(), opencode_model_entry(provider, trimmed));
        }
    }
    json!({
        "npm": managed_npm_package(provider.protocol_type),
        "name": provider.name.trim(),
        "options": {
            "baseURL": opencode_sdk_base_url(provider),
            "apiKey": provider.api_key,
        },
        "models": Value::Object(models),
    })
}

fn ensure_provider_object(config: &mut Value) {
    if !config.get("provider").is_some_and(Value::is_object) {
        if config.get("provider").is_some() {
            log::warn!("opencode 配置的 provider 不是对象，已重置为空对象");
        }
        config["provider"] = json!({});
    }
}

fn remove_managed_providers(providers: &mut Map<String, Value>) -> Vec<String> {
    let keys: Vec<String> = providers
        .keys()
        .filter(|key| is_managed_provider_key(key))
        .cloned()
        .collect();
    for key in &keys {
        providers.remove(key);
    }
    keys
}

fn clear_dangling_managed_model(config: &mut Value, removed_keys: &[String]) {
    let Some(model_ref) = config.get("model").and_then(Value::as_str).map(str::to_string) else {
        return;
    };
    let Some((provider_id, _)) = split_model_ref(&model_ref) else {
        return;
    };
    if removed_keys.iter().any(|key| key == provider_id) {
        if let Some(obj) = config.as_object_mut() {
            obj.remove("model");
        }
    }
}

/// 将 AI-Switcher 中全部 OpenCode 供应商同步写入配置（多供应商并存，无需切换）。
pub fn apply_all_providers_at(
    path: &Path,
    entries: &[(Provider, Vec<String>)],
) -> AppResult<()> {
    let _guard = lock_opencode_config()?;
    let mut config = read_opencode_config_at(path)?;
    ensure_provider_object(&mut config);

    let removed = if let Some(providers) = config.get_mut("provider").and_then(Value::as_object_mut)
    {
        let removed = remove_managed_providers(providers);
        for (provider, extra_models) in entries {
            if provider.model.trim().is_empty() {
                return Err(AppError::Config(format!(
                    "供应商「{}」默认模型不能为空",
                    provider.name
                )));
            }
            let key = managed_provider_key(&provider.id);
            providers.insert(key, managed_provider_value(provider, extra_models));
        }
        removed
    } else {
        Vec::new()
    };

    let active_keys: Vec<String> = entries
        .iter()
        .map(|(provider, _)| managed_provider_key(&provider.id))
        .collect();
    let stale: Vec<String> = removed
        .into_iter()
        .filter(|key| !active_keys.iter().any(|active| active == key))
        .collect();
    if !stale.is_empty() {
        clear_dangling_managed_model(&mut config, &stale);
    }

    write_opencode_config_at(path, &config)
}

pub fn apply_all_providers(entries: &[(Provider, Vec<String>)]) -> AppResult<()> {
    apply_all_providers_at(&get_opencode_config_path(), entries)
}

fn apply_provider_at(path: &Path, provider: &Provider, extra_models: &[String]) -> AppResult<()> {
    // 兼容单测 / 旧调用：等价于只同步这一条。
    apply_all_providers_at(path, &[(provider.clone(), extra_models.to_vec())])
}

/// 将单个供应商写入 OpenCode（会先清掉其它托管项再只留这一条）。
/// OpenCode 正常路径请用 [`apply_all_providers`]。
pub fn apply_provider(provider: &Provider, extra_models: &[String]) -> AppResult<()> {
    apply_provider_at(&get_opencode_config_path(), provider, extra_models)
}

fn clear_provider_at(path: &Path) -> AppResult<()> {
    let _guard = lock_opencode_config()?;
    if !path.exists() {
        return Ok(());
    }
    let mut config = read_opencode_config_at(path)?;
    let mut changed = false;

    let removed = if let Some(providers) = config.get_mut("provider").and_then(Value::as_object_mut)
    {
        let removed = remove_managed_providers(providers);
        changed |= !removed.is_empty();
        removed
    } else {
        Vec::new()
    };
    if !removed.is_empty() {
        let before = config.get("model").cloned();
        clear_dangling_managed_model(&mut config, &removed);
        if config.get("model") != before.as_ref() {
            changed = true;
        }
    }
    if changed {
        write_opencode_config_at(path, &config)?;
    }
    Ok(())
}

/// 移除全部托管 provider 段，用户自有配置保留。
pub fn clear_provider() -> AppResult<()> {
    clear_provider_at(&get_opencode_config_path())
}

/// `provider_id/model_id` 只按第一个 `/` 分段：models.dev 中存在 model id
/// 自身含 `/` 的情况（如 `zenmux/openai/gpt-5.5-fast`）。
fn split_model_ref(value: &str) -> Option<(&str, &str)> {
    let (provider_id, model_id) = value.split_once('/')?;
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider_id, model_id))
}

fn read_current_live_provider_at(path: &Path) -> AppResult<Option<LiveProviderInfo>> {
    let providers = read_live_providers_at(path)?;
    let candidate = providers
        .iter()
        .find(|provider| provider.current_model.is_some())
        .or_else(|| providers.first());
    let Some(provider) = candidate else {
        return Ok(None);
    };
    Ok(Some(LiveProviderInfo {
        base_url: provider.base_url.clone(),
        auth_token: provider.auth_token.clone(),
        model: provider
            .current_model
            .clone()
            .or_else(|| provider.models.first().cloned())
            .unwrap_or_default(),
        model_mapping: ClaudeModelMapping::default(),
        protocol_type: provider.protocol_type,
    }))
}

/// 从 opencode 配置读取的一个用户自有供应商（供批量导入）。
#[derive(Debug, Clone)]
pub struct OpenCodeLiveProvider {
    /// provider 段 key（如 `acme`）。
    pub id: String,
    /// 展示名：`name` 字段缺省时回退 provider id。
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    pub protocol_type: ProtocolType,
    /// `models` 映射的全部模型 id。
    pub models: Vec<String>,
    /// 顶层 `model` 指向该 provider 时的模型 id；未指向则为 None。
    pub current_model: Option<String>,
}

/// npm 包名推断协议：@ai-sdk/anthropic → Anthropic，其余按 OpenAI 兼容处理。
fn protocol_from_npm(value: &Value) -> ProtocolType {
    match value.get("npm").and_then(Value::as_str) {
        Some("@ai-sdk/anthropic") => ProtocolType::Anthropic,
        _ => ProtocolType::OpenAiChat,
    }
}

fn provider_options_base_url(value: &Value) -> String {
    value
        .pointer("/options/baseURL")
        .or_else(|| value.pointer("/options/baseUrl"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn provider_options_api_key(value: &Value) -> String {
    value
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn auth_key_for_provider(auth: &Value, provider_id: &str) -> String {
    let Some(entry) = auth.get(provider_id) else {
        return String::new();
    };
    if let Some(key) = entry.as_str() {
        return key.trim().to_string();
    }
    entry
        .get("key")
        .or_else(|| entry.get("apiKey"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn fill_auth_tokens(providers: &mut [OpenCodeLiveProvider], auth: &Value) {
    for provider in providers {
        if !provider.auth_token.trim().is_empty() {
            continue;
        }
        let key = auth_key_for_provider(auth, &provider.id);
        if !key.is_empty() {
            provider.auth_token = key;
        }
    }
}

fn merge_live_providers(
    mut into: Vec<OpenCodeLiveProvider>,
    extra: Vec<OpenCodeLiveProvider>,
) -> Vec<OpenCodeLiveProvider> {
    for provider in extra {
        if !into.iter().any(|existing| existing.id == provider.id) {
            into.push(provider);
        }
    }
    into
}

fn read_opencode_auth_at(path: &Path) -> AppResult<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|err| AppError::Io(format!("读取 OpenCode auth.json 失败: {err}")))?;
    json5::from_str(&content).map_err(|err| {
        AppError::Config(format!(
            "无法解析 OpenCode auth.json: {}: {err}",
            path.display()
        ))
    })
}

fn read_live_providers_at(path: &Path) -> AppResult<Vec<OpenCodeLiveProvider>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let config = read_opencode_config_at(path)?;
    let Some(providers) = config.get("provider").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let current_ref = config.get("model").and_then(Value::as_str).and_then(split_model_ref);

    let mut result = Vec::new();
    for (provider_id, value) in providers {
        // 跳过本应用托管项，避免「更新本地已有配置」把 aisw-* 再导回 DB 造成循环。
        if is_managed_provider_key(provider_id) {
            continue;
        }
        let base_url = provider_options_base_url(value);
        if base_url.is_empty() {
            continue;
        }
        let mut models: Vec<String> = value
            .get("models")
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        models.sort();
        models.dedup();
        let current_model = current_ref
            .filter(|(pid, _)| *pid == provider_id.as_str())
            .map(|(_, model_id)| model_id.to_string());
        if let Some(model_id) = &current_model {
            if !models.iter().any(|m| m == model_id) {
                models.push(model_id.clone());
            }
        }
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(provider_id.as_str())
            .to_string();
        result.push(OpenCodeLiveProvider {
            id: provider_id.clone(),
            name,
            base_url,
            auth_token: provider_options_api_key(value),
            protocol_type: protocol_from_npm(value),
            models,
            current_model,
        });
    }
    Ok(result)
}

/// 读取 OpenCode 配置中的全部用户自有供应商（供「扫描本地配置」）。
/// 主配置 + 旧版 `config.json`；`options.apiKey` 为空时从 `auth.json` 补密钥。
pub fn read_live_providers() -> AppResult<Vec<OpenCodeLiveProvider>> {
    let primary = get_opencode_config_path();
    let mut providers = read_live_providers_at(&primary)?;
    let legacy = crate::config::paths::get_opencode_legacy_config_path();
    if legacy.is_file() && legacy != primary {
        match read_live_providers_at(&legacy) {
            Ok(extra) => providers = merge_live_providers(providers, extra),
            Err(error) => log::warn!("读取 OpenCode 旧版 config.json 失败: {error}"),
        }
    }
    match read_opencode_auth_at(&crate::config::paths::get_opencode_auth_path()) {
        Ok(auth) => fill_auth_tokens(&mut providers, &auth),
        Err(error) => log::warn!("读取 OpenCode auth.json 失败: {error}"),
    }
    Ok(providers)
}

/// 读取 OpenCode 当前生效的第三方供应商（供「导入 live 配置」）。
pub fn read_current_live_provider() -> AppResult<Option<LiveProviderInfo>> {
    read_current_live_provider_at(&get_opencode_config_path())
}

// ---- MCP 服务器同步 ---------------------------------------------------------
//
// OpenCode 的 MCP 配置在 `mcp` 段，格式与 Claude 不同：
//   本地: {"type": "local", "command": ["cmd", ...args], "enabled": true, "environment": {...}}
//   远程: {"type": "remote", "url": "...", "enabled": true, "headers": {...}}
// 只管理 DB 中同名的键（含已禁用/已删除的会被清掉），用户自写的其它键保留。

/// 将 Claude 格式的 server_config 转成 OpenCode `mcp` 段条目；无法识别返回 None。
fn mcp_entry_to_opencode(config: &Value) -> Option<Value> {
    let obj = config.as_object()?;
    if let Some(url) = obj.get("url").and_then(Value::as_str) {
        let mut out = Map::new();
        out.insert("type".to_string(), json!("remote"));
        out.insert("url".to_string(), json!(url));
        out.insert("enabled".to_string(), json!(true));
        if let Some(headers) = obj.get("headers").and_then(Value::as_object) {
            if !headers.is_empty() {
                out.insert("headers".to_string(), Value::Object(headers.clone()));
            }
        }
        return Some(Value::Object(out));
    }
    let command = obj.get("command").and_then(Value::as_str)?;
    let mut cmd = vec![json!(command)];
    if let Some(args) = obj.get("args").and_then(Value::as_array) {
        cmd.extend(args.iter().filter_map(Value::as_str).map(|s| json!(s)));
    }
    let mut out = Map::new();
    out.insert("type".to_string(), json!("local"));
    out.insert("command".to_string(), Value::Array(cmd));
    out.insert("enabled".to_string(), json!(true));
    if let Some(env) = obj.get("env").and_then(Value::as_object) {
        if !env.is_empty() {
            out.insert("environment".to_string(), Value::Object(env.clone()));
        }
    }
    Some(Value::Object(out))
}

/// OpenCode `mcp` 段条目转回 Claude 格式（导入用）；无法识别返回 None。
fn mcp_entry_from_opencode(config: &Value) -> Option<Value> {
    let obj = config.as_object()?;
    match obj.get("type").and_then(Value::as_str) {
        Some("remote") => {
            let url = obj.get("url").and_then(Value::as_str)?;
            let mut out = Map::new();
            out.insert("type".to_string(), json!("http"));
            out.insert("url".to_string(), json!(url));
            if let Some(headers) = obj.get("headers").and_then(Value::as_object) {
                if !headers.is_empty() {
                    out.insert("headers".to_string(), Value::Object(headers.clone()));
                }
            }
            Some(Value::Object(out))
        }
        Some("local") => {
            let cmd = obj.get("command").and_then(Value::as_array)?;
            let mut parts = cmd.iter().filter_map(Value::as_str);
            let command = parts.next()?;
            let args: Vec<Value> = parts.map(|s| json!(s)).collect();
            let mut out = Map::new();
            out.insert("command".to_string(), json!(command));
            if !args.is_empty() {
                out.insert("args".to_string(), Value::Array(args));
            }
            if let Some(env) = obj.get("environment").and_then(Value::as_object) {
                if !env.is_empty() {
                    out.insert("env".to_string(), Value::Object(env.clone()));
                }
            }
            Some(Value::Object(out))
        }
        _ => None,
    }
}

/// 把启用了 OpenCode 的 MCP 服务器写入 `mcp` 段（指定路径，便于测试）。
pub fn sync_mcp_servers_at(path: &Path, servers: &[McpServer]) -> AppResult<()> {
    let _guard = lock_opencode_config()?;
    let mut config = read_opencode_config_at(path)?;
    let obj = config.as_object_mut().expect("read_opencode_config_at 保证对象");
    let managed: std::collections::HashSet<&str> =
        servers.iter().map(|s| s.name.as_str()).collect();

    let mut mcp = obj
        .get("mcp")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    mcp.retain(|key, _| !managed.contains(key.as_str()));
    for server in servers.iter().filter(|s| s.enabled_opencode) {
        if let Some(entry) = mcp_entry_to_opencode(&server.server_config) {
            mcp.insert(server.name.clone(), entry);
        }
    }
    if mcp.is_empty() {
        obj.remove("mcp");
    } else {
        obj.insert("mcp".to_string(), Value::Object(mcp));
    }
    write_opencode_config_at(path, &config)
}

/// 把启用了 OpenCode 的 MCP 服务器同步到 live `opencode.json`。
pub fn sync_mcp_servers(servers: &[McpServer]) -> AppResult<()> {
    sync_mcp_servers_at(&get_opencode_config_path(), servers)
}

/// 读取 `mcp` 段并转回 Claude 格式（「导入 live 配置」用）。
pub fn read_mcp_servers() -> AppResult<Map<String, Value>> {
    let config = read_opencode_config()?;
    let mut out = Map::new();
    if let Some(mcp) = config.get("mcp").and_then(Value::as_object) {
        for (name, entry) in mcp {
            if let Some(converted) = mcp_entry_from_opencode(entry) {
                out.insert(name.clone(), converted);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderKind, ProviderTarget};

    fn test_provider(protocol: ProtocolType) -> Provider {
        Provider {
            id: "p1".into(),
            name: "测试供应商".into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: "sk-test".into(),
            api_key_set: true,
            model: "claude-sonnet-5".into(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: protocol,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::OpenCode,
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

    fn managed_key() -> String {
        managed_provider_key("p1")
    }

    #[test]
    fn opencode_sdk_base_url_appends_v1_for_anthropic_gateway() {
        let provider = Provider {
            id: "p1".into(),
            name: "AG".into(),
            base_url: "http://127.0.0.1:15830".into(),
            api_key: "k".into(),
            api_key_set: true,
            model: "gemini-3.6-flash-high".into(),
            model_context_window: None,
            web_search_enabled: None,
            auto_review_model_override: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            notes: String::new(),
            target_app: ProviderTarget::OpenCode,
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        };
        let value = managed_provider_value(&provider, &[]);
        assert_eq!(value["options"]["baseURL"], "http://127.0.0.1:15830/v1");
    }

    #[test]
    fn openai_compatible_models_declare_vision_and_reasoning_variants() {
        let mut provider = test_provider(ProtocolType::OpenAiChat);
        provider.model = "qwen3.6-plus".into();
        provider.model_context_window = Some(200_000);
        let value = managed_provider_value(&provider, &["mini".into()]);
        let model = &value["models"]["qwen3.6-plus"];
        assert_eq!(model["attachment"], true);
        assert_eq!(model["reasoning"], true);
        assert_eq!(model["modalities"]["input"], json!(["text", "image"]));
        assert_eq!(model["modalities"]["output"], json!(["text"]));
        assert_eq!(model["variants"]["high"]["reasoningEffort"], "high");
        assert_eq!(model["limit"]["context"], 200_000);
        assert_eq!(value["models"]["mini"]["variants"]["low"]["reasoningEffort"], "low");
    }

    #[test]
    fn anthropic_models_declare_reasoning_without_openai_variants() {
        let value = managed_provider_value(&test_provider(ProtocolType::Anthropic), &[]);
        let model = &value["models"]["claude-sonnet-5"];
        assert_eq!(model["attachment"], true);
        assert_eq!(model["reasoning"], true);
        assert_eq!(model["modalities"]["input"], json!(["text", "image"]));
        assert!(model.get("variants").is_none());
        assert_eq!(model["limit"]["context"], 200_000);
    }

    #[test]
    fn missing_context_window_defaults_to_200k() {
        let mut provider = test_provider(ProtocolType::OpenAiChat);
        provider.model_context_window = None;
        let value = managed_provider_value(&provider, &[]);
        assert_eq!(value["models"]["claude-sonnet-5"]["limit"]["context"], 200_000);

        provider.model_context_window = Some(0);
        let value = managed_provider_value(&provider, &[]);
        assert_eq!(value["models"]["claude-sonnet-5"]["limit"]["context"], 200_000);

        provider.model_context_window = Some(128_000);
        let value = managed_provider_value(&provider, &[]);
        assert_eq!(value["models"]["claude-sonnet-5"]["limit"]["context"], 128_000);
    }

    #[test]
    fn apply_writes_managed_provider_without_forcing_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{ "model": "my-own/model-x", "provider": { "my-own": { "npm": "@ai-sdk/anthropic" } } }"#,
        )
        .expect("seed");

        apply_provider_at(&path, &test_provider(ProtocolType::Anthropic), &["gpt-5.5".into()])
            .expect("apply");

        let config = read_opencode_config_at(&path).expect("reload");
        let key = managed_key();
        let managed = &config["provider"][&key];
        assert_eq!(managed["npm"], "@ai-sdk/anthropic");
        assert_eq!(managed["options"]["baseURL"], "https://api.example.test/v1");
        assert_eq!(managed["options"]["apiKey"], "sk-test");
        assert!(managed["models"]["claude-sonnet-5"].is_object());
        assert!(managed["models"]["gpt-5.5"].is_object());
        assert_eq!(config["model"], "my-own/model-x", "不得强行改写顶层 model");
        assert!(config["provider"].get(MANAGED_PROVIDER_ID).is_none());
    }

    #[test]
    fn apply_all_writes_multiple_providers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        let mut a = test_provider(ProtocolType::Anthropic);
        a.id = "a".into();
        a.name = "A".into();
        let mut b = test_provider(ProtocolType::OpenAiChat);
        b.id = "b".into();
        b.name = "B".into();
        b.model = "gpt-5.5".into();

        apply_all_providers_at(&path, &[(a, vec![]), (b, vec!["mini".into()])]).expect("apply");

        let config = read_opencode_config_at(&path).expect("reload");
        assert!(config["provider"][managed_provider_key("a")].is_object());
        assert_eq!(
            config["provider"][managed_provider_key("b")]["npm"],
            "@ai-sdk/openai-compatible"
        );
        assert!(config["provider"][managed_provider_key("b")]["models"]["mini"].is_object());
    }

    #[test]
    fn apply_maps_openai_protocol_to_compatible_package() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");

        apply_provider_at(&path, &test_provider(ProtocolType::OpenAiChat), &[]).expect("apply");

        let config = read_opencode_config_at(&path).expect("reload");
        assert_eq!(
            config["provider"][managed_key()]["npm"],
            "@ai-sdk/openai-compatible"
        );
    }

    #[test]
    fn apply_preserves_user_config_and_parses_jsonc() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.jsonc");
        std::fs::write(
            &path,
            r#"{
  // 用户注释
  "theme": "dark",
  "model": "my-own/model-x",
  "provider": {
    "my-own": { "npm": "@ai-sdk/anthropic", "options": { "baseURL": "https://other.test" } }
  }
}"#,
        )
        .expect("write jsonc");

        apply_provider_at(&path, &test_provider(ProtocolType::Anthropic), &[]).expect("apply");

        let config = read_opencode_config_at(&path).expect("reload");
        assert_eq!(config["theme"], "dark");
        assert!(config["provider"]["my-own"].is_object(), "用户自有 provider 必须保留");
        assert!(config["provider"][managed_key()].is_object());
        assert_eq!(config["model"], "my-own/model-x");
    }

    #[test]
    fn read_rejects_non_object_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        for malformed in ["[]", "42", "\"oops\""] {
            std::fs::write(&path, malformed).expect("write");
            assert!(read_opencode_config_at(&path).is_err(), "must reject: {malformed}");
        }
    }

    #[test]
    fn clear_removes_managed_section_and_dangling_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        let key = managed_key();
        std::fs::write(
            &path,
            format!(
                r#"{{ "model": "{key}/claude-sonnet-5", "provider": {{ "{key}": {{ "npm": "@ai-sdk/anthropic" }} }} }}"#
            ),
        )
        .expect("seed");

        clear_provider_at(&path).expect("clear");

        let config = read_opencode_config_at(&path).expect("reload");
        assert!(config["provider"].get(&key).is_none());
        assert!(config.get("model").is_none(), "悬空 model 引用必须移除");
    }

    #[test]
    fn clear_keeps_user_model_ref() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "model": "my-own/model-x",
  "provider": {
    "ai-switcher": { "npm": "@ai-sdk/anthropic" },
    "aisw-p1": { "npm": "@ai-sdk/anthropic" },
    "my-own": { "npm": "@ai-sdk/anthropic" }
  }
}"#,
        )
        .expect("write");

        clear_provider_at(&path).expect("clear");

        let config = read_opencode_config_at(&path).expect("reload");
        assert!(config["provider"].get(MANAGED_PROVIDER_ID).is_none());
        assert!(config["provider"].get("aisw-p1").is_none());
        assert_eq!(config["model"], "my-own/model-x", "用户自己的 model 引用必须保留");
    }

    #[test]
    fn split_model_ref_splits_on_first_slash_only() {
        assert_eq!(split_model_ref("pid/mid"), Some(("pid", "mid")));
        assert_eq!(
            split_model_ref("zenmux/openai/gpt-5.5-fast"),
            Some(("zenmux", "openai/gpt-5.5-fast"))
        );
        assert_eq!(split_model_ref("no-slash"), None);
        assert_eq!(split_model_ref("/mid"), None);
        assert_eq!(split_model_ref("pid/"), None);
    }

    #[test]
    fn live_provider_reads_unmanaged_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "model": "acme/gpt-5.5",
  "provider": {
    "acme": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://acme.test/v1", "apiKey": "sk-acme" }
    }
  }
}"#,
        )
        .expect("write");

        let live = read_current_live_provider_at(&path).expect("read").expect("some");
        assert_eq!(live.base_url, "https://acme.test/v1");
        assert_eq!(live.auth_token, "sk-acme");
        assert_eq!(live.model, "gpt-5.5");
    }

    #[test]
    fn live_provider_skips_managed_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");

        apply_provider_at(&path, &test_provider(ProtocolType::Anthropic), &[]).expect("apply");

        assert!(read_current_live_provider_at(&path).expect("read").is_none());
    }

    #[test]
    fn live_providers_reads_all_entries_with_models_and_current() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "model": "acme/gpt-5.5",
  "provider": {
    "ai-switcher": { "npm": "@ai-sdk/anthropic", "options": { "baseURL": "http://127.0.0.1:15830" } },
    "aisw-p1": { "npm": "@ai-sdk/anthropic", "options": { "baseURL": "http://127.0.0.1:15830" } },
    "acme": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Acme 网关",
      "options": { "baseURL": "https://acme.test/v1", "apiKey": "sk-acme" },
      "models": { "gpt-5.5": {}, "gpt-5.5-mini": {} }
    },
    "beta": {
      "npm": "@ai-sdk/anthropic",
      "options": { "baseURL": "https://beta.test", "apiKey": "sk-beta" },
      "models": { "claude-sonnet-5": {} }
    },
    "no-base-url": { "npm": "@ai-sdk/openai-compatible", "options": { "apiKey": "sk-x" } }
  }
}"#,
        )
        .expect("write");

        let providers = read_live_providers_at(&path).expect("read");
        assert_eq!(providers.len(), 2, "托管项与无 baseURL 项必须跳过: {providers:?}");

        let acme = providers.iter().find(|p| p.id == "acme").expect("acme");
        assert_eq!(acme.name, "Acme 网关");
        assert_eq!(acme.base_url, "https://acme.test/v1");
        assert_eq!(acme.auth_token, "sk-acme");
        assert_eq!(acme.protocol_type, ProtocolType::OpenAiChat);
        assert_eq!(acme.models, vec!["gpt-5.5".to_string(), "gpt-5.5-mini".to_string()]);
        assert_eq!(acme.current_model.as_deref(), Some("gpt-5.5"));

        let beta = providers.iter().find(|p| p.id == "beta").expect("beta");
        assert_eq!(beta.name, "beta", "缺省 name 回退 provider id");
        assert_eq!(beta.protocol_type, ProtocolType::Anthropic);
        assert!(beta.current_model.is_none());
    }

    #[test]
    fn live_providers_appends_current_model_missing_from_models_map() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "model": "acme/gpt-5.5-fast",
  "provider": {
    "acme": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://acme.test/v1" },
      "models": { "gpt-5.5": {} }
    }
  }
}"#,
        )
        .expect("write");

        let providers = read_live_providers_at(&path).expect("read");
        let acme = &providers[0];
        assert_eq!(acme.current_model.as_deref(), Some("gpt-5.5-fast"));
        assert!(acme.models.iter().any(|m| m == "gpt-5.5-fast"), "当前模型必须并入 models");
    }

    #[test]
    fn live_providers_accepts_camel_case_base_url() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "provider": {
    "acme": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseUrl": "https://acme.test/v1", "apiKey": "sk-acme" },
      "models": { "gpt-5.5": {} }
    }
  }
}"#,
        )
        .expect("write");

        let providers = read_live_providers_at(&path).expect("read");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].base_url, "https://acme.test/v1");
        assert_eq!(providers[0].auth_token, "sk-acme");
    }

    #[test]
    fn fill_auth_tokens_only_fills_empty_keys() {
        let auth = json!({
            "acme": { "type": "api", "key": "sk-from-auth" },
            "beta": "sk-plain"
        });
        let mut providers = vec![
            OpenCodeLiveProvider {
                id: "acme".into(),
                name: "Acme".into(),
                base_url: "https://acme.test/v1".into(),
                auth_token: String::new(),
                protocol_type: ProtocolType::OpenAiChat,
                models: vec!["gpt-5.5".into()],
                current_model: None,
            },
            OpenCodeLiveProvider {
                id: "beta".into(),
                name: "Beta".into(),
                base_url: "https://beta.test".into(),
                auth_token: "sk-keep".into(),
                protocol_type: ProtocolType::Anthropic,
                models: vec!["claude".into()],
                current_model: None,
            },
        ];
        fill_auth_tokens(&mut providers, &auth);
        assert_eq!(providers[0].auth_token, "sk-from-auth");
        assert_eq!(providers[1].auth_token, "sk-keep");
    }

    #[test]
    fn merge_live_providers_skips_duplicate_ids() {
        let primary = vec![OpenCodeLiveProvider {
            id: "acme".into(),
            name: "From json".into(),
            base_url: "https://acme.test/v1".into(),
            auth_token: String::new(),
            protocol_type: ProtocolType::OpenAiChat,
            models: vec!["a".into()],
            current_model: None,
        }];
        let extra = vec![
            OpenCodeLiveProvider {
                id: "acme".into(),
                name: "From legacy".into(),
                base_url: "https://legacy.test/v1".into(),
                auth_token: String::new(),
                protocol_type: ProtocolType::OpenAiChat,
                models: vec!["b".into()],
                current_model: None,
            },
            OpenCodeLiveProvider {
                id: "other".into(),
                name: "Other".into(),
                base_url: "https://other.test/v1".into(),
                auth_token: String::new(),
                protocol_type: ProtocolType::OpenAiChat,
                models: vec!["c".into()],
                current_model: None,
            },
        ];
        let merged = merge_live_providers(primary, extra);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "From json");
        assert_eq!(merged[1].id, "other");
    }

    fn mcp_server(name: &str, enabled_opencode: bool, config: Value) -> McpServer {
        McpServer {
            id: format!("mcp_{name}"),
            name: name.to_string(),
            server_config: config,
            enabled_claude_code: false,
            enabled_claude_desktop: false,
            enabled_codex: false,
            enabled_opencode,
            enabled_pi: false,
            sort_index: 0,
            created_at: 0,
        }
    }

    #[test]
    fn sync_mcp_converts_formats_and_preserves_unmanaged() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  "mcp": {
    "user-own": { "type": "local", "command": ["foo"], "enabled": true },
    "stale-managed": { "type": "remote", "url": "https://old.test/mcp" }
  }
}"#,
        )
        .expect("write");

        let servers = vec![
            mcp_server(
                "fs",
                true,
                json!({"command": "npx", "args": ["-y", "srv"], "env": {"KEY": "v"}}),
            ),
            mcp_server(
                "web",
                true,
                json!({"type": "sse", "url": "https://web.test/mcp", "headers": {"A": "b"}}),
            ),
            // DB 中存在但未启用 OpenCode → 其同名键应从 mcp 段清除
            mcp_server("stale-managed", false, json!({"command": "x"})),
        ];
        sync_mcp_servers_at(&path, &servers).expect("sync");

        let config = read_opencode_config_at(&path).expect("reload");
        let mcp = config["mcp"].as_object().expect("mcp object");
        assert!(mcp.contains_key("user-own"), "用户自有键必须保留");
        assert!(!mcp.contains_key("stale-managed"), "托管但禁用的键必须清除");
        assert_eq!(
            mcp["fs"],
            json!({"type": "local", "command": ["npx", "-y", "srv"], "enabled": true, "environment": {"KEY": "v"}})
        );
        assert_eq!(
            mcp["web"],
            json!({"type": "remote", "url": "https://web.test/mcp", "enabled": true, "headers": {"A": "b"}})
        );
    }

    #[test]
    fn mcp_entry_from_opencode_round_trips() {
        let local = mcp_entry_from_opencode(&json!({
            "type": "local", "command": ["npx", "-y", "srv"], "environment": {"K": "v"}
        }))
        .expect("local");
        assert_eq!(local, json!({"command": "npx", "args": ["-y", "srv"], "env": {"K": "v"}}));

        let remote = mcp_entry_from_opencode(&json!({
            "type": "remote", "url": "https://x.test/mcp", "headers": {"A": "b"}
        }))
        .expect("remote");
        assert_eq!(remote, json!({"type": "http", "url": "https://x.test/mcp", "headers": {"A": "b"}}));

        assert!(mcp_entry_from_opencode(&json!({"type": "unknown"})).is_none());
    }
}
