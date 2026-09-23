//! Antigravity outbound proxy (Google / Cloud Code).
//!
//! Domestic users typically need Clash (`127.0.0.1:17891`); overseas can use direct.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::info;
use serde::{Deserialize, Serialize};

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::system_proxy;

const MODE_SETTING: &str = "antigravity_outbound_mode";
const URL_SETTING: &str = "antigravity_outbound_proxy_url";
const EXIT_ENABLED_SETTING: &str = "antigravity_exit_proxy_enabled";
const EXIT_URL_SETTING: &str = "antigravity_exit_proxy_url";
const EXIT_LIST_SETTING: &str = "antigravity_exit_proxies";
const EXIT_DEFAULT_NAME: &str = "链式代理出口IP";
/// Connect budget when the residential hop is on. Ordinary request deadlines stay as callers set them.
const EXIT_CONNECT_FLOOR_SECS: u64 = 20;
pub const DEFAULT_CLASH_PROXY_URL: &str = "socks5://127.0.0.1:17891";
/// Legacy default before SOCKS5 fix — auto-migrated on load.
const LEGACY_HTTP_CLASH_PROXY_URL: &str = "http://127.0.0.1:17891";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OutboundProxyMode {
    /// No proxy — suitable when Google is reachable directly.
    Direct,
    /// Detect Windows / env system proxy (Clash as system proxy, etc.).
    System,
    /// Explicit proxy URL (default Clash mixed port).
    #[default]
    Custom,
}

