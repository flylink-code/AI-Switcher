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
    "claude-opus-4-6-thinking",
    "gemini-3.7-flash",
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

/// Explicit `-low` / `-medium` / `-high` suffix on a client-requested model id.
pub fn explicit_level_suffix(id: &str) -> Option<&'static str> {
    match split_level_suffix(&id.trim().to_ascii_lowercase()).1 {
        Some("low") => Some("low"),
        Some("medium") => Some("medium"),
        Some("high") => Some("high"),
        _ => None,
    }
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
/// still return the composed suffix so Desktop effort is not silently dropped
/// to the bare name.
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

/// Retired / internal ids that should not appear in catalog or failover lists.
pub fn is_retired_model(id: &str) -> bool {
    let lower = id.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    // Gemini 2.5 / 3.1 / 3.5 generations no longer work on Cloud Code.
    if lower.starts_with("gemini-2.5-")
        || lower.starts_with("gemini-3.1-")
        || lower.starts_with("gemini-3.5-")
    {
        return true;
    }
    // Router / agent ids (not chat models).
    if lower.ends_with("-agent") {
        return true;
    }
    if lower.contains("flash-lite") {
        return true;
    }
    if lower.starts_with("gemini-") && lower.ends_with("-thinking") {
        return true;
    }
    if lower.starts_with("gpt-oss-") {
        return true;
    }
    if lower.ends_with("-tiered") {
        return true;
    }
    false
}

fn catalog_has_gemini_37(models: &[CatalogModel]) -> bool {
    models
        .iter()
        .any(|model| model.id.to_ascii_lowercase().starts_with("gemini-3.7"))
}

/// When 3.7 is live, hide older Gemini generations from catalog/failover lists.
fn is_superseded_gemini(id: &str, has_37: bool) -> bool {
    if !has_37 {
        return false;
    }
    let lower = id.trim().to_ascii_lowercase();
    lower.starts_with("gemini-") && !lower.starts_with("gemini-3.7")
}

/// Remap legacy Gemini ids that clients may still send after catalog pruning.
pub fn should_remap_legacy_gemini(id: &str) -> bool {
    let lower = id.trim().to_ascii_lowercase();
    if !lower.starts_with("gemini-") || lower.starts_with("gemini-3.7") {
        return false;
    }
    if is_retired_model(&lower) {
        return true;
    }
    list_model_ids()
        .iter()
        .any(|model| model.to_ascii_lowercase().starts_with("gemini-3.7"))
}

/// Drop retired and superseded models from a catalog snapshot.
pub fn prune_catalog_models(mut models: Vec<CatalogModel>) -> Vec<CatalogModel> {
    let has_37 = catalog_has_gemini_37(&models);
    models.retain(|model| {
        let lower = model.id.to_ascii_lowercase();
        is_agent_facing_model(&lower)
            && !is_retired_model(&lower)
            && !is_superseded_gemini(&lower, has_37)
    });
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

/// Keep only ids that are still listable in the live catalog.
pub fn filter_listable_model_ids(ids: &[String]) -> Vec<String> {
    let catalog: std::collections::HashSet<String> = list_model_ids()
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let trimmed = id.trim();
        if trimmed.is_empty() || is_retired_model(trimmed) || should_remap_legacy_gemini(trimmed) {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if !catalog.is_empty() && !catalog.contains(&lower) {
            continue;
        }
        if !out.iter().any(|existing| existing.eq_ignore_ascii_case(trimmed)) {
            out.push(trimmed.to_string());
        }
    }
    out
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
    let incoming: Vec<CatalogModel> = models
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

    let mut guard = lock_write();
    let merged = merge_catalog_snapshots(&guard.models, incoming);
    let pruned = prune_catalog_models(merged);
    if pruned.is_empty() {
        return;
    }
    guard.models = pruned;
    guard.updated_at = chrono::Utc::now().timestamp();
    log::info!(
        "Antigravity model catalog updated: {} agent-facing models",
        guard.models.len()
    );
}

fn catalog_family(id: &str) -> Option<&'static str> {
    let lower = id.trim().to_ascii_lowercase();
    if lower.starts_with("gemini-") {
        Some("gemini")
    } else if lower.starts_with("claude-") {
        Some("claude")
    } else {
        None
    }
}

