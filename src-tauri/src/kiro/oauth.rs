//! Builder ID device-code login and the Kiro IDE browser sign-in (Google / GitHub /
//! Builder ID / Enterprise portal).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::account::{
    b64url, expires_after, store, KiroAccount, KiroAccountPublic, DEFAULT_REGION,
};
use super::outbound::build_async_client;

const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
const PORTAL_URL: &str = "https://app.kiro.dev";
const SOCIAL_AUTH: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
const CALLBACK_PORTS: &[u16] = &[
    3128, 4649, 6588, 8008, 9091, 49153, 50153, 51153, 52153, 53153,
];
const SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];
/// Host the portal gets as `redirect_uri` base. The Kiro IDE and every reference
/// implementation send `localhost` here; the Cognito client only accepts this
/// origin form (no path), otherwise it answers `redirect_mismatch`.
const PORTAL_REDIRECT_HOST: &str = "localhost";
/// AWS IAM Identity Center redirect used for the second (OIDC) leg.
const AWS_REDIRECT_HOST: &str = "127.0.0.1";
const AWS_REGISTER_REDIRECT: &str = "http://127.0.0.1/oauth/callback";
const CALLBACK_PATHS: &[&str] = &["/oauth/callback", "/signin/callback"];
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(600);
const IDE_VERSION: &str = "1.1.70";

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
    let deadline =
        std::time::Instant::now() + Duration::from_secs(device.expires_in.max(30) as u64);
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
        return Err(AppError::Other(format!(
            "Builder ID 登录失败 HTTP {status}: {body}"
        )));
    }
}

/// Second leg of a Builder ID / Enterprise sign-in coming back through the portal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AwsStage {
    state: String,
    verifier: String,
    region: String,
    client_id: String,
    client_secret: String,
    provider: &'static str,
}

type LoginResult = AppResult<KiroAccountPublic>;

struct LoginSession {
    state: String,
    verifier: String,
    port: u16,
    aws: Mutex<Option<AwsStage>>,
    done: AtomicBool,
    result_tx: mpsc::UnboundedSender<LoginResult>,
    cancel: Notify,
}

impl LoginSession {
    fn base(&self) -> String {
        format!("http://{PORTAL_REDIRECT_HOST}:{}", self.port)
    }

    fn aws_redirect(&self) -> String {
        format!("http://{AWS_REDIRECT_HOST}:{}/oauth/callback", self.port)
    }

    fn finish(&self, result: LoginResult) {
        if !self.done.swap(true, Ordering::SeqCst) {
            let _ = self.result_tx.send(result);
        }
    }
}

static SESSION: Mutex<Option<Arc<LoginSession>>> = Mutex::new(None);

fn current_session() -> Option<Arc<LoginSession>> {
    SESSION.lock().ok().and_then(|guard| guard.clone())
}

/// Clears the global session and stops the accept loops when the login ends,
/// including when the command future is dropped.
struct SessionGuard {
    session: Arc<LoginSession>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Ok(mut guard) = SESSION.lock() {
            if guard
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.session))
            {
                *guard = None;
            }
        }
    }
}

/// What a callback request means for the current sign-in. Pure so the listener and
/// the pasted-URL path share it.
#[derive(Debug, PartialEq, Eq)]
enum CallbackAction {
    NotCallback,
    Ignore,
    Fail(String),
    Social {
        code: String,
        redirect_uri: String,
        provider: &'static str,
    },
    AwsRegister {
        issuer: String,
        region: String,
        provider: &'static str,
    },
    AwsToken {
        code: String,
        stage: AwsStage,
    },
}