impl OutboundProxyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "direct" => Self::Direct,
            "system" => Self::System,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitProxyEntry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub proxy_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitProxyView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub proxy_url: String,
    pub proxy_redacted: String,
    pub probe_ok: Option<bool>,
    pub probe_ip: Option<String>,
    pub probe_hop: Option<String>,
    pub probe_message: Option<String>,
    #[serde(default)]
    pub latency_ok: Option<bool>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub latency_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitProxyProbeResult {
    pub id: String,
    pub ok: bool,
    pub ip: Option<String>,
    pub hop: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitProxyLatencyResult {
    pub id: String,
    pub ok: bool,
    pub millis: Option<u64>,
    pub hop: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundProxySettings {
    pub mode: OutboundProxyMode,
    pub proxy_url: String,
    /// Effective URL actually used after resolving mode (empty when direct).
    pub effective_proxy_url: Option<String>,
    pub exit_proxies: Vec<ExitProxyView>,
    pub exit_chain_label: String,
    pub exit_error: Option<String>,
}

#[derive(Clone)]
struct CachedSettings {
    mode: OutboundProxyMode,
    proxy_url: String,
    exits: Vec<ExitProxyEntry>,
}

impl Default for CachedSettings {
    fn default() -> Self {
        Self {
            mode: OutboundProxyMode::Custom,
            proxy_url: DEFAULT_CLASH_PROXY_URL.to_string(),
            exits: Vec::new(),
        }
    }
}

static CACHE: RwLock<Option<CachedSettings>> = RwLock::new(None);

fn exit_probes() -> &'static Mutex<HashMap<String, super::exit_hop::ExitProbe>> {
    static SLOT: std::sync::OnceLock<Mutex<HashMap<String, super::exit_hop::ExitProbe>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn exit_latencies() -> &'static Mutex<HashMap<String, super::exit_hop::ExitLatency>> {
    static SLOT: std::sync::OnceLock<Mutex<HashMap<String, super::exit_hop::ExitLatency>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn default_settings() -> OutboundProxySettings {
    let mode = OutboundProxyMode::Custom;
    let proxy_url = DEFAULT_CLASH_PROXY_URL.to_string();
    view(mode, proxy_url, &[])
}

pub fn load_settings(db: &Database) -> AppResult<OutboundProxySettings> {
    let (mode, mut proxy_url, exits, migrated) = db.with_conn(|conn| {
        let mode = get_setting(conn, MODE_SETTING)?
            .map(|value| OutboundProxyMode::parse(&value))
            .unwrap_or(OutboundProxyMode::Custom);
        let proxy_url = get_setting(conn, URL_SETTING)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLASH_PROXY_URL.to_string());
        let list_json = get_setting(conn, EXIT_LIST_SETTING)?;
        let legacy_enabled = get_setting(conn, EXIT_ENABLED_SETTING)?;
        let legacy_url = get_setting(conn, EXIT_URL_SETTING)?;
        let (exits, migrated) = parse_exit_entries(
            list_json.as_deref(),
            legacy_enabled.as_deref(),
            legacy_url.as_deref(),
        );
        Ok((mode, proxy_url, exits, migrated))
    })?;
    // Port 17891 on this machine is Clash SOCKS5; old installs stored http:// and
    // every Cloud Code call failed → account cooldown → Desktop 502.
    if proxy_url
        .trim()
        .eq_ignore_ascii_case(LEGACY_HTTP_CLASH_PROXY_URL)
    {
        proxy_url = DEFAULT_CLASH_PROXY_URL.to_string();
        let _ = db.with_conn(|conn| set_setting(conn, URL_SETTING, &proxy_url));
        info!("Antigravity outbound proxy migrated {LEGACY_HTTP_CLASH_PROXY_URL} → {proxy_url}");
    }
    if migrated {
        persist_exit_entries(db, &exits)?;
    }
    let cached = CachedSettings {
        mode,
        proxy_url: proxy_url.clone(),
        exits,
    };
    write_cache(cached.clone());
    if let Err(error) = sync_exit_forwarder(&cached) {
        log::error!("Antigravity exit hop not applied: {error}");
        super::exit_hop::note_apply_error(error.to_string());
    }
    Ok(view(mode, proxy_url, &cached.exits))
}

pub fn save_settings(
    db: &Database,
    mode: OutboundProxyMode,
    proxy_url: &str,
) -> AppResult<OutboundProxySettings> {
    let normalized = normalize_proxy_url(proxy_url.trim())?;
    if mode == OutboundProxyMode::Custom && normalized.is_empty() {
        return Err(AppError::Config("自定义代理地址不能为空".into()));
    }
    let url = if normalized.is_empty() {
        DEFAULT_CLASH_PROXY_URL.to_string()
    } else {
        normalized
    };
    let exits = db.with_conn(|conn| {
        let list_json = get_setting(conn, EXIT_LIST_SETTING)?;
        let legacy_enabled = get_setting(conn, EXIT_ENABLED_SETTING)?;
        let legacy_url = get_setting(conn, EXIT_URL_SETTING)?;
        let (exits, _) = parse_exit_entries(
            list_json.as_deref(),
            legacy_enabled.as_deref(),
            legacy_url.as_deref(),
        );
        set_setting(conn, MODE_SETTING, mode.as_str())?;
        set_setting(conn, URL_SETTING, &url)?;
        Ok(exits)
    })?;
    let cached = CachedSettings {
        mode,
        proxy_url: url.clone(),
        exits,
    };
    write_cache(cached.clone());
    sync_exit_forwarder(&cached)?;
    info!(
        "Antigravity outbound proxy saved: mode={} url={url}",
        mode.as_str()
    );
    Ok(view(mode, url, &cached.exits))
}

pub fn save_exit_proxies(
    db: &Database,
    entries: Vec<ExitProxyEntry>,
) -> AppResult<OutboundProxySettings> {
    for entry in &entries {
        if entry.enabled && entry.proxy_url.trim().is_empty() {
            return Err(AppError::Config(
                "启用链式代理出口IP时主机和端口不能为空".into(),
            ));
        }
        if !entry.proxy_url.trim().is_empty() {
            super::exit_hop::parse_proxy_endpoint(&entry.proxy_url)?;
        }
    }
    let entries = normalize_exit_entries(entries);
    let (mode, outbound_url) = db.with_conn(|conn| {
        let mode = get_setting(conn, MODE_SETTING)?
            .map(|value| OutboundProxyMode::parse(&value))
            .unwrap_or(OutboundProxyMode::Custom);
        let outbound_url = get_setting(conn, URL_SETTING)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLASH_PROXY_URL.to_string());
        Ok((mode, outbound_url))
    })?;
    let cached = CachedSettings {
        mode,
        proxy_url: outbound_url,
        exits: entries.clone(),
    };
    // Validate the chain before persisting so a bad URL does not replace a working hop.
    sync_exit_forwarder(&cached)?;
    persist_exit_entries(db, &entries)?;
    retain_exit_probes(&entries);
    write_cache(cached.clone());
    let active = active_exit(&entries);
    info!(
        "Antigravity chain exit proxies saved: count={} active={}",
        entries.len(),
        active
            .map(|entry| super::exit_hop::redact_proxy_url(&entry.proxy_url))
            .unwrap_or_else(|| "-".into())
    );
    Ok(view(cached.mode, cached.proxy_url, &cached.exits))
}

