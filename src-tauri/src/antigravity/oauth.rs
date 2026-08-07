//! Browser Google OAuth login for Antigravity accounts.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use super::account::{
    store, AntigravityAccount, AntigravityAccountPublic, AntigravityToken, OAUTH_CLIENT_ID,
    OAUTH_CLIENT_SECRET,
};
use crate::error::{AppError, AppResult};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const OAUTH_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityOauthStart {
    pub auth_url: String,
    pub redirect_uri: String,
}

/// Open the system browser for Google login and wait for the localhost callback.
pub async fn login_with_browser(app: &AppHandle) -> AppResult<AntigravityAccountPublic> {
    match login_with_browser_inner(app).await {
        Ok(account) => {
            log::info!("Antigravity OAuth login succeeded for {}", account.email);
            Ok(account)
        }
        Err(error) => {
            log::error!("Antigravity OAuth login failed: {error}");
            Err(error)
        }
    }
}

async fn login_with_browser_inner(app: &AppHandle) -> AppResult<AntigravityAccountPublic> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| AppError::Io(format!("无法启动 OAuth 回调监听: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| AppError::Io(format!("无法读取回调端口: {error}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
    let state = Uuid::new_v4().to_string();
    let auth_url = build_auth_url(&redirect_uri, &state)?;

    app.opener()
        .open_url(auth_url, None::<&str>)
        .map_err(|error| AppError::Other(format!("无法打开浏览器: {error}")))?;

    let code = tokio::time::timeout(
        Duration::from_secs(OAUTH_TIMEOUT_SECS),
        wait_for_auth_code(listener, &state),
    )
    .await
    .map_err(|_| AppError::Other("OAuth 登录超时，请重试并在浏览器中完成授权".into()))??;

    let token = exchange_code(&code, &redirect_uri).await?;
    let (email, name) = fetch_userinfo(&token.access_token).await?;
    let now = Utc::now().timestamp();
    let account = AntigravityAccount {
        id: Uuid::new_v4().to_string(),
        email: email.clone(),
        name,
        token: AntigravityToken {
            access_token: token.access_token,
            refresh_token: token.refresh_token.ok_or_else(|| {
                AppError::Other(
                    "未拿到 refresh_token（请确认授权时选择了完整权限，或撤销后重试）".into(),
                )
            })?,
            expires_in: token.expires_in,
            expiry_timestamp: now + token.expires_in,
            token_type: "Bearer".into(),
            email: Some(email),
            project_id: None,
            session_id: None,
        },
        disabled: false,
        disabled_reason: None,
        is_active: false,
        created_at: now,
        last_used: now,
        health_score: 1.0,
        cooldown_until: None,
        remaining_quota: None,
        quota: None,
    };

    store().upsert_account(account)
}

fn build_auth_url(redirect_uri: &str, state: &str) -> AppResult<String> {
    let scopes = [
        "openid",
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs",
    ]
    .join(" ");
    let url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", OAUTH_CLIENT_ID),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", &scopes),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "true"),
            ("state", state),
        ],
    )
    .map_err(|error| AppError::Other(format!("构造授权链接失败: {error}")))?;
    Ok(url.to_string())
}

async fn wait_for_auth_code(listener: TcpListener, expected_state: &str) -> AppResult<String> {
    let (mut socket, _) = listener
        .accept()
        .await
        .map_err(|error| AppError::Io(format!("等待 OAuth 回调失败: {error}")))?;
    let mut buf = vec![0u8; 8192];
    let n = socket
        .read(&mut buf)
        .await
        .map_err(|error| AppError::Io(format!("读取 OAuth 回调失败: {error}")))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line.split_whitespace().nth(1).unwrap_or_default();
    let query = path.split('?').nth(1).unwrap_or_default();
    let params = parse_query(query);
    let html = if params.get("state").map(String::as_str) != Some(expected_state) {
        fail_html("OAuth state mismatch")
    } else if let Some(error) = params.get("error") {
        fail_html(error)
    } else if params.get("code").is_some() {
        success_html().to_string()
    } else {
        fail_html("missing authorization code")
    };
    let _ = socket.write_all(html.as_bytes()).await;
    let _ = socket.flush().await;

    if let Some(error) = params.get("error") {
        return Err(AppError::Other(format!("Google 授权失败: {error}")));
    }
    if params.get("state").map(String::as_str) != Some(expected_state) {
        return Err(AppError::Other("OAuth state 校验失败，请重试".into()));
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| AppError::Other("回调中缺少 authorization code".into()))
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if key.is_empty() {
            continue;
        }
        map.insert(
            urlencoding_decode(key),
            urlencoding_decode(value),
        );
    }
    map
}

fn urlencoding_decode(value: &str) -> String {
    let bytes: Vec<u8> = {
        let mut out = Vec::new();
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '+' => {
                    out.push(b' ');
                    i += 1;
                }
                '%' if i + 2 < chars.len() => {
                    let hex = format!("{}{}", chars[i + 1], chars[i + 2]);
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        out.push(byte);
                        i += 3;
                    } else {
                        out.push(b'%');
                        i += 1;
                    }
                }
                c => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    i += 1;
                }
            }
        }
        out
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

struct ExchangedToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

async fn exchange_code(code: &str, redirect_uri: &str) -> AppResult<ExchangedToken> {
    let client = crate::antigravity::outbound::build_async_client(15, 30);
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|error| {
            let proxy = crate::antigravity::outbound::current_effective_proxy()
                .or_else(crate::system_proxy::outbound_proxy_url)
                .unwrap_or_else(|| "未配置".into());
            AppError::Other(format!(
                "兑换授权码失败: {error}。若浏览器能打开 Google 但应用失败，请检查 Antigravity 出站代理（当前：{proxy}）"
            ))
        })?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::Other(format!("读取 Token 响应失败: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| AppError::Other(format!("解析 Token 响应失败: {error}")))?;
    if !status.is_success() {
        let message = value
            .get("error_description")
            .or_else(|| value.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or(body.as_str());
        return Err(AppError::Other(format!("兑换授权码失败: {message}")));
    }
    let access_token = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("Token 响应缺少 access_token".into()))?
        .to_string();
    let refresh_token = value
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let expires_in = value
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);
    Ok(ExchangedToken {
        access_token,
        refresh_token,
        expires_in,
    })
}

async fn fetch_userinfo(access_token: &str) -> AppResult<(String, Option<String>)> {
    let client = crate::antigravity::outbound::build_async_client(15, 20);
    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| AppError::Other(format!("获取用户信息失败: {error}")))?;
    let status = response.status();
    let value = response
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        return Err(AppError::Other(format!("获取用户信息失败: {status}")));
    }
    let email = value
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown@account")
        .to_string();
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok((email, name))
}

fn success_html() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
    <html><body style='font-family:sans-serif;text-align:center;padding:48px'>\
    <h1 style='color:green'>授权成功</h1>\
    <p>可以关闭此窗口，返回 AI-Switcher。</p>\
    <script>setTimeout(function(){window.close()},1500)</script>\
    </body></html>"
}

fn fail_html(message: &str) -> String {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
        <html><body style='font-family:sans-serif;text-align:center;padding:48px'>\
        <h1 style='color:#c00'>授权失败</h1>\
        <p>{}</p>\
        <p>请返回应用重试。</p>\
        </body></html>",
        html_escape(message)
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