fn classify_callback(
    path: &str,
    query: &HashMap<String, String>,
    state: &str,
    base: &str,
    aws: Option<&AwsStage>,
) -> CallbackAction {
    if !CALLBACK_PATHS.contains(&path) {
        return CallbackAction::NotCallback;
    }
    let get = |key: &str| query.get(key).map(String::as_str).filter(|v| !v.is_empty());
    let got_state = get("state");
    let ours = got_state == Some(state);
    let aws_leg = aws.filter(|stage| got_state == Some(stage.state.as_str()));
    if !ours && aws_leg.is_none() {
        return CallbackAction::Ignore;
    }
    if let Some(error) = get("error") {
        let detail = get("error_description").unwrap_or(error);
        return CallbackAction::Fail(detail.to_string());
    }
    if let Some(stage) = aws_leg {
        return match get("code") {
            Some(code) => CallbackAction::AwsToken {
                code: code.to_string(),
                stage: stage.clone(),
            },
            None => CallbackAction::Fail("AWS 没有返回授权码".into()),
        };
    }
    let option = get("login_option").unwrap_or("");
    match option {
        "google" | "github" => match get("code") {
            Some(code) => CallbackAction::Social {
                code: code.to_string(),
                redirect_uri: format!("{base}{path}?login_option={option}"),
                provider: if option == "google" {
                    "Google"
                } else {
                    "Github"
                },
            },
            None => CallbackAction::Fail("Kiro 没有返回授权码".into()),
        },
        "builderid" | "awsidc" | "internal" => {
            let (Some(issuer), Some(region)) = (get("issuer_url"), get("idc_region")) else {
                return CallbackAction::Fail("Kiro 登录页没有给出 AWS 登录地址".into());
            };
            if !valid_region(region) {
                return CallbackAction::Fail(format!("无法识别的 AWS 区域: {region}"));
            }
            CallbackAction::AwsRegister {
                issuer: issuer.to_string(),
                region: region.to_string(),
                provider: match option {
                    "builderid" => "BuilderId",
                    "awsidc" => "Enterprise",
                    _ => "Internal",
                },
            }
        }
        "external_idp" => CallbackAction::Fail(
            "暂不支持企业自有身份提供方登录，请改用 Google / GitHub / Builder ID".into(),
        ),
        other => CallbackAction::Fail(format!("Kiro 登录页返回了无法识别的登录方式: {other:?}")),
    }
}

fn valid_region(region: &str) -> bool {
    !region.is_empty()
        && region.len() <= 32
        && region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn portal_signin_url(state: &str, challenge: &str, base: &str) -> String {
    format!(
        "{PORTAL_URL}/signin?state={}&code_challenge={}&code_challenge_method=S256&redirect_uri={}&redirect_from=KiroIDE",
        urlencoding(state),
        urlencoding(challenge),
        urlencoding(base)
    )
}

fn aws_authorize_url(session: &LoginSession, stage: &AwsStage) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stage.verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", &stage.client_id)
        .append_pair("redirect_uri", &session.aws_redirect())
        .append_pair("scopes", &SCOPES.join(","))
        .append_pair("state", &stage.state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .finish();
    format!(
        "https://oidc.{}.amazonaws.com/authorize?{query}",
        stage.region
    )
}

pub fn kiro_ide_user_agent() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "ai-switcher".into());
    let mut hasher = Sha256::new();
    hasher.update(format!("ai-switcher:{host}").as_bytes());
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("KiroIDE-{IDE_VERSION}-{digest}")
}

enum Reply {
    NotFound,
    Ignored,
    Redirect(String),
    Done,
    Failed(String),
}

/// Runs one callback (from the listener or a pasted URL) and delivers a terminal
/// result to the waiting login task.
async fn process_callback(
    session: &Arc<LoginSession>,
    path: &str,
    query: &HashMap<String, String>,
) -> Reply {
    if session.done.load(Ordering::SeqCst) {
        return Reply::Ignored;
    }
    let aws = session.aws.lock().ok().and_then(|guard| guard.clone());
    let action = classify_callback(path, query, &session.state, &session.base(), aws.as_ref());
    match action {
        CallbackAction::NotCallback => Reply::NotFound,
        CallbackAction::Ignore => Reply::Ignored,
        CallbackAction::Fail(message) => {
            session.finish(Err(AppError::Config(message.clone())));
            Reply::Failed(message)
        }
        CallbackAction::AwsRegister {
            issuer,
            region,
            provider,
        } => match register_aws_client(session, &issuer, &region, provider).await {
            Ok(url) => Reply::Redirect(url),
            Err(error) => {
                let message = error.to_string();
                session.finish(Err(error));
                Reply::Failed(message)
            }
        },
        CallbackAction::Social {
            code,
            redirect_uri,
            provider,
        } => finish_with(
            session,
            exchange_social(session, &code, &redirect_uri, provider).await,
        ),
        CallbackAction::AwsToken { code, stage } => {
            finish_with(session, exchange_aws(session, &code, &stage).await)
        }
    }
}

fn finish_with(session: &LoginSession, result: LoginResult) -> Reply {
    match result {
        Ok(account) => {
            session.finish(Ok(account));
            Reply::Done
        }
        Err(error) => {
            let message = error.to_string();
            session.finish(Err(error));
            Reply::Failed(message)
        }
    }
}

