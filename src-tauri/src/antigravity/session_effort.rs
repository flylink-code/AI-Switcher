//! Per-session sticky Gemini reasoning effort for Claude Desktop.
//!
//! Desktop does not send `effort` on every request (side/auxiliary turns often
//! omit it). Remember the last explicit level keyed by `x-session-id` /
//! `x-claude-session-id` so bare Gemini names stay on the user's chosen tier.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const MAX_SESSIONS: usize = 256;

fn store() -> &'static Mutex<HashMap<String, &'static str>> {
    static SESSION_EFFORT: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    SESSION_EFFORT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, &'static str>> {
    match store().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn get(session_key: &str) -> Option<&'static str> {
    let key = session_key.trim();
    if key.is_empty() {
        return None;
    }
    lock().get(key).copied()
}

pub fn set(session_key: &str, level: &'static str) {
    let key = session_key.trim();
    if key.is_empty() {
        return;
    }
    let mut guard = lock();
    if guard.len() >= MAX_SESSIONS && !guard.contains_key(key) {
        // Drop an arbitrary entry to bound memory; sessions are short-lived.
        if let Some(evict) = guard.keys().next().cloned() {
            guard.remove(&evict);
        }
    }
    guard.insert(key.to_string(), level);
}

#[cfg(test)]
pub fn clear_all() {
    lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_returns_effort() {
        clear_all();
        assert_eq!(get("sess-a"), None);
        set("sess-a", "high");
        assert_eq!(get("sess-a"), Some("high"));
        set("sess-a", "low");
        assert_eq!(get("sess-a"), Some("low"));
        clear_all();
    }
}
