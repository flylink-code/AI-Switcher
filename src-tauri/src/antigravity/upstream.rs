//! Cloud Code v1internal upstream client (independent implementation).

use std::sync::RwLock;

use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const UPSTREAM_FALLBACKS: [&str; 3] = [
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal",
    "https://daily-cloudcode-pa.googleapis.com/v1internal",
    "https://cloudcode-pa.googleapis.com/v1internal",
];

const USER_AGENT: &str = "antigravity";

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
        let body = json!({
            "metadata": { "ideType": "ANTIGRAVITY" }
        });
        let mut last_error = String::from("loadCodeAssist failed");
        let client = self.http();
        for base in UPSTREAM_FALLBACKS {
            let url = format!("{base}:loadCodeAssist");
            match client
                .post(&url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("x-client-name", "antigravity")
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let value = response.json::<Value>().await.unwrap_or(Value::Null);
                    if status.is_success() {
                        if let Some(project) = value
                            .get("cloudaicompanionProject")
                            .and_then(Value::as_str)
                            .filter(|value| !value.is_empty())
                        {
                            return Ok(project.to_string());
                        }
                        last_error = "账号无法获取 cloudaicompanionProject".into();
                    } else {
                        last_error = format!("loadCodeAssist {status}: {value}");
                    }
                }
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(AppError::Other(last_error))
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
        let mut last_error = String::from("upstream request failed");
        let client = self.http();
        for base in UPSTREAM_FALLBACKS {
            let url = match query {
                Some(q) => format!("{base}:{method}?{q}"),
                None => format!("{base}:{method}"),
            };
            let request = client
                .post(&url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("x-client-name", "antigravity")
                .json(body);
            // Intentionally omit x-goog-user-project: consumer Antigravity OAuth
            // often gets a phantom cloudaicompanionProject that cannot enable
            // cloudcode-pa (403 SERVICE_DISABLED) when that header is set.
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }
                    // Account-level auth / rate limits should bubble for pool failover.
                    // Endpoint-specific 403/404/5xx should try the next Cloud Code host
                    // (sandbox often 403s while production still works).
                    if matches!(status.as_u16(), 401 | 429) {
                        return Ok(response);
                    }
                    let text = response.text().await.unwrap_or_default();
                    last_error = format!("upstream {status}: {text}");
                }
                Err(error) => last_error = error.to_string(),
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
