//! Antigravity outbound proxy (Google / Cloud Code).
//!
//! Domestic users typically need Clash (`127.0.0.1:17891`); overseas can use direct.

use std::sync::RwLock;
use std::time::Duration;

use log::info;
use serde::{Deserialize, Serialize};

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::system_proxy;

const MODE_SETTING: &str = "antigravity_outbound_mode";
const URL_SETTING: &str = "antigravity_outbound_proxy_url";
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
pub struct OutboundProxySettings {
    pub mode: OutboundProxyMode,
    pub proxy_url: String,
    /// Effective URL actually used after resolving mode (empty when direct).
    pub effective_proxy_url: Option<String>,
}

#[derive(Clone, Default)]
struct CachedSettings {
    mode: OutboundProxyMode,
    proxy_url: String,
}

static CACHE: RwLock<Option<CachedSettings>> = RwLock::new(None);

pub fn default_settings() -> OutboundProxySettings {
    let mode = OutboundProxyMode::Custom;
    let proxy_url = DEFAULT_CLASH_PROXY_URL.to_string();
    OutboundProxySettings {
        effective_proxy_url: resolve_effective(mode, &proxy_url),
        mode,
        proxy_url,
    }
}

pub fn load_settings(db: &Database) -> AppResult<OutboundProxySettings> {
    let (mode, mut proxy_url) = db.with_conn(|conn| {
        let mode = get_setting(conn, MODE_SETTING)?
            .map(|value| OutboundProxyMode::parse(&value))
            .unwrap_or(OutboundProxyMode::Custom);
        let proxy_url = get_setting(conn, URL_SETTING)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLASH_PROXY_URL.to_string());
        Ok((mode, proxy_url))
    })?;
    // Port 17891 on this machine is Clash SOCKS5; old installs stored http:// and
    // every Cloud Code call failed → account cooldown → Desktop 502.
    if proxy_url.trim().eq_ignore_ascii_case(LEGACY_HTTP_CLASH_PROXY_URL) {
        proxy_url = DEFAULT_CLASH_PROXY_URL.to_string();
        let _ = db.with_conn(|conn| set_setting(conn, URL_SETTING, &proxy_url));
        info!("Antigravity outbound proxy migrated {LEGACY_HTTP_CLASH_PROXY_URL} → {proxy_url}");
    }
    write_cache(mode, &proxy_url);
    Ok(OutboundProxySettings {
        effective_proxy_url: resolve_effective(mode, &proxy_url),
        mode,
        proxy_url,
    })
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
    db.with_conn(|conn| {
        set_setting(conn, MODE_SETTING, mode.as_str())?;
        set_setting(conn, URL_SETTING, &url)?;
        Ok(())
    })?;
    write_cache(mode, &url);
    info!(
        "Antigravity outbound proxy saved: mode={} url={url}",
        mode.as_str()
    );
    Ok(OutboundProxySettings {
        effective_proxy_url: resolve_effective(mode, &url),
        mode,
        proxy_url: url,
    })
}

fn write_cache(mode: OutboundProxyMode, proxy_url: &str) {
    let mut guard = match CACHE.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(CachedSettings {
        mode,
        proxy_url: proxy_url.to_string(),
    });
}

fn read_cache() -> CachedSettings {
    let guard = match CACHE.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(cached) = guard.clone() {
        return cached;
    }
    CachedSettings {
        mode: OutboundProxyMode::Custom,
        proxy_url: DEFAULT_CLASH_PROXY_URL.to_string(),
    }
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

pub fn build_async_client(connect_secs: u64, timeout_secs: u64) -> reqwest::Client {
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
    .or_else(|error| {
        log::error!("Antigravity async client build failed ({error}); falling back to direct");
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(connect_secs))
            .timeout(Duration::from_secs(timeout_secs))
            .no_proxy()
            .user_agent("antigravity")
            .build()
    })
    .expect("antigravity async http client fallback")
}

pub fn build_blocking_client(timeout_secs: u64) -> reqwest::blocking::Client {
    apply_cached_to_blocking(
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("ai-switcher-antigravity"),
    )
    .build()
    .or_else(|error| {
        log::error!("Antigravity blocking client build failed ({error}); falling back to direct");
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .no_proxy()
            .user_agent("ai-switcher-antigravity")
            .build()
    })
    .expect("antigravity blocking http client fallback")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!(OutboundProxyMode::parse("direct"), OutboundProxyMode::Direct);
        assert_eq!(OutboundProxyMode::parse("SYSTEM"), OutboundProxyMode::System);
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
}
