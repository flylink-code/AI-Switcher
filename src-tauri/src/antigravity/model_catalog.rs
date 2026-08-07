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

/// OpenAI-compatible `/v1/models` payload.
pub fn list_openai_models_payload() -> Value {
    let data: Vec<Value> = list_catalog_models()
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
    ids.iter()
        .find(|id| id.as_str() == "gemini-3-flash")
        .cloned()
        .or_else(|| {
            ids.iter()
                .find(|id| id.starts_with("gemini-3") && id.contains("flash") && !id.contains("image"))
                .cloned()
        })
        .or_else(|| {
            ids.iter()
                .find(|id| id.starts_with("gemini-") && id.contains("flash"))
                .cloned()
        })
}

pub fn preferred_claude_opus() -> Option<String> {
    let ids = list_model_ids();
    ids.iter()
        .find(|id| id.contains("opus"))
        .cloned()
}

pub fn preferred_gemini_pro() -> Option<String> {
    let ids = list_model_ids();
    ids.iter()
        .find(|id| id.as_str() == "gemini-3.1-pro-high")
        .cloned()
        .or_else(|| {
            ids.iter()
                .find(|id| id.contains("pro") && id.starts_with("gemini-"))
                .cloned()
        })
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
    for id in list_model_ids() {
        push(&mut out, id);
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
}
