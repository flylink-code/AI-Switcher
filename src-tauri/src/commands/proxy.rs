//! Commands for controlling and inspecting the local proxy.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::database::dao::providers::get_current_provider;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::AppResult;
use crate::proxy::{ProxyLifecycleEvent, ProxyStatus};
use crate::store::AppState;

const DEFAULT_PORT: u16 = 15821;

/// Emitted after post-update relaunch recovery finishes (proxy bind + Codex endpoint repair).
pub const RUNTIME_RECOVERED_EVENT: &str = "runtime-recovered";

pub type ProxyStatusInfo = ProxyStatus;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStatusUpdated {
    target: crate::provider::ProviderTarget,
    status: ProxyStatus,
}

#[tauri::command]
pub async fn get_proxy_status(target: Option<crate::provider::ProviderTarget>, state: tauri::State<'_, AppState>) -> AppResult<ProxyStatusInfo> {
    let started = std::time::Instant::now();
    let target = target.unwrap_or(crate::provider::ProviderTarget::ClaudeDesktop);
    let status = status_snapshot(&state, target).await;
    log::info!(
        "代理状态快照读取完成: target={target:?}, duration_us={}",
        started.elapsed().as_micros()
    );
    Ok(status)
}

#[tauri::command]
pub async fn start_proxy(
    port: Option<u16>,
    target: Option<crate::provider::ProviderTarget>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProxyStatusInfo> {
    let target = target.unwrap_or(crate::provider::ProviderTarget::ClaudeDesktop);
    let target_port = port.unwrap_or_else(|| get_saved_port(&state, target));
    publish_status(
        &app,
        &state,
        target,
        status_value(&state, target, target_port, "starting", None),
    )
    .await;
    let mut proxy = state.proxy.lock().await;
    match proxy.start(target_port, target).await {
        Ok(()) => {
            persist_port(&state, target, target_port)?;
            let status = proxy.status_for(target);
            drop(proxy);
            publish_status(&app, &state, target, status.clone()).await;
            Ok(status)
        }
        Err(error) => {
            drop(proxy);
            let status = status_value(
                &state,
                target,
                target_port,
                "error",
                Some(sanitize_status_error(&error.to_string())),
            );
            publish_status(&app, &state, target, status).await;
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn stop_proxy(
    target: Option<crate::provider::ProviderTarget>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProxyStatusInfo> {
    let mut proxy = state.proxy.lock().await;
    match target {
        Some(target) => {
            proxy.stop_target(target);
            drop(proxy);
            let status = status_value(&state, target, get_saved_port(&state, target), "stopped", None);
            publish_status(&app, &state, target, status.clone()).await;
            Ok(status)
        }
        None => {
            proxy.stop();
            drop(proxy);
            for target in [
                crate::provider::ProviderTarget::ClaudeCode,
                crate::provider::ProviderTarget::ClaudeDesktop,
                crate::provider::ProviderTarget::Codex,
            ] {
                let status =
                    status_value(&state, target, get_saved_port(&state, target), "stopped", None);
                publish_status(&app, &state, target, status).await;
            }
            Ok(status_snapshot(&state, crate::provider::ProviderTarget::ClaudeDesktop).await)
        }
    }
}

#[tauri::command]
pub fn set_proxy_port(port: u16, target: Option<crate::provider::ProviderTarget>, state: tauri::State<'_, AppState>) -> AppResult<()> {
    persist_port(&state, target.unwrap_or(crate::provider::ProviderTarget::ClaudeDesktop), port)
}

/// Automatic failover is deliberately opt-in so existing proxy configurations
/// keep routing every request to their selected provider.
#[tauri::command]
pub fn get_proxy_failover_enabled(state: tauri::State<'_, AppState>) -> AppResult<bool> {
    Ok(state.db.with_conn(|conn| get_setting(conn, crate::proxy::PROXY_FAILOVER_ENABLED_KEY))?
        .as_deref() == Some("true"))
}

#[tauri::command]
pub fn set_proxy_failover_enabled(enabled: bool, state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.db.with_conn(|conn| set_setting(conn, crate::proxy::PROXY_FAILOVER_ENABLED_KEY, if enabled { "true" } else { "false" }))
}

#[tauri::command]
pub fn get_proxy_retryable_status_codes(state: tauri::State<'_, AppState>) -> AppResult<String> {
    let codes = state.db.with_conn(crate::proxy::load_retryable_status_codes)?;
    Ok(crate::proxy::format_retryable_status_codes(&codes))
}

#[tauri::command]
pub fn set_proxy_retryable_status_codes(value: String, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let codes = crate::proxy::parse_retryable_status_codes(&value)?;
    let formatted = crate::proxy::format_retryable_status_codes(&codes);
    state.db.with_conn(|conn| {
        set_setting(conn, crate::proxy::PROXY_RETRYABLE_STATUS_CODES_KEY, &formatted)
    })
}

#[tauri::command]
pub fn get_proxy_streaming_idle_timeout_secs(state: tauri::State<'_, AppState>) -> AppResult<u64> {
    state.db.with_conn(crate::proxy::load_streaming_idle_timeout_secs)
}

#[tauri::command]
pub fn set_proxy_streaming_idle_timeout_secs(secs: u64, state: tauri::State<'_, AppState>) -> AppResult<()> {
    let secs = secs.clamp(5, 3600);
    state.db.with_conn(|conn| {
        set_setting(
            conn,
            crate::proxy::PROXY_STREAMING_IDLE_TIMEOUT_KEY,
            &secs.to_string(),
        )
    })
}

fn port_key(target: crate::provider::ProviderTarget) -> &'static str {
    match target {
        crate::provider::ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        crate::provider::ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
        crate::provider::ProviderTarget::Codex => "proxy_port_codex",
        crate::provider::ProviderTarget::OpenCode => "proxy_port_opencode",
        crate::provider::ProviderTarget::Pi => "proxy_port_pi",
        crate::provider::ProviderTarget::Dsh => "proxy_port_dsh",
    }
}

fn get_saved_port(state: &AppState, target: crate::provider::ProviderTarget) -> u16 {
    get_saved_port_from_db(&state.db, target)
}

fn get_saved_port_from_db(db: &Database, target: crate::provider::ProviderTarget) -> u16 {
    db
        .with_conn(|conn| get_setting(conn, port_key(target)))
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(match target {
            crate::provider::ProviderTarget::ClaudeCode => DEFAULT_PORT,
            crate::provider::ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1,
            crate::provider::ProviderTarget::Codex => DEFAULT_PORT + 2,
            crate::provider::ProviderTarget::OpenCode => DEFAULT_PORT + 3,
            crate::provider::ProviderTarget::Pi => DEFAULT_PORT + 4,
            crate::provider::ProviderTarget::Dsh => DEFAULT_PORT + 5,
        })
}

pub fn initial_proxy_statuses(
    db: &Database,
) -> HashMap<crate::provider::ProviderTarget, ProxyStatus> {
    [
        crate::provider::ProviderTarget::ClaudeCode,
        crate::provider::ProviderTarget::ClaudeDesktop,
        crate::provider::ProviderTarget::Codex,
    ]
    .into_iter()
    .map(|target| {
        let status = ProxyStatus {
            running: false,
            port: get_saved_port_from_db(db, target),
            target_provider: db
                .with_conn(|conn| get_current_provider(conn, target))
                .ok()
                .flatten()
                .map(|provider| provider.name),
            phase: "stopped".to_string(),
            last_error: None,
            checked_at: chrono::Utc::now().timestamp_millis(),
        };
        (target, status)
    })
    .collect()
}

pub fn spawn_proxy_lifecycle_listener(
    app: tauri::AppHandle,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<ProxyLifecycleEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let state = app.state::<AppState>();
            let status = status_value(
                &state,
                event.target,
                get_saved_port(&state, event.target),
                "error",
                Some(sanitize_status_error(&event.error)),
            );
            publish_status(&app, &state, event.target, status).await;
        }
    });
}

fn status_value(
    state: &AppState,
    target: crate::provider::ProviderTarget,
    port: u16,
    phase: &str,
    last_error: Option<String>,
) -> ProxyStatus {
    let running = phase == "running";
    ProxyStatus {
        running,
        port,
        target_provider: state
            .db
            .with_conn(|conn| get_current_provider(conn, target))
            .ok()
            .flatten()
            .map(|provider| provider.name),
        phase: phase.to_string(),
        last_error,
        checked_at: chrono::Utc::now().timestamp_millis(),
    }
}

async fn status_snapshot(state: &AppState, target: crate::provider::ProviderTarget) -> ProxyStatus {
    state
        .proxy_status
        .read()
        .await
        .get(&target)
        .cloned()
        .unwrap_or_else(|| status_value(state, target, get_saved_port(state, target), "stopped", None))
}

pub(crate) async fn publish_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    target: crate::provider::ProviderTarget,
    status: ProxyStatus,
) {
    state.proxy_status.write().await.insert(target, status.clone());
    if let Err(error) = app.emit(
        "proxy-status-updated",
        ProxyStatusUpdated { target, status },
    ) {
        log::warn!("发送代理状态事件失败: {error}");
    }
}

