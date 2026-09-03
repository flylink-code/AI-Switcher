//! Antigravity / Google OAuth account storage and token refresh.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::antigravity::quota::QuotaSnapshot;
use crate::config;
use crate::error::{AppError, AppResult};

/// Public OAuth client used by the Antigravity / Cloud Code desktop product.
/// Refresh tokens exported from that product must be refreshed with the same client.
/// Credentials are injected at compile time from the repo-root `.env`
/// (see `.env.example`); CI may set the same env vars directly.
pub(crate) const OAUTH_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
pub(crate) const OAUTH_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REFRESH_SKEW_SECS: i64 = 300;
const ACCOUNTS_FILE: &str = "antigravity_accounts.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub expiry_timestamp: i64,
    #[serde(default)]
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityAccount {
    pub id: String,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub token: AntigravityToken,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    pub created_at: i64,
    pub last_used: i64,
    /// Soft health score used by the pool scheduler (1.0 = healthy).
    #[serde(default = "default_health")]
    pub health_score: f32,
    /// Unix seconds until which this account should be skipped after 429/401.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<i64>,
    /// Remaining quota hint (higher is better); optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_quota: Option<i32>,
    /// Latest Cloud Code quota snapshot (5h / weekly + per-model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<QuotaSnapshot>,
}

fn default_health() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityAccountPublic {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    pub is_active: bool,
    pub created_at: i64,
    pub last_used: i64,
    pub health_score: f32,
    pub cooldown_until: Option<i64>,
    pub remaining_quota: Option<i32>,
    pub has_project_id: bool,
    pub token_expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_5h_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_weekly_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_gemini_5h_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_gemini_weekly_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_claude_5h_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_claude_weekly_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_updated_at: Option<i64>,
    #[serde(default)]
    pub quota_forbidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<QuotaSnapshot>,
}

