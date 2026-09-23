//! Second hop in front of the Antigravity outbound proxy.
//!
//! Reqwest accepts one proxy. A static residential exit is reached by a
//! localhost HTTP CONNECT forwarder: existing outbound first (Clash / system /
//! direct), then the residential gateway, then the real target. Hostnames are
//! sent to the proxy that should resolve them (SOCKS5 domain ATYP / HTTP
//! CONNECT), so DNS for Google does not leak to the local resolver.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use log::info;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpListener, TcpStream};
use tokio::time::timeout;

use crate::error::{AppError, AppResult};

const HOP_TIMEOUT: Duration = Duration::from_secs(10);
const HEADER_LIMIT: usize = 16 * 1024;
const FAIL_CLOSED_PROXY: &str = "http://127.0.0.1:1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyKind {
    Socks5,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyEndpoint {
    pub fn redacted(&self) -> String {
        let scheme = match self.kind {
            ProxyKind::Socks5 => "socks5",
            ProxyKind::Http => "http",
        };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    fn has_auth(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstHop {
    /// Dial the residential gateway from this machine.
    Direct,
    /// Dial the residential gateway through the existing outbound proxy.
    Proxy(ProxyEndpoint),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainConfig {
    pub first: FirstHop,
    pub exit: ProxyEndpoint,
}

impl ChainConfig {
    pub fn redacted_label(&self) -> String {
        match &self.first {
            FirstHop::Direct => format!("direct -> {}", self.exit.redacted()),
            FirstHop::Proxy(hop) => format!("{} -> {}", hop.redacted(), self.exit.redacted()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitProbe {
    pub ok: bool,
    pub ip: Option<String>,
    pub hop: Option<String>,
    pub message: String,
}

#[derive(Debug)]
enum HopFail {
    First(String),
    Second(String),
}

impl HopFail {
    fn name(&self) -> &'static str {
        match self {
            Self::First(_) => "first",
            Self::Second(_) => "second",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::First(detail) => format!("第一跳失败：连不上链式代理出口IP（{detail}）"),
            Self::Second(detail) => {
                format!("第二跳失败：链式代理出口IP已连通，但访问目标失败（{detail}）")
            }
        }
    }
}

/// `None` when the exit hop is off or the URL is blank — caller must not bind.
pub fn chain_from_parts(
    enabled: bool,
    exit_url: &str,
    first: FirstHop,
) -> AppResult<Option<ChainConfig>> {
    if !enabled || exit_url.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(ChainConfig {
        first,
        exit: parse_proxy_endpoint(exit_url)?,
    }))
}

pub fn parse_proxy_endpoint(value: &str) -> AppResult<ProxyEndpoint> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Config("代理地址不能为空".into()));
    }
    let parsed = url::Url::parse(value)
        .map_err(|_| AppError::Config(format!("代理地址无效: {}", strip_userinfo(value))))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    let kind = match scheme.as_str() {
        "socks5" | "socks5h" => ProxyKind::Socks5,
        "http" => ProxyKind::Http,
        other => {
            return Err(AppError::Config(format!(
                "不支持的代理协议 {other}，请用 socks5 或 http"
            )));
        }
    };
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| AppError::Config("代理地址缺少主机".into()))?
        .to_string();
    let port = match parsed.port() {
        Some(port) => port,
        None if kind == ProxyKind::Http => 80,
        None => return Err(AppError::Config("SOCKS5 地址必须包含端口".into())),
    };
    Ok(ProxyEndpoint {
        kind,
        host,
        port,
        username: percent_decode(parsed.username()),
        password: percent_decode(parsed.password().unwrap_or("")),
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn redact_proxy_url(value: &str) -> String {
    match parse_proxy_endpoint(value) {
        Ok(endpoint) => endpoint.redacted(),
        Err(_) => strip_userinfo(value),
    }
}

fn strip_userinfo(value: &str) -> String {
    let trimmed = value.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return trimmed.to_string();
    };
    let host = rest.split('@').next_back().unwrap_or(rest);
    format!("{scheme}://{host}")
}

struct ForwarderSlot {
    applied: Option<ChainConfig>,
    runtime: Option<ExitRuntime>,
    probe: Option<ExitProbe>,
    apply_error: Option<String>,
}

fn slot() -> &'static Mutex<ForwarderSlot> {
    static SLOT: OnceLock<Mutex<ForwarderSlot>> = OnceLock::new();
    SLOT.get_or_init(|| {
        Mutex::new(ForwarderSlot {
            applied: None,
            runtime: None,
            probe: None,
            apply_error: None,
        })
    })
}

fn lock_slot() -> std::sync::MutexGuard<'static, ForwarderSlot> {
    match slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn apply_chain(next: Option<ChainConfig>) -> AppResult<()> {
    let mut guard = lock_slot();
    if guard.applied == next
        && (next.is_none() || guard.runtime.is_some())
        && guard.apply_error.is_none()
    {
        return Ok(());
    }
    if let Some(cfg) = next.clone() {
        let label = cfg.redacted_label();
        // Bind the replacement before dropping the previous listener so a failed
        // save does not tear down a hop that is still working.
        let runtime = match ExitRuntime::start(cfg) {
            Ok(runtime) => runtime,
            Err(error) => {
                guard.apply_error = Some(error.to_string());
                return Err(error);
            }
        };
        info!(
            "Antigravity exit hop listening on {} ({label})",
            runtime.local_url()
        );
        guard.runtime = Some(runtime);
        guard.applied = next;
        guard.probe = None;
        guard.apply_error = None;
    } else {
        guard.runtime.take();
        guard.applied = None;
        guard.probe = None;
        guard.apply_error = None;
    }
    Ok(())
}

pub fn local_forwarder_url() -> Option<String> {
    lock_slot().runtime.as_ref().map(ExitRuntime::local_url)
}

pub fn fail_closed_proxy_url() -> &'static str {
    FAIL_CLOSED_PROXY
}

pub fn current_label() -> Option<String> {
    lock_slot()
        .applied
        .as_ref()
        .map(ChainConfig::redacted_label)
}

pub fn note_apply_error(message: String) {
    lock_slot().apply_error = Some(message);
}

pub fn apply_error() -> Option<String> {
    lock_slot().apply_error.clone()
}

pub fn last_probe() -> Option<ExitProbe> {
    lock_slot().probe.clone()
}

pub async fn probe_current() -> AppResult<ExitProbe> {
    let (cfg, local) = {
        let guard = lock_slot();
        let cfg = guard
            .applied
            .clone()
            .ok_or_else(|| AppError::Config("链式代理出口IP未启用".into()))?;
        let local = guard
            .runtime
            .as_ref()
            .map(ExitRuntime::local_url)
            .ok_or_else(|| AppError::Config("链式代理出口IP转发器未在监听".into()))?;
        (cfg, local)
    };
    let probe = run_probe(&cfg, &local).await;
    {
        let mut guard = lock_slot();
        guard.probe = Some(probe.clone());
    }
    Ok(probe)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitLatency {
    pub ok: bool,
    pub millis: Option<u64>,
    pub hop: Option<String>,
    pub message: String,
}

/// Probe one chain without turning it on for live traffic.
/// A short-lived forwarder is started and dropped; the active hop is left alone.
pub async fn probe_detached(cfg: ChainConfig) -> ExitProbe {
    let runtime = match open_detached(cfg.clone()).await {
        Ok(runtime) => runtime,
        Err(message) => {
            return ExitProbe {
                ok: false,
                ip: None,
                hop: None,
                message,
            };
        }
    };
    let local = runtime.local_url();
    let probe = run_probe(&cfg, &local).await;
    close_detached(runtime).await;
    probe
}

/// Time one HTTPS round trip through the chain. Does not enable it for live traffic.
pub async fn probe_latency_detached(cfg: ChainConfig) -> ExitLatency {
    let runtime = match open_detached(cfg.clone()).await {
        Ok(runtime) => runtime,
        Err(message) => {
            return ExitLatency {
                ok: false,
                millis: None,
                hop: None,
                message,
            };
        }
    };
    let local = runtime.local_url();
    let latency = run_latency(&cfg, &local).await;
    close_detached(runtime).await;
    latency
}

async fn open_detached(cfg: ChainConfig) -> Result<ExitRuntime, String> {
    match tokio::task::spawn_blocking(move || ExitRuntime::start(cfg)).await {
        Ok(Ok(runtime)) => Ok(runtime),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(format!("启动链式代理出口IP检测失败：{error}")),
    }
}

async fn close_detached(runtime: ExitRuntime) {
    let _ = tokio::task::spawn_blocking(move || drop(runtime)).await;
}

async fn run_probe(cfg: &ChainConfig, local_proxy: &str) -> ExitProbe {
    if let Err(fail) = dial_through(cfg, "api.ipify.org", 443).await {
        return ExitProbe {
            ok: false,
            ip: None,
            hop: Some(fail.name().to_string()),
            message: fail.message(),
        };
    }
    let client = match reqwest::Client::builder()
        .proxy(match reqwest::Proxy::all(local_proxy) {
            Ok(proxy) => proxy,
            Err(error) => {
                return ExitProbe {
                    ok: false,
                    ip: None,
                    hop: None,
                    message: format!("隧道已建立，但读取出口 IP 失败：{error}"),
                };
            }
        })
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(25))
        .user_agent("ai-switcher-antigravity")
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return ExitProbe {
                ok: false,
                ip: None,
                hop: None,
                message: format!("隧道已建立，但读取出口 IP 失败：{error}"),
            };
        }
    };
    match client.get("https://api.ipify.org").send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(body) => {
                let ip = body.trim().to_string();
                if ip.is_empty() || ip.contains(char::is_whitespace) {
                    ExitProbe {
                        ok: false,
                        ip: None,
                        hop: None,
                        message: "隧道已建立，但出口 IP 响应为空".into(),
                    }
                } else {
                    ExitProbe {
                        ok: true,
                        ip: Some(ip.clone()),
                        hop: None,
                        message: format!("出口 IP {ip}"),
                    }
                }
            }
            Err(error) => ExitProbe {
                ok: false,
                ip: None,
                hop: None,
                message: format!("隧道已建立，但读取出口 IP 失败：{error}"),
            },
        },
        Ok(response) => ExitProbe {
            ok: false,
            ip: None,
            hop: None,
            message: format!("隧道已建立，但读取出口 IP 失败：HTTP {}", response.status()),
        },
        Err(error) => ExitProbe {
            ok: false,
            ip: None,
            hop: None,
            message: format!("隧道已建立，但读取出口 IP 失败：{error}"),
        },
    }
}

