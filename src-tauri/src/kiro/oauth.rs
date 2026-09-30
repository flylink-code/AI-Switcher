//! Builder ID device-code login and Social PKCE login.

use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::account::{b64url, expires_after, store, KiroAccount, KiroAccountPublic, DEFAULT_REGION};
use super::outbound::build_async_client;

const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
const PORTAL_URL: &str = "https://app.kiro.dev";
const SOCIAL_AUTH: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
const CALLBACK_PORTS: &[u16] = &[3128, 4649, 6588, 8008, 9091, 49153, 50153, 51153, 52153, 53153];
const SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];

pub async fn login_builder_id(app: &AppHandle) -> AppResult<KiroAccountPublic> {
    let client = build_async_client(30);
    let region = DEFAULT_REGION;
    let registered: RegisterResponse = send_json(
        &client,
        &format!("https://oidc.{region}.amazonaws.com/client/register"),
        &serde_json::json!({
            "clientName": "AI-Switcher",
            "clientType": "public",
            "scopes": SCOPES,
            "grantTypes": [
                "urn:ietf:params:oauth:grant-type:device_code",
                "refresh_token"
            ],
            "issuerUrl": BUILDER_ID_START_URL
        }),
    )
    .await?;
    let device: DeviceResponse = send_json(
        &client,
        &format!("https://oidc.{region}.amazonaws.com/device_authorization"),
        &serde_json::json!({
            "clientId": registered.client_id,
            "clientSecret": registered.client_secret,
            "startUrl": BUILDER_ID_START_URL
        }),
    )
    .await?;
    let open_url = device
        .verification_uri_complete
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or(device.verification_uri.clone());
    let _ = app.opener().open_url(open_url, None::<&str>);
    let interval = device.interval.max(1) as u64;
    let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in.max(30) as u64);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(AppError::Config("Builder ID 登录超时".into()));
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let response = client
            .post(format!("https://oidc.{region}.amazonaws.com/token"))
            .json(&serde_json::json!({
                "clientId": registered.client_id,
                "clientSecret": registered.client_secret,
                "grantType": "urn:ietf:params:oauth:grant-type:device_code",
                "deviceCode": device.device_code
            }))
            .send()
            .await
            .map_err(|error| AppError::Network(error.to_string()))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            let token: TokenResponse = serde_json::from_str(&body)?;
            return store().upsert(account_from_token(
                "idc",
                "BuilderId",
                registered.client_id.clone(),
                registered.client_secret.clone(),
                token,
            ));
        }
        if body.contains("authorization_pending") || body.contains("slow_down") {
            continue;
        }
        if body.contains("expired_token") {
            return Err(AppError::Config("Builder ID 设备码已过期".into()));
        }
        return Err(AppError::Other(format!("Builder ID 登录失败 HTTP {status}: {body}")));
    }
}

pub async fn login_social(app: &AppHandle) -> AppResult<KiroAccountPublic> {
    let (verifier, challenge) = pkce();
    let state = Uuid::new_v4().simple().to_string();
    let (port, listener) = bind_callback().await?;
    let redirect = format!("http://127.0.0.1:{port}/oauth/callback");
    let url = format!(
        "{PORTAL_URL}/signin?state={}&code_challenge={}&code_challenge_method=S256&redirect_uri={}&redirect_from=KiroIDE",
        urlencoding(&state),
        urlencoding(&challenge),
        urlencoding(&redirect)
    );
    let _ = app.opener().open_url(url, None::<&str>);
    let code = wait_callback(listener, &state).await?;
    let client = build_async_client(30);
    let token: SocialToken = send_json(
        &client,
        &format!("{SOCIAL_AUTH}/oauth/token"),
        &serde_json::json!({
            "code": code,
            "code_verifier": verifier,
            "redirect_uri": redirect
        }),
    )
    .await?;
    store().upsert(KiroAccount {
        id: String::new(),
        label: String::new(),
        auth_method: "social".into(),
        provider: "Google".into(),
        refresh_token: token.refresh_token.unwrap_or_default(),
        access_token: token.access_token,
        expires_at: token
            .expires_in
            .map(expires_after)
            .or(token.expires_at),
        client_id: String::new(),
        client_secret: String::new(),
        profile_arn: token.profile_arn.unwrap_or_default(),
        machine_id: String::new(),
        region: DEFAULT_REGION.into(),
        disabled: false,
        disable_reason: String::new(),
        cooldown_until_ms: 0,
        quota: None,
    })
}

fn account_from_token(
    method: &str,
    provider: &str,
    client_id: String,
    client_secret: String,
    token: TokenResponse,
) -> KiroAccount {
    KiroAccount {
        id: String::new(),
        label: String::new(),
        auth_method: method.into(),
        provider: provider.into(),
        refresh_token: token.refresh_token.unwrap_or_default(),
        access_token: token.access_token,
        expires_at: token.expires_in.map(expires_after),
        client_id,
        client_secret,
        profile_arn: String::new(),
        machine_id: String::new(),
        region: DEFAULT_REGION.into(),
        disabled: false,
        disable_reason: String::new(),
        cooldown_until_ms: 0,
        quota: None,
    }
}

async fn send_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> AppResult<T> {
    let response = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AppError::Other(format!("Kiro 登录失败 HTTP {status}: {text}")));
    }
    Ok(serde_json::from_str(&text)?)
}

async fn bind_callback() -> AppResult<(u16, TcpListener)> {
    for port in CALLBACK_PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", *port)).await {
            return Ok((*port, listener));
        }
    }
    Err(AppError::Config("Kiro 登录回调端口都被占用".into()))
}

async fn wait_callback(listener: TcpListener, expected_state: &str) -> AppResult<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(AppError::Config("Kiro Social 登录超时".into()));
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| AppError::Config("Kiro Social 登录超时".into()))?
            .map_err(|error| AppError::Io(error.to_string()))?;
        let mut buf = vec![0u8; 4096];
        let size = stream.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..size]);
        let path = request.lines().next().unwrap_or("");
        let page = "<html><body><h2>登录成功，可以关闭这个页面。</h2></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        if let Some(code) = code_from_request(path, expected_state) {
            return Ok(code);
        }
    }
}

fn code_from_request(start_line: &str, expected_state: &str) -> Option<String> {
    let query = start_line.split('?').nth(1)?.split(' ').next()?;
    let pairs: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    if pairs.get("state").map(String::as_str) != Some(expected_state) {
        return None;
    }
    pairs.get("code").filter(|value| !value.is_empty()).cloned()
}

fn pkce() -> (String, String) {
    let bytes = Uuid::new_v4().as_bytes().to_vec();
    let mut extra = Uuid::new_v4().as_bytes().to_vec();
    extra.extend(bytes);
    let verifier = b64url(&extra);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterResponse {
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceResponse {
    device_code: String,
    #[allow(dead_code)]
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: i64,
    interval: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SocialToken {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    profile_arn: Option<String>,
}
