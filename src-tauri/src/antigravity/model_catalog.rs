//! Live Antigravity / Cloud Code model catalog.
//!
//! Populated from `fetchAvailableModels` (via quota refresh) and served to agents
//! through `/v1/models` and provider failover suggestions.

use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::antigravity::quota::ModelQuota;

const FALLBACK_IDS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-sonnet-4-6-thinking",
    "claude-opus-4-6-thinking",
    "gemini-3-flash",
    "gemini-3.1-pro-high",
    "gemini-2.5-flash",
    "gemini-2.5-pro",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Default)]
struct CatalogState {
    models: Vec<CatalogModel>,
    updated_at: i64,
}

static CATALOG: RwLock<CatalogState> = RwLock::new(CatalogState {
    models: Vec::new(),
    updated_at: 0,
});

/// Gemini reasoning levels, ordered low → high for bare-name fallback.
///
/// Cloud Code has no separate reasoning parameter — the level is encoded in the
/// model id suffix (`gemini-3.6-flash-high`). Level variants are exposed to
/// clients (Claude Code / Desktop / Codex) as-is so each client picks its own
/// level in its model selector; the gateway only composes a fallback variant
/// when a client sends a bare Gemini name with no suffix.
const LEVEL_SUFFIXES: [&str; 3] = ["low", "medium", "high"];

/// Client-facing alias id for Gemini 3.6 Flash. Claude Desktop's reasoning
/// effort slider is gated by a hardcoded model table (`claude-sonnet-5` is in
/// it), so the alias makes the slider available; the gateway maps it back to
/// the real Gemini flash variant at request time.
pub const GEMINI_FLASH_ALIAS_ID: &str = "claude-sonnet-5";

/// Split a model id into (base, explicit level suffix) when it ends with
/// `-low` / `-medium` / `-high`.
fn split_level_suffix(id: &str) -> (&str, Option<&str>) {
    for suffix in ["-low", "-medium", "-high"] {
        if let Some(base) = id.strip_suffix(suffix) {
            return (base, Some(&suffix[1..]));
        }
    }
    (id, None)
}

/// Compose the real upstream model id from a (possibly bare) Gemini name.
///
/// - Non-Gemini ids and ids with an explicit level suffix pass through
///   unchanged (explicit client choice wins — clients select the level via the
///   suffixed ids exposed in `/v1/models`).
/// - Bare Gemini names: keep the bare id when it exists upstream; otherwise
///   pick the first available variant in low → medium → high order.
pub fn with_reasoning_level(id: &str) -> String {
    let trimmed = id.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("gemini-") {
        return trimmed.to_string();
    }
    let (base, explicit) = split_level_suffix(&lower);
    if explicit.is_some() {
        return trimmed.to_string();
    }
    let ids = list_model_ids();
    let exists = |candidate: &str| {
        ids.iter().any(|model| model.eq_ignore_ascii_case(candidate))
    };
    if exists(&lower) {
        return trimmed.to_string();
    }
    for level in LEVEL_SUFFIXES {
        let candidate = format!("{base}-{level}");
        if exists(&candidate) {
            return candidate;
        }
    }
    trimmed.to_string()
}

/// Force a specific level variant for a Gemini id: strip any existing suffix
/// and compose `base-{level}` when that variant exists upstream; otherwise
/// fall back to the first available variant via [`with_reasoning_level`].
///
/// When the catalog only has a bare id (no `-low`/`-medium`/`-high` siblings),
/// still return the composed suffix so Desktop effort / alias defaults are not
/// silently dropped to the bare name.
pub fn with_forced_level(id: &str, level: &str) -> String {
    let trimmed = id.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("gemini-") {
        return trimmed.to_string();
    }
    let (base, _) = split_level_suffix(&lower);
    let ids = list_model_ids();
    let candidate = format!("{base}-{level}");
    if ids.iter().any(|model| model.eq_ignore_ascii_case(&candidate)) {
        return candidate;
    }
    let has_any_level = LEVEL_SUFFIXES.iter().any(|suffix| {
        let sibling = format!("{base}-{suffix}");
        ids.iter()
            .any(|model| model.eq_ignore_ascii_case(&sibling))
    });
    if has_any_level {
        return with_reasoning_level(base);
    }
    candidate
}

/// Prefer the Gemini 3.6 Flash bare base (target of [`GEMINI_FLASH_ALIAS_ID`]);
/// fall back to any flash bare base in the catalog.
pub fn preferred_gemini_36_flash_base() -> Option<String> {
    let ids = list_model_ids();
    let is_flash = |id: &&String| {
        id.starts_with("gemini-") && id.contains("flash") && !id.contains("image")
    };
    let pick = ids
        .iter()
        .find(|id| id.starts_with("gemini-3.6") && is_flash(id))
        .or_else(|| ids.iter().find(is_flash))?;
    let lower_pick = pick.to_ascii_lowercase();
    let (base, _) = split_level_suffix(&lower_pick);
    Some(base.to_string())
}

