//! Cloud Code v1internal upstream client (independent implementation).

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const UPSTREAM_FALLBACKS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal",
    "https://cloudcode-pa.googleapis.com/v1internal",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal",
];
const GENERATE_CONNECT_SECS: u64 = 5;
const GENERATE_TIMEOUT_SECS: u64 = 600;

/// Skip the daily cluster for this long after a URL-level 429.
const DAILY_HOST_LIMITED_TTL: Duration = Duration::from_secs(90);

/// Same Cloud Code client fingerprint as quota probes. A generic `antigravity`
/// UA is accepted inconsistently and can 429 newer Gemini variants.
const USER_AGENT: &str = "vscode/1.X.X (Antigravity/4.3.0)";

/// Anthropic beta marker for Claude models served via Cloud Code
/// (mirrors Antigravity-Manager's claude.rs handling).
const ANTHROPIC_BETA_CLAUDE_CODE: &str = "claude-code-20250219";

/// How a Cloud Code 429 should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitKind {
    /// Daily-cluster node RPM; a single failover to production may help.
    UrlLevel,
    /// Account/project RPM on a SKU; same-host backoff only.
    AccountRateLimit,
    /// Per-model quota exhausted; bubble to dispatch for model downgrade.
    ModelQuotaExhausted,
}

impl RateLimitKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::UrlLevel => "url_rpm",
            Self::AccountRateLimit => "account_rpm",
            Self::ModelQuotaExhausted => "model_quota",
        }
    }
}

/// Classify a 429 response body. `host_index` is the index into
/// [`UPSTREAM_FALLBACKS`] (0 = daily, 1 = prod, 2 = sandbox).
pub fn classify_rate_limit_429(body: &str, host_index: usize) -> RateLimitKind {
    let lower = body.to_ascii_lowercase();
    if lower.contains("capacity on this model") {
        return RateLimitKind::ModelQuotaExhausted;
    }
    let generic = lower.contains("resource has been exhausted")
        || lower.contains("resource_exhausted")
        || lower.contains("too many requests");
    if !generic {
        return RateLimitKind::AccountRateLimit;
    }
    // Generic "resource exhausted" on the daily host is often URL-level; on
    // production it is usually account/project RPM and must not fan out to sandbox.
    if host_index == 0 {
        RateLimitKind::UrlLevel
    } else {
        RateLimitKind::AccountRateLimit
    }
}

/// Classify a 429 body when the originating host is unknown (e.g. gateway logs).
pub fn classify_rate_limit_body(body: &str) -> RateLimitKind {
    classify_rate_limit_429(body, usize::MAX)
}

/// Helper to detect URL/node-level rate limits (e.g. "Resource has been exhausted" on daily cluster)
/// where failing over to production cloudcode-pa endpoint can succeed (mirrors sub2api).
pub fn is_url_level_rate_limit(body: &str) -> bool {
    matches!(
        classify_rate_limit_429(body, 0),
        RateLimitKind::UrlLevel
    )
}

/// Parse the `Retry-After` header as whole seconds (integer form only).
pub fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after)
}

fn parse_retry_after(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

/// Seconds to back off before retrying a 503/529 on the same host
/// (exponential 10s → 20s, mirroring Antigravity-Manager's 10s~60s policy).
fn server_error_backoff(attempt: u32) -> Duration {
    Duration::from_millis(400 * (1u64 << attempt.min(2)))
}

fn rpm_backoff_delay(attempt: u32) -> Duration {
    match attempt {
        1 => Duration::from_millis(400),
        _ => Duration::from_millis(1200),
    }
}

fn rebuild_rate_limited_response(
    text: String,
    retry_after: Option<u64>,
) -> reqwest::Response {
    let mut builder = http::Response::builder().status(429);
    if let Some(secs) = retry_after {
        builder = builder.header("retry-after", secs.to_string());
    }
    let http_response = builder
        .body(text)
        .unwrap_or_else(|_| http::Response::new(String::new()));
    reqwest::Response::from(http_response)
}

fn rebuild_status_response(status: reqwest::StatusCode, text: String) -> reqwest::Response {
    let http_response = http::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(text)
        .unwrap_or_else(|_| http::Response::new(String::new()));
    reqwest::Response::from(http_response)
}

fn classify_reqwest_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_decode() {
        "decode"
    } else if error.is_body() {
        "body"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    }
    .to_string()
}

fn format_network_error(kind: &str, hosts_tried: u32, elapsed: Duration) -> String {
    let proxy = crate::antigravity::outbound::current_effective_proxy()
        .or_else(crate::system_proxy::outbound_proxy_url)
        .map(|url| format!(" via {url}"))
        .unwrap_or_default();
    format!(
        "network/{kind}: {hosts_tried} hosts unreachable in {}ms{proxy}",
        elapsed.as_millis()
    )
}

#[derive(Clone)]
pub struct UpstreamClient {
    client: std::sync::Arc<RwLock<Client>>,
    daily_limited_until: Arc<RwLock<Option<Instant>>>,
}

