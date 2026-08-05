//! Usage dashboard refresh notifications.
//!
//! Database writers do not hold an `AppHandle`, so this module provides a
//! coalesced, best-effort notification after request-log changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

pub const USAGE_LOG_RECORDED_EVENT: &str = "usage-log-recorded";

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static EMIT_SCHEDULED: AtomicBool = AtomicBool::new(false);

pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::debug!("usage event emitter already initialized");
    }
}

/// Notify the frontend that request-log data changed.
///
/// The caller never waits for this notification. Repeated writes within the
/// debounce window share one async task and one frontend event.
pub fn notify_log_recorded() {
    let Some(handle) = APP_HANDLE.get().cloned() else {
        return;
    };
    if EMIT_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DEBOUNCE_WINDOW).await;
        EMIT_SCHEDULED.store(false, Ordering::Release);
        if let Err(error) = handle.emit(USAGE_LOG_RECORDED_EVENT, ()) {
            log::warn!("emit {USAGE_LOG_RECORDED_EVENT} failed: {error}");
        }
    });
}
