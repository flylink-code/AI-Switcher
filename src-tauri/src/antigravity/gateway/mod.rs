//! Local Antigravity API gateway (Anthropic + OpenAI compatible).

mod handlers;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::routing::{any, get};
use axum::Router;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use super::pool::AccountPool;
use super::upstream::UpstreamClient;
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

pub const DEFAULT_GATEWAY_PORT: u16 = 15830;
const PORT_SETTING: &str = "antigravity_gateway_port";
const API_KEY_SETTING: &str = "antigravity_gateway_api_key";
const DEFAULT_API_KEY: &str = "sk-ai-switcher-antigravity";

#[derive(Clone)]
pub struct GatewayState {
    pub db: Arc<Database>,
    pub pool: Arc<AccountPool>,
    pub upstream: Arc<UpstreamClient>,
    pub api_key: Arc<Mutex<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityGatewayStatus {
    pub running: bool,
    pub port: u16,
    pub api_key: String,
    pub account_count: usize,
    pub base_url: String,
}

struct GatewayRuntime {
    handle: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
    port: u16,
}

struct GatewayManager {
    db: Arc<Database>,
    runtime: Option<GatewayRuntime>,
    pool: Arc<AccountPool>,
    upstream: Arc<UpstreamClient>,
    api_key: Arc<Mutex<String>>,
}

static MANAGER: OnceLock<Mutex<Option<GatewayManager>>> = OnceLock::new();

fn manager_slot() -> &'static Mutex<Option<GatewayManager>> {
    MANAGER.get_or_init(|| Mutex::new(None))
}

pub fn init_gateway(db: Arc<Database>) {
    let api_key = db
        .with_conn(|conn| get_setting(conn, API_KEY_SETTING))
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY.to_string());
    let mut slot = manager_slot().lock().expect("gateway manager lock");
    *slot = Some(GatewayManager {
        db,
        runtime: None,
        pool: Arc::new(AccountPool::new()),
        upstream: Arc::new(UpstreamClient::new()),
        api_key: Arc::new(Mutex::new(api_key)),
    });
}

fn with_manager<T>(f: impl FnOnce(&mut GatewayManager) -> AppResult<T>) -> AppResult<T> {
    let mut slot = manager_slot()
        .lock()
        .map_err(|_| AppError::Other("Antigravity 网关锁不可用".into()))?;
    let manager = slot
        .as_mut()
        .ok_or_else(|| AppError::Other("Antigravity 网关尚未初始化".into()))?;
    f(manager)
}

pub fn gateway_status() -> AppResult<AntigravityGatewayStatus> {
    with_manager(|manager| {
        let port = saved_port(&manager.db);
        let running = manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.handle.is_finished());
        let effective_port = manager
            .runtime
            .as_ref()
            .map(|runtime| runtime.port)
            .unwrap_or(port);
        let api_key = manager
            .api_key
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| DEFAULT_API_KEY.to_string());
        let account_count = super::account::store()
            .list_public()
            .map(|list| list.len())
            .unwrap_or(0);
        Ok(AntigravityGatewayStatus {
            running,
            port: effective_port,
            api_key,
            account_count,
            base_url: format!("http://127.0.0.1:{effective_port}"),
        })
    })
}

pub fn set_gateway_port(port: u16) -> AppResult<()> {
    if port == 0 {
        return Err(AppError::Config("端口无效".into()));
    }
    with_manager(|manager| {
        manager.db.with_conn(|conn| set_setting(conn, PORT_SETTING, &port.to_string()))?;
        Ok(())
    })
}

pub fn set_gateway_api_key(api_key: String) -> AppResult<()> {
    let trimmed = api_key.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::Config("API Key 不能为空".into()));
    }
    with_manager(|manager| {
        manager
            .db
            .with_conn(|conn| set_setting(conn, API_KEY_SETTING, &trimmed))?;
        if let Ok(mut guard) = manager.api_key.lock() {
            *guard = trimmed;
        }
        Ok(())
    })
}

pub async fn start_gateway(port: Option<u16>) -> AppResult<AntigravityGatewayStatus> {
    let (state, bind_port) = with_manager(|manager| {
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.handle.is_finished())
        {
            return Err(AppError::Config("Antigravity 网关已在运行".into()));
        }
        let bind_port = port.unwrap_or_else(|| saved_port(&manager.db));
        manager
            .db
            .with_conn(|conn| set_setting(conn, PORT_SETTING, &bind_port.to_string()))?;
        let state = GatewayState {
            db: manager.db.clone(),
            pool: manager.pool.clone(),
            upstream: manager.upstream.clone(),
            api_key: manager.api_key.clone(),
        };
        Ok((state, bind_port))
    })?;

    let listener = TcpListener::bind(("127.0.0.1", bind_port))
        .await
        .map_err(|error| AppError::Io(format!("无法绑定 Antigravity 网关端口 {bind_port}: {error}")))?;
    let actual_port = listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or(bind_port);

    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/healthz", get(handlers::health))
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/messages", any(handlers::anthropic_messages))
        .route("/v1/chat/completions", any(handlers::openai_chat_completions))
        .route("/chat/completions", any(handlers::openai_chat_completions))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(error) = server.await {
            log::error!("Antigravity gateway stopped with error: {error}");
        }
    });

    with_manager(|manager| {
        manager.runtime = Some(GatewayRuntime {
            handle,
            shutdown_tx,
            port: actual_port,
        });
        Ok(())
    })?;

    // Brief settle so status reflects running.
    tokio::time::sleep(Duration::from_millis(30)).await;
    gateway_status()
}

pub async fn stop_gateway() -> AppResult<AntigravityGatewayStatus> {
    let runtime = with_manager(|manager| Ok(manager.runtime.take()))?;
    if let Some(runtime) = runtime {
        let _ = runtime.shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(2), runtime.handle).await;
    }
    gateway_status()
}

pub fn builtin_base_url() -> String {
    let port = manager_slot()
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref().map(|manager| {
                manager
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.port)
                    .unwrap_or_else(|| saved_port(&manager.db))
            })
        })
        .unwrap_or(DEFAULT_GATEWAY_PORT);
    format!("http://127.0.0.1:{port}")
}

pub fn builtin_api_key() -> String {
    manager_slot()
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref().and_then(|manager| {
                manager.api_key.lock().ok().map(|guard| guard.clone())
            })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY.to_string())
}

pub fn ensure_api_key_seed() -> String {
    let key = format!("sk-{}", Uuid::new_v4().simple());
    key
}

fn saved_port(db: &Database) -> u16 {
    db.with_conn(|conn| get_setting(conn, PORT_SETTING))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_GATEWAY_PORT)
}