fn catalog_has_family(models: &[CatalogModel], family: &str) -> bool {
    models
        .iter()
        .any(|model| catalog_family(&model.id) == Some(family))
}

/// Keep previous Gemini/Claude ids when a refresh snapshot omits a whole family
/// (Cloud Code may only list Claude in `models` after Gemini moved to a shared pool).
fn merge_catalog_snapshots(previous: &[CatalogModel], incoming: Vec<CatalogModel>) -> Vec<CatalogModel> {
    let mut by_id = std::collections::BTreeMap::<String, CatalogModel>::new();
    for model in previous {
        by_id.insert(model.id.to_ascii_lowercase(), model.clone());
    }
    for model in incoming {
        by_id.insert(model.id.to_ascii_lowercase(), model);
    }
    let mut merged: Vec<CatalogModel> = by_id.into_values().collect();
    if !catalog_has_family(&merged, "gemini") {
        for fallback in fallback_models() {
            if catalog_family(&fallback.id) == Some("gemini") {
                merged.push(fallback);
            }
        }
    }
    if !catalog_has_family(&merged, "claude") {
        for fallback in fallback_models() {
            if catalog_family(&fallback.id) == Some("claude") {
                merged.push(fallback);
            }
        }
    }
    merged
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
    guard.models = prune_catalog_models(by_id.into_values().collect());
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
    let raw = if guard.models.is_empty() {
        fallback_models()
    } else {
        guard.models.clone()
    };
    prune_catalog_models(raw)
}

pub fn list_model_ids() -> Vec<String> {
    list_catalog_models()
        .into_iter()
        .map(|model| model.id)
        .collect()
}

