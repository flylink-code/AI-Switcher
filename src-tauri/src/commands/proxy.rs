//! Commands for controlling and inspecting the local proxy.

use serde::Serialize;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::AppResult;
use crate::proxy::ProxyStatus;
use crate::store::AppState;

const PROXY_PORT_KEY: &str = "proxy_port";
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
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> AppResult<ProxyStatusInfo> {
    let proxy = state
        .proxy
        .lock()
        .await;
    Ok(proxy.status().into())
}

#[tauri::command]
pub async fn start_proxy(
    port: Option<u16>,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProxyStatusInfo> {
    let target_port = port.unwrap_or_else(|| get_saved_port(&state));
    let mut proxy = state.proxy.lock().await;
    proxy.start(target_port, crate::provider::ProviderTarget::ClaudeDesktop).await?;
    persist_port(&state, target_port)?;
    Ok(proxy.status().into())
}

#[tauri::command]
pub async fn stop_proxy(state: tauri::State<'_, AppState>) -> AppResult<ProxyStatusInfo> {
    let mut proxy = state.proxy.lock().await;
    proxy.stop();
    Ok(proxy.status().into())
}

#[tauri::command]
pub fn set_proxy_port(port: u16, state: tauri::State<'_, AppState>) -> AppResult<()> {
    persist_port(&state, port)
}

fn get_saved_port(state: &AppState) -> u16 {
    state
        .db
        .with_conn(|conn| get_setting(conn, PROXY_PORT_KEY))
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn persist_port(state: &AppState, port: u16) -> AppResult<()> {
    state
        .db
        .with_conn(|conn| set_setting(conn, PROXY_PORT_KEY, &port.to_string()))
}