impl UpstreamClient {
    pub fn new() -> Self {
        Self {
            client: std::sync::Arc::new(RwLock::new(
                crate::antigravity::outbound::build_async_client(
                    GENERATE_CONNECT_SECS,
                    GENERATE_TIMEOUT_SECS,
                ),
            )),
            daily_limited_until: Arc::new(RwLock::new(None)),
        }
    }

    fn daily_host_limited(&self) -> bool {
        self.daily_limited_until
            .read()
            .ok()
            .and_then(|guard| *guard)
            .is_some_and(|until| Instant::now() < until)
    }

    fn mark_daily_limited(&self) {
        if let Ok(mut guard) = self.daily_limited_until.write() {
            *guard = Some(Instant::now() + DAILY_HOST_LIMITED_TTL);
        }
    }

    fn clear_daily_limited(&self) {
        if let Ok(mut guard) = self.daily_limited_until.write() {
            *guard = None;
        }
    }

    /// Rebuild the underlying HTTP client after outbound proxy settings change.
    pub fn reload(&self) {
        if let Ok(mut guard) = self.client.write() {
            *guard = crate::antigravity::outbound::build_async_client(
                GENERATE_CONNECT_SECS,
                GENERATE_TIMEOUT_SECS,
            );
        }
    }

    fn http(&self) -> Client {
        self.client
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| {
                crate::antigravity::outbound::build_async_client(
                    GENERATE_CONNECT_SECS,
                    GENERATE_TIMEOUT_SECS,
                )
            })
    }

    pub async fn fetch_project_id(&self, access_token: &str) -> AppResult<String> {
        crate::antigravity::quota::resolve_project_id(access_token).await
    }

    pub async fn generate(
        &self,
        access_token: &str,
        body: &Value,
        stream: bool,
    ) -> AppResult<reqwest::Response> {
        let method = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query = if stream { Some("alt=sse") } else { None };
        let is_claude = body
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|model| model.starts_with("claude-"));
        let mut last_error = String::from("upstream request failed");
        let mut last_was_network = false;
        let started = Instant::now();
        let client = self.http();
        let mut url_fallback_used = false;
        let mut hosts_tried = 0u32;
        for (idx, base) in UPSTREAM_FALLBACKS.iter().enumerate() {
            if idx == 0 && self.daily_host_limited() {
                log::debug!("Antigravity skipping daily host (recent URL-level 429)");
                continue;
            }
            if idx == 2 && last_was_network {
                log::warn!(
                    "Antigravity skipping sandbox host after network failure on earlier Cloud Code hosts"
                );
                break;
            }
            let url = match query {
                Some(q) => format!("{base}:{method}?{q}"),
                None => format!("{base}:{method}"),
            };
            let mut server_error_attempt = 0u32;
            let mut retry_after_attempts = 0u32;
            let mut rpm_backoff_attempt = 0u32;
            hosts_tried += 1;
            loop {
                let mut request = client
                    .post(&url)
                    .bearer_auth(access_token)
                    .header("Content-Type", "application/json")
                    .header("User-Agent", USER_AGENT)
                    .header("x-client-name", "antigravity")
                    .json(body);
                if is_claude {
                    request = request.header("anthropic-beta", ANTHROPIC_BETA_CLAUDE_CODE);
                }
                // Intentionally omit x-goog-user-project: consumer Antigravity OAuth
                // often gets a phantom cloudaicompanionProject that cannot enable
                // cloudcode-pa (403 SERVICE_DISABLED) when that header is set.
                match request.send().await {
                    Ok(response) => {
                        let status = response.status();
                        if status.is_success() {
                            log::info!("Antigravity generate {method} {url} → {status}");
                            self.clear_daily_limited();
                            return Ok(response);
                        }
                        if status.as_u16() == 429 {
                            let retry_after = retry_after_secs(&response);
                            if retry_after_attempts < 2 && retry_after.is_some_and(|s| s <= 5) {
                                retry_after_attempts += 1;
                                tokio::time::sleep(Duration::from_secs(
                                    retry_after.unwrap_or(1).max(1),
                                ))
                                .await;
                                continue;
                            }
                            let text = response.text().await.unwrap_or_default();
                            let kind = classify_rate_limit_429(&text, idx);
                            last_error = format!("upstream 429: {text}");
                            match kind {
                                RateLimitKind::UrlLevel
                                    if idx == 0 && !url_fallback_used =>
                                {
                                    url_fallback_used = true;
                                    self.mark_daily_limited();
                                    let next_base = UPSTREAM_FALLBACKS[1];
                                    log::warn!(
                                        "Antigravity URL fallback (429): {url} → {next_base} ({})",
                                        text.chars().take(120).collect::<String>()
                                    );
                                    tokio::time::sleep(Duration::from_millis(150)).await;
                                    break;
                                }
                                RateLimitKind::ModelQuotaExhausted
                                    if rpm_backoff_attempt < 1 =>
                                {
                                    rpm_backoff_attempt += 1;
                                    log::debug!(
                                        "Antigravity 429 model quota on {url}; same-host retry {rpm_backoff_attempt}/1"
                                    );
                                    tokio::time::sleep(rpm_backoff_delay(rpm_backoff_attempt)).await;
                                    continue;
                                }
                                RateLimitKind::ModelQuotaExhausted => {
                                    log::warn!(
                                        "Antigravity model quota exhausted: {}",
                                        text.chars().take(180).collect::<String>()
                                    );
                                    return Ok(rebuild_rate_limited_response(text, retry_after));
                                }
                                _ => {
                                    log::warn!(
                                        "Antigravity account RPM exhausted on {url}: {}",
                                        text.chars().take(180).collect::<String>()
                                    );
                                    return Ok(rebuild_rate_limited_response(text, retry_after));
                                }
                            }
                        }
                        if status.as_u16() == 401 {
                            return Ok(response);
                        }
                        // Request-body errors will fail the same way on every host.
                        // Returning them as-is keeps the real 400 from being laundered
                        // into a later 429 / 502 after endpoint failover.
                        if matches!(status.as_u16(), 400 | 422) {
                            let text = response.text().await.unwrap_or_default();
                            log::warn!(
                                "Antigravity generate {method} {url} → {status}: {}",
                                text.chars().take(180).collect::<String>()
                            );
                            return Ok(rebuild_status_response(status, text));
                        }
                        // 503/529 are usually transient: back off on the same host before
                        // failing over to the next Cloud Code host.
                        if matches!(status.as_u16(), 503 | 529) && server_error_attempt < 2 {
                            tokio::time::sleep(server_error_backoff(server_error_attempt)).await;
                            server_error_attempt += 1;
                            continue;
                        }
                        // Endpoint-specific 403/404/5xx should try the next Cloud Code host
                        // (sandbox often 403s while production still works).
                        let text = response.text().await.unwrap_or_default();
                        last_error = format!("upstream {status}: {text}");
                        log::warn!(
                            "Antigravity generate {method} {url} → {status}: {}",
                            text.chars().take(180).collect::<String>()
                        );
                        break;
                    }
                    Err(error) => {
                        last_was_network = true;
                        last_error = classify_reqwest_error(&error);
                        log::warn!(
                            "Antigravity generate {method} {url} network error: {last_error}"
                        );
                        break;
                    }
                }
            }
        }
        if last_was_network {
            Err(AppError::Network(format_network_error(
                &last_error,
                hosts_tried,
                started.elapsed(),
            )))
        } else {
            Err(AppError::Other(last_error))
        }
    }
}

