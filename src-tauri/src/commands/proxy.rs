//! Commands for controlling and inspecting the local proxy.

use serde::Serialize;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::AppResult;
use crate::proxy::ProxyStatus;
use crate::store::AppState;

const DEFAULT_PORT: u16 = 15821;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatusInfo {
    pub running: bool,
    pub port: u16,
    pub target_provider: Option<String>,
}

impl From<ProxyStatus> for ProxyStatusInfo {
    fn from(s: ProxyStatus) -> Self {
        Self {
            running: s.running,
            port: s.port,
            target_provider: s.target_provider,
        }
    }
}

#[tauri::command]
pub async fn get_proxy_status(target: Option<crate::provider::ProviderTarget>, state: tauri::State<'_, AppState>) -> AppResult<ProxyStatusInfo> {
    let proxy = state
        .proxy
        .lock()
        .await;
    Ok(match target {
        Some(target) => proxy.status_for(target).into(),
        None => proxy.status().into(),
    })
}

#[tauri::command]
pub async fn start_proxy(
    port: Option<u16>,
    target: Option<crate::provider::ProviderTarget>,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProxyStatusInfo> {
    let target = target.unwrap_or(crate::provider::ProviderTarget::ClaudeDesktop);
    let target_port = port.unwrap_or_else(|| get_saved_port(&state, target));
    let mut proxy = state.proxy.lock().await;
    proxy.start(target_port, target).await?;
    persist_port(&state, target, target_port)?;
    Ok(proxy.status().into())
}

#[tauri::command]
pub async fn stop_proxy(target: Option<crate::provider::ProviderTarget>, state: tauri::State<'_, AppState>) -> AppResult<ProxyStatusInfo> {
    let mut proxy = state.proxy.lock().await;
    match target {
        Some(target) => { proxy.stop_target(target); Ok(proxy.status_for(target).into()) }
        None => { proxy.stop(); Ok(proxy.status().into()) }
    }
}

#[tauri::command]
pub fn set_proxy_port(port: u16, target: Option<crate::provider::ProviderTarget>, state: tauri::State<'_, AppState>) -> AppResult<()> {
    persist_port(&state, target.unwrap_or(crate::provider::ProviderTarget::ClaudeDesktop), port)
}

fn port_key(target: crate::provider::ProviderTarget) -> &'static str {
    match target {
        crate::provider::ProviderTarget::ClaudeCode => "proxy_port_claude_code",
        crate::provider::ProviderTarget::ClaudeDesktop => "proxy_port_claude_desktop",
    }
}

fn get_saved_port(state: &AppState, target: crate::provider::ProviderTarget) -> u16 {
    state
        .db
        .with_conn(|conn| get_setting(conn, port_key(target)))
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(match target { crate::provider::ProviderTarget::ClaudeCode => DEFAULT_PORT, crate::provider::ProviderTarget::ClaudeDesktop => DEFAULT_PORT + 1 })
}

fn persist_port(state: &AppState, target: crate::provider::ProviderTarget, port: u16) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| set_setting(conn, port_key(target), &port.to_string()))
}
