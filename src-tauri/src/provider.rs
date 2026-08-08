//! Provider data model.
//!
//! A provider is a third-party API endpoint (plus credentials and model name) that
//! can be activated for Claude Code by writing its env vars into `settings.json`.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// How requests are routed to a provider.
///
/// - `anthropic`: the provider speaks the Anthropic API protocol natively; Claude
///   Code points directly at `base_url` (P1).
/// - `proxy`: the provider needs protocol translation; Claude Code points at the
///   local proxy which forwards to `base_url` (P2, reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolType {
    Anthropic,
    /// OpenAI Chat Completions upstream, reached through the local proxy.
    #[serde(rename = "openai_chat", alias = "open_ai_chat")]
    OpenAiChat,
    /// OpenAI Responses upstream, reached through the local proxy.
    #[serde(rename = "openai_responses", alias = "open_ai_responses")]
    OpenAiResponses,
    /// Legacy P2 value. It remains readable and behaves as OpenAI Chat.
    Proxy,
}

/// The Claude application whose configuration owns a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTarget {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    /// Keep wire format `opencode` (not `open_code`) to match frontend / DB values.
    #[serde(rename = "opencode")]
    OpenCode,
}

impl ProviderTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderTarget::ClaudeCode => "claude_code",
            ProviderTarget::ClaudeDesktop => "claude_desktop",
            ProviderTarget::Codex => "codex",
            ProviderTarget::OpenCode => "opencode",
        }
    }

    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "claude_desktop" => ProviderTarget::ClaudeDesktop,
            "codex" => ProviderTarget::Codex,
            "opencode" => ProviderTarget::OpenCode,
            _ => ProviderTarget::ClaudeCode,
        }
    }
}

impl Default for ProviderTarget {
    fn default() -> Self {
        ProviderTarget::ClaudeCode
    }
}

impl Default for ProtocolType {
    fn default() -> Self {
        ProtocolType::Anthropic
    }
}

impl ProtocolType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProtocolType::Anthropic => "anthropic",
            ProtocolType::OpenAiChat => "openai_chat",
            ProtocolType::OpenAiResponses => "openai_responses",
            ProtocolType::Proxy => "proxy",
        }
    }

    /// Parse from the stored string, falling back to [`ProtocolType::Anthropic`].
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "openai_chat" | "open_ai_chat" => ProtocolType::OpenAiChat,
            "openai_responses" | "open_ai_responses" => ProtocolType::OpenAiResponses,
            "proxy" => ProtocolType::Proxy,
            _ => ProtocolType::Anthropic,
        }
    }

    pub fn uses_proxy(self) -> bool {
        !matches!(self, ProtocolType::Anthropic)
    }
}

/// How a provider authenticates / routes upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    Standard,
    /// ChatGPT Plus/Pro subscription via local proxy (OAuth tokens stay in app data).
    CodexOauth,
    /// Built-in Antigravity Google-account gateway.
    Antigravity,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Standard => "standard",
            ProviderKind::CodexOauth => "codex_oauth",
            ProviderKind::Antigravity => "antigravity",
        }
    }

    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "codex_oauth" => ProviderKind::CodexOauth,
            "antigravity" => ProviderKind::Antigravity,
            _ => ProviderKind::Standard,
        }
    }
}