/// Publish the live proxy manager status after provider switch / restore.
pub async fn publish_target_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    target: crate::provider::ProviderTarget,
) {
    let status = {
        let proxy = state.proxy.lock().await;
        proxy.status_for(target)
    };
    publish_status(app, state, target, status).await;
}

/// Publish a stopped snapshot (e.g. after official restore stops the target proxy).
pub async fn publish_target_stopped<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    target: crate::provider::ProviderTarget,
) {
    let status = status_value(state, target, get_saved_port(state, target), "stopped", None);
    publish_status(app, state, target, status).await;
}

fn sanitize_status_error(value: &str) -> String {
    value.chars().take(300).collect()
}

fn persist_port(state: &AppState, target: crate::provider::ProviderTarget, port: u16) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| set_setting(conn, port_key(target), &port.to_string()))
}

/// Start local proxies required by the active providers or live Desktop profile.
pub async fn ensure_runtime_proxies(app: &tauri::AppHandle, state: &AppState) {
    for target in [
        crate::provider::ProviderTarget::ClaudeCode,
        crate::provider::ProviderTarget::ClaudeDesktop,
        crate::provider::ProviderTarget::Codex,
    ] {
        let needs_proxy = state
            .db
            .with_conn(|conn| {
                if crate::catalog::enabled_for_conn(conn, target) {
                    return Ok(get_current_provider(conn, target)?.is_some());
                }
                Ok(get_current_provider(conn, target)?
                    .is_some_and(|provider| provider.requires_local_proxy()))
            })
            .unwrap_or(false)
            || (target == crate::provider::ProviderTarget::ClaudeDesktop
                && crate::config::claude_desktop::active_profile_uses_local_proxy());

        if !needs_proxy {
            continue;
        }

        let port = get_saved_port(state, target);
        publish_status(
            app,
            state,
            target,
            status_value(state, target, port, "starting", None),
        )
        .await;

        // After Windows updater hard-exit + NSIS `/R`, listen ports may still be
        // briefly busy (TIME_WAIT); retry with longer backoff instead of leaving
        // the proxy permanently in "error" until the user restarts again.
        const MAX_ATTEMPTS: u32 = 8;
        let mut last_error: Option<String> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            let mut proxy = state.proxy.lock().await;
            match proxy.start(port, target).await {
                Ok(()) => {
                    let status = proxy.status_for(target);
                    drop(proxy);
                    publish_status(app, state, target, status).await;
                    log::info!("已自动启动 {target:?} 本地代理: http://127.0.0.1:{port}");
                    last_error = None;
                    break;
                }
                Err(error) => {
                    drop(proxy);
                    last_error = Some(error.to_string());
                    log::warn!(
                        "自动启动 {target:?} 本地代理失败 (attempt {attempt}/{MAX_ATTEMPTS}): {error}"
                    );
                    if attempt < MAX_ATTEMPTS {
                        let delay_ms = 300u64.saturating_mul(u64::from(attempt)).min(2_000);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }
        if let Some(error) = last_error {
            let status = status_value(
                state,
                target,
                port,
                "error",
                Some(sanitize_status_error(&error)),
            );
            publish_status(app, state, target, status).await;
            log::error!("自动启动 {target:?} 本地代理最终失败: {error}");
        }
    }
}

/// Best-effort teardown before Windows updater `std::process::exit(0)`.
///
/// Must not call `Handle::block_on` here: the updater invokes this hook from an
/// async task, and nesting `block_on` panics/deadlocks. Use a short-lived thread.
pub fn prepare_for_updater_exit(app: &tauri::AppHandle) {
    use tauri::Manager;

    let app = app.clone();
    let join = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            log::warn!("updater exit: failed to create teardown runtime");
            return;
        };
        runtime.block_on(async {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            {
                let mut proxy = state.proxy.lock().await;
                proxy.stop_graceful().await;
            }
            if let Err(error) = state.db.checkpoint_wal() {
                log::warn!("updater exit: WAL checkpoint failed: {error}");
            }
            // Extra settle time so NSIS `/R` relaunch does not race TIME_WAIT ports.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        });
    });
    if let Err(error) = join.join() {
        log::warn!("updater exit: teardown thread panicked: {error:?}");
    }
}

/// Second-chance recovery used after an updater relaunch: ports may still be
/// settling when the first `ensure_runtime_proxies` pass runs.
///
/// Historical Codex session synchronization deliberately does not run here.
/// It rewrites JSONL and SQLite records and can race the Sessions page exactly
/// when an NSIS relaunch opens it. Provider activation and the explicit repair
/// action remain responsible for that one-time migration.
pub async fn recover_runtime_after_relaunch(app: &tauri::AppHandle, state: &AppState) {
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    const MAX_ENDPOINT_REPAIR_ATTEMPTS: u32 = 3;
    let mut repaired = false;
    for attempt in 1..=MAX_ENDPOINT_REPAIR_ATTEMPTS {
        match crate::commands::providers::repair_codex_managed_proxy_endpoint(state).await {
            Ok(()) => {
                repaired = true;
                break;
            }
            Err(error) if attempt < MAX_ENDPOINT_REPAIR_ATTEMPTS => {
                log::warn!(
                    "relaunch recovery: Codex endpoint repair attempt {attempt}/{MAX_ENDPOINT_REPAIR_ATTEMPTS} failed: {error}; retrying"
                );
                tokio::time::sleep(std::time::Duration::from_secs(u64::from(attempt) * 2)).await;
            }
            Err(error) => {
                log::warn!(
                    "relaunch recovery: Codex endpoint repair attempt {attempt}/{MAX_ENDPOINT_REPAIR_ATTEMPTS} failed: {error}"
                );
            }
        }
    }
    ensure_runtime_proxies(app, state).await;

    if let Err(error) = app.emit(RUNTIME_RECOVERED_EVENT, ()) {
        log::warn!("relaunch recovery: emit {RUNTIME_RECOVERED_EVENT} failed: {error}");
    }

    // A junction/symlinked CODEX_HOME whose target drive is temporarily
    // unreachable (spin-down, cleanup tool rebuilding the dir, …) makes the
    // fast attempts above fail within ~40s — far shorter than real outages.
    // Keep retrying in the background for up to 20 minutes so the repair
    // applies itself once the drive is back, without an app restart.
    if !repaired {
        let retry_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            const RETRY_INTERVAL_SECS: u64 = 30;
            const MAX_LONG_ATTEMPTS: u32 = 40; // 30s × 40 = 20 minutes
            for attempt in 1..=MAX_LONG_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                let state = retry_handle.state::<AppState>();
                match crate::commands::providers::repair_codex_managed_proxy_endpoint(&state).await
                {
                    Ok(()) => {
                        log::info!(
                            "long-window Codex endpoint repair succeeded (attempt {attempt}/{MAX_LONG_ATTEMPTS})"
                        );
                        return;
                    }
                    Err(error) => {
                        log::warn!(
                            "long-window Codex endpoint repair attempt {attempt}/{MAX_LONG_ATTEMPTS} failed: {error}"
                        );
                    }
                }
            }
            log::warn!(
                "long-window Codex endpoint repair exhausted {MAX_LONG_ATTEMPTS} attempts; giving up until next launch"
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_status_errors_are_bounded_without_breaking_unicode() {
        let input = "端口占用".repeat(100);
        let sanitized = sanitize_status_error(&input);
        assert_eq!(sanitized.chars().count(), 300);
        assert!(input.starts_with(&sanitized));
    }
}
