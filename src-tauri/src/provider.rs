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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// Auth token written to `ANTHROPIC_AUTH_TOKEN`. May be empty for presets.
    #[serde(default)]
    pub api_key: String,
    /// Primary model written to `ANTHROPIC_MODEL`. May be empty.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub sort_index: i64,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub created_at: i64,
}

/// Subset that can be created/updated from the frontend. `id` is optional on
/// create (assigned server-side); required on update.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub protocol_type: ProtocolType,
    #[serde(default)]
    pub notes: String,
}

/// Information parsed from the live `settings.json` env block.
#[derive(Debug, Clone, Serialize)]
pub struct LiveProviderInfo {
    pub base_url: String,
    pub auth_token: String,
    pub model: String,
}