impl From<&AntigravityAccount> for AntigravityAccountPublic {
    fn from(account: &AntigravityAccount) -> Self {
        let quota = account.quota.as_ref();
        Self {
            id: account.id.clone(),
            email: account.email.clone(),
            name: account.name.clone(),
            disabled: account.disabled,
            disabled_reason: account.disabled_reason.clone(),
            is_active: account.is_active,
            created_at: account.created_at,
            last_used: account.last_used,
            health_score: account.health_score,
            cooldown_until: account.cooldown_until,
            remaining_quota: account.remaining_quota,
            has_project_id: account
                .token
                .project_id
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
            token_expires_at: account.token.expiry_timestamp,
            subscription_tier: quota.and_then(|q| q.subscription_tier.clone()),
            quota_5h_percent: quota.and_then(|q| q.window_percent("5h")),
            quota_weekly_percent: quota.and_then(|q| q.window_percent("weekly")),
            quota_gemini_5h_percent: quota.and_then(|q| q.gemini_window_percent("5h")),
            quota_gemini_weekly_percent: quota.and_then(|q| q.gemini_window_percent("weekly")),
            quota_claude_5h_percent: quota.and_then(|q| q.claude_window_percent("5h")),
            quota_claude_weekly_percent: quota.and_then(|q| q.claude_window_percent("weekly")),
            quota_updated_at: quota.map(|q| q.last_updated),
            quota_forbidden: quota.is_some_and(|q| q.is_forbidden),
            quota: account.quota.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAccounts {
    #[serde(default)]
    accounts: Vec<AntigravityAccount>,
}

pub struct AccountStore {
    client: Mutex<Client>,
    inner: Mutex<StoredAccounts>,
}

pub fn store() -> &'static AccountStore {
    static STORE: OnceLock<AccountStore> = OnceLock::new();
    STORE.get_or_init(AccountStore::new)
}

fn accounts_path() -> PathBuf {
    config::get_app_config_dir().join(ACCOUNTS_FILE)
}

impl AccountStore {
    fn new() -> Self {
        let client = crate::antigravity::outbound::build_blocking_client(30);
        let inner = load_accounts().unwrap_or_default();
        Self {
            client: Mutex::new(client),
            inner: Mutex::new(inner),
        }
    }

    fn lock_accounts(&self) -> std::sync::MutexGuard<'_, StoredAccounts> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("Antigravity account store mutex was poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }

    fn lock_http_client(&self) -> std::sync::MutexGuard<'_, Client> {
        match self.client.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("Antigravity account HTTP client mutex was poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }

    pub fn reload_http_client(&self) {
        *self.lock_http_client() = crate::antigravity::outbound::build_blocking_client(30);
    }

    pub fn list_public(&self) -> AppResult<Vec<AntigravityAccountPublic>> {
        let guard = self.lock_accounts();
        crate::antigravity::model_catalog::seed_from_accounts(
            guard
                .accounts
                .iter()
                .filter_map(|account| account.quota.as_ref())
                .flat_map(|quota| quota.models.clone()),
        );
        Ok(guard
            .accounts
            .iter()
            .map(AntigravityAccountPublic::from)
            .collect())
    }

    pub fn list_accounts(&self) -> AppResult<Vec<AntigravityAccount>> {
        let guard = self.lock_accounts();
        Ok(guard.accounts.clone())
    }

    pub fn remove_account(&self, account_id: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let before = guard.accounts.len();
        guard.accounts.retain(|account| account.id != account_id);
        if guard.accounts.len() == before {
            return Err(AppError::Config("Antigravity 账号不存在".into()));
        }
        if guard.accounts.iter().all(|account| !account.is_active) {
            if let Some(first) = guard.accounts.first_mut() {
                first.is_active = true;
            }
        }
        persist(&guard)?;
        Ok(())
    }

    pub fn set_active_account(&self, account_id: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        if !guard
            .accounts
            .iter()
            .any(|account| account.id == account_id)
        {
            return Err(AppError::Config("Antigravity 账号不存在".into()));
        }
        for account in &mut guard.accounts {
            let activate = account.id == account_id;
            account.is_active = activate;
            if activate {
                // Explicit user choice should take effect on the next request,
                // even if a recent 429 left this account in cooldown.
                account.cooldown_until = None;
                if account.health_score < 0.5 {
                    account.health_score = 0.5;
                }
            }
        }
        persist(&guard)?;
        Ok(())
    }

    pub fn import_json(&self, raw: &str) -> AppResult<usize> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| AppError::Config(format!("无法解析账号 JSON: {error}")))?;
        let imported = parse_import_payload(&value)?;
        if imported.is_empty() {
            return Err(AppError::Config("未找到可导入的 Antigravity 账号".into()));
        }
        let mut added = 0usize;
        for account in imported {
            let _ = self.upsert_account(account)?;
            added += 1;
        }
        Ok(added.max(1))
    }

    pub fn upsert_account(
        &self,
        mut account: AntigravityAccount,
    ) -> AppResult<AntigravityAccountPublic> {
        let mut guard = self.lock_accounts();
        let existing_index = guard
            .accounts
            .iter()
            .position(|item| item.email.eq_ignore_ascii_case(&account.email));
        if let Some(index) = existing_index {
            let activate = guard.accounts.iter().all(|item| !item.is_active);
            let existing = &mut guard.accounts[index];
            existing.token = account.token;
            existing.name = account.name.or(existing.name.clone());
            existing.disabled = false;
            existing.disabled_reason = None;
            existing.cooldown_until = None;
            existing.health_score = 1.0;
            existing.last_used = Utc::now().timestamp();
            if activate {
                existing.is_active = true;
            }
            let public = AntigravityAccountPublic::from(&*existing);
            persist(&guard)?;
            return Ok(public);
        }
        if guard.accounts.is_empty() {
            account.is_active = true;
        }
        let public = AntigravityAccountPublic::from(&account);
        guard.accounts.push(account);
        persist(&guard)?;
        Ok(public)
    }

    pub fn mark_cooldown(&self, account_id: &str, seconds: i64, reason: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.cooldown_until = Some(Utc::now().timestamp() + seconds.max(1));
        account.health_score = (account.health_score * 0.7).max(0.05);
        if reason.contains("invalid_grant") || reason.contains("revoked") {
            account.disabled = true;
            account.disabled_reason = Some(reason.to_string());
        }
        persist(&guard)?;
        Ok(())
    }

    /// Persist OAuth revocation so stale quota rows cannot look healthy.
    pub fn mark_reauthorization_required(&self, account_id: &str, reason: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.disabled = true;
        account.disabled_reason = Some(reason.to_string());
        account.cooldown_until = None;
        account.health_score = 0.05;
        persist(&guard)
    }

    /// Mark account forbidden on 403 response with cooldown and warning.
    pub fn mark_forbidden_403(&self, account_id: &str, reason: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.cooldown_until = Some(Utc::now().timestamp() + 300);
        account.health_score = (account.health_score * 0.4).max(0.05);
        if let Some(quota) = account.quota.as_mut() {
            quota.is_forbidden = true;
            quota.forbidden_reason = Some(reason.to_string());
        } else {
            let snapshot = QuotaSnapshot::empty_forbidden(reason);
            account.remaining_quota = snapshot.remaining_hint_percent();
            account.quota = Some(snapshot);
        }
        persist(&guard)
    }

    /// Adjust only the cooldown window (e.g. honor an upstream Retry-After
    /// after the pool already applied its default cooldown) without the
    /// health-score penalty of [`Self::mark_cooldown`].
    pub fn adjust_cooldown_secs(&self, account_id: &str, seconds: i64) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.cooldown_until = Some(Utc::now().timestamp() + seconds.max(1));
        persist(&guard)?;
        Ok(())
    }