impl Default for UpstreamClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrap a Gemini-style request body into Cloud Code v1internal envelope.
pub fn wrap_v1internal(project_id: &str, model: &str, request: Value) -> Value {
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let random_hex = uuid::Uuid::new_v4().simple().to_string();
    let random_hex = &random_hex[..8];
    json!({
        "project": project_id,
        "model": model,
        "requestId": format!("agent/{timestamp_ms}/{random_hex}"),
        "request": request,
        "userAgent": USER_AGENT,
        "requestType": "agent",
    })
}

pub fn unwrap_v1internal(response: &Value) -> Value {
    response
        .get("response")
        .cloned()
        .unwrap_or_else(|| response.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_retry_after_seconds() {
        assert_eq!(parse_retry_after("5"), Some(5));
        assert_eq!(parse_retry_after(" 3 "), Some(3));
        assert_eq!(parse_retry_after("0"), Some(0));
        // HTTP-date form is not supported — treated as absent.
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn server_error_backoff_grows_exponentially() {
        assert_eq!(server_error_backoff(0), Duration::from_millis(400));
        assert_eq!(server_error_backoff(1), Duration::from_millis(800));
        assert_eq!(server_error_backoff(2), Duration::from_millis(1600));
        assert_eq!(server_error_backoff(9), Duration::from_millis(1600));
    }

    #[test]
    fn classifies_model_quota_vs_account_rpm() {
        assert_eq!(
            classify_rate_limit_429(
                r#"{"error":{"message":"capacity on this model is full"}}"#,
                1
            ),
            RateLimitKind::ModelQuotaExhausted
        );
        assert_eq!(
            classify_rate_limit_429(
                r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#,
                0
            ),
            RateLimitKind::UrlLevel
        );
        assert_eq!(
            classify_rate_limit_429(
                r#"{"error":{"message":"Resource has been exhausted (e.g. check quota)."}}"#,
                1
            ),
            RateLimitKind::AccountRateLimit
        );
        assert!(is_url_level_rate_limit(
            r#"{"error":{"message":"Resource has been exhausted"}}"#
        ));
    }

    #[test]
    fn rebuilds_client_error_status_without_laundering() {
        let response = rebuild_status_response(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"missing thought_signature"}}"#.into(),
        );
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn network_error_mentions_kind_and_host_count() {
        let message = format_network_error("connect", 3, Duration::from_millis(1200));
        assert!(message.starts_with("network/connect:"));
        assert!(message.contains("3 hosts"));
        assert!(message.contains("1200ms"));
        if let Some(proxy) = crate::antigravity::outbound::current_effective_proxy()
            .or_else(crate::system_proxy::outbound_proxy_url)
        {
            assert!(
                message.contains(&proxy),
                "network diagnostic must include outbound proxy {proxy}: {message}"
            );
        }
    }
}