async fn register_aws_client(
    session: &LoginSession,
    issuer: &str,
    region: &str,
    provider: &'static str,
) -> AppResult<String> {
    let client = build_async_client(30);
    let registered: RegisterResponse = send_json_ide(
        &client,
        &format!("https://oidc.{region}.amazonaws.com/client/register"),
        &serde_json::json!({
            "clientName": "Kiro IDE",
            "clientType": "public",
            "scopes": SCOPES,
            "grantTypes": ["authorization_code", "refresh_token"],
            "redirectUris": [AWS_REGISTER_REDIRECT],
            "issuerUrl": issuer
        }),
    )
    .await?;
    let (verifier, _) = pkce();
    let stage = AwsStage {
        state: random_state(),
        verifier,
        region: region.to_string(),
        client_id: registered.client_id,
        client_secret: registered.client_secret,
        provider,
    };
    let url = aws_authorize_url(session, &stage);
    if let Ok(mut guard) = session.aws.lock() {
        *guard = Some(stage);
    }
    Ok(url)
}

async fn exchange_social(
    session: &LoginSession,
    code: &str,
    redirect_uri: &str,
    provider: &str,
) -> LoginResult {
    let client = build_async_client(30);
    let token: SocialToken = send_json_ide(
        &client,
        &format!("{SOCIAL_AUTH}/oauth/token"),
        &serde_json::json!({
            "code": code,
            "code_verifier": session.verifier,
            "redirect_uri": redirect_uri
        }),
    )
    .await?;
    store().upsert(KiroAccount {
        id: String::new(),
        label: String::new(),
        auth_method: "social".into(),
        provider: provider.into(),
        refresh_token: token.refresh_token.unwrap_or_default(),
        access_token: token.access_token,
        expires_at: token.expires_in.map(expires_after).or(token.expires_at),
        client_id: String::new(),
        client_secret: String::new(),
        profile_arn: token.profile_arn.unwrap_or_default(),
        machine_id: String::new(),
        region: DEFAULT_REGION.into(),
        disabled: false,
        disable_reason: String::new(),
        cooldown_until_ms: 0,
        quota: None,
        device_profile: None,
    })
}

async fn exchange_aws(session: &LoginSession, code: &str, stage: &AwsStage) -> LoginResult {
    let client = build_async_client(30);
    let token: TokenResponse = send_json_ide(
        &client,
        &format!("https://oidc.{}.amazonaws.com/token", stage.region),
        &serde_json::json!({
            "clientId": stage.client_id,
            "clientSecret": stage.client_secret,
            "grantType": "authorization_code",
            "redirectUri": session.aws_redirect(),
            "code": code,
            "codeVerifier": stage.verifier
        }),
    )
    .await?;
    let mut account = account_from_token(
        "idc",
        stage.provider,
        stage.client_id.clone(),
        stage.client_secret.clone(),
        token,
    );
    account.region = stage.region.clone();
    store().upsert(account)
}

pub async fn login_social(app: &AppHandle) -> AppResult<KiroAccountPublic> {
    let (verifier, challenge) = pkce();
    let state = random_state();
    let (port, listeners) = bind_callbacks().await?;
    let (result_tx, mut result_rx) = mpsc::unbounded_channel();
    let session = Arc::new(LoginSession {
        state,
        verifier,
        port,
        aws: Mutex::new(None),
        done: AtomicBool::new(false),
        result_tx,
        cancel: Notify::new(),
    });
    let previous = SESSION
        .lock()
        .map_err(|_| AppError::Other("Kiro 登录状态锁已损坏".into()))?
        .replace(session.clone());
    if let Some(previous) = previous {
        previous.cancel.notify_one();
    }
    let tasks = listeners
        .into_iter()
        .map(|listener| tokio::spawn(serve_callbacks(listener, session.clone())))
        .collect();
    let _guard = SessionGuard {
        session: session.clone(),
        tasks,
    };
    let url = portal_signin_url(&session.state, &challenge, &session.base());
    let _ = app.opener().open_url(url, None::<&str>);
    tokio::select! {
        result = result_rx.recv() => {
            result.unwrap_or_else(|| Err(AppError::Config("Kiro Social 登录已中断".into())))
        }
        _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => Err(AppError::Config("Kiro Social 登录超时".into())),
        _ = session.cancel.notified() => Err(AppError::Config("已被新的 Kiro 登录取代".into())),
    }
}

