//! Remember the last explicit catalog model so Claude Code role ids stay pinned.
//!
//! Pins are keyed by `(target, session)` so one Auto window cannot wipe another
//! conversation's catalog pick. Auto only clears the current session.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::catalog::{is_explicit_catalog_passthrough, is_sticky_remap_role_id, CatalogEntry};
use crate::gateway::is_auto_model_id;
use crate::provider::ProviderTarget;

pub const FALLBACK_SESSION_KEY: &str = "-";

const MAX_STICKY_ENTRIES: usize = 256;
const STICKY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

type StickyKey = (ProviderTarget, String);

struct StickySlot {
    model: String,
    last_used: Instant,
}

fn table() -> &'static Mutex<HashMap<StickyKey, StickySlot>> {
    static TABLE: OnceLock<Mutex<HashMap<StickyKey, StickySlot>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_table() -> std::sync::MutexGuard<'static, HashMap<StickyKey, StickySlot>> {
    match table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn normalize_session_key(session_key: &str) -> String {
    let trimmed = session_key.trim();
    if trimmed.is_empty() {
        FALLBACK_SESSION_KEY.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Prefer Claude Code `metadata.user_id` (`…_session_<uuid>`), then session headers.
pub fn session_key_from(body: &Value, header_hint: Option<&str>) -> String {
    if let Some(user_id) = body
        .get("metadata")
        .and_then(|metadata| metadata.get("user_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return user_id.to_string();
    }
    if let Some(hint) = header_hint.map(str::trim).filter(|value| !value.is_empty()) {
        return hint.to_string();
    }
    FALLBACK_SESSION_KEY.to_string()
}

fn evict_expired(map: &mut HashMap<StickyKey, StickySlot>) {
    map.retain(|_, slot| slot.last_used.elapsed() < STICKY_TTL);
}

fn evict_overflow(map: &mut HashMap<StickyKey, StickySlot>) {
    while map.len() >= MAX_STICKY_ENTRIES {
        let oldest = map
            .iter()
            .min_by_key(|(_, slot)| slot.last_used)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            map.remove(&key);
        } else {
            break;
        }
    }
}

fn remember(target: ProviderTarget, session_key: &str, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    let key = (target, normalize_session_key(session_key));
    let mut map = lock_table();
    evict_expired(&mut map);
    evict_overflow(&mut map);
    map.insert(
        key,
        StickySlot {
            model: model.to_string(),
            last_used: Instant::now(),
        },
    );
}

fn clear(target: ProviderTarget, session_key: &str) {
    lock_table().remove(&(target, normalize_session_key(session_key)));
}

pub fn clear_for_target(target: ProviderTarget) {
    lock_table().retain(|(slot_target, _), _| *slot_target != target);
}

fn get(target: ProviderTarget, session_key: &str) -> Option<String> {
    let key = (target, normalize_session_key(session_key));
    let mut map = lock_table();
    let expired = map
        .get(&key)
        .is_some_and(|slot| slot.last_used.elapsed() >= STICKY_TTL);
    if expired {
        map.remove(&key);
        return None;
    }
    if let Some(slot) = map.get_mut(&key) {
        slot.last_used = Instant::now();
        return Some(slot.model.clone());
    }
    None
}

/// Live path only: pin official Sonnet/Opus/Fable roles to the last catalog pick.
pub fn rewrite_requested(
    target: ProviderTarget,
    requested: &str,
    entries: &[CatalogEntry],
    session_key: &str,
) -> String {
    let requested = requested.trim();
    if is_auto_model_id(requested) {
        clear(target, session_key);
        return requested.to_string();
    }
    if is_explicit_catalog_passthrough(entries, requested) {
        remember(target, session_key, requested);
        return requested.to_string();
    }
    if is_sticky_remap_role_id(requested) {
        if let Some(sticky) = get(target, session_key) {
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
        let session = "sess-a";
        assert_eq!(
            rewrite_requested(target, "claude.sub2api.gpt-6-astra", &entries, session),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries, session),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5[1m]", &entries, session),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude.auto", &entries, session),
            "claude.auto"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries, session),
            "claude-sonnet-5"
        );
    }

    #[test]
    fn auto_on_other_session_does_not_clear_pin() {
        let _guard = test_lock();
        reset_for_tests();
        let entries = vec![entry("claude.sub2api.gpt-6-astra", "sub2api")];
        let target = ProviderTarget::ClaudeCode;
        assert_eq!(
            rewrite_requested(target, "claude.sub2api.gpt-6-astra", &entries, "sess-b"),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude.auto", &entries, "sess-a"),
            "claude.auto"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries, "sess-b"),
            "claude.sub2api.gpt-6-astra"
        );
        assert_eq!(
            rewrite_requested(target, "claude-sonnet-5", &entries, "sess-a"),
            "claude-sonnet-5"
        );
    }

    #[test]
    fn session_key_prefers_metadata_user_id() {
        let body = serde_json::json!({
            "metadata": { "user_id": "user_session_abc" }
        });
        assert_eq!(
            session_key_from(&body, Some("header-session")),
            "user_session_abc"
        );
        assert_eq!(
            session_key_from(&serde_json::json!({}), Some("header-session")),
            "header-session"
        );
        assert_eq!(session_key_from(&serde_json::json!({}), None), FALLBACK_SESSION_KEY);
    }
}