async fn run_latency(cfg: &ChainConfig, local_proxy: &str) -> ExitLatency {
    if let Err(fail) = dial_through(cfg, "api.ipify.org", 443).await {
        return ExitLatency {
            ok: false,
            millis: None,
            hop: Some(fail.name().to_string()),
            message: fail.message(),
        };
    }
    let client = match proxy_probe_client(local_proxy) {
        Ok(client) => client,
        Err(message) => {
            return ExitLatency {
                ok: false,
                millis: None,
                hop: None,
                message,
            };
        }
    };
    let started = std::time::Instant::now();
    match client.get("https://api.ipify.org").send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(body) if !body.trim().is_empty() => {
                let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                ExitLatency {
                    ok: true,
                    millis: Some(millis),
                    hop: None,
                    message: format!("延迟 {millis} ms"),
                }
            }
            Ok(_) => ExitLatency {
                ok: false,
                millis: None,
                hop: None,
                message: "隧道已建立，但延迟测试响应为空".into(),
            },
            Err(error) => ExitLatency {
                ok: false,
                millis: None,
                hop: None,
                message: format!("隧道已建立，但延迟测试失败：{error}"),
            },
        },
        Ok(response) => ExitLatency {
            ok: false,
            millis: None,
            hop: None,
            message: format!("隧道已建立，但延迟测试失败：HTTP {}", response.status()),
        },
        Err(error) => ExitLatency {
            ok: false,
            millis: None,
            hop: None,
            message: format!("隧道已建立，但延迟测试失败：{error}"),
        },
    }
}

