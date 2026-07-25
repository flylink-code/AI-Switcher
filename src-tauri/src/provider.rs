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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTarget {
    ClaudeCode,
    ClaudeDesktop,
}

impl ProviderTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderTarget::ClaudeCode => "claude_code",
            ProviderTarget::ClaudeDesktop => "claude_desktop",
        }
    }

    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "claude_desktop" => ProviderTarget::ClaudeDesktop,
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

/// Normalize and validate a provider Base URL.
///
/// Provider URLs are HTTPS origins or gateway path prefixes. Known request
/// endpoint suffixes are stripped so the selected protocol remains the single
/// source of truth for the final upstream path.
pub fn normalize_base_url(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("API 基础地址不能为空".to_string()));
    }

    let mut url = reqwest::Url::parse(trimmed)
        .map_err(|_| AppError::Config("API 基础地址不是有效的 HTTPS URL".to_string()))?;
    if url.scheme() != "https" {
        return Err(AppError::Config("API 基础地址必须使用 https://".to_string()));
    }
    if url.host_str().is_none() {
        return Err(AppError::Config("API 基础地址缺少有效域名".to_string()));
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
    #[serde(default)]
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub sort_index: i64,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub health_status: Option<String>,
    #[serde(default)]
    pub health_checked_at: Option<i64>,
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
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub notes: String,
}

/// Sanitized result of a provider connectivity check.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub category: String,
    pub message: String,
    pub checked_at: i64,
}

/// Cached model-discovery result. Endpoint failures are represented as an empty
/// model list plus a sanitized message so manual model input remains possible.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDiscoveryResult {
    pub models: Vec<String>,
    pub message: String,
    pub checked_at: i64,
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
    pub protocol_type: ProtocolType,
    pub target_app: ProviderTarget,
    #[serde(default)]
    pub notes: String,
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
}

#[cfg(test)]
mod tests {
    use super::{api_endpoint_url, normalize_base_url, protocol_endpoint_path, ProtocolType};

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
}
