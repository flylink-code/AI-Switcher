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
const ENABLED_SETTING: &str = "antigravity_gateway_enabled";
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
    pub outbound_mode: String,
    pub outbound_proxy_url: String,
    pub effective_outbound_proxy: Option<String>,
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

/// Recover from a poisoned mutex (previous panic while holding the lock).
fn lock_manager() -> std::sync::MutexGuard<'static, Option<GatewayManager>> {
    match manager_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::error!(
                "Antigravity gateway manager mutex was poisoned; recovering inner state"
            );
            poisoned.into_inner()
        }
    }
}

pub fn init_gateway(db: Arc<Database>) {
    crate::antigravity::outbound::warm_from_db(&db);
    // Warm the account store on a sync thread so its blocking reqwest client is
    // not first created inside an async task (that panics Tokio).
    let _ = super::account::store();
    let api_key = db
        .with_conn(|conn| get_setting(conn, API_KEY_SETTING))
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY.to_string());
    // Build clients outside the lock so a proxy/client panic cannot poison the manager.
    let upstream = Arc::new(UpstreamClient::new());
    let api_key = Arc::new(Mutex::new(api_key));
    let mut slot = lock_manager();
    *slot = Some(GatewayManager {
        db,
        runtime: None,
        pool: Arc::new(AccountPool::new()),
        upstream,
        api_key,
    });
}

fn with_manager<T>(f: impl FnOnce(&mut GatewayManager) -> AppResult<T>) -> AppResult<T> {
    let mut slot = lock_manager();
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
        let outbound = crate::antigravity::outbound::load_settings(&manager.db)
            .unwrap_or_else(|_| crate::antigravity::outbound::default_settings());
        Ok(AntigravityGatewayStatus {
            running,
            port: effective_port,
            api_key,
            account_count,
            base_url: format!("http://127.0.0.1:{effective_port}"),
            outbound_mode: outbound.mode.as_str().to_string(),
            outbound_proxy_url: outbound.proxy_url,
            effective_outbound_proxy: outbound.effective_proxy_url,
        })
    })
}

pub fn set_outbound_proxy(mode: &str, proxy_url: &str) -> AppResult<AntigravityGatewayStatus> {
    with_manager(|manager| {
        let settings = crate::antigravity::outbound::save_settings(
            &manager.db,
            crate::antigravity::outbound::OutboundProxyMode::parse(mode),
            proxy_url,
        )?;
        manager.upstream.reload();
        super::account::store().reload_http_client();
        log::info!(
            "Antigravity outbound reloaded: mode={} effective={:?}",
            settings.mode.as_str(),
            settings.effective_proxy_url
        );
        Ok(())
    })?;
    gateway_status()
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
    // Prepare bind/config without holding the manager lock across await points.
    let (state, bind_port, db) = {
        let mut slot = lock_manager();
        let manager = slot
            .as_mut()
            .ok_or_else(|| AppError::Other("Antigravity 网关尚未初始化".into()))?;
        // Drop finished runtimes left after crashes / failed binds.
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.handle.is_finished())
        {
            manager.runtime = None;
        }
        if manager
            .runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.handle.is_finished())
        {
            // Idempotent start — UI / Desktop switch often re-enter.
            let _ = manager.db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
            drop(slot);
            return gateway_status();
        }
        let bind_port = port.unwrap_or_else(|| saved_port(&manager.db));
        manager
            .db
            .with_conn(|conn| set_setting(conn, PORT_SETTING, &bind_port.to_string()))?;
        let _ = manager
            .db
            .with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
        // Rebuild async upstream only here. Never build reqwest::blocking inside
        // this async fn — that creates/drops a nested Tokio runtime and panics
        // with "Cannot drop a runtime in a context where blocking is not allowed".
        let _ = crate::antigravity::outbound::load_settings(&manager.db);
        manager.upstream.reload();
        let _ = super::account::store().clear_all_cooldowns();
        let state = GatewayState {
            db: manager.db.clone(),
            pool: manager.pool.clone(),
            upstream: manager.upstream.clone(),
            api_key: manager.api_key.clone(),
        };
        (state, bind_port, manager.db.clone())
    };

    // Rebuild blocking token-refresh client off the async runtime.
    let _ = tokio::task::spawn_blocking(|| {
        super::account::store().reload_http_client();
    })
    .await;

    let listener = match TcpListener::bind(("127.0.0.1", bind_port)).await {
        Ok(listener) => listener,
        Err(error) => {
            let _ = db.with_conn(|conn| set_setting(conn, ENABLED_SETTING, "1"));
            return Err(AppError::Io(format!(
                "无法绑定 Antigravity 网关端口 {bind_port}: {error}"
            )));
        }
    };
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
        .route("/v1/responses", any(handlers::openai_responses))
        .route("/responses", any(handlers::openai_responses))
        .route("/v1/responses/compact", any(handlers::openai_responses_compact))
        .route("/responses/compact", any(handlers::openai_responses_compact))
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

    {
        let mut slot = lock_manager();
        if let Some(manager) = slot.as_mut() {
            manager.runtime = Some(GatewayRuntime {
                handle,
                shutdown_tx,
                port: actual_port,
            });
        }
    }

    // Brief settle so status reflects running.
    tokio::time::sleep(Duration::from_millis(30)).await;
    log::info!("Antigravity gateway listening on 127.0.0.1:{actual_port}");
    gateway_status()
}