fn proxy_probe_client(local_proxy: &str) -> Result<reqwest::Client, String> {
    let proxy = reqwest::Proxy::all(local_proxy).map_err(|error| {
        format!("隧道已建立，但延迟测试失败：{error}")
    })?;
    reqwest::Client::builder()
        .proxy(proxy)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(25))
        .user_agent("ai-switcher-antigravity")
        .build()
        .map_err(|error| format!("隧道已建立，但延迟测试失败：{error}"))
}

struct ExitRuntime {
    port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ExitRuntime {
    fn local_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn start(cfg: ChainConfig) -> AppResult<Self> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("ag-exit-hop".into())
            .spawn(move || serve(cfg, ready_tx))
            .map_err(|error| AppError::Other(format!("启动链式代理出口IP转发器失败: {error}")))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok((port, shutdown))) => Ok(Self {
                port,
                shutdown,
                thread: Some(thread),
            }),
            Ok(Err(error)) => Err(AppError::Other(format!("链式代理出口IP转发器监听失败: {error}"))),
            Err(_) => Err(AppError::Other("链式代理出口IP转发器启动超时".into())),
        }
    }
}

impl Drop for ExitRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    cfg: ChainConfig,
    ready_tx: std::sync::mpsc::Sender<Result<(u16, tokio::sync::watch::Sender<bool>), String>>,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("ag-exit-hop-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            return;
        }
    };
    let listener = match rt.block_on(TcpListener::bind("127.0.0.1:0")) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            return;
        }
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            return;
        }
    };
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    if ready_tx.send(Ok((port, shutdown_tx))).is_err() {
        return;
    }
    rt.block_on(async move {
        loop {
            tokio::select! {
                biased;
                result = shutdown_rx.changed() => {
                    if result.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let cfg = cfg.clone();
                            tokio::spawn(async move {
                                handle_client(stream, cfg).await;
                            });
                        }
                        Err(error) => {
                            log::warn!("Antigravity exit hop accept failed: {error}");
                            break;
                        }
                    }
                }
            }
        }
    });
}

