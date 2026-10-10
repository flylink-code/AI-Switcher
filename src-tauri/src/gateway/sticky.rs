//! 记录目录模型 pin，以及显式启用时的父会话上游亲和。
//!
//! 模型 pin 与上游亲和是两张独立的表：Auto 清理模型 pin 不会破坏
//! 已成功请求的上游记录。亲和键包含鉴权域、Agent、profile 和 session。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::catalog::{is_explicit_catalog_passthrough, is_sticky_remap_role_id, CatalogEntry};
use crate::gateway::is_auto_model_id;
use crate::provider::ProviderTarget;

pub const FALLBACK_SESSION_KEY: &str = "-";
pub const PARENT_SESSION_HEADER: &str = "x-cs-parent-session-id";

const MAX_STICKY_ENTRIES: usize = 256;
const STICKY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

type StickyKey = (ProviderTarget, String);
type AffinityKey = (String, ProviderTarget, String, String);

struct StickySlot {
    model: String,
    last_used: Instant,
}

struct AffinitySlot {
    upstream_id: String,
    last_used: Instant,
}

fn table() -> &'static Mutex<HashMap<StickyKey, StickySlot>> {
    static TABLE: OnceLock<Mutex<HashMap<StickyKey, StickySlot>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn affinity_table() -> &'static Mutex<HashMap<AffinityKey, AffinitySlot>> {
    static TABLE: OnceLock<Mutex<HashMap<AffinityKey, AffinitySlot>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_table() -> std::sync::MutexGuard<'static, HashMap<StickyKey, StickySlot>> {
    match table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_affinity_table() -> std::sync::MutexGuard<'static, HashMap<AffinityKey, AffinitySlot>> {
    match affinity_table().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn normalize_session_key(session_key: &str) -> String {
    let trimmed = session_key.trim();
    if trimmed.is_empty() { FALLBACK_SESSION_KEY.to_string() } else { trimmed.to_string() }
}

/// 优先使用 Claude Code `metadata.user_id`，其次使用已知会话头。
pub fn session_key_from(body: &Value, header_hint: Option<&str>) -> String {
    if let Some(user_id) = body.get("metadata")
        .and_then(|metadata| metadata.get("user_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return user_id.to_string();
    }
    header_hint.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| FALLBACK_SESSION_KEY.to_string())
}

/// 只接受明确的本地协议头；不会从 user_id 后缀或最近请求猜测父会话。
pub fn parent_session_key_from(headers: &HeaderMap) -> Option<String> {
    headers.get(PARENT_SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != FALLBACK_SESSION_KEY)
        .map(str::to_string)
}

/// 仅保存不可逆 fingerprint，调用方不得把原始 token 写入诊断。
pub fn auth_domain_fingerprint(presented_token: &str) -> String {
    hex::encode(Sha256::digest(presented_token.trim().as_bytes()))
}

fn evict_expired(map: &mut HashMap<StickyKey, StickySlot>) {
    map.retain(|_, slot| slot.last_used.elapsed() < STICKY_TTL);
}

fn evict_overflow(map: &mut HashMap<StickyKey, StickySlot>) {
    while map.len() >= MAX_STICKY_ENTRIES {
        let oldest = map.iter().min_by_key(|(_, slot)| slot.last_used).map(|(key, _)| key.clone());
        if let Some(key) = oldest { map.remove(&key); } else { break; }
    }
}

fn evict_affinity_expired(map: &mut HashMap<AffinityKey, AffinitySlot>) {
    map.retain(|_, slot| slot.last_used.elapsed() < STICKY_TTL);
}

fn evict_affinity_overflow(map: &mut HashMap<AffinityKey, AffinitySlot>) {
    while map.len() >= MAX_STICKY_ENTRIES {
        let oldest = map.iter().min_by_key(|(_, slot)| slot.last_used).map(|(key, _)| key.clone());
        if let Some(key) = oldest { map.remove(&key); } else { break; }
    }
}