/// Check one saved or draft chain. Does not enable it for live traffic.
pub async fn probe_exit_proxy(id: &str, proxy_url: &str) -> AppResult<ExitProxyProbeResult> {
    let chain = chain_for_probe(proxy_url)?;
    let probe = super::exit_hop::probe_detached(chain).await;
    if !id.is_empty() {
        let mut guard = match exit_probes().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(id.to_string(), probe.clone());
    }
    Ok(ExitProxyProbeResult {
        id: id.to_string(),
        ok: probe.ok,
        ip: probe.ip,
        hop: probe.hop,
        message: probe.message,
    })
}

/// Time one round trip on a saved or draft chain. Does not enable it for live traffic.
pub async fn probe_exit_latency(id: &str, proxy_url: &str) -> AppResult<ExitProxyLatencyResult> {
    let chain = chain_for_probe(proxy_url)?;
    let latency = super::exit_hop::probe_latency_detached(chain).await;
    if !id.is_empty() {
        let mut guard = match exit_latencies().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(id.to_string(), latency.clone());
    }
    Ok(ExitProxyLatencyResult {
        id: id.to_string(),
        ok: latency.ok,
        millis: latency.millis,
        hop: latency.hop,
        message: latency.message,
    })
}

fn chain_for_probe(proxy_url: &str) -> AppResult<super::exit_hop::ChainConfig> {
    let proxy_url = proxy_url.trim();
    if proxy_url.is_empty() {
        return Err(AppError::Config(
            "检测链式代理出口IP需要填写主机和端口".into(),
        ));
    }
    super::exit_hop::parse_proxy_endpoint(proxy_url)?;
    let cached = read_cache();
    let first = first_hop(&cached)?;
    super::exit_hop::chain_from_parts(true, proxy_url, first)?.ok_or_else(|| {
        AppError::Config("检测链式代理出口IP需要填写主机和端口".into())
    })
}

fn write_cache(cached: CachedSettings) {
    let mut guard = match CACHE.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(cached);
}

fn read_cache() -> CachedSettings {
    let guard = match CACHE.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(cached) = guard.clone() {
        return cached;
    }
    CachedSettings::default()
}

fn view(
    mode: OutboundProxyMode,
    proxy_url: String,
    exits: &[ExitProxyEntry],
) -> OutboundProxySettings {
    let probes = match exit_probes().lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let latencies = match exit_latencies().lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let exit_proxies = exits
        .iter()
        .map(|entry| {
            let probe = probes.get(&entry.id);
            let latency = latencies.get(&entry.id);
            ExitProxyView {
                id: entry.id.clone(),
                name: entry.name.clone(),
                enabled: entry.enabled,
                proxy_redacted: super::exit_hop::redact_proxy_url(&entry.proxy_url),
                probe_ok: probe.map(|item| item.ok),
                probe_ip: probe.and_then(|item| item.ip.clone()),
                probe_hop: probe.and_then(|item| item.hop.clone()),
                probe_message: probe.map(|item| item.message.clone()),
                latency_ok: latency.map(|item| item.ok),
                latency_ms: latency.and_then(|item| item.millis),
                latency_message: latency.map(|item| item.message.clone()),
                proxy_url: entry.proxy_url.clone(),
            }
        })
        .collect();
    OutboundProxySettings {
        effective_proxy_url: resolve_effective(mode, &proxy_url),
        mode,
        proxy_url,
        exit_proxies,
        exit_chain_label: super::exit_hop::current_label().unwrap_or_default(),
        exit_error: super::exit_hop::apply_error(),
    }
}

