//! Cloud Code v1internal upstream client (independent implementation).

use std::time::Duration;

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
    client: Client,
}

impl UpstreamClient {
    pub fn new() -> Self {
        let client = crate::system_proxy::apply_to_builder(
            Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(600))
                .pool_max_idle_per_host(8)
                .user_agent(USER_AGENT),
        )
        .build()
        .expect("antigravity upstream client");
        Self { client }
    }

    pub async fn fetch_project_id(&self, access_token: &str) -> AppResult<String> {
        let body = json!({
            "metadata": { "ideType": "ANTIGRAVITY" }
        });
        let mut last_error = String::from("loadCodeAssist failed");
        for base in UPSTREAM_FALLBACKS {
            let url = format!("{base}:loadCodeAssist");
            match self
                .client
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
        for base in UPSTREAM_FALLBACKS {
            let url = match query {
                Some(q) => format!("{base}:{method}?{q}"),
                None => format!("{base}:{method}"),
            };
            let mut request = self
                .client
                .post(&url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("x-client-name", "antigravity")
                .json(body);
            if let Some(project) = body.get("project").and_then(Value::as_str) {
                if !project.is_empty() && project != "test-project" {
                    request = request.header("x-goog-user-project", project);
                }
            }
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() || status.as_u16() < 500 {
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
