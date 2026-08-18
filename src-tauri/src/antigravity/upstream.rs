//! Cloud Code v1internal upstream client (independent implementation).

use std::sync::RwLock;
use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const UPSTREAM_FALLBACKS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal",
    "https://cloudcode-pa.googleapis.com/v1internal",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal",
];

/// Same Cloud Code client fingerprint as quota probes. A generic `antigravity`
/// UA is accepted inconsistently and can 429 newer Gemini variants.
const USER_AGENT: &str = "vscode/1.X.X (Antigravity/4.3.0)";

/// Anthropic beta marker for Claude models served via Cloud Code
/// (mirrors Antigravity-Manager's claude.rs handling).
const ANTHROPIC_BETA_CLAUDE_CODE: &str = "claude-code-20250219";

/// Helper to detect URL/node-level rate limits (e.g. "Resource has been exhausted" on daily cluster)
/// where failing over to production cloudcode-pa endpoint can succeed (mirrors sub2api).
pub fn is_url_level_rate_limit(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    (lower.contains("resource has been exhausted")
        || lower.contains("resource_exhausted")
        || lower.contains("too many requests"))
        && !lower.contains("capacity on this model")
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
    Duration::from_secs(10 << attempt.min(2))
}

#[derive(Clone)]
pub struct UpstreamClient {
    client: std::sync::Arc<RwLock<Client>>,
}

impl UpstreamClient {
    pub fn new() -> Self {
        Self {
            client: std::sync::Arc::new(RwLock::new(
                crate::antigravity::outbound::build_async_client(20, 600),
            )),
        }
    }

    /// Rebuild the underlying HTTP client after outbound proxy settings change.
    pub fn reload(&self) {
        if let Ok(mut guard) = self.client.write() {
            *guard = crate::antigravity::outbound::build_async_client(20, 600);
        }
    }

    fn http(&self) -> Client {
        self.client
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| crate::antigravity::outbound::build_async_client(20, 600))
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
        let client = self.http();
        for (idx, base) in UPSTREAM_FALLBACKS.iter().enumerate() {
            let has_next_url = idx + 1 < UPSTREAM_FALLBACKS.len();
            let url = match query {
                Some(q) => format!("{base}:{method}?{q}"),
                None => format!("{base}:{method}"),
            };
            let mut server_error_attempt = 0u32;
            let mut retried_429_in_place = false;
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
                            return Ok(response);
                        }
                        // 429: honor a short Retry-After in place once; otherwise check
                        // for URL fallback (daily → prod) or bubble for pool failover.
                        if status.as_u16() == 429 {
                            let retry_after = retry_after_secs(&response);
                            if !retried_429_in_place && retry_after.is_some_and(|s| s <= 2) {
                                retried_429_in_place = true;
                                tokio::time::sleep(Duration::from_secs(retry_after.unwrap_or(1).max(1)))
                                    .await;
                                continue;
                            }
                            if has_next_url {
                                let text = response.text().await.unwrap_or_default();
                                if is_url_level_rate_limit(&text) {
                                    let next_base = UPSTREAM_FALLBACKS[idx + 1];
                                    log::warn!(
                                        "Antigravity URL fallback (429): {url} → {next_base} ({})",
                                        text.chars().take(120).collect::<String>()
                                    );
                                    tokio::time::sleep(Duration::from_millis(150)).await;
                                    break;
                                }
                                last_error = format!("upstream 429: {text}");
                                log::warn!("Antigravity account quota exhausted: {text}");
                                break;
                            }
                            return Ok(response);
                        }
                        if status.as_u16() == 401 {
                            return Ok(response);
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
                        last_error = error.to_string();
                        break;
                    }
                }
            }
        }
        Err(AppError::Other(last_error))
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
        assert_eq!(server_error_backoff(0), Duration::from_secs(10));
        assert_eq!(server_error_backoff(1), Duration::from_secs(20));
        assert_eq!(server_error_backoff(2), Duration::from_secs(40));
        // Capped shift — attempts beyond 2 stay at 40s.
        assert_eq!(server_error_backoff(9), Duration::from_secs(40));
    }
}
