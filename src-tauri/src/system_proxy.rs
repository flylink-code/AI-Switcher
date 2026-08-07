//! Detect OS HTTP(S) proxy settings for outbound Google / Cloud Code calls.
//!
//! Browser OAuth can succeed while Rust `reqwest` fails when the machine only
//! reaches Google through the Windows system proxy (common with local VPN clients).

use std::sync::OnceLock;

use log::{info, warn};
use reqwest::Proxy;

/// Best-effort proxy URL for outbound HTTPS (e.g. `http://127.0.0.1:17891`).
pub fn outbound_proxy_url() -> Option<&'static str> {
    static URL: OnceLock<Option<String>> = OnceLock::new();
    URL.get_or_init(detect_outbound_proxy)
        .as_deref()
}

/// Attach the detected system proxy to a reqwest client builder (async or blocking).
pub fn apply_to_builder<T>(mut builder: T) -> T
where
    T: ProxyConfigurable,
{
    if let Some(url) = outbound_proxy_url() {
        match Proxy::all(url) {
            Ok(proxy) => {
                info!("Outbound HTTP client using system proxy: {url}");
                builder = builder.with_proxy(proxy);
            }
            Err(error) => {
                warn!("Invalid system proxy URL `{url}`: {error}");
            }
        }
    }
    builder
}

pub trait ProxyConfigurable {
    fn with_proxy(self, proxy: Proxy) -> Self;
}

impl ProxyConfigurable for reqwest::ClientBuilder {
    fn with_proxy(self, proxy: Proxy) -> Self {
        self.proxy(proxy)
    }
}

impl ProxyConfigurable for reqwest::blocking::ClientBuilder {
    fn with_proxy(self, proxy: Proxy) -> Self {
        self.proxy(proxy)
    }
}

fn detect_outbound_proxy() -> Option<String> {
    if let Some(url) = proxy_from_env() {
        info!("Detected outbound proxy from environment: {url}");
        return Some(url);
    }
    #[cfg(windows)]
    {
        if let Some(url) = proxy_from_wininet() {
            info!("Detected outbound proxy from Windows Internet Settings: {url}");
            return Some(url);
        }
    }
    None
}

fn proxy_from_env() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
    {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return normalize_proxy_url(trimmed);
            }
        }
    }
    None
}

#[cfg(windows)]
fn proxy_from_wininet() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    parse_wininet_proxy_server(&server)
}

#[cfg(windows)]
fn parse_wininet_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if !server.contains('=') {
        return normalize_proxy_url(server);
    }

    let mut https = None;
    let mut http = None;
    let mut socks = None;
    for part in server.split(';') {
        let mut parts = part.splitn(2, '=');
        let key = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let value = parts.next().unwrap_or_default().trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "https" => https = Some(value.to_string()),
            "http" => http = Some(value.to_string()),
            "socks" | "socks5" => socks = Some(value.to_string()),
            _ => {}
        }
    }
    let host = https
        .as_deref()
        .or(http.as_deref())
        .or(socks.as_deref())?;
    if socks.is_some() && https.is_none() && http.is_none() {
        if host.contains("://") {
            normalize_proxy_url(host)
        } else {
            normalize_proxy_url(&format!("socks5://{host}"))
        }
    } else {
        normalize_proxy_url(host)
    }
}

fn normalize_proxy_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        Some(value.to_string())
    } else if value.starts_with("socks") {
        Some(format!("socks5://{value}"))
    } else {
        Some(format!("http://{value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_http_scheme() {
        assert_eq!(
            normalize_proxy_url("127.0.0.1:17891").as_deref(),
            Some("http://127.0.0.1:17891")
        );
    }

    #[cfg(windows)]
    #[test]
    fn parses_simple_and_protocol_specific_wininet() {
        assert_eq!(
            parse_wininet_proxy_server("127.0.0.1:17891").as_deref(),
            Some("http://127.0.0.1:17891")
        );
        assert_eq!(
            parse_wininet_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }
}