    pub fn clear_cooldown(&self, account_id: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        if account.cooldown_until.take().is_some() {
            persist(&guard)?;
        }
        Ok(())
    }

    /// Drop cooldowns for every account (used when starting the gateway / binding Desktop).
    pub fn clear_all_cooldowns(&self) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let mut changed = false;
        for account in &mut guard.accounts {
            if account.cooldown_until.take().is_some() {
                changed = true;
            }
        }
        if changed {
            persist(&guard)?;
        }
        Ok(())
    }

    pub fn mark_success(&self, account_id: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.last_used = Utc::now().timestamp();
        account.cooldown_until = None;
        account.health_score = (account.health_score + 0.1).min(1.0);
        persist(&guard)?;
        Ok(())
    }

    pub fn update_project_id(&self, account_id: &str, project_id: &str) -> AppResult<()> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Ok(());
        };
        account.token.project_id = Some(project_id.to_string());
        persist(&guard)?;
        Ok(())
    }

    pub fn update_quota(
        &self,
        account_id: &str,
        quota: QuotaSnapshot,
        project_id: Option<String>,
    ) -> AppResult<AntigravityAccountPublic> {
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Err(AppError::Config("Antigravity 账号不存在".into()));
        };
        let mut quota = quota;
        quota.retain_groups_if_empty(account.quota.as_ref());
        account.remaining_quota = quota.remaining_hint_percent();
        if let Some(pid) = project_id.filter(|value| !value.trim().is_empty()) {
            account.token.project_id = Some(pid);
        }
        if quota.is_forbidden {
            account.health_score = (account.health_score * 0.5).max(0.05);
        }
        account.quota = Some(quota);
        if let Some(snapshot) = account.quota.as_ref() {
            crate::antigravity::model_catalog::update_from_quota_models(&snapshot.models);
        }
        let public = AntigravityAccountPublic::from(&*account);
        persist(&guard)?;
        Ok(public)
    }

    /// Return a usable access token, refreshing when close to expiry.
    pub fn ensure_access_token(&self, account_id: &str) -> AppResult<(String, AntigravityAccount)> {
        let snapshot = {
            let guard = self.lock_accounts();
            guard
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .cloned()
                .ok_or_else(|| AppError::Config("Antigravity 账号不存在".into()))?
        };
        if snapshot.disabled {
            return Err(AppError::Config(format!(
                "账号 {} 已禁用: {}",
                snapshot.email,
                snapshot.disabled_reason.as_deref().unwrap_or("unknown")
            )));
        }
        let now = Utc::now().timestamp();
        if snapshot.token.expiry_timestamp - REFRESH_SKEW_SECS > now
            && !snapshot.token.access_token.is_empty()
        {
            return Ok((snapshot.token.access_token.clone(), snapshot));
        }
        self.force_refresh_access_token(account_id)
    }

    /// Force a token renewal after Cloud Code rejects an access token before its
    /// locally recorded expiry. Google can invalidate a token early after a
    /// session/policy change, so timestamp checks alone are insufficient.
    pub fn force_refresh_access_token(
        &self,
        account_id: &str,
    ) -> AppResult<(String, AntigravityAccount)> {
        let refresh_token = {
            let guard = self.lock_accounts();
            guard
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .map(|account| account.token.refresh_token.clone())
                .ok_or_else(|| AppError::Config("Antigravity 账号不存在".into()))?
        };
        let refreshed = self.refresh_token(&refresh_token)?;
        let mut guard = self.lock_accounts();
        let Some(account) = guard.accounts.iter_mut().find(|item| item.id == account_id) else {
            return Err(AppError::Config("Antigravity 账号不存在".into()));
        };
        account.token.access_token = refreshed.access_token.clone();
        account.token.expires_in = refreshed.expires_in;
        account.token.expiry_timestamp = Utc::now().timestamp() + refreshed.expires_in;
        if let Some(refresh) = refreshed.refresh_token {
            if !refresh.is_empty() {
                account.token.refresh_token = refresh;
            }
        }
        let result = (account.token.access_token.clone(), account.clone());
        persist(&guard)?;
        Ok(result)
    }

    fn refresh_token(&self, refresh_token: &str) -> AppResult<TokenRefreshResponse> {
        // Google OAuth is reachable directly in this environment while the
        // configured Clash SOCKS endpoint can hang indefinitely. Prefer a
        // bounded direct refresh; fall back to the configured route for users
        // whose network requires it.
        let direct = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(20))
            .no_proxy()
            .build()
            .map_err(|error| {
                AppError::Other(format!("创建直连 Google Token 客户端失败: {error}"))
            })?;
        match refresh_token_with_client(&direct, refresh_token) {
            Ok(response) => Ok(response),
            Err(direct_error) => {
                log::warn!(
                    "Antigravity Google token refresh direct failed: {direct_error}; retrying configured proxy"
                );
                let client = self.lock_http_client().clone();
                refresh_token_with_client(&client, refresh_token)
            }
        }
    }
}

