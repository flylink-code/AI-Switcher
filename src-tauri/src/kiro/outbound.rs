//! Direct / system / custom outbound proxy for the Kiro gateway, plus an optional chain exit.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client as BlockingClient;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::antigravity::exit_hop::{self, HopOwner};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

const MODE_SETTING: &str = "kiro_outbound_mode";
const URL_SETTING: &str = "kiro_outbound_proxy_url";
const EXIT_LIST_SETTING: &str = "kiro_exit_proxies";
const EXIT_DEFAULT_NAME: &str = "链式代理出口IP";

static PROXY: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundMode {
    Direct,
    System,
    Custom,
}

impl OutboundMode {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "direct" => Self::Direct,
            "custom" => Self::Custom,
            _ => Self::System,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutboundSettings {
    pub mode: OutboundMode,
    pub proxy_url: String,
    pub effective_proxy_url: Option<String>,
}

pub fn load_settings(db: &Database) -> AppResult<OutboundSettings> {
    db.with_conn(|conn| {
        let mode = OutboundMode::parse(
            &get_setting(conn, MODE_SETTING)?.unwrap_or_else(|| "system".to_string()),
        );
        let proxy_url = get_setting(conn, URL_SETTING)?.unwrap_or_default();
        Ok(finish(mode, proxy_url))
    })
}

pub fn save_settings(db: &Database, mode: OutboundMode, proxy_url: &str) -> AppResult<OutboundSettings> {
    let proxy_url = proxy_url.trim().to_string();
    if mode == OutboundMode::Custom && proxy_url.is_empty() {
        return Err(AppError::Config("自定义代理地址不能为空".into()));
    }
    db.with_conn(|conn| {
        set_setting(conn, MODE_SETTING, mode.as_str())?;
        set_setting(conn, URL_SETTING, &proxy_url)?;
        Ok(())
    })?;
    let settings = finish(mode, proxy_url);
    set_effective_proxy(settings.effective_proxy_url.clone());
    let cached = read_cache();
    let cached = CachedSettings {
        mode: settings.mode,
        proxy_url: settings.proxy_url.clone(),
        exits: cached.exits,
    };
    let _ = sync_exit_forwarder(&cached);
    write_cache(cached);
    Ok(settings)
}

pub fn apply_loaded(settings: &OutboundSettings) {
    set_effective_proxy(settings.effective_proxy_url.clone());
}

fn finish(mode: OutboundMode, proxy_url: String) -> OutboundSettings {
    let effective_proxy_url = match mode {
        OutboundMode::Direct => None,
        OutboundMode::System => crate::system_proxy::outbound_proxy_url(),
        OutboundMode::Custom => Some(proxy_url.clone()).filter(|value| !value.is_empty()),
    };
    OutboundSettings {
        mode,
        proxy_url,
        effective_proxy_url,
    }
}

pub fn set_effective_proxy(url: Option<String>) {
    if let Ok(mut guard) = PROXY.lock() {
        *guard = url.filter(|value| !value.trim().is_empty());
    }
}

fn current_proxy() -> Option<String> {
    PROXY.lock().ok().and_then(|guard| guard.clone())
}

pub fn build_async_client(timeout_secs: u64) -> Client {
    client_builder(timeout_secs)
        .build()
        .unwrap_or_else(|_| Client::new())
}

pub fn build_blocking_client(timeout_secs: u64) -> BlockingClient {
    blocking_builder(timeout_secs)
        .build()
        .unwrap_or_else(|_| BlockingClient::new())
}

fn client_builder(timeout_secs: u64) -> reqwest::ClientBuilder {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(timeout_secs));
    builder = attach_proxy(builder);
    builder
}

fn blocking_builder(timeout_secs: u64) -> reqwest::blocking::ClientBuilder {
    let mut builder = BlockingClient::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(timeout_secs));
    builder = attach_blocking_proxy(builder);
    builder
}

fn attach_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if exit_hop_active() {
        return apply_exit_proxy(builder);
    }
    if let Some(proxy) = current_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(proxy) {
            return builder.proxy(proxy);
        }
    }
    builder.no_proxy()
}

fn attach_blocking_proxy(builder: reqwest::blocking::ClientBuilder) -> reqwest::blocking::ClientBuilder {
    if exit_hop_active() {
        return apply_exit_proxy(builder);
    }
    if let Some(proxy) = current_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(proxy) {
            return builder.proxy(proxy);
        }
    }
    builder.no_proxy()
}

