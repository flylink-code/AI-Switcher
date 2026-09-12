//! Remember the last explicit catalog model so Claude Code role ids stay pinned.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::catalog::{is_explicit_catalog_passthrough, is_sticky_remap_role_id, CatalogEntry};
use crate::gateway::is_auto_model_id;
use crate::provider::ProviderTarget;

fn table() -> &'static Mutex<HashMap<ProviderTarget, String>> {
    static TABLE: OnceLock<Mutex<HashMap<ProviderTarget, String>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_table() -> std::sync::MutexGuard<'static, HashMap<ProviderTarget, String>> {
    match table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn remember(target: ProviderTarget, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    lock_table().insert(target, model.to_string());
}

fn clear(target: ProviderTarget) {
    lock_table().remove(&target);
}

fn get(target: ProviderTarget) -> Option<String> {
    lock_table().get(&target).cloned()
}

/// Live path only: pin official Sonnet/Opus/Fable roles to the last catalog pick.
pub fn rewrite_requested(
    target: ProviderTarget,
    requested: &str,
    entries: &[CatalogEntry],
) -> String {
    let requested = requested.trim();
    if is_auto_model_id(requested) {
        clear(target);
        return requested.to_string();
    }
    if is_explicit_catalog_passthrough(entries, requested) {
        remember(target, requested);
        return requested.to_string();
    }
    if is_sticky_remap_role_id(requested) {
        if let Some(sticky) = get(target) {
            return sticky;
        }
    }
    requested.to_string()
}

#[cfg(test)]
pub fn reset_for_tests() {
    lock_table().clear();
}

#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogEntry;

    fn entry(public_id: &str, provider_id: &str) -> CatalogEntry {
        CatalogEntry {
            public_id: public_id.into(),
            display_name: public_id.into(),
            upstream_slug: "gpt-6-astra".into(),
            provider_id: provider_id.into(),
            context_window: 200_000,
            anthropic_upstream: false,
            web_search_enabled: false,
        }
    }

    #[test]
    fn sticky_pins_sonnet_role_to_last_explicit_catalog_id() {
        let _guard = test_lock();
        reset_for_tests();
        let entries = vec![entry("claude.sub2api.gpt-6-astra", "sub2api")];
        let target = ProviderTarget::ClaudeCode;
        assert_eq!(
            rewrite_requested(target, "claude.sub2api.gpt-6-astra", &entries),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5[1m]", &entries),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(rewrite_requested(target, "claude.auto", &entries), "claude.auto");
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries),
            "claude-sonnet-5"
        );
    }
}