fn sync_exit_forwarder(cached: &CachedSettings) -> AppResult<()> {
    let Some(active) = active_exit(&cached.exits) else {
        return super::exit_hop::apply_chain(None);
    };
    let first = first_hop(cached)?;
    let chain = super::exit_hop::chain_from_parts(true, &active.proxy_url, first)?;
    super::exit_hop::apply_chain(chain)
}

fn first_hop(cached: &CachedSettings) -> AppResult<super::exit_hop::FirstHop> {
    match cached.mode {
        OutboundProxyMode::Direct => Ok(super::exit_hop::FirstHop::Direct),
        OutboundProxyMode::System => {
            let url = system_proxy::outbound_proxy_url().ok_or_else(|| {
                AppError::Config(
                    "未检测到系统代理，无法经过出站代理使用链式代理出口IP".into(),
                )
            })?;
            Ok(super::exit_hop::FirstHop::Proxy(
                super::exit_hop::parse_proxy_endpoint(&url)?,
            ))
        }
        OutboundProxyMode::Custom => {
            let url = if cached.proxy_url.trim().is_empty() {
                DEFAULT_CLASH_PROXY_URL
            } else {
                cached.proxy_url.trim()
            };
            Ok(super::exit_hop::FirstHop::Proxy(
                super::exit_hop::parse_proxy_endpoint(url)?,
            ))
        }
    }
}

fn active_exit(entries: &[ExitProxyEntry]) -> Option<&ExitProxyEntry> {
    entries
        .iter()
        .find(|entry| entry.enabled && !entry.proxy_url.trim().is_empty())
}

pub fn exit_hop_active() -> bool {
    let cached = read_cache();
    active_exit(&cached.exits).is_some()
}

/// Redacted route for logs and errors. Chain credentials are never included.
pub fn diagnostic_route() -> Option<String> {
    if exit_hop_active() {
        if let Some(label) = super::exit_hop::current_label() {
            return Some(label);
        }
        let cached = read_cache();
        if let Some(active) = active_exit(&cached.exits) {
            return Some(super::exit_hop::redact_proxy_url(&active.proxy_url));
        }
    }
    current_effective_proxy()
}

fn parse_exit_entries(
    list_json: Option<&str>,
    legacy_enabled: Option<&str>,
    legacy_url: Option<&str>,
) -> (Vec<ExitProxyEntry>, bool) {
    if let Some(raw) = list_json {
        if let Ok(parsed) = serde_json::from_str::<Vec<ExitProxyEntry>>(raw) {
            return (normalize_exit_entries(parsed), false);
        }
    }
    let url = legacy_url.unwrap_or("").trim().to_string();
    if url.is_empty() {
        return (Vec::new(), list_json.is_none());
    }
    let enabled = legacy_enabled
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    (
        normalize_exit_entries(vec![ExitProxyEntry {
            id: "exit-legacy".into(),
            name: EXIT_DEFAULT_NAME.to_string(),
            enabled,
            proxy_url: url,
        }]),
        true,
    )
}