fn refresh_token_with_client(
    client: &Client,
    refresh_token: &str,
) -> AppResult<TokenRefreshResponse> {
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .map_err(|error| {
            let kind = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect"
            } else {
                "transport"
            };
            let proxy = crate::system_proxy::outbound_proxy_url()
                .map(|url| format!(" via {url}"))
                .unwrap_or_default();
            AppError::Network(format!(
                "network/{kind}: google token refresh failed{proxy}: {error}"
            ))
        })?;
    let status = response.status();
    let body_text = response
        .text()
        .map_err(|error| AppError::Other(format!("读取 Token 响应失败: {error}")))?;
    let body: Value = serde_json::from_str(&body_text)
        .map_err(|error| AppError::Other(format!("解析 Token 响应失败: {error}")))?;
    if !status.is_success() {
        let message = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("token refresh failed");
        return Err(AppError::Other(format!(
            "刷新 Google Token 失败: {message}"
        )));
    }
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Other("Token 响应缺少 access_token".into()))?
        .to_string();
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(TokenRefreshResponse {
        access_token,
        expires_in,
        refresh_token,
    })
}

struct TokenRefreshResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

pub fn list_accounts() -> AppResult<Vec<AntigravityAccountPublic>> {
    store().list_public()
}

pub fn remove_account(account_id: &str) -> AppResult<()> {
    store().remove_account(account_id)
}

