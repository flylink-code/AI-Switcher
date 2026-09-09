//! ChatGPT/Codex device authorization and refresh-token storage.
//!
//! This is an independent implementation of the public OAuth device flow. Only
//! refresh tokens are persisted; access tokens live in process memory.

use std::collections::HashMap;
use std::fs;
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config;
use crate::error::{AppError, AppResult};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEVICE_AUTH_USERCODE_URL: &str =
    "https://auth.openai.com/api/accounts/deviceauth/usercode";
pub const DEVICE_AUTH_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
pub const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
pub const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
pub const USER_AGENT: &str = "ai-switcher-codex-oauth";
pub const CODEX_OAUTH_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const CODEX_OAUTH_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
pub const ORIGINATOR: &str = "codex_cli_rs";
/// Official rust catalog: gpt-6-astra requires >= 0.153.0. Bump with cc-switch.
pub const CLIENT_VERSION: &str = "0.153.4";

/// `GET /backend-api/codex/models` with the identity ChatGPT uses to gate Astra.
pub fn build_codex_oauth_models_url() -> String {
    format!("{CODEX_OAUTH_MODELS_URL}?client_version={CLIENT_VERSION}")
}
pub const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOauthDeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOauthAccount {
    pub account_id: String,
    pub email: String,
    pub authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOauthPollResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<CodexOauthAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAccount {
    #[serde(flatten)]
    account: CodexOauthAccount,
    refresh_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAuth {
    #[serde(default)]
    default_account_id: Option<String>,
    #[serde(default)]
    accounts: Vec<StoredAccount>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: i64,
}

#[derive(Debug, Clone)]
struct PendingDevice {
    user_code: String,
    expires_at_ms: i64,
}

pub struct CodexOauthManager {
    client: Client,
    stored: RwLock<StoredAuth>,
    access_tokens: Mutex<HashMap<String, CachedToken>>,
    pending: Mutex<HashMap<String, PendingDevice>>,
}

pub fn manager() -> &'static CodexOauthManager {
    static MANAGER: OnceLock<CodexOauthManager> = OnceLock::new();
    MANAGER.get_or_init(CodexOauthManager::new)
}

impl CodexOauthManager {
    fn new() -> Self {
        let stored = load_auth().unwrap_or_default();
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|error| {
                log::error!("Codex OAuth HTTP client build failed ({error}); using reqwest defaults");
                Client::new()
            });
        Self {
            client,
            stored: RwLock::new(stored),
            access_tokens: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn start_device_flow(&self) -> AppResult<CodexOauthDeviceStart> {
        let response = self
            .client
            .post(DEVICE_AUTH_USERCODE_URL)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&json!({ "client_id": CLIENT_ID }))?)
            .send()
            .map_err(http_error)?;
        let status = response.status();
        let value = response_json(response)?;
        if !status.is_success() {
            return Err(AppError::Other(oauth_message(
                &value,
                "无法启动 ChatGPT 登录",
            )));
        }
        let device_code = string_field(&value, &["device_auth_id", "device_code"])?;
        let user_code = string_field(&value, &["user_code"])?;
        let expires_in = value
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(900);
        let now = Utc::now().timestamp_millis();
        {
            let mut pending = self.pending.lock().map_err(lock_error)?;
            pending.retain(|_, item| item.expires_at_ms > now);
            pending.insert(
                device_code.clone(),
                PendingDevice {
                    user_code: user_code.clone(),
                    expires_at_ms: now + (expires_in as i64) * 1000,
                },
            );
        }
        Ok(CodexOauthDeviceStart {
            device_code,
            user_code,
            verification_uri: value
                .get("verification_uri")
                .and_then(Value::as_str)
                .unwrap_or(DEVICE_VERIFICATION_URL)
                .to_string(),
            interval: value.get("interval").and_then(Value::as_u64).unwrap_or(5),
            expires_in,
        })
    }

    pub fn poll_device_flow(&self, device_code: &str) -> AppResult<CodexOauthPollResult> {
        if device_code.trim().is_empty() {
            return Err(AppError::Config("设备授权码不能为空".to_string()));
        }
        let now = Utc::now().timestamp_millis();
        let user_code = {
            let mut pending = self.pending.lock().map_err(lock_error)?;
            pending.retain(|_, item| item.expires_at_ms > now);
            let Some(entry) = pending.get(device_code) else {
                return Ok(poll_status(
                    "expired",
                    Some("登录流程已过期，请重新开始".to_string()),
                ));
            };
            entry.user_code.clone()
        };
        let response = self
            .client
            .post(DEVICE_AUTH_TOKEN_URL)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&json!({
                "device_auth_id": device_code,
                "user_code": user_code,
            }))?)
            .send()
            .map_err(http_error)?;
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            return Ok(poll_status("pending", None));
        }
        if status == reqwest::StatusCode::GONE {
            self.pending.lock().map_err(lock_error)?.remove(device_code);
            return Ok(poll_status("expired", Some("设备码已过期".to_string())));
        }
        let value = response_json(response)?;
        if !status.is_success() {
            let code = oauth_code(&value);
            return Ok(poll_status(
                match code.as_str() {
                    "authorization_pending" | "slow_down" => "pending",
                    "expired_token" => "expired",
                    "access_denied" | "authorization_declined" => "denied",
                    _ => "error",
                },
                Some(oauth_message(&value, "设备登录失败")),
            ));
        }

        if value.get("access_token").is_some() {
            return self.complete_login(value);
        }
        let code = string_field(&value, &["authorization_code", "code"])?;
        let verifier = string_field(&value, &["code_verifier"])?;
        let response = self
            .client
            .post(OAUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                ("code", code.as_str()),
                ("redirect_uri", DEVICE_REDIRECT_URI),
                ("code_verifier", verifier.as_str()),
            ])
            .send()
            .map_err(http_error)?;
        let status = response.status();
        let token = response_json(response)?;
        if !status.is_success() {
            return Ok(poll_status(
                "error",
                Some(oauth_message(&token, "换取令牌失败")),
            ));
        }
        self.complete_login(token)
    }

    fn complete_login(&self, token: Value) -> AppResult<CodexOauthPollResult> {
        let access_token = string_field(&token, &["access_token"])?;
        let refresh_token = string_field(&token, &["refresh_token"])?;
        let id_token = string_field(&token, &["id_token"])?;
        let claims = parse_jwt_claims(&id_token)?;
        let account_id = claims
            .get("chatgpt_account_id")
            .or_else(|| claims.pointer("/https://api.openai.com/auth/chatgpt_account_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Config("登录令牌缺少 ChatGPT account id".to_string()))?
            .to_string();
        let email = claims
            .get("email")
            .or_else(|| claims.pointer("/https://api.openai.com/profile/email"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let account = CodexOauthAccount {
            account_id: account_id.clone(),
            email,
            authenticated_at: Utc::now().timestamp_millis(),
        };
        let expires_in = token
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(3600);
        self.access_tokens.lock().map_err(lock_error)?.insert(
            account_id.clone(),
            CachedToken {
                access_token,
                expires_at: Utc::now().timestamp_millis() + expires_in * 1000,
            },
        );
        self.pending.lock().map_err(lock_error)?.clear();
        {
            let mut stored = self.stored.write().map_err(lock_error)?;
            stored
                .accounts
                .retain(|item| item.account.account_id != account_id);
            stored.accounts.push(StoredAccount {
                account: account.clone(),
                refresh_token,
            });
            stored.default_account_id = Some(account_id);
            save_auth(&stored)?;
        }
        Ok(CodexOauthPollResult {
            status: "complete".to_string(),
            account: Some(account),
            message: None,
        })
    }

    pub fn list_accounts(&self) -> Vec<CodexOauthAccount> {
        self.stored
            .read()
            .map(|stored| {
                stored
                    .accounts
                    .iter()
                    .map(|item| item.account.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn remove_account(&self, account_id: &str) -> AppResult<()> {
        let mut stored = self.stored.write().map_err(lock_error)?;
        let before = stored.accounts.len();
        stored
            .accounts
            .retain(|item| item.account.account_id != account_id);
        if before == stored.accounts.len() {
            return Err(AppError::Config("ChatGPT 账户不存在".to_string()));
        }
        if stored.default_account_id.as_deref() == Some(account_id) {
            stored.default_account_id = stored
                .accounts
                .first()
                .map(|item| item.account.account_id.clone());
        }
        self.access_tokens
            .lock()
            .map_err(lock_error)?
            .remove(account_id);
        save_auth(&stored)
    }

    pub fn default_account_id(&self) -> Option<String> {
        self.stored.read().ok()?.default_account_id.clone()
    }

    pub fn set_default_account(&self, account_id: &str) -> AppResult<()> {
        let mut stored = self.stored.write().map_err(lock_error)?;
        if !stored
            .accounts
            .iter()
            .any(|item| item.account.account_id == account_id)
        {
            return Err(AppError::Config("ChatGPT 账户不存在".to_string()));
        }
        stored.default_account_id = Some(account_id.to_string());
        save_auth(&stored)
    }

    pub fn get_valid_token(&self, account_id: Option<&str>) -> AppResult<(String, String)> {
        let id = account_id
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| self.default_account_id())
            .ok_or_else(|| AppError::Config("尚未登录 ChatGPT 订阅账户".to_string()))?;
        let now = Utc::now().timestamp_millis();
        if let Some(token) = self.access_tokens.lock().map_err(lock_error)?.get(&id) {
            if token.expires_at - TOKEN_REFRESH_BUFFER_MS > now {
                return Ok((token.access_token.clone(), id));
            }
        }
        let refresh_token = self
            .stored
            .read()
            .map_err(lock_error)?
            .accounts
            .iter()
            .find(|item| item.account.account_id == id)
            .map(|item| item.refresh_token.clone())
            .ok_or_else(|| AppError::Config("ChatGPT 账户授权不存在".to_string()))?;
        let response = self
            .client
            .post(OAUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", refresh_token.as_str()),
            ])
            .send()
            .map_err(http_error)?;
        let status = response.status();
        let value = response_json(response)?;
        if !status.is_success() {
            return Err(AppError::Other(oauth_message(
                &value,
                "刷新 ChatGPT 登录失败",
            )));
        }
        let access_token = string_field(&value, &["access_token"])?;
        let expires_in = value
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(3600);
        if let Some(new_refresh) = value.get("refresh_token").and_then(Value::as_str) {
            let mut stored = self.stored.write().map_err(lock_error)?;
            if let Some(item) = stored
                .accounts
                .iter_mut()
                .find(|item| item.account.account_id == id)
            {
                item.refresh_token = new_refresh.to_string();
                save_auth(&stored)?;
            }
        }
        self.access_tokens.lock().map_err(lock_error)?.insert(
            id.clone(),
            CachedToken {
                access_token: access_token.clone(),
                expires_at: now + expires_in * 1000,
            },
        );
        Ok((access_token, id))
    }
}

fn auth_path() -> std::path::PathBuf {
    config::get_app_config_dir().join("codex_oauth_auth.json")
}

fn load_auth() -> AppResult<StoredAuth> {
    let path = auth_path();
    if !path.exists() {
        return Ok(StoredAuth::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn save_auth(auth: &StoredAuth) -> AppResult<()> {
    config::atomic_write(&auth_path(), serde_json::to_string_pretty(auth)?.as_bytes())
}

pub fn parse_jwt_claims(token: &str) -> AppResult<Value> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| AppError::Config("无效的 JWT".to_string()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| AppError::Config(format!("无效的 JWT payload: {error}")))?;
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn string_field(value: &Value, names: &[&str]) -> AppResult<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::Other(format!("OAuth 响应缺少 {}", names.join("/"))))
}

fn oauth_code(value: &Value) -> String {
    value
        .get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("code").and_then(Value::as_str))
        })
        .unwrap_or_default()
        .to_string()
}

fn oauth_message(value: &Value, fallback: &str) -> String {
    value
        .get("error_description")
        .or_else(|| value.get("message"))
        .or_else(|| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn poll_status(status: &str, message: Option<String>) -> CodexOauthPollResult {
    CodexOauthPollResult {
        status: status.to_string(),
        account: None,
        message,
    }
}

fn http_error(error: reqwest::Error) -> AppError {
    AppError::Other(format!("ChatGPT OAuth 网络请求失败: {error}"))
}

fn response_json(response: reqwest::blocking::Response) -> AppResult<Value> {
    let bytes = response.bytes().map_err(http_error)?;
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::Other("ChatGPT OAuth 状态锁已损坏".to_string())
}

/// ChatGPT Codex model ids from `GET /backend-api/codex/models` (not `/v1/models`).
pub fn parse_codex_oauth_model_ids(value: &Value) -> Vec<String> {
    let mut models = Vec::new();
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());
    if let Some(entries) = entries {
        for entry in entries {
            push_codex_oauth_model_id(&mut models, entry, None);
        }
    }
    if let Some(model_map) = value.get("models").and_then(Value::as_object) {
        for (key, entry) in model_map {
            push_codex_oauth_model_id(&mut models, entry, Some(key));
        }
    }
    models.sort();
    models.dedup();
    models
}

fn push_codex_oauth_model_id(models: &mut Vec<String>, entry: &Value, fallback_id: Option<&str>) {
    if let Some(id) = entry.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        models.push(id.to_string());
        return;
    }
    let Some(obj) = entry.as_object() else {
        if let Some(id) = fallback_id.map(str::trim).filter(|id| !id.is_empty()) {
            models.push(id.to_string());
        }
        return;
    };
    let Some(id) = ["slug", "id", "model", "name"]
        .iter()
        .find_map(|key| obj.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            fallback_id
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
    else {
        return;
    };
    models.push(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn client_version_meets_gpt6_astra_minimum() {
        assert_eq!(CLIENT_VERSION, "0.153.4");
        let parts: Vec<u32> = CLIENT_VERSION
            .split('.')
            .map(|part| part.parse().unwrap())
            .collect();
        assert!(parts.as_slice() >= [0, 153, 0].as_slice());
        assert_eq!(
            build_codex_oauth_models_url(),
            "https://chatgpt.com/backend-api/codex/models?client_version=0.153.4"
        );
    }

    #[test]
    fn parse_codex_oauth_models_reads_slug_and_openai_data() {
        let models = parse_codex_oauth_model_ids(&json!({
            "models": [{ "slug": "gpt-6-astra", "minimal_client_version": "0.153.0" }]
        }));
        assert_eq!(models, vec!["gpt-6-astra".to_string()]);

        let models = parse_codex_oauth_model_ids(&json!({
            "data": [
                { "id": "gpt-5.4", "owned_by": "openai" },
                { "model": "gpt-5.4" },
                "gpt-5.5"
            ]
        }));
        assert_eq!(models, vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]);
    }
}