fn apply_exit_proxy<T: crate::system_proxy::ProxyConfigurable>(builder: T) -> T {
    let url = exit_hop::local_forwarder_url_for(HopOwner::Kiro)
        .unwrap_or_else(|| exit_hop::fail_closed_proxy_url().to_string());
    match reqwest::Proxy::all(&url) {
        Ok(proxy) => builder.with_proxy(proxy),
        Err(_) => {
            let proxy = reqwest::Proxy::all(exit_hop::fail_closed_proxy_url())
                .expect("static fail-closed proxy");
            builder.with_proxy(proxy)
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

#[derive(Debug, Clone)]
pub struct OutboundView {
    pub mode: OutboundMode,
    pub proxy_url: String,
    pub effective_proxy_url: Option<String>,
    pub exit_proxies: Vec<ExitProxyView>,
    pub exit_chain_label: String,
    pub exit_error: Option<String>,
}

#[derive(Clone)]
struct CachedSettings {
    mode: OutboundMode,
    proxy_url: String,
    exits: Vec<ExitProxyEntry>,
}

static CACHE: RwLock<Option<CachedSettings>> = RwLock::new(None);

fn exit_probes() -> &'static Mutex<HashMap<String, exit_hop::ExitProbe>> {
    static SLOT: std::sync::OnceLock<Mutex<HashMap<String, exit_hop::ExitProbe>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn exit_latencies() -> &'static Mutex<HashMap<String, exit_hop::ExitLatency>> {
    static SLOT: std::sync::OnceLock<Mutex<HashMap<String, exit_hop::ExitLatency>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn load_view(db: &Database) -> AppResult<OutboundView> {
    let settings = load_settings(db)?;
    let exits = db.with_conn(|conn| {
        let raw = get_setting(conn, EXIT_LIST_SETTING)?;
        let parsed = raw
            .as_deref()
            .and_then(|value| serde_json::from_str::<Vec<ExitProxyEntry>>(value).ok())
            .unwrap_or_default();
        Ok(normalize_exit_entries(parsed))
    })?;
    let cached = CachedSettings {
        mode: settings.mode,
        proxy_url: settings.proxy_url.clone(),
        exits,
    };
    let _ = sync_exit_forwarder(&cached);
    write_cache(cached.clone());
    Ok(view_of(&cached))
}

pub fn save_exit_proxies(db: &Database, entries: Vec<ExitProxyEntry>) -> AppResult<OutboundView> {
    for entry in &entries {
        if entry.enabled && entry.proxy_url.trim().is_empty() {
            return Err(AppError::Config("启用链式代理出口IP时主机和端口不能为空".into()));
        }
        if !entry.proxy_url.trim().is_empty() {
            exit_hop::parse_proxy_endpoint(&entry.proxy_url)?;
        }
    }
    let entries = normalize_exit_entries(entries);
    let settings = load_settings(db)?;
    let cached = CachedSettings {
        mode: settings.mode,
        proxy_url: settings.proxy_url,
        exits: entries.clone(),
    };
    sync_exit_forwarder(&cached)?;
    let json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
    db.with_conn(|conn| set_setting(conn, EXIT_LIST_SETTING, &json))?;
    retain_exit_probes(&entries);
    write_cache(cached.clone());
    set_effective_proxy(finish(cached.mode, cached.proxy_url.clone()).effective_proxy_url);
    Ok(view_of(&cached))
}

pub async fn probe_exit_proxy(id: &str, proxy_url: &str) -> AppResult<ExitProxyProbeResult> {
    let chain = chain_for_probe(proxy_url)?;
    let probe = exit_hop::probe_detached(chain).await;
    if !id.is_empty() {
        let mut guard = exit_probes().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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

pub async fn probe_exit_latency(id: &str, proxy_url: &str) -> AppResult<ExitProxyLatencyResult> {
    let chain = chain_for_probe(proxy_url)?;
    let latency = exit_hop::probe_latency_detached(chain).await;
    if !id.is_empty() {
        let mut guard = exit_latencies().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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

pub fn resync_system_exit(db: &Database) {
    let Ok(settings) = load_settings(db) else {
        return;
    };
    if settings.mode != OutboundMode::System {
        return;
    }
    let cached = read_cache();
    if active_exit(&cached.exits).is_none() {
        set_effective_proxy(settings.effective_proxy_url);
        return;
    }
    let cached = CachedSettings {
        mode: settings.mode,
        proxy_url: settings.proxy_url,
        exits: cached.exits,
    };
    let _ = sync_exit_forwarder(&cached);
    write_cache(cached);
}

fn exit_hop_active() -> bool {
    active_exit(&read_cache().exits).is_some()
}

fn chain_for_probe(proxy_url: &str) -> AppResult<exit_hop::ChainConfig> {
    let proxy_url = proxy_url.trim();
    if proxy_url.is_empty() {
        return Err(AppError::Config("检测链式代理出口IP需要填写主机和端口".into()));
    }
    exit_hop::parse_proxy_endpoint(proxy_url)?;
    let cached = read_cache();
    let first = first_hop(&cached)?;
    exit_hop::chain_from_parts(true, proxy_url, first)?
        .ok_or_else(|| AppError::Config("检测链式代理出口IP需要填写主机和端口".into()))
}

fn sync_exit_forwarder(cached: &CachedSettings) -> AppResult<()> {
    let Some(active) = active_exit(&cached.exits) else {
        return exit_hop::apply_owned(HopOwner::Kiro, None);
    };
    let first = match first_hop(cached) {
        Ok(first) => first,
        Err(error) => {
            exit_hop::note_apply_error_for(HopOwner::Kiro, error.to_string());
            return Err(error);
        }
    };
    let chain = exit_hop::chain_from_parts(true, &active.proxy_url, first)?;
    exit_hop::apply_owned(HopOwner::Kiro, chain)
}

fn first_hop(cached: &CachedSettings) -> AppResult<exit_hop::FirstHop> {
    match cached.mode {
        OutboundMode::Direct => Ok(exit_hop::FirstHop::Direct),
        OutboundMode::System => {
            let url = crate::system_proxy::outbound_proxy_url().ok_or_else(|| {
                AppError::Config("未检测到系统代理，无法经过出站代理使用链式代理出口IP".into())
            })?;
            Ok(exit_hop::FirstHop::Proxy(exit_hop::parse_proxy_endpoint(&url)?))
        }
        OutboundMode::Custom => {
            let url = cached.proxy_url.trim();
            if url.is_empty() {
                return Err(AppError::Config("自定义出站代理地址不能为空".into()));
            }
            Ok(exit_hop::FirstHop::Proxy(exit_hop::parse_proxy_endpoint(url)?))
        }
    }
}

fn active_exit(entries: &[ExitProxyEntry]) -> Option<&ExitProxyEntry> {
    entries.iter().find(|entry| entry.enabled && !entry.proxy_url.trim().is_empty())
}

fn view_of(cached: &CachedSettings) -> OutboundView {
    let probes = exit_probes().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let latencies = exit_latencies().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let exit_proxies = cached
        .exits
        .iter()
        .map(|entry| {
            let probe = probes.get(&entry.id);
            let latency = latencies.get(&entry.id);
            ExitProxyView {
                id: entry.id.clone(),
                name: entry.name.clone(),
                enabled: entry.enabled,
                proxy_url: entry.proxy_url.clone(),
                proxy_redacted: exit_hop::redact_proxy_url(&entry.proxy_url),
                probe_ok: probe.map(|item| item.ok),
                probe_ip: probe.and_then(|item| item.ip.clone()),
                probe_hop: probe.and_then(|item| item.hop.clone()),
                probe_message: probe.map(|item| item.message.clone()),
                latency_ok: latency.map(|item| item.ok),
                latency_ms: latency.and_then(|item| item.millis),
                latency_message: latency.map(|item| item.message.clone()),
            }
        })
        .collect();
    OutboundView {
        effective_proxy_url: finish(cached.mode, cached.proxy_url.clone()).effective_proxy_url,
        mode: cached.mode,
        proxy_url: cached.proxy_url.clone(),
        exit_proxies,
        exit_chain_label: exit_hop::current_label_for(HopOwner::Kiro).unwrap_or_default(),
        exit_error: exit_hop::apply_error_for(HopOwner::Kiro),
    }
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
        out.push(ExitProxyEntry { id, name, enabled, proxy_url });
    }
    out
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn retain_exit_probes(entries: &[ExitProxyEntry]) {
    let keep = |id: &String| entries.iter().any(|entry| entry.id == *id);
    exit_probes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|id, _| keep(id));
    exit_latencies()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|id, _| keep(id));
}

fn write_cache(cached: CachedSettings) {
    let mut guard = CACHE.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(cached);
}

fn read_cache() -> CachedSettings {
    let guard = CACHE.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.clone().unwrap_or(CachedSettings {
        mode: OutboundMode::System,
        proxy_url: String::new(),
        exits: Vec::new(),
    })
}