fn normalize_exit_entries(entries: Vec<ExitProxyEntry>) -> Vec<ExitProxyEntry> {
    let mut seen = HashSet::new();
    let mut enabled_taken = false;
    let mut out = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let proxy_url = entry.proxy_url.trim().to_string();
        if proxy_url.is_empty() {
            continue;
        }
        let mut id = entry.id.trim().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            id = format!("exit-{index}-{}", unix_nanos());
            seen.insert(id.clone());
        }
        let name = {
            let trimmed = entry.name.trim();
            if trimmed.is_empty() {
                EXIT_DEFAULT_NAME.to_string()
            } else {
                trimmed.to_string()
            }
        };
        let enabled = entry.enabled && !enabled_taken;
        if enabled {
            enabled_taken = true;
        }
        out.push(ExitProxyEntry {
            id,
            name,
            enabled,
            proxy_url,
        });
    }
    out
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn persist_exit_entries(db: &Database, entries: &[ExitProxyEntry]) -> AppResult<()> {
    let json = serde_json::to_string(entries).unwrap_or_else(|_| "[]".to_string());
    let active = active_exit(entries);
    db.with_conn(|conn| {
        set_setting(conn, EXIT_LIST_SETTING, &json)?;
        set_setting(
            conn,
            EXIT_ENABLED_SETTING,
            if active.is_some() { "1" } else { "0" },
        )?;
        set_setting(
            conn,
            EXIT_URL_SETTING,
            active.map(|entry| entry.proxy_url.as_str()).unwrap_or(""),
        )?;
        Ok(())
    })
}

fn retain_exit_probes(entries: &[ExitProxyEntry]) {
    let keep = |id: &String| entries.iter().any(|entry| entry.id == *id);
    let mut probes = match exit_probes().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    probes.retain(|id, _| keep(id));
    drop(probes);
    let mut latencies = match exit_latencies().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    latencies.retain(|id, _| keep(id));
}

/// Ensure cache is warm from DB (call after gateway init).
pub fn warm_from_db(db: &Database) {
    let _ = load_settings(db);
}

/// Currently resolved proxy URL (empty/None = direct).
pub fn current_effective_proxy() -> Option<String> {
    let cached = read_cache();
    resolve_effective(cached.mode, &cached.proxy_url)
}

fn resolve_effective(mode: OutboundProxyMode, proxy_url: &str) -> Option<String> {
    match mode {
        OutboundProxyMode::Direct => None,
        OutboundProxyMode::System => system_proxy::outbound_proxy_url(),
        OutboundProxyMode::Custom => {
            let url = if proxy_url.trim().is_empty() {
                DEFAULT_CLASH_PROXY_URL
            } else {
                proxy_url.trim()
            };
            Some(url.to_string())
        }
    }
}

fn normalize_proxy_url(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.contains("://") {
        // Common mistake: http:// on a Clash SOCKS-only port.
        if let Some(rest) = value
            .strip_prefix("http://")
            .or_else(|| value.strip_prefix("HTTP://"))
        {
            if rest.eq_ignore_ascii_case("127.0.0.1:17891")
                || rest.eq_ignore_ascii_case("localhost:17891")
            {
                return Ok(format!("socks5://{rest}"));
            }
        }
        Ok(value.to_string())
    } else if value.starts_with("socks") {
        Ok(format!("socks5://{value}"))
    } else if value.ends_with(":17891")
        || value.ends_with(":1080")
        || value.ends_with(":7891")
        || value.ends_with(":10808")
    {
        // Typical Clash / mihomo SOCKS ports when scheme omitted.
        Ok(format!("socks5://{value}"))
    } else {
        Ok(format!("http://{value}"))
    }
}

fn apply_cached_to_async(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if exit_hop_active() {
        return apply_exit_hop(builder);
    }
    let cached = read_cache();
    match cached.mode {
        OutboundProxyMode::Direct => {
            info!("Antigravity outbound: direct (no proxy)");
            builder.no_proxy()
        }
        OutboundProxyMode::System => system_proxy::apply_to_builder(builder),
        OutboundProxyMode::Custom => {
            let url = if cached.proxy_url.trim().is_empty() {
                DEFAULT_CLASH_PROXY_URL
            } else {
                cached.proxy_url.trim()
            };
            system_proxy::apply_proxy_url(builder, url)
        }
    }
}

fn apply_cached_to_blocking(
    builder: reqwest::blocking::ClientBuilder,
) -> reqwest::blocking::ClientBuilder {
    if exit_hop_active() {
        return apply_exit_hop(builder);
    }
    let cached = read_cache();
    match cached.mode {
        OutboundProxyMode::Direct => {
            info!("Antigravity outbound: direct (no proxy)");
            builder.no_proxy()
        }
        OutboundProxyMode::System => system_proxy::apply_to_builder(builder),
        OutboundProxyMode::Custom => {
            let url = if cached.proxy_url.trim().is_empty() {
                DEFAULT_CLASH_PROXY_URL
            } else {
                cached.proxy_url.trim()
            };
            system_proxy::apply_proxy_url(builder, url)
        }
    }
}