/// Catalog plus the synthetic Gemini-flash alias entry (see
/// [`GEMINI_FLASH_ALIAS_ID`]). Used for `/v1/models` and provider model lists
/// so clients can bind the alias directly.
pub fn list_catalog_models_with_alias() -> Vec<CatalogModel> {
    let mut models = list_catalog_models();
    if !models.iter().any(|model| model.id == GEMINI_FLASH_ALIAS_ID) {
        models.push(CatalogModel {
            id: GEMINI_FLASH_ALIAS_ID.to_string(),
            display_name: Some("Gemini Flash (Desktop 推理档位别名)".to_string()),
        });
    }
    models
}

fn lock_write() -> std::sync::RwLockWriteGuard<'static, CatalogState> {
    match CATALOG.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_read() -> std::sync::RwLockReadGuard<'static, CatalogState> {
    match CATALOG.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Whether this Cloud Code model id is useful for Claude Code / Codex agents.
pub fn is_agent_facing_model(id: &str) -> bool {
    let lower = id.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    // Internal IDE / autocomplete / chat-router ids.
    if lower.starts_with("chat_")
        || lower.starts_with("tab_")
        || lower.contains("flash-image")
        || lower.ends_with("-image")
    {
        return false;
    }
    lower.starts_with("claude-")
        || lower.starts_with("gemini-")
        || lower.starts_with("gpt-")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}

/// Merge models discovered from a quota refresh into the live catalog.
pub fn update_from_quota_models(models: &[ModelQuota]) {
    let mut incoming: Vec<CatalogModel> = models
        .iter()
        .filter(|model| is_agent_facing_model(&model.name))
        .map(|model| CatalogModel {
            id: model.name.clone(),
            display_name: model.display_name.clone(),
        })
        .collect();
    if incoming.is_empty() {
        return;
    }
    incoming.sort_by(|left, right| left.id.cmp(&right.id));
    incoming.dedup_by(|left, right| left.id == right.id);

    let mut guard = lock_write();
    // Prefer the freshest non-empty refresh; replace wholesale so retired models drop.
    guard.models = incoming;
    guard.updated_at = chrono::Utc::now().timestamp();
    log::info!(
        "Antigravity model catalog updated: {} agent-facing models",
        guard.models.len()
    );
}

/// Seed catalog from any already-persisted account quotas (app startup / empty cache).
pub fn seed_from_accounts(account_models: impl IntoIterator<Item = ModelQuota>) {
    let mut by_id = std::collections::BTreeMap::<String, CatalogModel>::new();
    for model in account_models {
        if !is_agent_facing_model(&model.name) {
            continue;
        }
        by_id.entry(model.name.clone()).or_insert(CatalogModel {
            id: model.name,
            display_name: model.display_name,
        });
    }
    if by_id.is_empty() {
        return;
    }
    let mut guard = lock_write();
    if !guard.models.is_empty() {
        return;
    }
    guard.models = by_id.into_values().collect();
    guard.updated_at = chrono::Utc::now().timestamp();
}

fn fallback_models() -> Vec<CatalogModel> {
    FALLBACK_IDS
        .iter()
        .map(|id| CatalogModel {
            id: (*id).to_string(),
            display_name: None,
        })
        .collect()
}

pub fn list_catalog_models() -> Vec<CatalogModel> {
    let guard = lock_read();
    if guard.models.is_empty() {
        fallback_models()
    } else {
        guard.models.clone()
    }
}

pub fn list_model_ids() -> Vec<String> {
    list_catalog_models()
        .into_iter()
        .map(|model| model.id)
        .collect()
}

/// OpenAI-compatible `/v1/models` payload — full upstream catalog, Gemini
/// level variants included so clients can pick the reasoning level directly,
/// plus the synthetic Gemini-flash alias for Claude Desktop's effort slider.
pub fn list_openai_models_payload() -> Value {
    let data: Vec<Value> = list_catalog_models_with_alias()
        .into_iter()
        .map(|model| {
            json!({
                "id": model.id,
                "object": "model",
                "owned_by": "antigravity",
                "display_name": model.display_name,
            })
        })
        .collect();
    json!(data)
}

/// Prefer a Claude Sonnet default when present; otherwise first catalog id.
pub fn preferred_default_model() -> String {
    let ids = list_model_ids();
    ids.iter()
        .find(|id| id.as_str() == "claude-sonnet-4-6")
        .cloned()
        .or_else(|| {
            ids.iter()
                .find(|id| id.starts_with("claude-sonnet"))
                .cloned()
        })
        .or_else(|| ids.first().cloned())
        .unwrap_or_else(|| "claude-sonnet-4-6".into())
}