async fn handle_client(mut client: TcpStream, cfg: ChainConfig) {
    let _ = client.set_nodelay(true);
    let header = match timeout(HOP_TIMEOUT, read_http_header(&mut client)).await {
        Ok(Ok(header)) => header,
        Ok(Err(error)) => {
            let _ = write_simple(&mut client, 400, &error).await;
            return;
        }
        Err(_) => return,
    };
    let request_line = header.lines().next().unwrap_or("");
    let Some((host, port)) = parse_connect_target(request_line) else {
        let _ = write_simple(&mut client, 400, "only CONNECT is supported").await;
        return;
    };
    match dial_through(&cfg, &host, port).await {
        Ok(mut upstream) => {
            let _ = upstream.set_nodelay(true);
            if client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .is_err()
            {
                return;
            }
            let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        }
        Err(fail) => {
            log::warn!("Antigravity exit hop {}: {}", fail.name(), fail.message());
            let _ = write_hop_failure(&mut client, &fail).await;
        }
    }
}

async fn write_hop_failure(stream: &mut TcpStream, fail: &HopFail) {
    let body = fail.message();
    let payload = format!(
        "HTTP/1.1 502 Bad Gateway\r\nX-Aisw-Exit-Hop: {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        fail.name(),
        body.len()
    );
    let _ = stream.write_all(payload.as_bytes()).await;
}

async fn write_simple(stream: &mut TcpStream, status: u16, reason: &str) {
    let payload =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(payload.as_bytes()).await;
}

async fn dial_through(cfg: &ChainConfig, host: &str, port: u16) -> Result<TcpStream, HopFail> {
    let mut stream = match &cfg.first {
        FirstHop::Direct => connect_proxy_socket(&cfg.exit)
            .await
            .map_err(HopFail::First)?,
        FirstHop::Proxy(hop) => {
            let mut linked = connect_proxy_socket(hop).await.map_err(HopFail::First)?;
            proxy_connect(&mut linked, hop, &cfg.exit.host, cfg.exit.port)
                .await
                .map_err(HopFail::First)?;
            linked
        }
    };
    proxy_connect(&mut stream, &cfg.exit, host, port)
        .await
        .map_err(HopFail::Second)?;
    Ok(stream)
}

