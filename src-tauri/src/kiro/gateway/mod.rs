//! Local Kiro API gateway. Listens on 127.0.0.1 only.

mod handlers;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::routing::{any, get, post};
use axum::Router;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

use super::outbound::{self, OutboundMode, OutboundSettings};
use super::pool::AccountPool;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

pub const DEFAULT_GATEWAY_PORT: u16 = 15831;
const PORT_SETTING: &str = "kiro_gateway_port";
const API_KEY_SETTING: &str = "kiro_gateway_api_key";
const ENABLED_SETTING: &str = "kiro_gateway_enabled";
const DEFAULT_API_KEY: &str = "sk-ai-switcher-kiro";

#[derive(Clone)]
pub struct GatewayState {
    pub db: Arc<Database>,
    pub pool: Arc<AccountPool>,
    pub api_key: Arc<Mutex<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroGatewayStatus {
    pub running: bool,
    pub port: u16,
    pub api_key: String,
    pub account_count: usize,
    pub base_url: String,
    pub outbound_mode: String,
    pub outbound_proxy_url: String,
    pub effective_outbound_proxy: Option<String>,
    pub exit_proxies: Vec<super::outbound::ExitProxyView>,
    pub exit_chain_label: String,
    pub exit_error: Option<String>,
}

struct GatewayRuntime {
    handle: JoinHandle<()>,
    watch_abort: tokio::task::AbortHandle,
    shutdown_tx: oneshot::Sender<()>,
    port: u16,
}

struct GatewayManager {
    db: Arc<Database>,
    runtime: Option<GatewayRuntime>,
    pool: Arc<AccountPool>,
    api_key: Arc<Mutex<String>>,
}

static MANAGER: OnceLock<Mutex<Option<GatewayManager>>> = OnceLock::new();

fn lock_manager() -> std::sync::MutexGuard<'static, Option<GatewayManager>> {
    let slot = MANAGER.get_or_init(|| Mutex::new(None));
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn builtin_api_key() -> String {
    DEFAULT_API_KEY.to_string()
}

pub fn init_gateway(db: Arc<Database>) {
    let _ = super::account::store();
    let settings = outbound::load_settings(&db).unwrap_or_else(|_| OutboundSettings {
        mode: OutboundMode::System,
        proxy_url: String::new(),
        effective_proxy_url: crate::system_proxy::outbound_proxy_url(),
    });
    outbound::apply_loaded(&settings);
    let _ = outbound::load_view(&db);
    let api_key = db
        .with_conn(|conn| get_setting(conn, API_KEY_SETTING))
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY.to_string());
    let mut slot = lock_manager();
    *slot = Some(GatewayManager {
        db,
        runtime: None,
        pool: Arc::new(AccountPool::new()),
        api_key: Arc::new(Mutex::new(api_key)),
    });
}

pub fn gateway_status() -> AppResult<KiroGatewayStatus> {
    let mut slot = lock_manager();
    let manager = slot
        .as_mut()
        .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
    let port = saved_port(&manager.db);
    let running = manager
        .runtime
        .as_ref()
        .is_some_and(|runtime| !runtime.handle.is_finished());
    let effective_port = manager.runtime.as_ref().map(|runtime| runtime.port).unwrap_or(port);
    let api_key = manager
        .api_key
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_else(|_| DEFAULT_API_KEY.to_string());
    let account_count = super::account::store().list_public().map(|list| list.len()).unwrap_or(0);
    let outbound = outbound::load_view(&manager.db).unwrap_or(outbound::OutboundView {
        mode: OutboundMode::System,
        proxy_url: String::new(),
        effective_proxy_url: None,
        exit_proxies: Vec::new(),
        exit_chain_label: String::new(),
        exit_error: None,
    });
    Ok(KiroGatewayStatus {
        running,
        port: effective_port,
        api_key,
        account_count,
        base_url: format!("http://127.0.0.1:{effective_port}"),
        outbound_mode: outbound.mode.as_str().to_string(),
        outbound_proxy_url: outbound.proxy_url,
        effective_outbound_proxy: outbound.effective_proxy_url,
        exit_proxies: outbound.exit_proxies,
        exit_chain_label: outbound.exit_chain_label,
        exit_error: outbound.exit_error,
    })
}

pub fn set_gateway_port(port: u16) -> AppResult<()> {
    if port == 0 {
        return Err(AppError::Config("端口无效".into()));
    }
    let mut slot = lock_manager();
    let manager = slot
        .as_mut()
        .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
    manager
        .db
        .with_conn(|conn| set_setting(conn, PORT_SETTING, &port.to_string()))
}

pub fn set_gateway_api_key(api_key: String) -> AppResult<()> {
    let trimmed = api_key.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::Config("API Key 不能为空".into()));
    }
    let mut slot = lock_manager();
    let manager = slot
        .as_mut()
        .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
    manager
        .db
        .with_conn(|conn| set_setting(conn, API_KEY_SETTING, &trimmed))?;
    if let Ok(mut guard) = manager.api_key.lock() {
        *guard = trimmed;
    }
    Ok(())
}

