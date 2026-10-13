//! Optional out-of-process host for the Antigravity / Kiro reverse-proxy gateways.
//!
//! The sidecar is this same executable started with `--gateway-sidecar`, so
//! version skew is impossible by construction. The main process only supervises
//! it (start, health, backoff restart, graceful stop); listeners, ports, upstream
//! ids and usage logging are unchanged. Design: `docs/smart-gateway/sidecar-design.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

/// `sidecar` (default) or `inproc`. Changing it takes effect after an app restart.
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
const COMMIT_MAX_BYTES: usize = 8 * 1024 * 1024;
const SELFTEST_ENV: &str = "AISW_SUPERVISOR_SELFTEST";

/// Account snapshots the main process sends over the sidecar's stdin.
pub const STORE_AG: &str = "ag";
pub const STORE_KIRO: &str = "kiro";
pub const STORE_KIRO_MODELS: &str = "kiro-models";

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
        Some(MODE_INPROC) => MODE_INPROC,
        _ => MODE_SIDECAR,
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
    running: AtomicBool,
    pid: AtomicU32,
    restarts: AtomicU32,
    exit_known: AtomicBool,
    exit_code_present: AtomicBool,
    last_exit_code: AtomicI32,
    backoff_ms: AtomicU64,
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
        running: AtomicBool::new(false),
        pid: AtomicU32::new(0),
        restarts: AtomicU32::new(0),
        exit_known: AtomicBool::new(false),
        exit_code_present: AtomicBool::new(false),
        last_exit_code: AtomicI32::new(0),
        backoff_ms: AtomicU64::new(BACKOFF_MIN.as_millis() as u64),
    });
    if SUPERVISOR.set(Arc::clone(&supervisor)).is_err() {
        return;
    }
    // Installs that never set the flag but had an Antigravity provider active used
    // to be restored in-process at launch; persist that decision for the sidecar.
    if crate::antigravity::gateway::should_autostart() {
        crate::antigravity::gateway::set_enabled_flag(true);
    }
    spawn_supervise(supervise(supervisor, db));
}

fn spawn_supervise(task: impl std::future::Future<Output = ()> + Send + 'static) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(task);
        return;
    }
    tauri::async_runtime::spawn(task);
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

struct ChildSession {
    child: Child,
    stop_io: Arc<AtomicBool>,
    io_thread: Option<std::thread::JoinHandle<()>>,
}

fn spawn_child() -> AppResult<ChildSession> {
    let exe = std::env::current_exe()
        .map_err(|error| AppError::Io(format!("无法定位自身可执行文件: {error}")))?;
    let mut command = Command::new(exe);
    command
        .arg(SIDECAR_FLAG)
        .arg(VERSION_FLAG)
        .arg(env!("CARGO_PKG_VERSION"))
        .env_remove(SELFTEST_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(open_log());
    crate::process_util::apply_no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| AppError::Io(format!("启动网关 sidecar 失败: {error}")))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Io("网关 sidecar 缺少 stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Io("网关 sidecar 缺少 stdout".into()))?;
    let stop_io = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop_io);
    let io_thread = std::thread::spawn(move || admin_loop(stdin, stdout, stop_for_thread));
    Ok(ChildSession {
        child,
        stop_io,
        io_thread: Some(io_thread),
    })
}

fn release_io(session: &mut ChildSession) {
    session.stop_io.store(true, Ordering::Release);
    if let Some(thread) = session.io_thread.take() {
        let _ = thread.join();
    }
}

