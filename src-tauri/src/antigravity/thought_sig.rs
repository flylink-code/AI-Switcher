//! Gemini 3 thought_signature 缓存。
//!
//! Gemini 3.x 要求回放历史中的 functionCall part 携带模型当时生成的
//! thought_signature，缺失会被上游 400 拒绝
//! （"Function call is missing a thought_signature in functionCall parts"）。
//! Claude 客户端看不到、也不会回传该字段，因此网关在响应侧捕获，
//! 按 tool_use_id、(session_key, index) 与同会话三级缓存，请求侧转换历史消息时回注。
//!
//! v1.5.5 起引入独立 L1/L2 混合存储架构：
//! - 运行读仅访问 L1 内存缓存（tool 1024 / session 256 / session_index 2048），热路径零磁盘 I/O 阻塞；
//! - 读取命中带节流 Touch 异步提交，L2 持久缓存（`thought-signatures.db`）滑动 TTL（15天）跨进程重启不丢失；
//! - 后台有界队列异步批写，异常/损坏自动平滑降级纯内存；
//! - 启动时在 init 中有界预热并隔离修复真实损坏文件；
//! - 支持运行时 shutdown，并在应用 setup 阶段提供 `init_early()` 异步提前唤醒预热。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::thought_sig_store::{StoreConfig, ThoughtSigStore};
pub use super::thought_sig_store::{
    DEFAULT_L1_SESSION_CAP, DEFAULT_L1_SESSION_INDEX_CAP, DEFAULT_L1_TOOL_CAP, DEFAULT_L2_CAPACITY,
    DEFAULT_TTL_SECS,
};

/// 无真实签名时的哨兵值：让 Gemini 跳过签名校验（仅 Vertex AI 拒绝该值，
/// 本网关走 Cloud Code 上游，可用；对照参考实现 FIX #2167）。
pub const SKIP_VALIDATOR_SENTINEL: &str = "skip_thought_signature_validator";

static STORE: RwLock<Option<Arc<ThoughtSigStore>>> = RwLock::new(None);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn store() -> Option<Arc<ThoughtSigStore>> {
    if SHUTTING_DOWN.load(Ordering::Acquire) {
        return None;
    }
    if let Ok(guard) = STORE.read() {
        if let Some(s) = guard.as_ref() {
            return Some(Arc::clone(s));
        }
    }
    let mut guard = match STORE.write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if SHUTTING_DOWN.load(Ordering::Acquire) {
        return None;
    }
    if let Some(s) = guard.as_ref() {
        return Some(Arc::clone(s));
    }
    let new_store = Arc::new(ThoughtSigStore::init(StoreConfig::default()));
    *guard = Some(Arc::clone(&new_store));
    Some(new_store)
}

/// 在应用 setup 阶段异步提前初始化存储与预热，避免首次请求时发生冷启动。
pub fn init_early() {
    let _ = store();
}

/// 正常退出、服务停止或路径迁移时的尽力刷盘。
pub fn flush() {
    if let Ok(guard) = STORE.read() {
        if let Some(s) = guard.as_ref() {
            s.flush();
        }
    }
}

/// 兼容别名。
pub fn flush_thought_signatures() {
    flush();
}

/// 关闭持久化并安全释放数据库句柄（在退出时调用）。
pub fn shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Release);
    let mut guard = match STORE.write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(s) = guard.take() {
        s.shutdown();
    }
}

pub fn cache_tool_signature(tool_use_id: &str, signature: &str) {
    if let Some(store) = store() {
        store.cache_tool(tool_use_id, signature);
    }
}

pub fn get_tool_signature(tool_use_id: &str) -> Option<String> {
    store()?.get_tool(tool_use_id)
}

pub fn cache_session_signature(session_key: &str, signature: &str) {
    if let Some(store) = store() {
        store.cache_session(session_key, signature);
    }
}

pub fn get_session_signature(session_key: &str) -> Option<String> {
    store()?.get_session(session_key)
}

pub fn cache_session_index_signature(session_key: &str, index: usize, signature: &str) {
    if let Some(store) = store() {
        store.cache_session_index(session_key, index, signature);
    }
}

pub fn get_session_index_signature(session_key: &str, index: usize) -> Option<String> {
    store()?.get_session_index(session_key, index)
}

pub fn resolve_function_call_signature(
    tool_use_id: Option<&str>,
    session_key: Option<&str>,
    message_index: Option<usize>,
) -> String {
    store()
        .map(|store| store.resolve(tool_use_id, session_key, message_index))
        .unwrap_or_else(|| SKIP_VALIDATOR_SENTINEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_and_session_cache_roundtrip() {
        assert_eq!(get_tool_signature("toolu_mod_rt"), None);
        cache_tool_signature("toolu_mod_rt", "sig-abc");
        assert_eq!(
            get_tool_signature("toolu_mod_rt").as_deref(),
            Some("sig-abc")
        );

        assert_eq!(get_session_signature("sess_mod_rt"), None);
        cache_session_signature("sess_mod_rt", "short");
        cache_session_signature("sess_mod_rt", "longer-signature");
        assert_eq!(
            get_session_signature("sess_mod_rt").as_deref(),
            Some("longer-signature")
        );
        // 更短的签名不覆盖已有值。
        cache_session_signature("sess_mod_rt", "tiny");
        assert_eq!(
            get_session_signature("sess_mod_rt").as_deref(),
            Some("longer-signature")
        );
    }

    #[test]
    fn sentinel_and_empty_are_not_cached() {
        cache_tool_signature("toolu_mod_sentinel", SKIP_VALIDATOR_SENTINEL);
        cache_tool_signature("toolu_mod_sentinel", "  ");
        cache_session_signature("sess_mod_sentinel", SKIP_VALIDATOR_SENTINEL);
        assert_eq!(get_tool_signature("toolu_mod_sentinel"), None);
        assert_eq!(get_session_signature("sess_mod_sentinel"), None);
    }

    #[test]
    fn session_index_roundtrip_is_independent_of_latest() {
        cache_session_signature("sess_idx_rt", "latest-sig");
        cache_session_index_signature("sess_idx_rt", 2, "turn-2-sig");
        assert_eq!(
            get_session_index_signature("sess_idx_rt", 2).as_deref(),
            Some("turn-2-sig")
        );
        assert_eq!(
            resolve_function_call_signature(None, Some("sess_idx_rt"), Some(2)).as_str(),
            "turn-2-sig"
        );
        assert_eq!(
            resolve_function_call_signature(None, Some("sess_idx_rt"), Some(9)).as_str(),
            "latest-sig"
        );
    }
}