pub async fn stop_gateway() -> AppResult<AntigravityGatewayStatus> {
    let runtime = with_manager(|manager| {
        let _ = manager
            .db
            .with_conn(|conn| set_setting(conn, ENABLED_SETTING, "0"));
        Ok(manager.runtime.take())
    })?;
    if let Some(runtime) = runtime {
        let _ = runtime.shutdown_tx.send(());
        // Detach join so Drop does not run a nested runtime teardown on this task.
        match tokio::time::timeout(Duration::from_secs(2), runtime.handle).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("Antigravity gateway task join error: {error}"),
            Err(_) => log::warn!("Antigravity gateway shutdown timed out"),
        }
    }
    gateway_status()
}

/// If the user left the gateway enabled, or an Antigravity provider is current,
/// start it after app launch.
pub async fn restore_gateway_if_enabled() {
    let should_start = with_manager(|manager| {
        let flagged = manager
            .db
            .with_conn(|conn| get_setting(conn, ENABLED_SETTING))
            .ok()
            .flatten()
            .is_some_and(|value| {
                matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on")
            });
        if flagged {
            return Ok(true);
        }
        // First-time / pre-flag installs: keep gateway up when AG is active.
        let active = manager.db.with_conn(|conn| {
            use crate::database::dao;
            use crate::provider::{ProviderKind, ProviderTarget};
            for target in [
                ProviderTarget::ClaudeCode,
                ProviderTarget::ClaudeDesktop,
                ProviderTarget::Codex,
                ProviderTarget::OpenCode,
            ] {
                if let Some(provider) = dao::get_current_provider(conn, target)? {
                    if provider.provider_kind == ProviderKind::Antigravity {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })?;
        Ok(active)
    })
    .unwrap_or(false);
    if !should_start {
        return;
    }
    match start_gateway(None).await {
        Ok(status) => log::info!(
            "Antigravity gateway auto-restored: running={} port={}",
            status.running,
            status.port
        ),
        Err(error) => log::error!("Antigravity gateway auto-restore failed: {error}"),
    }
}

pub fn is_gateway_enabled(db: &Database) -> bool {
    db.with_conn(|conn| get_setting(conn, ENABLED_SETTING))
        .ok()
        .flatten()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
}

pub fn builtin_base_url() -> String {
    let port = lock_manager()
        .as_ref()
        .map(|manager| {
            manager
                .runtime
                .as_ref()
                .map(|runtime| runtime.port)
                .unwrap_or_else(|| saved_port(&manager.db))
        })
        .unwrap_or(DEFAULT_GATEWAY_PORT);
    format!("http://127.0.0.1:{port}")
}

/// Drop in-memory session→account bindings so a newly marked active account
/// is used on the next request instead of a previous sticky session.
pub fn clear_sticky_sessions() {
    let slot = lock_manager();
    if let Some(manager) = slot.as_ref() {
        manager.pool.clear_sticky();
    }
}

pub fn builtin_api_key() -> String {
    lock_manager()
        .as_ref()
        .and_then(|manager| manager.api_key.lock().ok().map(|guard| guard.clone()))
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
