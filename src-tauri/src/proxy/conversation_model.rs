//! Model the user picked in the current Claude conversation.
//!
//! Claude Code sends the full catalog id on the main turn, and the stable role
//! id (`claude-sonnet-5`, haiku, …) on subagent turns. Remember the full id so
//! role turns can follow it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use axum::http::HeaderMap;

use crate::provider::ProviderTarget;

const MAX_SESSIONS: usize = 256;

struct Store {
    sessions: HashMap<String, String>,
    last_by_target: HashMap<ProviderTarget, String>,
    last: Option<String>,
}

fn store() -> &'static Mutex<Store> {
    static CONVERSATION_MODEL: OnceLock<Mutex<Store>> = OnceLock::new();
    CONVERSATION_MODEL.get_or_init(|| {
        Mutex::new(Store {
            sessions: HashMap::new(),
            last_by_target: HashMap::new(),
            last: None,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Store> {
    match store().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn session_key(headers: &HeaderMap) -> Option<String> {
    for name in [
        "x-claude-session-id",
        "x-session-id",
        "x-anthropic-session-id",
        "x-conversation-id",
        "openai-conversation-id",
        "x-vscode-session-id",
    ] {
        let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) else {
            continue;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn remember(session_key: Option<&str>, model: &str) {
    remember_for_target(ProviderTarget::ClaudeCode, session_key, model);
}

pub fn remember_for_target(target: ProviderTarget, session_key: Option<&str>, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    let mut guard = lock();
    if let Some(key) = session_key.map(str::trim).filter(|key| !key.is_empty()) {
        let scoped = format!("{}:{key}", target.as_str());
        if guard.sessions.len() >= MAX_SESSIONS && !guard.sessions.contains_key(&scoped) {
            if let Some(evict) = guard.sessions.keys().next().cloned() {
                guard.sessions.remove(&evict);
            }
        }
        guard.sessions.insert(scoped, model.to_string());
    }
    guard.last_by_target.insert(target, model.to_string());
    guard.last = Some(model.to_string());
}

/// Known sessions stay isolated; headerless requests use this target's latest model.
pub fn recall(session_key: Option<&str>) -> Option<String> {
    recall_for_target(ProviderTarget::ClaudeCode, session_key)
}

pub fn recall_for_target(target: ProviderTarget, session_key: Option<&str>) -> Option<String> {
    let guard = lock();
    if let Some(key) = session_key.map(str::trim).filter(|key| !key.is_empty()) {
        let scoped = format!("{}:{key}", target.as_str());
        return guard.sessions.get(&scoped).cloned();
    }
    guard.last_by_target.get(&target).cloned()
}

#[cfg(test)]
pub fn clear_all() {
    let mut guard = lock();
    guard.sessions.clear();
    guard.last_by_target.clear();
    guard.last = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_model_wins_over_the_latest_other_conversation() {
        clear_all();
        remember(Some("conv-a"), "claude-sonnet-5-5-medium");
        remember(Some("conv-b"), "gemini-3.8-flash-high");
        assert_eq!(
            recall(Some("conv-a")).as_deref(),
            Some("claude-sonnet-5-5-medium")
        );
        assert_eq!(recall(Some("conv-new")), None);
        assert_eq!(
            recall(None).as_deref(),
            Some("gemini-3.8-flash-high")
        );
        clear_all();
    }

    #[test]
    fn targets_do_not_leak_remembered_models_across_agents() {
        clear_all();
        remember_for_target(ProviderTarget::VscodeCopilot, Some("s1"), "gpt-5.6-sol");
        assert_eq!(
            recall_for_target(ProviderTarget::ClaudeCode, None),
            None,
            "Claude Code must not inherit VS Code Copilot's last model"
        );
        assert_eq!(
            recall_for_target(ProviderTarget::ClaudeCode, Some("s1")),
            None,
            "Claude Code must not inherit VS Code Copilot's session model"
        );
        clear_all();
    }
}