/// Codex accepts direct OpenAI-compatible model providers only. Claude model
/// roles have no meaning for this target.
pub fn validate_target_protocol(target: ProviderTarget, protocol: ProtocolType) -> AppResult<()> {
    if target == ProviderTarget::Codex
        && !matches!(
            protocol,
            ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Anthropic
        )
    {
        return Err(AppError::Config(
            "Codex 供应商仅支持 OpenAI Chat、OpenAI Responses 或 Anthropic Messages 协议".to_string(),
        ));
    }
    if target == ProviderTarget::OpenCode
        && !matches!(
            protocol,
            ProtocolType::Anthropic | ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses
        )
    {
        return Err(AppError::Config(
            "OpenCode 供应商仅支持 Anthropic Messages、OpenAI Chat 或 OpenAI Responses 协议".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_provider_kind(target: ProviderTarget, kind: ProviderKind) -> AppResult<()> {
    if kind == ProviderKind::CodexOauth
        && !matches!(target, ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop)
    {
        return Err(AppError::Config(
            "ChatGPT 订阅目前仅支持 Claude Code 和 Claude Desktop".to_string(),
        ));
    }
    Ok(())
}

pub fn normalized_model_mapping(target: ProviderTarget, mapping: ClaudeModelMapping) -> ClaudeModelMapping {
    if matches!(target, ProviderTarget::Codex | ProviderTarget::OpenCode) {
        ClaudeModelMapping::default()
    } else {
        mapping
    }
}

/// Normalize optional Codex auto-review model override; non-Codex targets always clear it.
pub fn normalized_auto_review_model_override(
    target: ProviderTarget,
    value: Option<String>,
) -> Option<String> {
    if target != ProviderTarget::Codex {
        return None;
    }
    value.and_then(|model| {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Codex bundled models.json uses 272000 as the default context window.
pub const CODEX_DEFAULT_CONTEXT_WINDOW: u64 = 272_000;

/// Resolve the effective Codex context window for catalog generation.
pub fn effective_model_context_window(provider: &Provider) -> u64 {
    provider
        .model_context_window
        .filter(|window| *window > 0)
        .unwrap_or(CODEX_DEFAULT_CONTEXT_WINDOW)
}

/// Normalize and validate a provider Base URL.
///
/// Provider URLs are HTTPS origins or gateway path prefixes. Known request
/// endpoint suffixes are stripped so the selected protocol remains the single
/// source of truth for the final upstream path.
fn is_local_http_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

pub fn normalize_base_url(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("API 基础地址不能为空".to_string()));
    }

    let mut url = reqwest::Url::parse(trimmed)
        .map_err(|_| AppError::Config("API 基础地址不是有效的 URL".to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::Config("API 基础地址缺少有效域名".to_string()))?;
    let allow_local_http = url.scheme() == "http" && is_local_http_host(host);
    if url.scheme() != "https" && !allow_local_http {
        return Err(AppError::Config(
            "API 基础地址必须使用 https://（本地网关可用 http://127.0.0.1）".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Config("API 基础地址不能包含用户名或密码".to_string()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::Config("API 基础地址不能包含查询参数或片段".to_string()));
    }

    let mut path = url.path().trim_end_matches('/').to_string();
    let lower_path = path.to_ascii_lowercase();
    for suffix in ["/chat/completions", "/messages", "/responses", "/models"] {
        if lower_path.ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
            path = path.trim_end_matches('/').to_string();
            break;
        }
    }
    url.set_path(if path.is_empty() { "/" } else { &path });

    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// Whether an OpenAI-compatible Base URL should end with `/v1`.
pub fn openai_compatible_base_url_needs_v1(base_url: &str) -> bool {
    let Ok(normalized) = normalize_base_url(base_url) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(&normalized) else {
        return false;
    };
    let path = url.path().trim_end_matches('/');
    path.is_empty() || path == "/"
}

/// Append `/v1` when the OpenAI-compatible Base URL is only a host root.
pub fn ensure_openai_v1_suffix(base_url: &str) -> AppResult<String> {
    let normalized = normalize_base_url(base_url)?;
    if openai_compatible_base_url_needs_v1(&normalized) {
        Ok(format!("{normalized}/v1"))
    } else {
        Ok(normalized)
    }
}

/// Normalize Base URL for persistence. Codex OpenAI providers auto-append `/v1`
/// when the path is empty so `config.toml` base_url matches OpenAI-compatible gateways.
/// OpenCode 的 `@ai-sdk/openai-compatible` 同样需要以 `/v1` 结尾的 baseURL。
pub fn normalize_provider_base_url(
    target: ProviderTarget,
    protocol: ProtocolType,
    base_url: &str,
) -> AppResult<String> {
    let normalized = normalize_base_url(base_url)?;
    if matches!(target, ProviderTarget::Codex | ProviderTarget::OpenCode)
        && matches!(protocol, ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses)
    {
        ensure_openai_v1_suffix(&normalized)
    } else {
        Ok(normalized)
    }
}

/// Build an API endpoint URL while preserving provider-specific gateway paths.
///
/// Providers commonly store either a host root (`https://api.example.com`) or
/// a versioned root (`https://api.example.com/v1`). Callers always use normal
/// API endpoint paths such as `/v1/messages`; this helper avoids accidentally
/// producing `/v1/v1/messages` for the latter form.
pub fn api_endpoint_url(base_url: &str, endpoint: &str) -> AppResult<String> {
    let base = normalize_base_url(base_url)?;
    let endpoint = if endpoint.starts_with('/') {
        endpoint
    } else {
        return Err(AppError::Config("API 端点路径必须以 / 开头".to_string()));
    };
    let endpoint = if base.ends_with("/v1") && endpoint.starts_with("/v1/") {
        &endpoint[3..]
    } else {
        endpoint
    };
    Ok(format!("{base}{endpoint}"))
}

/// Final request path selected by the provider API protocol.
pub fn protocol_endpoint_path(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Anthropic => "/v1/messages",
        ProtocolType::OpenAiChat | ProtocolType::Proxy => "/v1/chat/completions",
        ProtocolType::OpenAiResponses => "/v1/responses",
    }
}

/// ChatGPT Codex backend uses `/responses` under `.../backend-api/codex`.
pub fn protocol_endpoint_path_for_provider(provider: &Provider) -> &'static str {
    if provider.is_codex_oauth() {
        "/responses"
    } else {
        protocol_endpoint_path(provider.protocol_type)
    }
}

/// Optional upstream model overrides for Claude's built-in model roles.
///
/// `Provider.model` remains the required default. Empty role values fall back
/// to that default so legacy single-model providers continue to work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelMapping {
    #[serde(default)]
    pub sonnet: String,
    #[serde(default)]
    pub opus: String,
    #[serde(default)]
    pub haiku: String,
    #[serde(default)]
    pub fable: String,
    #[serde(default)]
    pub subagent: String,
}

impl ClaudeModelMapping {
    pub fn has_explicit_roles(&self) -> bool {
        [
            &self.sonnet,
            &self.opus,
            &self.haiku,
            &self.fable,
            &self.subagent,
        ]
        .iter()
        .any(|model| !model.trim().is_empty())
    }

    pub fn for_role<'a>(&'a self, role: ClaudeModelRole, default: &'a str) -> &'a str {
        let configured = match role {
            ClaudeModelRole::Sonnet => &self.sonnet,
            ClaudeModelRole::Opus => &self.opus,
            ClaudeModelRole::Haiku => &self.haiku,
            ClaudeModelRole::Fable => &self.fable,
            ClaudeModelRole::Subagent => &self.subagent,
        };
        if configured.trim().is_empty() {
            default.trim()
        } else {
            configured.trim()
        }
    }

    pub fn configured_models(&self) -> impl Iterator<Item = &str> {
        [
            self.sonnet.as_str(),
            self.opus.as_str(),
            self.haiku.as_str(),
            self.fable.as_str(),
            self.subagent.as_str(),
        ]
        .into_iter()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeModelRole {
    Sonnet,
    Opus,
    Haiku,
    Fable,
    Subagent,
}

pub const CLAUDE_SONNET_ROLE_ID: &str = "claude-sonnet-5";
pub const CLAUDE_OPUS_ROLE_ID: &str = "claude-opus-5";
pub const CLAUDE_HAIKU_ROLE_ID: &str = "claude-haiku-4-5";
pub const CLAUDE_FABLE_ROLE_ID: &str = "claude-fable-5";

fn classify_claude_model_role(model: &str) -> Option<ClaudeModelRole> {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("subagent") {
        Some(ClaudeModelRole::Subagent)
    } else if normalized.contains("fable") {
        Some(ClaudeModelRole::Fable)
    } else if normalized.contains("haiku") {
        Some(ClaudeModelRole::Haiku)
    } else if normalized.contains("opus") {
        Some(ClaudeModelRole::Opus)
    } else if normalized.contains("sonnet") {
        Some(ClaudeModelRole::Sonnet)
    } else {
        None
    }
}

/// Resolve a Claude-facing model id to the configured upstream model.
///
/// Requests already carrying a configured upstream id pass through unchanged.
/// Claude role names use their override, and all unknown values fall back to
/// the provider default.
pub fn resolve_upstream_model(provider: &Provider, requested: &str) -> String {
    let requested = requested.trim();
    let default = provider.model.trim();
    if requested == default
        || provider
            .model_mapping
            .configured_models()
            .any(|configured| configured == requested)
    {
        return requested.to_string();
    }
    classify_claude_model_role(requested)
        .map(|role| provider.model_mapping.for_role(role, default))
        .unwrap_or(default)
        .to_string()
}

/// A single API provider. Field names are camelCase on the wire for the frontend.
///
/// Note on `api_key`: since P7 this field holds either a keyring reference
/// (`kr://<id>`) or an empty string — **never the plaintext secret**. It is
/// skipped during serialization so the secret/reference never reaches the
/// frontend. Use [`crate::secrets`] / [`crate::database::dao::providers::resolve_api_key`]
/// to obtain the real key at runtime. The frontend instead reads `api_key_set`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// Keyring reference (`kr://<id>`) or empty. Never serialized to the frontend.
    #[serde(skip_serializing, default)]
    pub api_key: String,
    /// Whether an API key is stored for this provider (derived from `api_key`).
    #[serde(default)]
    pub api_key_set: bool,
    /// Primary model written to `ANTHROPIC_MODEL`. May be empty.
    #[serde(default)]
    pub model: String,
    /// Optional Codex model catalog context window on provider rows.
    #[serde(default)]
    pub model_context_window: Option<u64>,
    /// Optional Codex catalog web search tool flag. `None` = auto (enabled unless Anthropic upstream).
    #[serde(default)]
    pub web_search_enabled: Option<bool>,
    /// Optional upstream model for Codex guardian/auto_review subagent requests.
    #[serde(default)]
    pub auto_review_model_override: Option<String>,
    #[serde(default)]
    pub model_mapping: ClaudeModelMapping,
    #[serde(default)]
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub provider_kind: ProviderKind,
    #[serde(default)]
    pub auth_binding: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub sort_index: i64,
    /// Lower group number = higher failover priority. Same group uses `sort_index`.
    #[serde(default)]
    pub failover_group: i64,
    /// Empty = accept any model as failover candidate; otherwise request model must match.
    #[serde(default)]
    pub failover_models: Vec<String>,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub health_status: Option<String>,
    #[serde(default)]
    pub health_checked_at: Option<i64>,
    /// Parsed from last health probe detail (`|latency_ms=N`).
    #[serde(default)]
    pub health_latency_ms: Option<u64>,
}

impl Provider {
    pub fn is_codex_oauth(&self) -> bool {
        self.provider_kind == ProviderKind::CodexOauth
    }

    pub fn is_antigravity(&self) -> bool {
        self.provider_kind == ProviderKind::Antigravity
    }

    pub fn requires_local_proxy(&self) -> bool {
        if self.is_codex_oauth() {
            return true;
        }
        // Built-in Antigravity gateway already speaks Anthropic/OpenAI; Code/Codex
        // can point at it directly. Desktop may still need the local proxy for
        // role-based model rewriting.
        if self.is_antigravity() {
            return self.target_app == ProviderTarget::ClaudeDesktop;
        }
        // Codex speaks OpenAI wire APIs natively. Keep OpenAI-compatible Codex
        // providers on the real upstream so chat works without the desktop
        // proxy process. Anthropic upstream still needs protocol translation.
        if self.target_app == ProviderTarget::Codex {
            return self.protocol_type == ProtocolType::Anthropic;
        }
        // OpenCode 通过 AI SDK 包（@ai-sdk/anthropic / @ai-sdk/openai-compatible）
        // 原生支持各协议，直连写入 baseURL/apiKey 即可，无需本地代理。
        if self.target_app == ProviderTarget::OpenCode {
            return false;
        }
        if self.protocol_type.uses_proxy() {
            return true;
        }
        self.target_app == ProviderTarget::ClaudeDesktop
            && (self.model_mapping.has_explicit_roles()
                || !is_claude_desktop_safe_model(self.model.trim()))
    }

    /// Whether this provider may serve `requested_model` during failover.
    pub fn allows_failover_model(&self, requested_model: &str) -> bool {
        if self.failover_models.is_empty() {
            return true;
        }
        let requested = requested_model.trim();
        if requested.is_empty() {
            return true;
        }
        let requested_lower = requested.to_ascii_lowercase();
        self.failover_models.iter().any(|entry| {
            let needle = entry.trim();
            if needle.is_empty() {
                return false;
            }
            needle.eq_ignore_ascii_case(requested)
                || requested_lower.starts_with(&needle.to_ascii_lowercase())
        })
    }

    /// Accepts when either the client-requested model or the mapped upstream model matches.
    pub fn allows_failover_for_request(&self, requested_model: &str) -> bool {
        if self.failover_models.is_empty() {
            return true;
        }
        let mapped = resolve_upstream_model(self, requested_model);
        self.allows_failover_model(requested_model) || self.allows_failover_model(&mapped)
    }
}

fn is_claude_desktop_safe_model(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    normalized.starts_with("claude-") || normalized.starts_with("anthropic/claude-")
}

/// Subset that can be created/updated from the frontend. `id` is optional on
/// create (assigned server-side); required on update.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// Canonical HTTPS origin or gateway path prefix, never a request endpoint.
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    /// Empty `api_key` keeps the credential when updating; this explicit flag
    /// requests its removal.
    #[serde(default)]
    pub clear_api_key: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub model_context_window: Option<u64>,
    #[serde(default)]
    pub auto_review_model_override: Option<String>,
    #[serde(default)]
    pub web_search_enabled: Option<bool>,
    #[serde(default)]
    pub model_mapping: ClaudeModelMapping,
    #[serde(default)]
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub provider_kind: ProviderKind,
    #[serde(default)]
    pub auth_binding: String,
    #[serde(default)]
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub failover_group: i64,
    #[serde(default)]
    pub failover_models: Vec<String>,
}

/// Sanitized result of a provider connectivity check.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub category: String,
    pub message: String,
    pub checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

/// Lightweight RTT probe against a provider Base URL (no API auth required).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSpeedtestResult {
    pub ok: bool,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub checked_at: i64,
    pub url: String,
}

/// Cached model-discovery result. Endpoint failures are represented as an empty
/// model list plus a sanitized message so manual model input remains possible.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDiscoveryResult {
    pub models: Vec<String>,
    pub message: String,
    pub checked_at: i64,
    pub source: String,
    pub stale: bool,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

/// Versioned, intentionally credential-free provider export format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExportBundle {
    pub version: u32,
    pub providers: Vec<ProviderExportEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExportEntry {
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub model_context_window: Option<u64>,
    #[serde(default)]
    pub web_search_enabled: Option<bool>,
    #[serde(default)]
    pub model_mapping: ClaudeModelMapping,
    pub protocol_type: ProtocolType,
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub failover_group: i64,
    #[serde(default)]
    pub failover_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportResult {
    pub imported: usize,
    pub skipped: usize,
}

/// Information parsed from a live third-party configuration.
#[derive(Debug, Clone, Serialize)]
pub struct LiveProviderInfo {
    pub base_url: String,
    pub auth_token: String,
    pub model: String,
    pub model_mapping: ClaudeModelMapping,
    /// 导入时使用的协议。历史目标（Claude/Codex）构造时保持 Anthropic 默认值，
    /// 与导入流程原先硬编码的行为一致；OpenCode 按 npm 包名推断。
    #[serde(default)]
    pub protocol_type: ProtocolType,
}

#[cfg(test)]
mod tests {
    use super::{
        api_endpoint_url, ensure_openai_v1_suffix, normalize_base_url, normalize_provider_base_url,
        openai_compatible_base_url_needs_v1, protocol_endpoint_path, resolve_upstream_model,
        normalized_model_mapping, validate_target_protocol, ClaudeModelMapping, ProtocolType,
        Provider, ProviderKind, ProviderTarget,
    };

    #[test]
    fn openai_compatible_base_url_appends_v1_for_host_roots() {
        assert!(openai_compatible_base_url_needs_v1("https://api.example.test"));
        assert!(!openai_compatible_base_url_needs_v1("https://api.example.test/v1"));
        assert!(!openai_compatible_base_url_needs_v1("https://gateway.example.test/openai/v1"));
        assert_eq!(
            ensure_openai_v1_suffix("https://api.example.test").unwrap(),
            "https://api.example.test/v1"
        );
        assert_eq!(
            normalize_provider_base_url(
                ProviderTarget::Codex,
                ProtocolType::OpenAiResponses,
                "https://api.example.test"
            )
            .unwrap(),
            "https://api.example.test/v1"
        );
        assert_eq!(
            normalize_provider_base_url(
                ProviderTarget::ClaudeCode,
                ProtocolType::OpenAiChat,
                "https://api.example.test"
            )
            .unwrap(),
            "https://api.example.test"
        );
    }

    #[test]
    fn endpoint_url_preserves_gateway_path_without_duplicate_v1() {
        assert_eq!(
            api_endpoint_url("https://api.example.test", "/v1/messages").unwrap(),
            "https://api.example.test/v1/messages"
        );
        assert_eq!(
            api_endpoint_url("https://api.example.test/v1/", "/v1/messages").unwrap(),
            "https://api.example.test/v1/messages"
        );
        assert_eq!(
            api_endpoint_url("https://gateway.example.test/openai/v1", "/v1/models").unwrap(),
            "https://gateway.example.test/openai/v1/models"
        );
    }

    #[test]
    fn base_url_normalization_strips_known_request_endpoints() {
        for (input, expected) in [
            (" https://api.example.test/ ", "https://api.example.test"),
            ("https://api.example.test/v1/messages", "https://api.example.test/v1"),
            (
                "https://gateway.example.test/openai/v1/chat/completions/",
                "https://gateway.example.test/openai/v1",
            ),
            ("https://api.example.test/v1/responses", "https://api.example.test/v1"),
            ("https://api.example.test/v1/models", "https://api.example.test/v1"),
        ] {
            assert_eq!(normalize_base_url(input).unwrap(), expected);
        }
    }

    #[test]
    fn base_url_validation_rejects_unsafe_or_ambiguous_urls() {
        for value in [
            "http://api.example.test",
            "https://user:pass@api.example.test/v1",
            "https://api.example.test/v1?tenant=one",
            "https://api.example.test/v1#section",
            "not-a-url",
        ] {
            assert!(normalize_base_url(value).is_err(), "{value} should be rejected");
        }
    }

    #[test]
    fn normalize_base_url_allows_local_http_gateways() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8045").unwrap(),
            "http://127.0.0.1:8045"
        );
        assert_eq!(
            normalize_base_url("http://localhost:15830/v1/messages").unwrap(),
            "http://localhost:15830/v1"
        );
        assert!(normalize_base_url("http://example.com").is_err());
    }

    #[test]
    fn protocol_selects_the_upstream_request_path() {
        assert_eq!(protocol_endpoint_path(ProtocolType::Anthropic), "/v1/messages");
        assert_eq!(
            protocol_endpoint_path(ProtocolType::OpenAiChat),
            "/v1/chat/completions"
        );
        assert_eq!(
            protocol_endpoint_path(ProtocolType::OpenAiResponses),
            "/v1/responses"
        );
    }

    #[test]
    fn openai_protocol_wire_values_match_the_frontend() {
        assert_eq!(
            serde_json::from_str::<ProtocolType>("\"openai_responses\"").unwrap(),
            ProtocolType::OpenAiResponses
        );
        assert_eq!(
            serde_json::to_string(&ProtocolType::OpenAiChat).unwrap(),
            "\"openai_chat\""
        );
    }

    #[test]
    fn codex_accepts_openai_and_anthropic_protocols_and_clears_claude_mapping() {
        assert!(validate_target_protocol(ProviderTarget::Codex, ProtocolType::Anthropic).is_ok());
        assert!(validate_target_protocol(ProviderTarget::Codex, ProtocolType::Proxy).is_err());
        assert!(validate_target_protocol(ProviderTarget::Codex, ProtocolType::OpenAiResponses).is_ok());
        let mapping = ClaudeModelMapping { sonnet: "claude-sonnet".into(), ..Default::default() };
        assert!(!normalized_model_mapping(ProviderTarget::Codex, mapping).has_explicit_roles());
    }

    #[test]
    fn codex_anthropic_requires_local_proxy() {
        let provider = Provider {
            id: "codex-anthropic".into(),
            name: "Anthropic Codex".into(),
            base_url: "https://api.anthropic.test".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "claude-sonnet-4-20250514".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::Codex,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        };
        assert!(provider.requires_local_proxy());
    }

    #[test]
    fn codex_openai_responses_does_not_require_local_proxy() {
        let provider = Provider {
            id: "codex-openai".into(),
            name: "OpenAI Codex".into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "gpt-5.6-terra".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiResponses,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::Codex,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        };
        assert!(!provider.requires_local_proxy());
    }

    fn provider_with_mapping() -> Provider {
        Provider {
            id: "mapped".into(),
            name: "Mapped".into(),
            base_url: "https://api.example.test".into(),
            api_key: String::new(),
            api_key_set: false,
            model: "default-model".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping {
                sonnet: "sonnet-upstream".into(),
                opus: "opus-upstream".into(),
                haiku: String::new(),
                fable: "fable-upstream".into(),
                subagent: "agent-upstream".into(),
            },
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
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
    fn role_models_resolve_with_exact_pass_through_and_default_fallback() {
        let provider = provider_with_mapping();
        assert_eq!(
            resolve_upstream_model(&provider, "claude-sonnet-5"),
            "sonnet-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-sonnet-4-6"),
            "sonnet-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-opus-5"),
            "opus-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-opus-5[1m]"),
            "opus-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-opus-4-8"),
            "opus-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-opus-4-8[1m]"),
            "opus-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-haiku-4-5"),
            "default-model"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "claude-fable-5"),
            "fable-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "agent-upstream"),
            "agent-upstream"
        );
        assert_eq!(
            resolve_upstream_model(&provider, "unknown-model"),
            "default-model"
        );
    }

    #[test]
    fn failover_model_whitelist_matches_request_or_mapped_upstream() {
        let mut provider = provider_with_mapping();
        provider.failover_models = vec!["opus-upstream".into()];
        assert!(provider.allows_failover_for_request("claude-opus-5"));
        assert!(!provider.allows_failover_for_request("claude-sonnet-5"));

        provider.failover_models = vec!["claude-sonnet".into()];
        assert!(provider.allows_failover_for_request("claude-sonnet-5"));
        assert!(provider.allows_failover_model(""));
    }
}