fn remember(target: ProviderTarget, session_key: &str, model: &str) {
    let model = model.trim();
    if model.is_empty() { return; }
    let mut map = lock_table();
    evict_expired(&mut map);
    evict_overflow(&mut map);
    map.insert((target, normalize_session_key(session_key)), StickySlot {
        model: model.to_string(), last_used: Instant::now(),
    });
}

fn clear(target: ProviderTarget, session_key: &str) {
    lock_table().remove(&(target, normalize_session_key(session_key)));
}

pub fn clear_for_target(target: ProviderTarget) {
    lock_table().retain(|(slot_target, _), _| *slot_target != target);
    lock_affinity_table().retain(|(_, slot_target, _, _), _| *slot_target != target);
}

/// 记录已经完成实际出站的最终上游；缺少明确 session 时不产生共享记录。
pub fn remember_successful_upstream(
    auth_domain: &str,
    target: ProviderTarget,
    profile_id: &str,
    session_key: &str,
    upstream_id: &str,
) {
    let session_key = normalize_session_key(session_key);
    if session_key == FALLBACK_SESSION_KEY || profile_id.trim().is_empty() || upstream_id.trim().is_empty() { return; }
    let key = (auth_domain_fingerprint(auth_domain), target, profile_id.trim().to_string(), session_key);
    let mut map = lock_affinity_table();
    evict_affinity_expired(&mut map);
    evict_affinity_overflow(&mut map);
    map.insert(key, AffinitySlot { upstream_id: upstream_id.trim().to_string(), last_used: Instant::now() });
}

pub fn inherited_upstream(
    auth_domain: &str,
    target: ProviderTarget,
    profile_id: &str,
    parent_session_key: Option<&str>,
) -> Option<String> {
    let session_key = normalize_session_key(parent_session_key?);
    if session_key == FALLBACK_SESSION_KEY || profile_id.trim().is_empty() { return None; }
    let key = (auth_domain_fingerprint(auth_domain), target, profile_id.trim().to_string(), session_key);
    let mut map = lock_affinity_table();
    let Some(slot) = map.get_mut(&key) else { return None; };
    if slot.last_used.elapsed() >= STICKY_TTL {
        map.remove(&key);
        return None;
    }
    slot.last_used = Instant::now();
    Some(slot.upstream_id.clone())
}

fn get(target: ProviderTarget, session_key: &str) -> Option<String> {
    let key = (target, normalize_session_key(session_key));
    let mut map = lock_table();
    if map.get(&key).is_some_and(|slot| slot.last_used.elapsed() >= STICKY_TTL) {
        map.remove(&key);
        return None;
    }
    map.get_mut(&key).map(|slot| { slot.last_used = Instant::now(); slot.model.clone() })
}

/// Live path only: pin official Sonnet/Opus/Fable roles to the last catalog pick.
pub fn rewrite_requested(target: ProviderTarget, requested: &str, entries: &[CatalogEntry], session_key: &str) -> String {
    let requested = requested.trim();
    if is_auto_model_id(requested) { clear(target, session_key); return requested.to_string(); }
    if is_explicit_catalog_passthrough(entries, requested) { remember(target, session_key, requested); return requested.to_string(); }
    if is_sticky_remap_role_id(requested) {
        if let Some(sticky) = get(target, session_key) {
            return sticky;
        }
    }
    requested.to_string()
}

#[cfg(test)]
pub fn reset_for_tests() { lock_table().clear(); lock_affinity_table().clear(); }

