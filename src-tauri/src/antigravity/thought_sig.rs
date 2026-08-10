//! Gemini 3 thought_signature 缓存。
//!
//! Gemini 3.x 要求回放历史中的 functionCall part 携带模型当时生成的
//! thought_signature，缺失会被上游 400 拒绝
//! （"Function call is missing a thought_signature in functionCall parts"）。
//! Claude 客户端看不到、也不会回传该字段，因此网关在响应侧捕获，
//! 按 tool_use_id 与会话两级缓存，请求侧转换历史消息时回注
//! （对照 Antigravity-Manager proxy/signature_cache.rs）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 无真实签名时的哨兵值：让 Gemini 跳过签名校验（仅 Vertex AI 拒绝该值，
/// 本网关走 Cloud Code 上游，可用；对照参考实现 FIX #2167）。
pub const SKIP_VALIDATOR_SENTINEL: &str = "skip_thought_signature_validator";

const MAX_TOOL_SIGS: usize = 1024;
const MAX_SESSION_SIGS: usize = 256;

#[derive(Default)]
struct Cache {
    /// tool_use_id → signature（最精确，随客户端回放的 tool_use 块命中）。
    tool: HashMap<String, String>,
    /// session_key → 最近一次签名（同会话兜底）。
    session: HashMap<String, String>,
}

fn store() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

fn lock() -> std::sync::MutexGuard<'static, Cache> {
    match store().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn usable(signature: &str) -> bool {
    !signature.trim().is_empty() && signature != SKIP_VALIDATOR_SENTINEL
}

pub fn cache_tool_signature(tool_use_id: &str, signature: &str) {
    let id = tool_use_id.trim();
    if id.is_empty() || !usable(signature) {
        return;
    }
    let mut guard = lock();
    if guard.tool.len() >= MAX_TOOL_SIGS && !guard.tool.contains_key(id) {
        if let Some(evict) = guard.tool.keys().next().cloned() {
            guard.tool.remove(&evict);
        }
    }
    guard.tool.insert(id.to_string(), signature.to_string());
}

pub fn get_tool_signature(tool_use_id: &str) -> Option<String> {
    let id = tool_use_id.trim();
    if id.is_empty() {
        return None;
    }
    lock().tool.get(id).cloned()
}

pub fn cache_session_signature(session_key: &str, signature: &str) {
    let key = session_key.trim();
    if key.is_empty() || !usable(signature) {
        return;
    }
    let mut guard = lock();
    // 只接受不短于已存值的新签名，避免流式截断的短签名覆盖完整签名。
    if let Some(existing) = guard.session.get(key) {
        if signature.len() < existing.len() {
            return;
        }
    }
    if guard.session.len() >= MAX_SESSION_SIGS && !guard.session.contains_key(key) {
        if let Some(evict) = guard.session.keys().next().cloned() {
            guard.session.remove(&evict);
        }
    }
    guard.session.insert(key.to_string(), signature.to_string());
}

pub fn get_session_signature(session_key: &str) -> Option<String> {
    let key = session_key.trim();
    if key.is_empty() {
        return None;
    }
    lock().session.get(key).cloned()
}

#[cfg(test)]
pub fn clear_all() {
    let mut guard = lock();
    guard.tool.clear();
    guard.session.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_and_session_cache_roundtrip() {
        clear_all();
        assert_eq!(get_tool_signature("toolu_1"), None);
        cache_tool_signature("toolu_1", "sig-abc");
        assert_eq!(get_tool_signature("toolu_1").as_deref(), Some("sig-abc"));

        assert_eq!(get_session_signature("sess-1"), None);
        cache_session_signature("sess-1", "short");
        cache_session_signature("sess-1", "longer-signature");
        assert_eq!(
            get_session_signature("sess-1").as_deref(),
            Some("longer-signature")
        );
        // 更短的签名不覆盖已有值。
        cache_session_signature("sess-1", "tiny");
        assert_eq!(
            get_session_signature("sess-1").as_deref(),
            Some("longer-signature")
        );
        clear_all();
    }

    #[test]
    fn sentinel_and_empty_are_not_cached() {
        clear_all();
        cache_tool_signature("toolu_2", SKIP_VALIDATOR_SENTINEL);
        cache_tool_signature("toolu_2", "  ");
        cache_session_signature("sess-2", SKIP_VALIDATOR_SENTINEL);
        assert_eq!(get_tool_signature("toolu_2"), None);
        assert_eq!(get_session_signature("sess-2"), None);
        clear_all();
    }
}