/// Finishes a sign-in from a pasted browser address (the redirect to localhost could
/// not be opened). Returns the next URL to open for the AWS leg, if any.
pub async fn submit_login_callback(raw: &str) -> AppResult<Option<String>> {
    let session =
        current_session().ok_or_else(|| AppError::Config("当前没有进行中的 Kiro 登录".into()))?;
    let parsed =
        url::Url::parse(raw.trim()).map_err(|_| AppError::Config("回调地址格式不正确".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::Config("回调地址必须以 http:// 开头".into()));
    }
    let host_ok = matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !host_ok || parsed.port_or_known_default() != Some(session.port) {
        return Err(AppError::Config(
            "回调地址与当前登录的本机端口不匹配".into(),
        ));
    }
    let query: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    match process_callback(&session, parsed.path(), &query).await {
        Reply::Done => Ok(None),
        Reply::Redirect(next) => Ok(Some(next)),
        Reply::Failed(message) => Err(AppError::Config(message)),
        Reply::NotFound => Err(AppError::Config("回调地址路径不正确".into())),
        Reply::Ignored => Err(AppError::Config(
            "回调地址不属于当前登录，请重新点击登录".into(),
        )),
    }
}

async fn serve_callbacks(listener: TcpListener, session: Arc<LoginSession>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let session = session.clone();
                tokio::spawn(async move { handle_connection(stream, session).await });
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

async fn handle_connection(mut stream: TcpStream, session: Arc<LoginSession>) {
    let Some(target) = read_request_target(&mut stream).await else {
        return;
    };
    let (path, query_text) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let query: HashMap<String, String> = url::form_urlencoded::parse(query_text.as_bytes())
        .into_owned()
        .collect();
    let response = match process_callback(&session, path, &query).await {
        Reply::NotFound => http_response("404 Not Found", None, &page(false, "未找到", "")),
        Reply::Ignored => http_response(
            "200 OK",
            None,
            &page(false, "此链接不属于当前登录", "请回到应用重新点击登录。"),
        ),
        Reply::Redirect(location) => http_response("302 Found", Some(&location), ""),
        Reply::Done => http_response(
            "200 OK",
            None,
            &page(true, "登录成功", "可以关闭这个页面。"),
        ),
        Reply::Failed(message) => {
            http_response("200 OK", None, &page(false, "登录没有完成", &message))
        }
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut data = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    let read = async {
        while data.len() < 8192 && !data.windows(2).any(|w| w == b"\r\n") {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => data.extend_from_slice(&buf[..n]),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&data);
    let mut parts = text.lines().next()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    parts.next().map(str::to_string)
}

fn http_response(status: &str, location: Option<&str>, body: &str) -> String {
    let location = location
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    format!(
        "HTTP/1.1 {status}\r\n{location}Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn page(ok: bool, title: &str, detail: &str) -> String {
    let color = if ok { "#237804" } else { "#a8071a" };
    format!(
        "<html><head><meta charset=\"utf-8\"><title>{t}</title></head><body style=\"font-family:sans-serif;padding:2em\"><h2 style=\"color:{color}\">{t}</h2><p>{d}</p></body></html>",
        t = html_escape(title),
        d = html_escape(detail)
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn random_state() -> String {
    let mut bytes = Uuid::new_v4().as_bytes().to_vec();
    bytes.extend(Uuid::new_v4().as_bytes());
    b64url(&bytes)
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
        device_profile: None,
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
        return Err(AppError::Other(format!(
            "Kiro 登录失败 HTTP {status}: {text}"
        )));
    }
    Ok(serde_json::from_str(&text)?)
}

/// Binds `127.0.0.1:{port}` for the first free callback port, plus `[::1]:{port}` on a
/// best-effort basis (`localhost` can resolve to IPv6 first on Windows).
async fn bind_callbacks() -> AppResult<(u16, Vec<TcpListener>)> {
    for port in CALLBACK_PORTS {
        if let Ok(v4) = TcpListener::bind(("127.0.0.1", *port)).await {
            let mut listeners = vec![v4];
            if let Ok(v6) = TcpListener::bind(("::1", *port)).await {
                listeners.push(v6);
            }
            return Ok((*port, listeners));
        }
    }
    Err(AppError::Config(
        "Kiro 登录回调端口都被占用，请关闭 Kiro IDE 的登录窗口后重试".into(),
    ))
}

/// Same as [`send_json`] but identifies as the Kiro IDE, which the portal-era
/// endpoints (auth.desktop.kiro.dev, oidc) expect.
async fn send_json_ide<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> AppResult<T> {
    let response = client
        .post(url)
        .header("User-Agent", kiro_ide_user_agent())
        .header("Accept", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AppError::Other(format!(
            "Kiro 登录失败 HTTP {status}: {text}"
        )));
    }
    Ok(serde_json::from_str(&text)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = "st-1";
    const BASE: &str = "http://localhost:3128";

    fn query(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn classify(path: &str, pairs: &[(&str, &str)], aws: Option<&AwsStage>) -> CallbackAction {
        classify_callback(path, &query(pairs), STATE, BASE, aws)
    }

    fn stage() -> AwsStage {
        AwsStage {
            state: "aws-st".into(),
            verifier: "v".into(),
            region: "us-east-1".into(),
            client_id: "cid".into(),
            client_secret: "sec".into(),
            provider: "BuilderId",
        }
    }

    #[test]
    fn portal_url_redirect_has_no_path() {
        let url = portal_signin_url("s", "c", BASE);
        assert!(url.starts_with("https://app.kiro.dev/signin?"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A3128&"));
        assert!(!url.contains("oauth%2Fcallback"));
        assert!(url.contains("redirect_from=KiroIDE"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn social_callback_keeps_login_option_in_token_redirect() {
        let action = classify(
            "/oauth/callback",
            &[
                ("state", STATE),
                ("code", "abc"),
                ("login_option", "github"),
            ],
            None,
        );
        assert_eq!(
            action,
            CallbackAction::Social {
                code: "abc".into(),
                redirect_uri: "http://localhost:3128/oauth/callback?login_option=github".into(),
                provider: "Github",
            }
        );
        let action = classify(
            "/signin/callback",
            &[
                ("state", STATE),
                ("code", "abc"),
                ("login_option", "google"),
            ],
            None,
        );
        assert!(matches!(
            action,
            CallbackAction::Social {
                provider: "Google",
                ..
            }
        ));
    }

    #[test]
    fn builder_id_callback_registers_aws_client() {
        let action = classify(
            "/oauth/callback",
            &[
                ("state", STATE),
                ("login_option", "builderid"),
                ("issuer_url", "https://view.awsapps.com/start"),
                ("idc_region", "us-east-1"),
            ],
            None,
        );
        assert!(matches!(
            action,
            CallbackAction::AwsRegister {
                provider: "BuilderId",
                ..
            }
        ));
        let bad_region = classify(
            "/oauth/callback",
            &[
                ("state", STATE),
                ("login_option", "awsidc"),
                ("issuer_url", "https://x"),
                ("idc_region", "evil.com/"),
            ],
            None,
        );
        assert!(matches!(bad_region, CallbackAction::Fail(_)));
    }

    #[test]
    fn aws_leg_matches_its_own_state() {
        let stage = stage();
        let action = classify(
            "/oauth/callback",
            &[("state", "aws-st"), ("code", "xyz")],
            Some(&stage),
        );
        assert_eq!(
            action,
            CallbackAction::AwsToken {
                code: "xyz".into(),
                stage: stage.clone()
            }
        );
        assert_eq!(
            classify(
                "/oauth/callback",
                &[("state", "aws-st"), ("code", "xyz")],
                None
            ),
            CallbackAction::Ignore
        );
    }

    #[test]
    fn rejects_errors_foreign_state_and_unknown_paths() {
        assert!(matches!(
            classify(
                "/oauth/callback",
                &[("state", STATE), ("error", "access_denied"), ("error_description", "no")],
                None
            ),
            CallbackAction::Fail(message) if message == "no"
        ));
        assert_eq!(
            classify(
                "/oauth/callback",
                &[("state", "other"), ("code", "c")],
                None
            ),
            CallbackAction::Ignore
        );
        assert_eq!(
            classify("/favicon.ico", &[("state", STATE)], None),
            CallbackAction::NotCallback
        );
        assert!(matches!(
            classify(
                "/oauth/callback",
                &[("state", STATE), ("login_option", "external_idp")],
                None
            ),
            CallbackAction::Fail(_)
        ));
    }

    #[test]
    fn ide_user_agent_has_kiro_prefix() {
        assert!(kiro_ide_user_agent().starts_with("KiroIDE-"));
    }
}