#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() { Ok(guard) => guard, Err(poisoned) => poisoned.into_inner() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogEntry;

    fn entry(public_id: &str, provider_id: &str) -> CatalogEntry {
        CatalogEntry { public_id: public_id.into(), display_name: public_id.into(), upstream_slug: "gpt-6-astra".into(), provider_id: provider_id.into(), context_window: 200_000, anthropic_upstream: false, web_search_enabled: false }
    }

    #[test]
    fn sticky_pins_sonnet_role_to_last_explicit_catalog_id() {
        let _guard = test_lock(); reset_for_tests();
        let entries = vec![entry("claude.sub2api.gpt-6-astra", "sub2api")];
        let target = ProviderTarget::ClaudeCode;
        assert_eq!(rewrite_requested(target, "claude.sub2api.gpt-6-astra", &entries, "sess-a"), "claude.sub2api.gpt-6-astra");
        assert_eq!(rewrite_requested(target, "claude-sonnet-5", &entries, "sess-a"), "claude.sub2api.gpt-6-astra");
        assert_eq!(rewrite_requested(target, "claude-sonnet-5[1m]", &entries, "sess-a"), "claude.sub2api.gpt-6-astra");
        assert_eq!(rewrite_requested(target, "claude.auto", &entries, "sess-a"), "claude.auto");
        assert_eq!(rewrite_requested(target, "claude-sonnet-5", &entries, "sess-a"), "claude-sonnet-5");
    }

    #[test]
    fn auto_on_other_session_does_not_clear_pin() {
        let _guard = test_lock(); reset_for_tests();
        let entries = vec![entry("claude.sub2api.gpt-6-astra", "sub2api")];
        let target = ProviderTarget::ClaudeCode;
        assert_eq!(rewrite_requested(target, "claude.sub2api.gpt-6-astra", &entries, "sess-b"), "claude.sub2api.gpt-6-astra");
        assert_eq!(rewrite_requested(target, "claude.auto", &entries, "sess-a"), "claude.auto");
        assert_eq!(rewrite_requested(target, "claude-sonnet-5", &entries, "sess-b"), "claude.sub2api.gpt-6-astra");
        assert_eq!(rewrite_requested(target, "claude-sonnet-5", &entries, "sess-a"), "claude-sonnet-5");
    }

    #[test]
    fn session_key_prefers_metadata_user_id() {
        let body = serde_json::json!({ "metadata": { "user_id": "user_session_abc" } });
        assert_eq!(session_key_from(&body, Some("header-session")), "user_session_abc");
        assert_eq!(session_key_from(&serde_json::json!({}), Some("header-session")), "header-session");
        assert_eq!(session_key_from(&serde_json::json!({}), None), FALLBACK_SESSION_KEY);
    }

    #[test]
    fn parent_affinity_is_scoped_and_ignores_missing_session() {
        let _guard = test_lock(); reset_for_tests();
        let target = ProviderTarget::ClaudeCode;
        remember_successful_upstream("secret-a", target, "profile-a", "parent", "up-a");
        assert_eq!(inherited_upstream("secret-a", target, "profile-a", Some("parent")), Some("up-a".into()));
        assert_eq!(inherited_upstream("secret-b", target, "profile-a", Some("parent")), None);
        assert_eq!(inherited_upstream("secret-a", target, "profile-b", Some("parent")), None);
        assert_eq!(inherited_upstream("secret-a", target, "profile-a", None), None);
        remember_successful_upstream("secret-a", target, "profile-a", FALLBACK_SESSION_KEY, "up-nope");
        assert_eq!(inherited_upstream("secret-a", target, "profile-a", Some(FALLBACK_SESSION_KEY)), None);
        rewrite_requested(target, "claude.auto", &[], "parent");
        assert_eq!(inherited_upstream("secret-a", target, "profile-a", Some("parent")), Some("up-a".into()));
    }

    #[test]
    fn parent_header_is_not_forwarded_as_a_shared_session() {
        let headers = HeaderMap::new();
        assert_eq!(parent_session_key_from(&headers), None);
        let mut headers = HeaderMap::new();
        headers.insert(PARENT_SESSION_HEADER, "-".parse().unwrap());
        assert_eq!(parent_session_key_from(&headers), None);
        headers.insert(PARENT_SESSION_HEADER, "parent-session".parse().unwrap());
        assert_eq!(parent_session_key_from(&headers).as_deref(), Some("parent-session"));
    }
}
