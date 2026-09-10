//! Standalone smart-gateway listener on 127.0.0.1:15828.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::database::dao::gateway::{has_any_binding, list_bindings};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::gateway::SMART_GATEWAY_PORT;
use crate::store::AppState;

pub const PORT_SETTING: &str = "smart_gateway_port";
pub const ENABLED_SETTING: &str = "smart_gateway_enabled";
pub const API_KEY_SETTING: &str = "smart_gateway_api_key";
pub const STATUS_EVENT: &str = "smart-gateway-status-updated";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartGatewayStatus {
    pub running: bool,
    pub port: u16,
    pub phase: String,
    pub last_error: Option<String>,
    pub base_url: String,
    pub api_key: String,
    pub binding_count: usize,
    pub checked_at: i64,
}

struct ServiceRuntime {
    handle: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
    port: u16,
}

struct ServiceManager {
    db: Arc<Database>,
    runtime: Option<ServiceRuntime>,
    phase: String,
    last_error: Option<String>,
}

static MANAGER: OnceLock<Mutex<Option<ServiceManager>>> = OnceLock::new();

fn lock_manager() -> std::sync::MutexGuard<'static, Option<ServiceManager>> {
    match MANAGER.get_or_init(|| Mutex::new(None)).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn init_service(db: Arc<Database>) {
    let mut slot = lock_manager();
    *slot = Some(ServiceManager {
        db,
        runtime: None,
        phase: "stopped".into(),
        last_error: None,
    });
}

fn saved_port(db: &Database) -> u16 {
    db.with_conn(|conn| get_setting(conn, PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(SMART_GATEWAY_PORT)
}

pub fn current_status() -> SmartGatewayStatus {
    let slot = lock_manager();
    let Some(manager) = slot.as_ref() else {
        return SmartGatewayStatus {
            running: false,
            port: SMART_GATEWAY_PORT,
            phase: "stopped".into(),
            last_error: Some("智能网关尚未初始化".into()),
            base_url: format!("http://127.0.0.1:{SMART_GATEWAY_PORT}"),
            api_key: String::new(),
            binding_count: 0,
            checked_at: chrono::Utc::now().timestamp_millis(),
        };
    };
    let running = manager
        .runtime
        .as_ref()
        .is_some_and(|runtime| !runtime.handle.is_finished());
    let port = manager
        .runtime
        .as_ref()
        .map(|runtime| runtime.port)
        .unwrap_or_else(|| saved_port(&manager.db));
    let binding_count = manager
        .db
        .with_conn(|conn| Ok(list_bindings(conn)?.len()))
        .unwrap_or(0);
    let api_key = ensure_api_key(&manager.db);
    SmartGatewayStatus {
        running,
        port,
        phase: if running {
            "running".into()
        } else {
            manager.phase.clone()
        },
        last_error: manager.last_error.clone(),
        base_url: format!("http://127.0.0.1:{port}"),
        api_key,
        binding_count,
        checked_at: chrono::Utc::now().timestamp_millis(),
    }
}

pub fn api_key_for_conn(conn: &rusqlite::Connection) -> Option<String> {
    get_setting(conn, API_KEY_SETTING)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn api_key_matches(conn: &rusqlite::Connection, presented: &str) -> bool {
    let presented = presented.trim();
    !presented.is_empty() && api_key_for_conn(conn).as_deref() == Some(presented)
}

fn new_api_key() -> String {
    format!("sk-aisw-{}", uuid::Uuid::new_v4().simple())
}

/// Public API key for custom agents that are not a bound App. Created on first read.
pub fn ensure_api_key(db: &Database) -> String {
    db.with_conn(|conn| {
        if let Some(existing) = api_key_for_conn(conn) {
            return Ok(existing);
        }
        let key = new_api_key();
        set_setting(conn, API_KEY_SETTING, &key)?;
        Ok(key)
    })
    .unwrap_or_default()
}

pub fn rotate_api_key(db: &Database) -> AppResult<String> {
    db.with_conn(|conn| {
        let key = new_api_key();
        set_setting(conn, API_KEY_SETTING, &key)?;
        Ok(key)
    })
}

pub fn persist_api_key(db: &Database, api_key: &str) -> AppResult<String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return rotate_api_key(db);
    }
    if trimmed.len() < 8 {
        return Err(AppError::Config("API Key 至少 8 个字符".into()));
    }
    db.with_conn(|conn| {
        set_setting(conn, API_KEY_SETTING, trimmed)?;
        Ok(trimmed.to_string())
    })
}

pub fn persist_port(db: &Database, port: u16) -> AppResult<()> {
    if port < 1024 {
        return Err(AppError::Config("端口必须大于等于 1024".into()));
    }
    if crate::gateway::reserved_listener_ports()
        .iter()
        .any(|reserved| *reserved == port && *reserved != SMART_GATEWAY_PORT)
    {
        return Err(AppError::Config(format!(
            "端口 {port} 已被本地代理或反代网关占用，请换一个"
        )));
    }
    if port_in_use(port) {
        return Err(AppError::Config(format!("端口 {port} 已被占用")));
    }
    db.with_conn(|conn| set_setting(conn, PORT_SETTING, &port.to_string()))
}

fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

pub async fn start_service(db: Arc<Database>, port: Option<u16>) -> AppResult<SmartGatewayStatus> {
    let bind_port = port.unwrap_or_else(|| saved_port(&db));
    {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("智能网关尚未初始化".into()))?;
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.port == bind_port && !runtime.handle.is_finished())
        {
            manager.phase = "running".into();
            manager.last_error = None;
            let _ = db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
            drop(slot);
            return Ok(current_status());
        }
        manager.phase = "starting".into();
        manager.last_error = None;
    }

    let addr = format!("127.0.0.1:{bind_port}");
    let listener = match TcpListener::bind(("127.0.0.1", bind_port)).await {
        Ok(listener) => listener,
        Err(error) => {
            let mut slot = lock_manager();
            if let Some(manager) = slot.as_mut() {
                manager.phase = "error".into();
                manager.last_error = Some(format!("无法绑定智能网关端口 {bind_port}: {error}"));
            }
            return Err(AppError::Io(format!(
                "无法绑定智能网关端口 {bind_port}: {error}"
            )));
        }
    };

    let app = crate::proxy::smart_gateway_router(Arc::clone(&db), bind_port);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    let handle = tokio::spawn(async move {
        if let Err(error) = server.await {
            log::error!("智能网关服务异常退出: {error}");
            let mut slot = lock_manager();
            if let Some(manager) = slot.as_mut() {
                manager.phase = "error".into();
                manager.last_error = Some(error.to_string());
            }
        }
    });

    {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("智能网关尚未初始化".into()))?;
        if let Some(previous) = manager.runtime.replace(ServiceRuntime {
            handle,
            shutdown_tx,
            port: bind_port,
        }) {
            let _ = previous.shutdown_tx.send(());
            previous.handle.abort();
        }
        manager.phase = "running".into();
        manager.last_error = None;
    }
    let _ = db.with_conn(|conn| {
        set_setting(conn, PORT_SETTING, &bind_port.to_string())?;
        set_setting(conn, ENABLED_SETTING, "1")
    });
    log::info!("智能网关已启动: {addr}");
    Ok(current_status())
}

