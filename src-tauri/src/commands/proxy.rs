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

fn port_key(target: crate::provider::ProviderTarget) -> &'static str {
    match target {
        crate::provider::ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        crate::provider::ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
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
        .unwrap_or(match target { crate::provider::ProviderTarget::ClaudeCode => DEFAULT_PORT, crate::provider::ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1 })
}

pub fn initial_proxy_statuses(
    db: &Database,
) -> HashMap<crate::provider::ProviderTarget, ProxyStatus> {
    [
        crate::provider::ProviderTarget::ClaudeCode,
        crate::provider::ProviderTarget::ClaudeDesktop,
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

async fn publish_status(
    app: &tauri::AppHandle,
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
    ] {
        let needs_proxy = state
            .db
            .with_conn(|conn| {
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
        let mut proxy = state.proxy.lock().await;
        match proxy.start(port, target).await {
            Ok(()) => {
                let status = proxy.status_for(target);
                drop(proxy);
                publish_status(app, state, target, status).await;
                log::info!("已自动启动 {target:?} 本地代理: http://127.0.0.1:{port}");
            }
            Err(error) => {
                drop(proxy);
                let status = status_value(
                    state,
                    target,
                    port,
                    "error",
                    Some(sanitize_status_error(&error.to_string())),
                );
                publish_status(app, state, target, status).await;
                log::error!("自动启动 {target:?} 本地代理失败: {error}");
            }
        }
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
