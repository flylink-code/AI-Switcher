//! Call the Kiro IDE endpoint and return the raw event-stream body.

use bytes::Bytes;
use uuid::Uuid;

use super::account::KiroAccount;
use super::outbound::build_async_client;

pub struct UpstreamReply {
    pub status: u16,
    pub body: Bytes,
    pub network_error: Option<String>,
}

pub async fn generate(account: &KiroAccount, payload: &serde_json::Value) -> UpstreamReply {
    let region = if account.region.trim().is_empty() {
        "us-east-1"
    } else {
        account.region.trim()
    };
    let url = format!("https://q.{region}.amazonaws.com/generateAssistantResponse");
    let client = build_async_client(180);
    let user_agent = format!(
        "aws-sdk-js/1.0.34 ua/2.1 os/{} lang/js md/nodejs#20.0.0 api/codewhispererstreaming#1.0.34 m/E KiroIDE-0.7.0-{}",
        std::env::consts::OS,
        account.machine_id
    );
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-amzn-codewhisperer-optout", "true")
        .header("x-amzn-kiro-agent-mode", "vibe")
        .header("user-agent", user_agent)
        .header("amz-sdk-invocation-id", Uuid::new_v4().to_string())
        .header("amz-sdk-request", "attempt=1; max=3")
        .header(
            "authorization",
            format!("Bearer {}", account.access_token.trim()),
        )
        .json(payload)
        .send()
        .await;
    match response {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.bytes().await.unwrap_or_default();
            UpstreamReply {
                status,
                body,
                network_error: None,
            }
        }
        Err(error) => UpstreamReply {
            status: 0,
            body: Bytes::new(),
            network_error: Some(error.to_string()),
        },
    }
}