async fn proxy_connect(
    stream: &mut TcpStream,
    proxy: &ProxyEndpoint,
    host: &str,
    port: u16,
) -> Result<(), String> {
    timeout(HOP_TIMEOUT, async {
        match proxy.kind {
            ProxyKind::Socks5 => socks5_connect(stream, proxy, host, port).await,
            ProxyKind::Http => http_connect(stream, proxy, host, port).await,
        }
    })
    .await
    .map_err(|_| format!("连接 {host}:{port} 超时"))?
}

async fn connect_proxy_socket(proxy: &ProxyEndpoint) -> Result<TcpStream, String> {
    timeout(HOP_TIMEOUT, async {
        let addrs = lookup_host((proxy.host.as_str(), proxy.port))
            .await
            .map_err(|error| format!("解析 {} 失败: {error}", proxy.host))?;
        let mut last = format!("没有可用地址 {}:{}", proxy.host, proxy.port);
        for addr in addrs {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    return Ok(stream);
                }
                Err(error) => last = error.to_string(),
            }
        }
        Err(last)
    })
    .await
    .map_err(|_| format!("连接 {}:{} 超时", proxy.host, proxy.port))?
}

async fn socks5_connect(
    stream: &mut TcpStream,
    proxy: &ProxyEndpoint,
    host: &str,
    port: u16,
) -> Result<(), String> {
    if proxy.has_auth() {
        stream
            .write_all(&[0x05, 0x01, 0x02])
            .await
            .map_err(|e| e.to_string())?;
    } else {
        stream
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut selected = [0u8; 2];
    stream
        .read_exact(&mut selected)
        .await
        .map_err(|e| e.to_string())?;
    if selected[0] != 0x05 {
        return Err("对端不是 SOCKS5".into());
    }
    if proxy.has_auth() {
        if selected[1] != 0x02 {
            return Err("SOCKS5 未接受用户名密码认证".into());
        }
        write_socks_auth(stream, &proxy.username, &proxy.password).await?;
    } else if selected[1] != 0x00 {
        return Err("SOCKS5 未接受无认证".into());
    }
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err("目标主机名过长".into());
    }
    let mut request = Vec::with_capacity(7 + host_bytes.len());
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(|e| e.to_string())?;
    let mut response = [0u8; 4];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|e| e.to_string())?;
    if response[1] != 0x00 {
        return Err(format!("SOCKS5 CONNECT 被拒绝 ({})", response[1]));
    }
    skip_socks_addr(stream, response[3]).await
}

async fn write_socks_auth(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let user = username.as_bytes();
    let pass = password.as_bytes();
    if user.len() > 255 || pass.len() > 255 {
        return Err("SOCKS5 账号或密码过长".into());
    }
    let mut payload = Vec::with_capacity(3 + user.len() + pass.len());
    payload.push(0x01);
    payload.push(user.len() as u8);
    payload.extend_from_slice(user);
    payload.push(pass.len() as u8);
    payload.extend_from_slice(pass);
    stream
        .write_all(&payload)
        .await
        .map_err(|e| e.to_string())?;
    let mut answer = [0u8; 2];
    stream
        .read_exact(&mut answer)
        .await
        .map_err(|e| e.to_string())?;
    if answer[1] != 0x00 {
        return Err("SOCKS5 用户名或密码被拒绝".into());
    }
    Ok(())
}

async fn skip_socks_addr(stream: &mut TcpStream, atyp: u8) -> Result<(), String> {
    match atyp {
        0x01 => {
            let mut buf = [0u8; 6];
            stream
                .read_exact(&mut buf)
                .await
                .map_err(|e| e.to_string())?;
        }
        0x04 => {
            let mut buf = [0u8; 18];
            stream
                .read_exact(&mut buf)
                .await
                .map_err(|e| e.to_string())?;
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream
                .read_exact(&mut len)
                .await
                .map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; len[0] as usize + 2];
            stream
                .read_exact(&mut buf)
                .await
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("SOCKS5 绑定地址类型无效".into()),
    }
    Ok(())
}