fn apply_exit_hop<T: system_proxy::ProxyConfigurable>(builder: T) -> T {
    let url = super::exit_hop::local_forwarder_url()
        .unwrap_or_else(|| super::exit_hop::fail_closed_proxy_url().to_string());
    let label = diagnostic_route().unwrap_or_else(|| url.clone());
    match reqwest::Proxy::all(&url) {
        Ok(proxy) => {
            info!("Antigravity outbound via exit hop {url} ({label})");
            builder.with_proxy(proxy)
        }
        Err(error) => {
            log::error!(
                "Antigravity exit hop proxy rejected ({error}); refusing a direct connection"
            );
            let proxy = reqwest::Proxy::all(super::exit_hop::fail_closed_proxy_url())
                .expect("static fail-closed proxy");
            builder.with_proxy(proxy)
        }
    }
}

fn connect_budget(connect_secs: u64, timeout_secs: u64) -> u64 {
    if exit_hop_active() {
        connect_secs.max(EXIT_CONNECT_FLOOR_SECS).min(timeout_secs)
    } else {
        connect_secs
    }
}

pub fn build_async_client(connect_secs: u64, timeout_secs: u64) -> reqwest::Client {
    let connect_secs = connect_budget(connect_secs, timeout_secs);
    let chained = exit_hop_active();
    apply_cached_to_async(
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(connect_secs))
            .timeout(Duration::from_secs(timeout_secs))
            // Pool tuning mirrors Antigravity-Manager's upstream client
            // (20 idle per host / 90s idle / 60s TCP keepalive).
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .user_agent("antigravity"),
    )
    .build()
    .unwrap_or_else(|error| {
        if chained {
            log::error!(
                "Antigravity async client build failed ({error}); exit hop enabled, not falling back to direct"
            );
            return fail_closed_async_client(connect_secs, timeout_secs);
        }
        log::error!("Antigravity async client build failed ({error}); falling back to direct");
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(connect_secs))
            .timeout(Duration::from_secs(timeout_secs))
            .no_proxy()
            .user_agent("antigravity")
            .build()
            .unwrap_or_else(|fallback| {
                log::error!(
                    "Antigravity async client fallback also failed ({fallback}); using reqwest defaults"
                );
                reqwest::Client::new()
            })
    })
}

fn fail_closed_async_client(connect_secs: u64, timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(connect_secs))
        .timeout(Duration::from_secs(timeout_secs))
        .proxy(
            reqwest::Proxy::all(super::exit_hop::fail_closed_proxy_url())
                .expect("static fail-closed proxy"),
        )
        .user_agent("antigravity")
        .build()
        .expect("fail-closed async client")
}

/// Bounded client for Google OAuth token refresh. Never falls back to an
/// unbounded `Client::new()` — Clash SOCKS can hang indefinitely without
/// connect/timeout caps.
pub fn build_blocking_token_refresh_client(direct: bool) -> AppResult<reqwest::blocking::Client> {
    let chained = !direct && exit_hop_active();
    // Two hops need a longer connect budget than the single-proxy 5s/10s caps.
    let connect_secs: u64 = if chained { 20 } else { 5 };
    let timeout_secs: u64 = if chained { 30 } else { 10 };
    let builder = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(connect_secs))
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("ai-switcher-antigravity");
    let builder = if direct {
        builder.no_proxy()
    } else {
        apply_cached_to_blocking(builder)
    };
    match builder.build() {
        Ok(client) => Ok(client),
        Err(error) if chained => Err(AppError::Network(format!(
            "创建 Google Token 客户端失败，链式代理出口IP已启用，未退回直连: {error}"
        ))),
        Err(error) => {
            log::error!(
                "Antigravity token-refresh client build failed ({error}); falling back to direct"
            );
            reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(connect_secs))
                .timeout(Duration::from_secs(timeout_secs))
                .no_proxy()
                .user_agent("ai-switcher-antigravity")
                .build()
                .map_err(|fallback| {
                    AppError::Other(format!("创建 Google Token 客户端失败: {fallback}"))
                })
        }
    }
}