pub fn set_outbound_proxy(mode: &str, proxy_url: &str) -> AppResult<KiroGatewayStatus> {
    let mut slot = lock_manager();
    let manager = slot
        .as_mut()
        .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
    outbound::save_settings(&manager.db, OutboundMode::parse(mode), proxy_url)?;
    let _ = outbound::load_view(&manager.db);
    super::account::store().reload_http_client();
    drop(slot);
    gateway_status()
}

pub fn set_exit_proxies(
    entries: Vec<super::outbound::ExitProxyEntry>,
) -> AppResult<KiroGatewayStatus> {
    let db = {
        let slot = lock_manager();
        slot.as_ref()
            .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?
            .db
            .clone()
    };
    super::outbound::save_exit_proxies(&db, entries)?;
    super::account::store().reload_http_client();
    gateway_status()
}

pub async fn start_gateway(port: Option<u16>) -> AppResult<KiroGatewayStatus> {
    let (state, bind_port, db) = {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.handle.is_finished())
        {
            if let Some(runtime) = manager.runtime.take() {
                runtime.watch_abort.abort();
            }
        }
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.handle.is_finished())
        {
            let _ = manager.db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
            drop(slot);
            return gateway_status();
        }
        let bind_port = port.unwrap_or_else(|| saved_port(&manager.db));
        manager
            .db
            .with_conn(|conn| set_setting(conn, PORT_SETTING, &bind_port.to_string()))?;
        let _ = manager.db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
        if let Ok(settings) = outbound::load_settings(&manager.db) {
            outbound::apply_loaded(&settings);
        }
        let _ = super::account::store().clear_cooldowns();
        let state = GatewayState {
            db: manager.db.clone(),
            pool: manager.pool.clone(),
            api_key: manager.api_key.clone(),
        };
        (state, bind_port, manager.db.clone())
    };
    let _ = tokio::task::spawn_blocking(|| super::account::store().reload_http_client()).await;
    let listener = TcpListener::bind(("127.0.0.1", bind_port)).await.map_err(|error| {
        AppError::Io(format!("无法绑定 Kiro 网关端口 {bind_port}: {error}"))
    })?;
    let actual_port = listener.local_addr().map(|addr| addr.port()).unwrap_or(bind_port);
    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/healthz", get(handlers::health))
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/messages/count_tokens", post(handlers::count_tokens))
        .route("/v1/messages", any(handlers::anthropic_messages))
        .route("/v1/chat/completions", any(handlers::openai_chat))
        .route("/v1/responses", any(handlers::openai_responses))
        .with_state(state)
        .layer(CorsLayer::permissive());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let watch = tokio::spawn(watch_system_proxy(db));
    let watch_abort = watch.abort_handle();
    let server_watch = watch_abort.clone();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(error) = server.await {
            log::error!("Kiro gateway stopped with error: {error}");
        }
        server_watch.abort();
    });
    {
        let mut slot = lock_manager();
        if let Some(manager) = slot.as_mut() {
            manager.runtime = Some(GatewayRuntime {
                handle,
                watch_abort,
                shutdown_tx,
                port: actual_port,
            });
        }
    }
    gateway_status()
}

pub async fn stop_gateway() -> AppResult<KiroGatewayStatus> {
    let runtime = {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("Kiro 网关尚未初始化".into()))?;
        let _ = manager.db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "0"));
        manager.runtime.take()
    };
    if let Some(runtime) = runtime {
        runtime.watch_abort.abort();
        let _ = runtime.shutdown_tx.send(());
        let _ = runtime.handle.await;
    }
    gateway_status()
}

pub async fn restore_gateway_if_enabled() {
    let enabled = lock_manager().as_ref().and_then(|manager| {
        manager
            .db
            .with_conn(|conn| get_setting(conn, ENABLED_SETTING))
            .ok()
            .flatten()
    });
    if enabled.as_deref() == Some("1") {
        if let Err(error) = start_gateway(None).await {
            log::warn!("恢复 Kiro 网关失败: {error}");
        }
    }
}

async fn watch_system_proxy(db: Arc<Database>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(10));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;
    let mut previous = crate::system_proxy::outbound_proxy_url();
    loop {
        ticker.tick().await;
        let Ok(settings) = outbound::load_settings(&db) else {
            continue;
        };
        if settings.mode != OutboundMode::System {
            continue;
        }
        let detected = crate::system_proxy::outbound_proxy_url();
        if detected != previous {
            previous = detected.clone();
            outbound::resync_system_exit(&db);
            let _ = tokio::task::spawn_blocking(|| super::account::store().reload_http_client()).await;
        }
    }
}

fn saved_port(db: &Database) -> u16 {
    db.with_conn(|conn| get_setting(conn, PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_GATEWAY_PORT)
}