async fn http_connect(
    stream: &mut TcpStream,
    proxy: &ProxyEndpoint,
    host: &str,
    port: u16,
) -> Result<(), String> {
    let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    if proxy.has_auth() {
        let token = B64.encode(format!("{}:{}", proxy.username, proxy.password));
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("Proxy-Connection: keep-alive\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let header = read_http_header(stream).await?;
    let status = header
        .lines()
        .next()
        .and_then(parse_http_status)
        .ok_or_else(|| "HTTP 代理响应无效".to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP 代理返回 {status}"));
    }
    Ok(())
}

fn parse_http_status(line: &str) -> Option<u16> {
    let mut parts = line.split_whitespace();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    parts.next()?.parse().ok()
}

fn parse_connect_target(line: &str) -> Option<(String, u16)> {
    let mut parts = line.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("CONNECT") {
        return None;
    }
    split_host_port(parts.next()?)
}

fn split_host_port(value: &str) -> Option<(String, u16)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        return Some((host.to_string(), port.parse().ok()?));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?))
}

async fn read_http_header(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if buf.len() >= HEADER_LIMIT {
            return Err("代理响应头过长".into());
        }
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|e| e.to_string())?;
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(buf).map_err(|_| "代理响应头不是 UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_and_redacts_proxy_urls() {
        let socks = parse_proxy_endpoint("socks5://user:p%40ss@gw.example:1080").unwrap();
        assert_eq!(socks.kind, ProxyKind::Socks5);
        assert_eq!(socks.host, "gw.example");
        assert_eq!(socks.port, 1080);
        assert_eq!(socks.username, "user");
        assert_eq!(socks.password, "p@ss");
        assert_eq!(socks.redacted(), "socks5://gw.example:1080");
        assert_eq!(
            redact_proxy_url("socks5h://user:secret@gw.example:1080"),
            "socks5://gw.example:1080"
        );
        let http = parse_proxy_endpoint("http://gw.example:8080").unwrap();
        assert_eq!(http.kind, ProxyKind::Http);
        assert!(http.username.is_empty());
        assert!(parse_proxy_endpoint("https://gw.example:443").is_err());
        assert!(parse_proxy_endpoint("socks5://gw.example").is_err());
    }

    #[test]
    fn disabled_or_blank_exit_does_not_configure_a_listener() {
        assert!(chain_from_parts(
            false,
            "socks5://user:secret@gw.example:1080",
            FirstHop::Direct
        )
        .unwrap()
        .is_none());
        assert!(chain_from_parts(true, "   ", FirstHop::Direct)
            .unwrap()
            .is_none());
        assert!(chain_from_parts(
            true,
            "socks5://user:secret@gw.example:1080",
            FirstHop::Direct
        )
        .unwrap()
        .is_some());
    }

    #[tokio::test]
    async fn forwarder_stops_listening_when_dropped() {
        let cfg = ChainConfig {
            first: FirstHop::Direct,
            exit: parse_proxy_endpoint("socks5://127.0.0.1:1").unwrap(),
        };
        let runtime = ExitRuntime::start(cfg).unwrap();
        let port = runtime.port;
        TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        drop(runtime);
        assert!(
            TcpStream::connect(("127.0.0.1", port)).await.is_err(),
            "dropped forwarder must close its listener"
        );
    }

    #[tokio::test]
    async fn socks5_username_password_chains_through_first_hop() {
        let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = backend.accept().await.unwrap();
            let _ = read_http_header(&mut sock).await;
            let body = "203.0.113.10";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
        });

        let seen = Arc::new(Mutex::new(Seen::default()));
        let hop2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hop2_addr = hop2.local_addr().unwrap();
        let seen_hop2 = Arc::clone(&seen);
        tokio::spawn(async move {
            serve_auth_socks(hop2, backend_addr, seen_hop2).await;
        });

        let hop1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hop1_addr = hop1.local_addr().unwrap();
        tokio::spawn(async move {
            serve_open_socks(hop1).await;
        });

        let cfg = ChainConfig {
            first: FirstHop::Proxy(ProxyEndpoint {
                kind: ProxyKind::Socks5,
                host: hop1_addr.ip().to_string(),
                port: hop1_addr.port(),
                username: String::new(),
                password: String::new(),
            }),
            exit: ProxyEndpoint {
                kind: ProxyKind::Socks5,
                host: hop2_addr.ip().to_string(),
                port: hop2_addr.port(),
                username: "user".into(),
                password: "secret".into(),
            },
        };
        let runtime = ExitRuntime::start(cfg).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", runtime.port))
            .await
            .unwrap();
        client
            .write_all(b"CONNECT example.com:80 HTTP/1.1\r\nHost: example.com:80\r\n\r\n")
            .await
            .unwrap();
        let established = read_http_header(&mut client).await.unwrap();
        assert!(
            established.starts_with("HTTP/1.1 200"),
            "tunnel failed: {established}"
        );
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut body = Vec::new();
        client.read_to_end(&mut body).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("203.0.113.10"), "{text}");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.user, "user");
        assert_eq!(seen.pass, "secret");
        assert_eq!(seen.host, "example.com");
        assert_eq!(seen.port, 80);
        drop(seen);
        drop(runtime);
    }

    #[derive(Default)]
    struct Seen {
        user: String,
        pass: String,
        host: String,
        port: u16,
    }

    async fn serve_open_socks(listener: TcpListener) {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut greeting = [0u8; 2];
        sock.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting[0], 0x05);
        let mut methods = vec![0u8; greeting[1] as usize];
        sock.read_exact(&mut methods).await.unwrap();
        assert!(methods.contains(&0x00));
        sock.write_all(&[0x05, 0x00]).await.unwrap();
        let (host, port) = read_socks_connect(&mut sock).await;
        let mut upstream = TcpStream::connect((host.as_str(), port)).await.unwrap();
        sock.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let _ = tokio::io::copy_bidirectional(&mut sock, &mut upstream).await;
    }

    async fn serve_auth_socks(
        listener: TcpListener,
        backend: std::net::SocketAddr,
        seen: Arc<Mutex<Seen>>,
    ) {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut greeting = [0u8; 2];
        sock.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting[0], 0x05);
        let mut methods = vec![0u8; greeting[1] as usize];
        sock.read_exact(&mut methods).await.unwrap();
        assert!(
            methods.contains(&0x02),
            "client must offer username/password"
        );
        sock.write_all(&[0x05, 0x02]).await.unwrap();
        let mut ver = [0u8; 1];
        sock.read_exact(&mut ver).await.unwrap();
        assert_eq!(ver[0], 0x01);
        let mut ulen = [0u8; 1];
        sock.read_exact(&mut ulen).await.unwrap();
        let mut user = vec![0u8; ulen[0] as usize];
        sock.read_exact(&mut user).await.unwrap();
        let mut plen = [0u8; 1];
        sock.read_exact(&mut plen).await.unwrap();
        let mut pass = vec![0u8; plen[0] as usize];
        sock.read_exact(&mut pass).await.unwrap();
        sock.write_all(&[0x01, 0x00]).await.unwrap();
        let (host, port) = read_socks_connect(&mut sock).await;
        {
            let mut guard = seen.lock().unwrap();
            guard.user = String::from_utf8(user).unwrap();
            guard.pass = String::from_utf8(pass).unwrap();
            guard.host = host;
            guard.port = port;
        }
        let mut upstream = TcpStream::connect(backend).await.unwrap();
        sock.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let _ = tokio::io::copy_bidirectional(&mut sock, &mut upstream).await;
    }

    async fn read_socks_connect(sock: &mut TcpStream) -> (String, u16) {
        let mut head = [0u8; 4];
        sock.read_exact(&mut head).await.unwrap();
        assert_eq!(head[0], 0x05);
        assert_eq!(head[1], 0x01);
        assert_eq!(head[3], 0x03, "target must be a domain so DNS stays remote");
        let mut len = [0u8; 1];
        sock.read_exact(&mut len).await.unwrap();
        let mut host = vec![0u8; len[0] as usize];
        sock.read_exact(&mut host).await.unwrap();
        let mut port_buf = [0u8; 2];
        sock.read_exact(&mut port_buf).await.unwrap();
        (
            String::from_utf8(host).unwrap(),
            u16::from_be_bytes(port_buf),
        )
    }
}