pub async fn stop_service() -> AppResult<SmartGatewayStatus> {
    let runtime = {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("智能网关尚未初始化".into()))?;
        manager.phase = "stopped".into();
        let _ = manager.db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "0"));
        manager.runtime.take()
    };
    if let Some(runtime) = runtime {
        let _ = runtime.shutdown_tx.send(());
        let abort = runtime.handle.abort_handle();
        match tokio::time::timeout(Duration::from_millis(1_500), runtime.handle).await {
            Ok(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            Err(_) => {
                abort.abort();
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        }
    }
    Ok(current_status())
}

pub async fn restore_if_enabled(db: Arc<Database>) {
    let enabled = db
        .with_conn(|conn| get_setting(conn, ENABLED_SETTING))
        .ok()
        .flatten()
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let has_binding = db.with_conn(|conn| Ok(has_any_binding(conn))).unwrap_or(false);
    if !enabled && !has_binding {
        return;
    }
    let port = saved_port(&db);
    for attempt in 1..=8u32 {
        match start_service(Arc::clone(&db), Some(port)).await {
            Ok(status) if status.running => {
                log::info!("智能网关自动恢复成功: port={}", status.port);
                return;
            }
            Ok(_) | Err(_) => {
                let backoff = (300 * attempt).min(2_000);
                tokio::time::sleep(Duration::from_millis(backoff as u64)).await;
            }
        }
    }
    {
        let mut slot = lock_manager();
        if let Some(manager) = slot.as_mut() {
            let running = manager
                .runtime
                .as_ref()
                .is_some_and(|runtime| !runtime.handle.is_finished());
            if !running {
                manager.phase = "error".into();
                if manager.last_error.is_none() {
                    manager.last_error = Some(format!("智能网关自动恢复失败: 端口 {port}"));
                }
            }
        }
    }
    log::error!("智能网关自动恢复失败: port={port}");
}

pub async fn restore_after_relaunch(db: Arc<Database>) {
    tokio::time::sleep(Duration::from_secs(3)).await;
    restore_if_enabled(db).await;
}

pub fn emit_status(app: &tauri::AppHandle) {
    let status = current_status();
    let _ = tauri::Emitter::emit(app, STATUS_EVENT, status);
}

pub async fn start_via_state(state: &AppState, port: Option<u16>) -> AppResult<SmartGatewayStatus> {
    start_service(Arc::clone(&state.db), port).await
}

/// Used by catalog-push / bind to force-enable the listener.
pub fn mark_enabled(db: &Database) -> AppResult<()> {
    db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_port_rejects_local_proxy_and_antigravity() {
        let db = crate::database::Database::memory().unwrap();
        assert!(persist_port(&db, 15821).is_err());
        assert!(persist_port(&db, 15827).is_err());
        assert!(persist_port(&db, 15830).is_err());
        match persist_port(&db, SMART_GATEWAY_PORT) {
            Ok(()) => {}
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("占用"),
                    "15828 should only fail when the port is actually occupied: {text}"
                );
            }
        }
    }

    #[test]
    fn public_api_key_is_stable_until_rotated() {
        let db = crate::database::Database::memory().unwrap();
        let first = ensure_api_key(&db);
        assert!(first.starts_with("sk-aisw-"));
        assert_eq!(ensure_api_key(&db), first);
        let rotated = rotate_api_key(&db).unwrap();
        assert_ne!(rotated, first);
        assert_eq!(ensure_api_key(&db), rotated);
        db.with_conn(|conn| {
            assert!(api_key_matches(conn, &rotated));
            assert!(!api_key_matches(conn, "sk-wrong"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn persist_api_key_rejects_short_and_keeps_custom() {
        let db = crate::database::Database::memory().unwrap();
        assert!(persist_api_key(&db, "short").is_err());
        let saved = persist_api_key(&db, "  sk-custom-agent-key  ").unwrap();
        assert_eq!(saved, "sk-custom-agent-key");
        assert_eq!(ensure_api_key(&db), "sk-custom-agent-key");
    }
}
