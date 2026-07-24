//! Provider data model.
//!
//! A provider is a third-party API endpoint (plus credentials and model name) that
//! can be activated for Claude Code by writing its env vars into `settings.json`.

use serde::{Deserialize, Serialize};

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
            ProtocolType::Proxy => "proxy",
        }
    }

    /// Parse from the stored string, falling back to [`ProtocolType::Anthropic`].
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "proxy" => ProtocolType::Proxy,
            _ => ProtocolType::Anthropic,
        }
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