/// Closing the child's stdin is the shutdown signal (also fires if we crash).
fn stop_session(mut session: ChildSession) {
    release_io(&mut session);
    let deadline = Instant::now() + GRACEFUL_STOP;
    while Instant::now() < deadline {
        if matches!(session.child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    log::warn!(
        "网关 sidecar 未在 {}s 内退出，强制结束",
        GRACEFUL_STOP.as_secs()
    );
    let _ = session.child.kill();
    let _ = session.child.wait();
}

/// Sleep duration and the backoff stored for the following crash.
/// A run that lasted `STABLE_AFTER` resets the sleep to the minimum, then the next crash doubles it.
fn next_backoff(stable: bool, current: Duration) -> (Duration, Duration) {
    let sleep_for = if stable { BACKOFF_MIN } else { current };
    let next = (sleep_for * 2).min(BACKOFF_MAX);
    (sleep_for, next)
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
        let mut session = match spawn_child() {
            Ok(session) => session,
            Err(error) => {
                log::error!("{error}");
                supervisor.running.store(false, Ordering::Release);
                supervisor.pid.store(0, Ordering::Release);
                supervisor
                    .backoff_ms
                    .store(backoff.as_millis() as u64, Ordering::Release);
                tokio::select! {
                    _ = supervisor.restart.notified() => {}
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        let pid = session.child.id();
        supervisor.pid.store(pid, Ordering::Release);
        supervisor.running.store(true, Ordering::Release);
        log::info!("网关 sidecar 已启动 pid={pid}");
        let started = Instant::now();
        let outcome = monitor(&supervisor, &db, &mut session.child, &mut last_log_id).await;
        supervisor.running.store(false, Ordering::Release);
        supervisor.pid.store(0, Ordering::Release);
        for kind in [GatewayKind::Antigravity, GatewayKind::Kiro] {
            supervisor.flag(kind).store(false, Ordering::Release);
        }
        match outcome {
            Exit::ChildExited => {
                release_io(&mut session);
                let code = session
                    .child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|status| status.code());
                let stable = started.elapsed() >= STABLE_AFTER;
                let (sleep_for, next) = next_backoff(stable, backoff);
                note_child_exit(&supervisor, code, sleep_for);
                if code == Some(VERSION_MISMATCH_EXIT) {
                    log::error!("网关 sidecar 版本与主程序不一致，已拒绝拉起");
                    return;
                }
                log::warn!(
                    "网关 sidecar 异常退出 code={code:?}，{}s 后重启",
                    sleep_for.as_secs()
                );
                tokio::select! {
                    _ = supervisor.restart.notified() => {}
                    _ = tokio::time::sleep(sleep_for) => {}
                }
                backoff = next;
            }
            Exit::RestartRequested => {
                let _ = tokio::task::spawn_blocking(move || stop_session(session)).await;
                backoff = BACKOFF_MIN;
                supervisor
                    .backoff_ms
                    .store(backoff.as_millis() as u64, Ordering::Release);
            }
            Exit::Stopping => {
                let _ = tokio::task::spawn_blocking(move || stop_session(session)).await;
                return;
            }
        }
    }
}

fn note_child_exit(supervisor: &Supervisor, code: Option<i32>, sleep_for: Duration) {
    supervisor.restarts.fetch_add(1, Ordering::AcqRel);
    supervisor.exit_known.store(true, Ordering::Release);
    match code {
        Some(code) => {
            supervisor.exit_code_present.store(true, Ordering::Release);
            supervisor.last_exit_code.store(code, Ordering::Release);
        }
        None => supervisor.exit_code_present.store(false, Ordering::Release),
    }
    supervisor
        .backoff_ms
        .store(sleep_for.as_millis() as u64, Ordering::Release);
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
                log::info!(
                    "{} 网关健康状态: {}",
                    kind.label(),
                    if up { "up" } else { "down" }
                );
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
        conn.query_row(
            "SELECT COALESCE(MAX(rowid), 0) FROM proxy_request_logs",
            [],
            |row| row.get::<_, i64>(0),
        )
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
    log::info!(
        "gateway sidecar starting; data directory: {}",
        config_dir.display()
    );

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

        // The parent holds our stdin open. JSON lines are account commits;
        // EOF means "stop" (or the parent died).
        let _ = tokio::task::spawn_blocking(|| {
            let stdin = std::io::stdin();
            let mut input = BufReader::new(stdin.lock());
            let mut output = std::io::stdout();
            let mut line = String::new();
            loop {
                line.clear();
                match input.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let response = handle_admin_line(line.trim());
                        if writeln!(output, "{response}").is_err() || output.flush().is_err() {
                            break;
                        }
                    }
                }
            }
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

// ---------------------------------------------------------------------------
// Account commit channel (stdin / stdout JSON lines, no extra port)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBase {
    pub exists: bool,
    pub mtime_ns: String,
    pub len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminRequest {
    id: u64,
    op: String,
    store: String,
    body: String,
    #[serde(default)]
    base: Option<FileBase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminResponse {
    id: u64,
    ok: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    mtime_ns: String,
    #[serde(default)]
    len: u64,
}

#[derive(Debug, Clone)]
pub struct CommitReceipt {
    pub mtime_ns: String,
    pub len: u64,
}

struct AdminJob {
    store: String,
    body: String,
    base: Option<FileBase>,
    reply: std::sync::mpsc::Sender<Result<CommitReceipt, String>>,
}

struct AdminTx {
    generation: u64,
    tx: std::sync::mpsc::Sender<AdminJob>,
}

static ADMIN_TX: Mutex<Option<AdminTx>> = Mutex::new(None);
static ADMIN_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Send an account snapshot to the sidecar and wait until that process has written it.
pub fn commit_store(store: &str, body: &str, base: Option<FileBase>) -> AppResult<CommitReceipt> {
    if body.len() > COMMIT_MAX_BYTES {
        return Err(AppError::Config("账号快照超过 8MB".into()));
    }
    let tx = {
        let slot = ADMIN_TX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slot.as_ref().map(|current| current.tx.clone())
    };
    let Some(tx) = tx else {
        return Err(AppError::Other("反代进程未在运行，账号更改未保存".into()));
    };
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(AdminJob {
        store: store.to_string(),
        body: body.to_string(),
        base,
        reply: reply_tx,
    })
    .map_err(|_| AppError::Other("反代进程管理通道已关闭".into()))?;
    match reply_rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(receipt)) => Ok(receipt),
        Ok(Err(error)) => Err(AppError::Other(error)),
        Err(_) => Err(AppError::Other("反代进程管理接口超时".into())),
    }
}

/// Commit helper used by account stores: on failure the next read reloads from disk.
pub fn commit_account_file(
    store: &str,
    raw: &str,
    disk: &crate::file_stamp::FileStamp,
) -> AppResult<()> {
    let observed = disk.observed();
    match commit_store(
        store,
        raw,
        Some(FileBase {
            exists: observed.exists,
            mtime_ns: observed.mtime_ns,
            len: observed.len,
        }),
    ) {
        Ok(receipt) => {
            if receipt.mtime_ns.is_empty() {
                disk.force_reload();
            } else {
                disk.note_signature(&receipt.mtime_ns, receipt.len);
            }
            Ok(())
        }
        Err(error) => {
            disk.force_reload();
            Err(error)
        }
    }
}

fn install_admin(generation: u64, tx: std::sync::mpsc::Sender<AdminJob>) {
    let mut slot = ADMIN_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(AdminTx { generation, tx });
}

fn clear_admin(generation: u64) {
    let mut slot = ADMIN_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if slot
        .as_ref()
        .is_some_and(|current| current.generation == generation)
    {
        *slot = None;
    }
}

fn admin_loop(mut stdin: ChildStdin, stdout: ChildStdout, stop: Arc<AtomicBool>) {
    let generation = ADMIN_GENERATION.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<AdminJob>();
    install_admin(generation, tx);
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if line_tx.send(line.trim().to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut next_id = 1u64;
    while !stop.load(Ordering::Acquire) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(job) => {
                if !dispatch_admin(&mut stdin, &line_rx, &stop, &mut next_id, job) {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    clear_admin(generation);
    drop(stdin);
}

fn dispatch_admin(
    stdin: &mut ChildStdin,
    line_rx: &std::sync::mpsc::Receiver<String>,
    stop: &AtomicBool,
    next_id: &mut u64,
    job: AdminJob,
) -> bool {
    let id = *next_id;
    *next_id = next_id.wrapping_add(1);
    let request = AdminRequest {
        id,
        op: "commit".into(),
        store: job.store,
        body: job.body,
        base: job.base,
    };
    let encoded = match serde_json::to_string(&request) {
        Ok(line) => line,
        Err(error) => {
            let _ = job.reply.send(Err(error.to_string()));
            return true;
        }
    };
    if writeln!(stdin, "{encoded}").is_err() || stdin.flush().is_err() {
        let _ = job.reply.send(Err("反代进程管理通道写入失败".into()));
        return false;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if stop.load(Ordering::Acquire) {
            let _ = job.reply.send(Err("反代进程正在停止".into()));
            return false;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = job.reply.send(Err("反代进程管理接口超时".into()));
            return true;
        }
        match line_rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(line) => {
                let parsed: AdminResponse = match serde_json::from_str(&line) {
                    Ok(parsed) => parsed,
                    Err(_) => continue,
                };
                if parsed.id != id {
                    continue;
                }
                let result = if parsed.ok {
                    Ok(CommitReceipt {
                        mtime_ns: parsed.mtime_ns,
                        len: parsed.len,
                    })
                } else {
                    Err(if parsed.error.is_empty() {
                        "反代进程拒绝了账号快照".into()
                    } else {
                        parsed.error
                    })
                };
                let _ = job.reply.send(result);
                return true;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = job.reply.send(Err("反代进程管理通道已关闭".into()));
                return false;
            }
        }
    }
}

fn handle_admin_line(line: &str) -> String {
    if line.is_empty() {
        return admin_reply(0, Err("空的管理请求".into()));
    }
    let request: AdminRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => return admin_reply(0, Err(format!("无效的管理请求: {error}"))),
    };
    if request.body.len() > COMMIT_MAX_BYTES {
        return admin_reply(request.id, Err("账号快照超过 8MB".into()));
    }
    let result = match request.op.as_str() {
        "commit" => apply_commit(&request),
        _ => Err(format!("未知操作: {}", request.op)),
    };
    admin_reply(request.id, result)
}

fn apply_commit(request: &AdminRequest) -> Result<(String, u64), String> {
    match request.store.as_str() {
        STORE_AG | STORE_KIRO => {
            let Some(base) = request.base.as_ref() else {
                return Err("缺少账号文件基线".into());
            };
            let applied = if request.store == STORE_AG {
                crate::antigravity::account::apply_committed_snapshot(
                    &request.body,
                    base.exists,
                    &base.mtime_ns,
                    base.len,
                )
            } else {
                crate::kiro::account::apply_committed_snapshot(
                    &request.body,
                    base.exists,
                    &base.mtime_ns,
                    base.len,
                )
            };
            applied.map_err(|error| error.to_string())
        }
        STORE_KIRO_MODELS => crate::kiro::models::apply_committed_snapshot(&request.body)
            .map_err(|error| error.to_string()),
        other => Err(format!("未知账号库: {other}")),
    }
}

fn admin_reply(id: u64, result: Result<(String, u64), String>) -> String {
    let response = match result {
        Ok((mtime_ns, len)) => AdminResponse {
            id,
            ok: true,
            error: String::new(),
            mtime_ns,
            len,
        },
        Err(error) => AdminResponse {
            id,
            ok: false,
            error,
            mtime_ns: String::new(),
            len: 0,
        },
    };
    serde_json::to_string(&response)
        .unwrap_or_else(|_| format!(r#"{{"id":{id},"ok":false,"error":"无法编码管理响应"}}"#))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub mode: String,
    pub supervised: bool,
    pub running: bool,
    pub pid: u32,
    pub antigravity_up: bool,
    pub kiro_up: bool,
    pub restarts: u32,
    pub last_exit_code: Option<i32>,
    pub backoff_ms: u64,
}

pub fn status(db: &Database) -> SidecarStatus {
    let mode = read_mode(db);
    let Some(supervisor) = SUPERVISOR.get() else {
        return SidecarStatus {
            mode,
            supervised: false,
            running: false,
            pid: 0,
            antigravity_up: false,
            kiro_up: false,
            restarts: 0,
            last_exit_code: None,
            backoff_ms: 0,
        };
    };
    let last_exit_code = (supervisor.exit_known.load(Ordering::Acquire)
        && supervisor.exit_code_present.load(Ordering::Acquire))
    .then(|| supervisor.last_exit_code.load(Ordering::Acquire));
    SidecarStatus {
        mode,
        supervised: true,
        running: supervisor.running.load(Ordering::Acquire),
        pid: supervisor.pid.load(Ordering::Acquire),
        antigravity_up: supervisor.ag_up.load(Ordering::Acquire),
        kiro_up: supervisor.kiro_up.load(Ordering::Acquire),
        restarts: supervisor.restarts.load(Ordering::Acquire),
        last_exit_code,
        backoff_ms: supervisor.backoff_ms.load(Ordering::Acquire),
    }
}

/// `AISW_SUPERVISOR_SELFTEST=1` plus an isolated home: launch, kill, restart, stop.
/// Checked only when `--gateway-sidecar` did not claim the process.
pub fn run_supervisor_selftest_if_requested() -> bool {
    if std::env::var(SELFTEST_ENV).ok().as_deref() != Some("1") {
        return false;
    }
    match supervisor_selftest() {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("supervisor selftest failed: {error}");
            std::process::exit(1);
        }
    }
}

fn supervisor_selftest() -> Result<(), String> {
    if std::env::var(crate::config::paths::TEST_HOME_ENV).is_err() {
        return Err("AISW_TEST_HOME is required".into());
    }
    if std::env::var("AISW_ALLOW_TEST_HOME").ok().as_deref() != Some("1") {
        return Err("AISW_ALLOW_TEST_HOME=1 is required".into());
    }
    let db = Arc::new(Database::init().map_err(|error| error.to_string())?);
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("bind free port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    db.with_conn(|conn| {
        set_setting(conn, "kiro_gateway_enabled", "1")?;
        set_setting(conn, "kiro_gateway_port", &port.to_string())
    })
    .map_err(|error| error.to_string())?;
    crate::kiro::gateway::init_gateway(Arc::clone(&db));
    crate::antigravity::gateway::init_gateway(Arc::clone(&db));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        start_supervisor(Arc::clone(&db));
        if !wait_until(Duration::from_secs(40), || probe_health(port)).await {
            return Err(format!("kiro gateway never healthy on {port}"));
        }
        let pid = status(&db).pid;
        if pid == 0 {
            return Err("supervisor did not record a running sidecar".into());
        }
        let snapshot = r#"{"accounts":[],"activeId":"supervisor-selftest"}"#;
        let accounts_path = crate::config::get_app_config_dir().join("kiro_accounts.json");
        // Gateway startup clears cooldowns and writes the file, so the baseline
        // is whatever is on disk now, not "missing".
        let base = match std::fs::metadata(&accounts_path) {
            Ok(meta) => {
                let mtime_ns = meta
                    .modified()
                    .map_err(|error| error.to_string())?
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_string();
                FileBase {
                    exists: true,
                    mtime_ns,
                    len: meta.len(),
                }
            }
            Err(_) => FileBase {
                exists: false,
                mtime_ns: "0".into(),
                len: 0,
            },
        };
        commit_store(STORE_KIRO, snapshot, Some(base)).map_err(|error| error.to_string())?;
        let written = std::fs::read_to_string(&accounts_path).map_err(|error| error.to_string())?;
        if written != snapshot {
            return Err(format!(
                "sidecar did not write the account snapshot: {written}"
            ));
        }
        kill_pid(pid)?;
        let restarted = wait_until(Duration::from_secs(25), || {
            let snap = status(&db);
            snap.restarts >= 1 && snap.running && probe_health(port)
        })
        .await;
        if !restarted {
            let snap = status(&db);
            return Err(format!(
                "sidecar did not restart: restarts={} running={} pid={} exit={:?}",
                snap.restarts, snap.running, snap.pid, snap.last_exit_code
            ));
        }
        shutdown();
        if !wait_until(Duration::from_secs(10), || !probe_health(port)).await {
            return Err("port still answering after shutdown".into());
        }
        Ok(())
    })
}

async fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn kill_pid(pid: u32) -> Result<(), String> {
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status()
    } else {
        Command::new("kill").args(["-9", &pid.to_string()]).status()
    }
    .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("failed to kill sidecar pid {pid}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_to_sidecar() {
        assert_eq!(normalize_mode(None), MODE_SIDECAR);
        assert_eq!(normalize_mode(Some("")), MODE_SIDECAR);
        assert_eq!(normalize_mode(Some("garbage")), MODE_SIDECAR);
        assert_eq!(normalize_mode(Some(" sidecar ")), MODE_SIDECAR);
        assert_eq!(normalize_mode(Some(" inproc ")), MODE_INPROC);
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

    #[test]
    fn backoff_resets_after_a_stable_run_then_doubles() {
        let (sleep_for, next) = next_backoff(true, Duration::from_secs(16));
        assert_eq!(sleep_for, BACKOFF_MIN);
        assert_eq!(next, Duration::from_secs(2));
    }

    #[test]
    fn backoff_doubles_until_the_cap() {
        let mut current = BACKOFF_MIN;
        for _ in 0..8 {
            let (sleep_for, next) = next_backoff(false, current);
            assert_eq!(sleep_for, current);
            current = next;
        }
        assert_eq!(current, BACKOFF_MAX);
        let (sleep_for, next) = next_backoff(false, current);
        assert_eq!(sleep_for, BACKOFF_MAX);
        assert_eq!(next, BACKOFF_MAX);
    }

    #[test]
    fn admin_line_rejects_an_unknown_op() {
        let response: serde_json::Value = serde_json::from_str(&handle_admin_line(
            r#"{"id":7,"op":"nope","store":"ag","body":"{}"}"#,
        ))
        .unwrap();
        assert_eq!(response["id"], 7);
        assert_eq!(response["ok"], false);
    }
}