/// OpenAI-compatible `/v1/models` payload — full upstream catalog, Gemini
/// level variants included so clients can pick the reasoning level directly.
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
        .find(|id| id.as_str() == "claude-sonnet-5")
        .or_else(|| ids.iter().find(|id| id.as_str() == "claude-sonnet-4-6"))
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
        .find(|id| id.as_str() == "gemini-3.7-flash")
        .or_else(|| ids.iter().find(|id| id.as_str() == "gemini-3.7-flash-high"))
        .or_else(|| ids.iter().find(|id| id.starts_with("gemini-3.7-") && is_flash(id)))
        .or_else(|| ids.iter().find(|id| id.as_str() == "gemini-3.6-flash-high"))
        .or_else(|| ids.iter().find(|id| id.as_str() == "gemini-3.6-flash"))
        .or_else(|| {
            ids.iter()
                .find(|id| id.starts_with("gemini-3.6-") && is_flash(id))
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
    let is_pro = |id: &&String| id.contains("pro") && id.starts_with("gemini-3.6-");
    ids.iter()
        .find(|id| id.as_str() == "gemini-3.6-pro-high")
        .or_else(|| ids.iter().find(is_pro))
        .cloned()
        .or_else(preferred_gemini_flash)
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
        if id != preferred_gemini_flash().unwrap_or_default() {
            push(&mut out, id);
        }
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
        assert!(is_agent_facing_model("gemini-3.6-flash"));
        assert!(is_agent_facing_model("claude-sonnet-4-6"));
        assert!(!is_agent_facing_model("chat_20706"));
        assert!(!is_agent_facing_model("tab_flash_lite_preview"));
        assert!(!is_agent_facing_model("gemini-3.1-flash-image"));
    }

    #[test]
    fn filters_retired_models() {
        assert!(is_retired_model("gemini-2.5-flash"));
        assert!(is_retired_model("gemini-2.5-pro"));
        assert!(is_retired_model("gemini-3.1-pro-high"));
        assert!(is_retired_model("gemini-3.5-flash-low"));
        assert!(is_retired_model("gemini-3-flash-agent"));
        assert!(is_retired_model("gemini-3.1-flash-lite"));
        assert!(is_retired_model("gpt-oss-120b-medium"));
        assert!(is_retired_model("gemini-3.6-flash-tiered"));
        assert!(!is_retired_model("gemini-3.6-flash-high"));
    }

    #[test]
    fn prune_drops_retired_and_legacy_gemini() {
        let pruned = prune_catalog_models(vec![
            CatalogModel {
                id: "gemini-3.7-flash".into(),
                display_name: None,
            },
            CatalogModel {
                id: "gemini-3.6-flash-high".into(),
                display_name: None,
            },
            CatalogModel {
                id: "gemini-3-flash".into(),
                display_name: None,
            },
            CatalogModel {
                id: "gemini-3.1-pro-high".into(),
                display_name: None,
            },
            CatalogModel {
                id: "gemini-2.5-flash".into(),
                display_name: None,
            },
            CatalogModel {
                id: "gemini-3.5-flash-low".into(),
                display_name: None,
            },
            CatalogModel {
                id: "claude-sonnet-4-6".into(),
                display_name: None,
            },
        ]);
        let ids: Vec<_> = pruned.iter().map(|model| model.id.as_str()).collect();
        assert!(ids.contains(&"gemini-3.7-flash"));
        assert!(!ids.contains(&"gemini-3.6-flash-high"));
        assert!(ids.contains(&"claude-sonnet-4-6"));
        assert!(!ids.contains(&"gemini-3-flash"));
        assert!(!ids.contains(&"gemini-3.1-pro-high"));
        assert!(!ids.contains(&"gemini-2.5-flash"));
        assert!(!ids.contains(&"gemini-3.5-flash-low"));
    }

    #[test]
    fn with_reasoning_level_passthrough_and_bare_fallback() {
        // Fallback catalog exposes the bare Gemini 3.7 Flash id.
        assert_eq!(with_reasoning_level("gemini-3.6-flash-low"), "gemini-3.6-flash-low");
        assert_eq!(with_reasoning_level("gemini-3.6-flash-high"), "gemini-3.6-flash-high");
        assert_eq!(with_reasoning_level("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(
            with_reasoning_level("gemini-3.7-flash"),
            "gemini-3.7-flash"
        );
        assert_eq!(with_reasoning_level("gemini-9.9-flash"), "gemini-9.9-flash");
    }

    #[test]
    fn catalog_exposes_gemini_37_flash() {
        let ids = list_model_ids();
        assert!(ids.iter().any(|id| id == "gemini-3.7-flash"));
        assert!(!ids.iter().any(|id| id == "gemini-3.6-flash-high"));
        assert!(!ids.iter().any(|id| id == "gemini-3.1-pro-high"));
        assert_eq!(
            preferred_gemini_pro().as_deref(),
            preferred_gemini_flash().as_deref()
        );
    }

    #[test]
    fn with_forced_level_composes_or_falls_back() {
        assert_eq!(
            with_forced_level("gemini-3.6-flash-high", "high"),
            "gemini-3.6-flash-high"
        );
        assert_eq!(
            with_forced_level("gemini-3.6-flash-high", "low"),
            "gemini-3.6-flash-low"
        );
        assert_eq!(with_forced_level("gemini-3.6-flash", "high"), "gemini-3.6-flash-high");
        assert_eq!(with_forced_level("claude-sonnet-4-6", "low"), "claude-sonnet-4-6");
    }

    #[test]
    fn catalog_has_no_synthetic_claude_sonnet_5_alias() {
        let models = list_catalog_models();
        assert!(!models.iter().any(|model| model.id == "claude-sonnet-5"));
        assert_eq!(explicit_level_suffix("gemini-3.6-flash-high"), Some("high"));
        assert_eq!(explicit_level_suffix("gemini-3.6-flash"), None);
    }

    #[test]
    fn merge_keeps_gemini_when_incoming_snapshot_is_claude_only() {
        let previous = vec![CatalogModel {
            id: "gemini-3.7-flash".into(),
            display_name: None,
        }];
        let incoming = vec![CatalogModel {
            id: "claude-sonnet-4-6".into(),
            display_name: Some("Sonnet".into()),
        }];
        let merged = prune_catalog_models(merge_catalog_snapshots(&previous, incoming));
        let ids: Vec<_> = merged.iter().map(|model| model.id.as_str()).collect();
        assert!(ids.contains(&"gemini-3.7-flash"));
        assert!(ids.contains(&"claude-sonnet-4-6"));
    }

    #[test]
    fn merge_adds_fallback_gemini_when_neither_side_has_it() {
        let incoming = vec![CatalogModel {
            id: "claude-sonnet-4-6".into(),
            display_name: None,
        }];
        let merged = prune_catalog_models(merge_catalog_snapshots(&[], incoming));
        assert!(merged.iter().any(|model| model.id.starts_with("gemini-")));
        assert!(merged.iter().any(|model| model.id == "claude-sonnet-4-6"));
    }
}