pub fn set_active_account(account_id: &str) -> AppResult<()> {
    store().set_active_account(account_id)
}

pub fn import_accounts_json(raw: &str) -> AppResult<usize> {
    store().import_json(raw)
}

fn load_accounts() -> AppResult<StoredAccounts> {
    let path = accounts_path();
    if !path.exists() {
        return Ok(StoredAccounts::default());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| AppError::Io(format!("读取 Antigravity 账号失败: {error}")))?;
    serde_json::from_str(&raw)
        .map_err(|error| AppError::Config(format!("Antigravity 账号文件损坏: {error}")))
}

fn persist(stored: &StoredAccounts) -> AppResult<()> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Io(format!("创建配置目录失败: {error}")))?;
    }
    let raw = serde_json::to_string_pretty(stored)
        .map_err(|error| AppError::Other(format!("序列化账号失败: {error}")))?;
    crate::config::atomic_write(&path, raw.as_bytes())
}

fn parse_import_payload(value: &Value) -> AppResult<Vec<AntigravityAccount>> {
    let now = Utc::now().timestamp();
    let mut out = Vec::new();

    let items: Vec<&Value> = if let Some(array) = value.as_array() {
        array.iter().collect()
    } else if let Some(accounts) = value.get("accounts").and_then(Value::as_array) {
        accounts.iter().collect()
    } else {
        vec![value]
    };

    for item in items {
        if let Some(account) = parse_one_account(item, now) {
            out.push(account);
        }
    }
    Ok(out)
}

fn parse_one_account(item: &Value, now: i64) -> Option<AntigravityAccount> {
    let token_obj = item.get("token").unwrap_or(item);
    let refresh = token_obj
        .get("refresh_token")
        .or_else(|| token_obj.get("refreshToken"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let access = token_obj
        .get("access_token")
        .or_else(|| token_obj.get("accessToken"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let expires_in = token_obj
        .get("expires_in")
        .or_else(|| token_obj.get("expiresIn"))
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let expiry_timestamp = token_obj
        .get("expiry_timestamp")
        .or_else(|| token_obj.get("expiryTimestamp"))
        .and_then(Value::as_i64)
        .unwrap_or(now + expires_in);
    let email = item
        .get("email")
        .and_then(Value::as_str)
        .or_else(|| token_obj.get("email").and_then(Value::as_str))
        .unwrap_or("unknown@account")
        .to_string();
    let project_id = token_obj
        .get("project_id")
        .or_else(|| token_obj.get("projectId"))
        .or_else(|| item.get("project_id"))
        .or_else(|| item.get("projectId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let name = item.get("name").and_then(Value::as_str).map(str::to_string);
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    Some(AntigravityAccount {
        id,
        email,
        name,
        token: AntigravityToken {
            access_token: access,
            refresh_token: refresh.to_string(),
            expires_in,
            expiry_timestamp,
            token_type: "Bearer".into(),
            email: None,
            project_id,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_import_accepts_ag_style_account_object() {
        let raw = serde_json::json!({
            "email": "user@example.com",
            "token": {
                "access_token": "ya29.a",
                "refresh_token": "1//refresh",
                "expires_in": 3600,
                "project_id": "proj-1"
            }
        });
        let accounts = parse_import_payload(&raw).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "user@example.com");
        assert_eq!(accounts[0].token.refresh_token, "1//refresh");
        assert_eq!(accounts[0].token.project_id.as_deref(), Some("proj-1"));
    }

    #[test]
    fn parse_import_accepts_accounts_array_wrapper() {
        let raw = serde_json::json!({
            "accounts": [{
                "email": "a@example.com",
                "refresh_token": "r1",
                "access_token": "a1"
            }]
        });
        let accounts = parse_import_payload(&raw).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].token.refresh_token, "r1");
    }
}
