//! Optional out-of-process host for the Antigravity / Kiro reverse-proxy gateways.
//!
//! The sidecar is this same executable started with `--gateway-sidecar`, so
//! version skew is impossible by construction. The main process only supervises
//! it (start, health, backoff restart, graceful stop); listeners, ports, upstream
//! ids and usage logging are unchanged. Design: `docs/smart-gateway/sidecar-design.md`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

/// `inproc` (default) or `sidecar`. Changing it takes effect after an app restart.
pub const RUNTIME_MODE_SETTING: &str = "gateway_runtime_mode";
pub const MODE_INPROC: &str = "inproc";
pub const MODE_SIDECAR: &str = "sidecar";

const SIDECAR_FLAG: &str = "--gateway-sidecar";
const VERSION_FLAG: &str = "--sidecar-version";
const LOG_FILE: &str = "gateway-sidecar.log";
const LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
const VERSION_MISMATCH_EXIT: i32 = 3;

const HEALTH_INTERVAL: Duration = Duration::from_secs(3);
const GRACEFUL_STOP: Duration = Duration::from_secs(5);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const STABLE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayKind {
    Antigravity,
    Kiro,
}

impl GatewayKind {
    fn label(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Kiro => "Kiro",
        }
    }

    fn configured_port(self) -> u16 {
        match self {
            Self::Antigravity => crate::antigravity::gateway::configured_port(),
            Self::Kiro => crate::kiro::gateway::configured_port(),
        }
    }

    fn enabled(self) -> bool {
        match self {
            Self::Antigravity => crate::antigravity::gateway::enabled_flag(),
            Self::Kiro => crate::kiro::gateway::enabled_flag(),
        }
    }
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

pub fn read_mode(db: &Database) -> String {
    let raw = db
        .with_conn(|conn| get_setting(conn, RUNTIME_MODE_SETTING))
        .ok()
        .flatten();
    normalize_mode(raw.as_deref()).to_string()
}

fn normalize_mode(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim) {
        Some(MODE_SIDECAR) => MODE_SIDECAR,
        _ => MODE_INPROC,
    }
}

pub fn write_mode(db: &Database, mode: &str) -> AppResult<String> {
    let normalized = match mode.trim() {
        MODE_SIDECAR => MODE_SIDECAR,
        MODE_INPROC => MODE_INPROC,
        other => return Err(AppError::Config(format!("未知的网关运行方式: {other}"))),
    };
    db.with_conn(|conn| set_setting(conn, RUNTIME_MODE_SETTING, normalized))?;
    Ok(normalized.to_string())
}

// ---------------------------------------------------------------------------
// Main-process side: supervisor
// ---------------------------------------------------------------------------

struct Supervisor {
    restart: Notify,
    stopping: AtomicBool,
    ag_up: AtomicBool,
    kiro_up: AtomicBool,
}

static SUPERVISOR: OnceLock<Arc<Supervisor>> = OnceLock::new();

impl Supervisor {
    fn flag(&self, kind: GatewayKind) -> &AtomicBool {
        match kind {
            GatewayKind::Antigravity => &self.ag_up,
            GatewayKind::Kiro => &self.kiro_up,
        }
    }
}

/// True in the main process once the supervisor owns the gateways. Always false
/// inside the sidecar itself, so its own `start_gateway` runs the real listener.
pub fn delegated() -> bool {
    SUPERVISOR.get().is_some()
}

pub fn gateway_up(kind: GatewayKind) -> bool {
    SUPERVISOR
        .get()
        .is_some_and(|supervisor| supervisor.flag(kind).load(Ordering::Acquire))
}

/// Ask the sidecar to restart so it re-reads gateway settings. No-op in-process.
pub fn request_restart() {
    if let Some(supervisor) = SUPERVISOR.get() {
        supervisor.restart.notify_one();
    }
}

/// Settings written by the main process only take effect inside the sidecar after a restart.
pub fn notify_settings_changed() {
    request_restart();
}