pub fn preferred_gemini_flash() -> Option<String> {
    let ids = list_model_ids();
    let is_flash = |id: &&String| {
        id.starts_with("gemini-") && id.contains("flash") && !id.contains("image")
    };
    ids.iter()
        .find(|id| id.as_str() == "gemini-3-flash")
        .or_else(|| {
            ids.iter()
                .find(|id| id.starts_with("gemini-3") && is_flash(id))
        })
        .or_else(|| ids.iter().find(is_flash))
        .cloned()
}

pub fn preferred_claude_opus() -> Option<String> {
    let ids = list_model_ids();
    ids.iter()
        .find(|id| id.contains("opus"))
        .cloned()
}

pub fn preferred_gemini_pro() -> Option<String> {
    let ids = list_model_ids();
    let is_pro = |id: &&String| id.contains("pro") && id.starts_with("gemini-");
    ids.iter()
        .find(|id| id.as_str() == "gemini-3.1-pro-high")
        .or_else(|| ids.iter().find(is_pro))
        .cloned()
}

/// Failover / suggestion list for providers (default + flash + pro + opus + rest).
pub fn provider_suggestion_ids(limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<String>, id: String| {
        if !out.iter().any(|existing| existing == &id) {
            out.push(id);
        }
    };
    push(&mut out, preferred_default_model());
    if let Some(id) = preferred_gemini_flash() {
        push(&mut out, id);
    }
    if let Some(id) = preferred_gemini_pro() {
        push(&mut out, id);
    }
    if let Some(id) = preferred_claude_opus() {
        push(&mut out, id);
    }
    for model in list_catalog_models() {
        push(&mut out, model.id);
        if out.len() >= limit {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_internal_ids() {
        assert!(is_agent_facing_model("gemini-3-flash"));
        assert!(is_agent_facing_model("claude-sonnet-4-6"));
        assert!(!is_agent_facing_model("chat_20706"));
        assert!(!is_agent_facing_model("tab_flash_lite_preview"));
        assert!(!is_agent_facing_model("gemini-3.1-flash-image"));
    }

    #[test]
    fn with_reasoning_level_passthrough_and_bare_fallback() {
        // Fallback catalog: gemini-3-flash (bare), gemini-3.1-pro-high, gemini-2.5-*.
        // Explicit suffix wins — untouched.
        assert_eq!(with_reasoning_level("gemini-3.6-flash-low"), "gemini-3.6-flash-low");
        assert_eq!(with_reasoning_level("gemini-3.6-flash-high"), "gemini-3.6-flash-high");
        // Claude ids untouched.
        assert_eq!(with_reasoning_level("claude-sonnet-4-6"), "claude-sonnet-4-6");
        // Bare id that exists upstream as-is stays bare.
        assert_eq!(with_reasoning_level("gemini-3-flash"), "gemini-3-flash");
        // Bare base with only a -high variant composes to it.
        assert_eq!(with_reasoning_level("gemini-3.1-pro"), "gemini-3.1-pro-high");
        // Unknown bare id with no catalog variant passes through.
        assert_eq!(with_reasoning_level("gemini-9.9-flash"), "gemini-9.9-flash");
    }

    #[test]
    fn catalog_exposes_gemini_level_variants() {
        // Level variants are listed as-is so clients can pick the level.
        let ids = list_model_ids();
        assert!(ids.iter().any(|id| id == "gemini-3.1-pro-high"));
        assert!(!ids.iter().any(|id| id == "gemini-3.1-pro"));
        // Preferred picks expose real upstream ids too.
        assert_eq!(preferred_gemini_pro().as_deref(), Some("gemini-3.1-pro-high"));
    }

    #[test]
    fn with_forced_level_composes_or_falls_back() {
        // Existing variant composes (suffix stripped first).
        assert_eq!(with_forced_level("gemini-3.1-pro-high", "high"), "gemini-3.1-pro-high");
        // Missing variant in a leveled family falls back to an available sibling.
        assert_eq!(with_forced_level("gemini-3.1-pro-high", "low"), "gemini-3.1-pro-high");
        // Bare-only family (fallback catalog has gemini-3-flash, no -high sibling)
        // still honors the forced suffix so effort is not dropped.
        assert_eq!(with_forced_level("gemini-3-flash", "high"), "gemini-3-flash-high");
        // Non-Gemini ids pass through untouched.
        assert_eq!(with_forced_level("claude-sonnet-4-6", "low"), "claude-sonnet-4-6");
    }

    #[test]
    fn catalog_with_alias_adds_desktop_alias_once() {
        let models = list_catalog_models_with_alias();
        let count = models
            .iter()
            .filter(|model| model.id == GEMINI_FLASH_ALIAS_ID)
            .count();
        assert_eq!(count, 1);
    }
}