pub fn build_blocking_client(timeout_secs: u64) -> reqwest::blocking::Client {
    let connect_secs = connect_budget(timeout_secs.min(10), timeout_secs);
    let chained = exit_hop_active();
    apply_cached_to_blocking(
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(connect_secs))
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("ai-switcher-antigravity"),
    )
    .build()
    .unwrap_or_else(|error| {
        if chained {
            log::error!(
                "Antigravity blocking client build failed ({error}); exit hop enabled, not falling back to direct"
            );
            return reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(connect_secs))
                .timeout(Duration::from_secs(timeout_secs))
                .proxy(
                    reqwest::Proxy::all(super::exit_hop::fail_closed_proxy_url())
                        .expect("static fail-closed proxy"),
                )
                .user_agent("ai-switcher-antigravity")
                .build()
                .expect("fail-closed blocking client");
        }
        log::error!("Antigravity blocking client build failed ({error}); falling back to direct");
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(connect_secs))
            .timeout(Duration::from_secs(timeout_secs))
            .no_proxy()
            .user_agent("ai-switcher-antigravity")
            .build()
            .unwrap_or_else(|fallback| {
                log::error!(
                    "Antigravity blocking client fallback also failed ({fallback}); using reqwest defaults"
                );
                reqwest::blocking::Client::new()
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!(
            OutboundProxyMode::parse("direct"),
            OutboundProxyMode::Direct
        );
        assert_eq!(
            OutboundProxyMode::parse("SYSTEM"),
            OutboundProxyMode::System
        );
        assert_eq!(OutboundProxyMode::parse(""), OutboundProxyMode::Custom);
    }

    #[test]
    fn normalize_proxy() {
        assert_eq!(
            normalize_proxy_url("127.0.0.1:17891").unwrap(),
            "socks5://127.0.0.1:17891"
        );
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:17891").unwrap(),
            "socks5://127.0.0.1:17891"
        );
        assert_eq!(
            normalize_proxy_url("socks5://127.0.0.1:17891").unwrap(),
            "socks5://127.0.0.1:17891"
        );
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
    }

    #[test]
    fn normalizes_multiple_exit_proxies_and_keeps_one_enabled() {
        let entries = normalize_exit_entries(vec![
            ExitProxyEntry {
                id: "a".into(),
                name: "  ".into(),
                enabled: true,
                proxy_url: " socks5://gw-a.example:1080 ".into(),
            },
            ExitProxyEntry {
                id: "a".into(),
                name: "第二条".into(),
                enabled: true,
                proxy_url: "http://gw-b.example:8080".into(),
            },
            ExitProxyEntry {
                id: "blank".into(),
                name: EXIT_DEFAULT_NAME.into(),
                enabled: false,
                proxy_url: "  ".into(),
            },
        ]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "a");
        assert_eq!(entries[0].name, EXIT_DEFAULT_NAME);
        assert!(entries[0].enabled);
        assert_eq!(entries[0].proxy_url, "socks5://gw-a.example:1080");
        assert_ne!(entries[1].id, "a");
        assert_eq!(entries[1].name, "第二条");
        assert!(!entries[1].enabled);
    }

    #[test]
    fn migrates_legacy_single_exit_url() {
        let (entries, migrated) = parse_exit_entries(
            None,
            Some("1"),
            Some("socks5://user:secret@gw.example:1080"),
        );
        assert!(migrated);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "exit-legacy");
        assert_eq!(entries[0].name, EXIT_DEFAULT_NAME);
        assert!(entries[0].enabled);
        let (again, migrated_again) = parse_exit_entries(Some("[]"), Some("1"), Some("socks5://old"));
        assert!(!migrated_again);
        assert!(again.is_empty());
    }
}