/// Poll the gateway port until it answers `/healthz` or the timeout elapses.
pub async fn wait_until_up(kind: GatewayKind, timeout: Duration) -> bool {
    let Some(supervisor) = SUPERVISOR.get().cloned() else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        let port = kind.configured_port();
        let up = tokio::task::spawn_blocking(move || probe_health(port))
            .await
            .unwrap_or(false);
        supervisor.flag(kind).store(up, Ordering::Release);
        if up || Instant::now() >= deadline {
            return up;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Mark a gateway as stopped right after the sidecar was told to drop it.
pub fn mark_down(kind: GatewayKind) {
    if let Some(supervisor) = SUPERVISOR.get() {
        supervisor.flag(kind).store(false, Ordering::Release);
    }
}

/// Start supervising the sidecar. Call once from app setup when mode is `sidecar`.
pub fn start_supervisor(db: Arc<Database>) {
    let supervisor = Arc::new(Supervisor {
        restart: Notify::new(),
        stopping: AtomicBool::new(false),
        ag_up: AtomicBool::new(false),
        kiro_up: AtomicBool::new(false),
    });
    if SUPERVISOR.set(Arc::clone(&supervisor)).is_err() {
        return;
    }
    // Installs that never set the flag but had an Antigravity provider active used
    // to be restored in-process at launch; persist that decision for the sidecar.
    if crate::antigravity::gateway::should_autostart() {
        crate::antigravity::gateway::set_enabled_flag(true);
    }
    tauri::async_runtime::spawn(supervise(supervisor, db));
}

/// Stop the sidecar on app exit (blocking, bounded by the graceful window).
pub fn shutdown() {
    if let Some(supervisor) = SUPERVISOR.get() {
        supervisor.stopping.store(true, Ordering::Release);
        supervisor.restart.notify_one();
        // The supervisor task stops the child; give it a moment before the process exits.
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn log_path() -> PathBuf {
    crate::config::get_app_config_dir().join(LOG_FILE)
}

fn open_log() -> Stdio {
    let path = log_path();
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null())
}

fn spawn_child() -> AppResult<Child> {
    let exe = std::env::current_exe()
        .map_err(|error| AppError::Io(format!("无法定位自身可执行文件: {error}")))?;
    let mut command = Command::new(exe);
    command
        .arg(SIDECAR_FLAG)
        .arg(VERSION_FLAG)
        .arg(env!("CARGO_PKG_VERSION"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(open_log());
    crate::process_util::apply_no_window(&mut command);
    command
        .spawn()
        .map_err(|error| AppError::Io(format!("启动网关 sidecar 失败: {error}")))
}

/// Closing the child's stdin is the shutdown signal (also fires if we crash).
fn stop_child(mut child: Child) {
    drop(child.stdin.take());
    let deadline = Instant::now() + GRACEFUL_STOP;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    log::warn!("网关 sidecar 未在 {}s 内退出，强制结束", GRACEFUL_STOP.as_secs());
    let _ = child.kill();
    let _ = child.wait();
}

enum Exit {
    ChildExited,
    RestartRequested,
    Stopping,
}

fn any_enabled() -> bool {
    [GatewayKind::Antigravity, GatewayKind::Kiro]
        .into_iter()
        .any(GatewayKind::enabled)
}

async fn supervise(supervisor: Arc<Supervisor>, db: Arc<Database>) {
    let mut backoff = BACKOFF_MIN;
    let mut last_log_id = latest_log_id(&db);
    loop {
        if supervisor.stopping.load(Ordering::Acquire) {
            return;
        }
        if !any_enabled() {
            // Nothing to host; wait for a start request instead of idling a process.
            tokio::select! {
                _ = supervisor.restart.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
            continue;
        }
        let mut child = match spawn_child() {
            Ok(child) => child,
            Err(error) => {
                log::error!("{error}");
                tokio::select! {
                    _ = supervisor.restart.notified() => {}
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        log::info!("网关 sidecar 已启动 pid={}", child.id());
        let started = Instant::now();
        let outcome = monitor(&supervisor, &db, &mut child, &mut last_log_id).await;
        for kind in [GatewayKind::Antigravity, GatewayKind::Kiro] {
            supervisor.flag(kind).store(false, Ordering::Release);
        }
        match outcome {
            Exit::ChildExited => {
                let code = child.try_wait().ok().flatten().and_then(|status| status.code());
                if code == Some(VERSION_MISMATCH_EXIT) {
                    log::error!("网关 sidecar 版本与主程序不一致，已拒绝拉起");
                    return;
                }
                log::warn!("网关 sidecar 异常退出 code={code:?}，{}s 后重启", backoff.as_secs());
                if started.elapsed() >= STABLE_AFTER {
                    backoff = BACKOFF_MIN;
                }
                tokio::select! {
                    _ = supervisor.restart.notified() => {}
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
            Exit::RestartRequested => {
                let _ = tokio::task::spawn_blocking(move || stop_child(child)).await;
                backoff = BACKOFF_MIN;
            }
            Exit::Stopping => {
                let _ = tokio::task::spawn_blocking(move || stop_child(child)).await;
                return;
            }
        }
    }
}

async fn monitor(
    supervisor: &Arc<Supervisor>,
    db: &Arc<Database>,
    child: &mut Child,
    last_log_id: &mut i64,
) -> Exit {
    let mut next_probe = Instant::now();
    loop {
        tokio::select! {
            _ = supervisor.restart.notified() => {
                return if supervisor.stopping.load(Ordering::Acquire) {
                    Exit::Stopping
                } else {
                    Exit::RestartRequested
                };
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
        if supervisor.stopping.load(Ordering::Acquire) {
            return Exit::Stopping;
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Exit::ChildExited;
        }
        if Instant::now() < next_probe {
            continue;
        }
        next_probe = Instant::now() + HEALTH_INTERVAL;
        for kind in [GatewayKind::Antigravity, GatewayKind::Kiro] {
            let port = kind.configured_port();
            let up = tokio::task::spawn_blocking(move || probe_health(port))
                .await
                .unwrap_or(false);
            let was = supervisor.flag(kind).swap(up, Ordering::AcqRel);
            if was != up {
                log::info!("{} 网关健康状态: {}", kind.label(), if up { "up" } else { "down" });
            }
        }
        // The sidecar writes request logs; tell the UI when new rows appeared.
        let db_for_logs = Arc::clone(db);
        let latest = tokio::task::spawn_blocking(move || latest_log_id(&db_for_logs))
            .await
            .unwrap_or(*last_log_id);
        if latest != *last_log_id {
            *last_log_id = latest;
            crate::usage_events::notify_log_recorded();
        }
    }
}

fn latest_log_id(db: &Database) -> i64 {
    db.with_read_conn(|conn| {
        conn.query_row("SELECT COALESCE(MAX(rowid), 0) FROM proxy_request_logs", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(AppError::from)
    })
    .unwrap_or(0)
}

/// Minimal HTTP/1.0 probe; avoids pulling the system proxy into a loopback check.
fn probe_health(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));
    if stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 32];
    let Ok(read) = stream.read(&mut head) else {
        return false;
    };
    let head = &head[..read];
    head.starts_with(b"HTTP/1.") && head.windows(4).any(|window| window == b" 200")
}

// ---------------------------------------------------------------------------
// Sidecar-process side
// ---------------------------------------------------------------------------

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

/// Entry hook for `main`: runs the sidecar and returns true when requested.
pub fn run_sidecar_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == SIDECAR_FLAG) {
        return false;
    }
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
    let expected = args
        .iter()
        .position(|arg| arg == VERSION_FLAG)
        .and_then(|index| args.get(index + 1));
    if expected.map(String::as_str) != Some(env!("CARGO_PKG_VERSION")) {
        eprintln!(
            "sidecar version mismatch: parent={expected:?} self={}",
            env!("CARGO_PKG_VERSION")
        );
        std::process::exit(VERSION_MISMATCH_EXIT);
    }
    match run_sidecar() {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("gateway sidecar failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run_sidecar() -> AppResult<()> {
    let config_dir = crate::config::get_app_config_dir();
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| AppError::Io(format!("创建数据目录失败: {error}")))?;
    log::info!("gateway sidecar starting; data directory: {}", config_dir.display());

    // Stores build blocking reqwest clients, which must not first happen on an async task.
    crate::antigravity::thought_sig::init_early();
    let db = Arc::new(Database::init()?);
    crate::antigravity::gateway::init_gateway(Arc::clone(&db));
    crate::kiro::gateway::init_gateway(Arc::clone(&db));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::Other(format!("创建运行时失败: {error}")))?;
    runtime.block_on(async {
        // Only the persisted flag decides here; the main process migrates legacy installs.
        if crate::antigravity::gateway::enabled_flag() {
            if let Err(error) = crate::antigravity::gateway::start_gateway(None).await {
                log::error!("Antigravity 网关启动失败: {error}");
            }
        }
        if crate::kiro::gateway::enabled_flag() {
            if let Err(error) = crate::kiro::gateway::start_gateway(None).await {
                log::error!("Kiro 网关启动失败: {error}");
            }
        }
        tokio::spawn(quota_refresh_loop());

        // The parent holds our stdin open; EOF means "stop" (or the parent died).
        let _ = tokio::task::spawn_blocking(|| {
            let mut sink = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut sink);
        })
        .await;
        log::info!("gateway sidecar stdin closed; shutting down");
        let _ = crate::antigravity::gateway::stop_gateway_keep_flag().await;
        let _ = crate::kiro::gateway::stop_gateway_keep_flag().await;
    });
    crate::antigravity::thought_sig::shutdown();
    // The main process owns WAL checkpointing; just release our handle.
    drop(db);
    // Skip runtime teardown waiting on the blocking stdin reader.
    runtime.shutdown_background();
    Ok(())
}

/// The main process skips its own quota refresh in sidecar mode so only one
/// process refreshes tokens and writes quota snapshots.
async fn quota_refresh_loop() {
    use crate::antigravity::{try_refresh_all_quotas, QUOTA_REFRESH_INTERVAL_SECS};
    tokio::time::sleep(Duration::from_secs(20)).await;
    let interval = Duration::from_secs(QUOTA_REFRESH_INTERVAL_SECS);
    loop {
        match try_refresh_all_quotas().await {
            Ok(summary) if summary.attempted > 0 => log::info!(
                "Antigravity quota auto-refresh: attempted={}, ok={}, failed={}",
                summary.attempted,
                summary.succeeded,
                summary.failed
            ),
            Ok(_) => {}
            Err(error) => log::warn!("Antigravity quota auto-refresh failed: {error}"),
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_to_inproc() {
        assert_eq!(normalize_mode(None), MODE_INPROC);
        assert_eq!(normalize_mode(Some("")), MODE_INPROC);
        assert_eq!(normalize_mode(Some("garbage")), MODE_INPROC);
        assert_eq!(normalize_mode(Some(" sidecar ")), MODE_SIDECAR);
    }

    #[test]
    fn probe_health_reports_closed_port_as_down() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!probe_health(port));
    }

    #[test]
    fn probe_health_accepts_http_200() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 64];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}");
            }
        });
        assert!(probe_health(port));
    }
}
