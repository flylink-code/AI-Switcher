//! Kiro account file. Tokens stay in `~/.claude-switcher/kiro_accounts.json`.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::config::get_app_config_dir;
use crate::error::{AppError, AppResult};

use super::outbound::build_blocking_client;

const ACCOUNTS_FILE: &str = "kiro_accounts.json";
pub const BUILDER_ID_PROFILE_ARN: &str =
    "arn:aws:codewhisperer:us-east-1:638616132270:profile/AAAACCCCXXXX";
pub const SOCIAL_PROFILE_ARN: &str =
    "arn:aws:codewhisperer:us-east-1:699475941385:profile/EHGA3GRVQMUK";
pub const DEFAULT_REGION: &str = "us-east-1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroAccount {
    pub id: String,
    pub label: String,
    pub auth_method: String,
    pub provider: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub profile_arn: String,
    pub machine_id: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub disable_reason: String,
    #[serde(default)]
    pub cooldown_until_ms: i64,
    #[serde(default)]
    pub quota: Option<super::quota::KiroQuotaSnapshot>,
}

fn default_region() -> String {
    DEFAULT_REGION.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredAccounts {
    accounts: Vec<KiroAccount>,
    #[serde(default)]
    active_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroAccountPublic {
    pub id: String,
    pub label: String,
    pub auth_method: String,
    pub provider: String,
    pub region: String,
    pub disabled: bool,
    pub disable_reason: String,
    pub has_refresh_token: bool,
    pub active: bool,
    #[serde(default)]
    pub quota: Option<super::quota::KiroQuotaSnapshot>,
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
    get_app_config_dir().join(ACCOUNTS_FILE)
}

impl AccountStore {
    fn new() -> Self {
        Self {
            client: Mutex::new(build_blocking_client(30)),
            inner: Mutex::new(load_accounts().unwrap_or_default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoredAccounts> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn list_public(&self) -> AppResult<Vec<KiroAccountPublic>> {
        let stored = self.lock();
        Ok(stored
            .accounts
            .iter()
            .map(|account| public_of(account, &stored.active_id))
            .collect())
    }

    pub fn upsert(&self, mut account: KiroAccount) -> AppResult<KiroAccountPublic> {
        normalize_account(&mut account)?;
        let mut stored = self.lock();
        let saved_id = if let Some(index) = stored.accounts.iter().position(|item| item.id == account.id) {
            stored.accounts[index] = account;
            stored.accounts[index].id.clone()
        } else if let Some(index) = stored
            .accounts
            .iter()
            .position(|item| item.refresh_token == account.refresh_token)
        {
            account.id = stored.accounts[index].id.clone();
            if account.machine_id.is_empty() {
                account.machine_id = stored.accounts[index].machine_id.clone();
            }
            stored.accounts[index] = account;
            stored.accounts[index].id.clone()
        } else {
            let saved_id = account.id.clone();
            stored.accounts.push(account);
            saved_id
        };
        if stored.active_id.is_empty() {
            stored.active_id = stored
                .accounts
                .first()
                .map(|item| item.id.clone())
                .unwrap_or_default();
        }
        save_accounts(&stored)?;
        let active = stored.active_id.clone();
        let saved = stored
            .accounts
            .iter()
            .find(|item| item.id == saved_id)
            .cloned()
            .ok_or_else(|| AppError::Config("没有可保存的 Kiro 账号".into()))?;
        Ok(public_of(&saved, &active))
    }

    pub fn import_json(&self, raw: &str) -> AppResult<usize> {
        let value: serde_json::Value = serde_json::from_str(raw)?;
        let items = match value {
            serde_json::Value::Array(items) => items,
            other => vec![other],
        };
        let mut count = 0;
        for item in items {
            let account = account_from_import(&item)?;
            self.upsert(account)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        let mut stored = self.lock();
        let before = stored.accounts.len();
        stored.accounts.retain(|account| account.id != id);
        if stored.accounts.len() == before {
            return Err(AppError::Config("Kiro 账号不存在".into()));
        }
        if stored.active_id == id {
            stored.active_id = stored
                .accounts
                .first()
                .map(|account| account.id.clone())
                .unwrap_or_default();
        }
        save_accounts(&stored)
    }

    pub fn selectable(&self) -> Vec<KiroAccount> {
        let now = now_ms();
        self.lock()
            .accounts
            .iter()
            .filter(|account| !account.disabled && account.cooldown_until_ms <= now)
            .cloned()
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<KiroAccount> {
        self.lock().accounts.iter().find(|account| account.id == id).cloned()
    }

    pub fn update_tokens(
        &self,
        id: &str,
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<String>,
        profile_arn: Option<String>,
    ) -> AppResult<()> {
        let mut stored = self.lock();
        let account = stored
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| AppError::Config("Kiro 账号不存在".into()))?;
        account.access_token = access_token;
        if let Some(refresh) = refresh_token.filter(|value| !value.is_empty()) {
            account.refresh_token = refresh;
        }
        if expires_at.is_some() {
            account.expires_at = expires_at;
        }
        if let Some(arn) = profile_arn.filter(|value| !value.is_empty()) {
            account.profile_arn = arn;
        }
        save_accounts(&stored)
    }

    pub fn mark_disabled(&self, id: &str, reason: &str) -> AppResult<()> {
        let mut stored = self.lock();
        if let Some(account) = stored.accounts.iter_mut().find(|account| account.id == id) {
            account.disabled = true;
            account.disable_reason = reason.to_string();
        }
        save_accounts(&stored)
    }

    pub fn mark_cooldown(&self, id: &str, for_ms: i64) -> AppResult<()> {
        let mut stored = self.lock();
        if let Some(account) = stored.accounts.iter_mut().find(|account| account.id == id) {
            account.cooldown_until_ms = now_ms() + for_ms;
        }
        save_accounts(&stored)
    }

    pub fn save_quota(
        &self,
        id: &str,
        quota: super::quota::KiroQuotaSnapshot,
    ) -> AppResult<KiroAccountPublic> {
        let mut stored = self.lock();
        let account = stored
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| AppError::Config("Kiro 账号不存在".into()))?;
        let next = if quota.error.is_empty() {
            quota
        } else {
            match account.quota.clone() {
                Some(mut previous) => {
                    previous.error = quota.error;
                    previous.queried_at = quota.queried_at;
                    previous
                }
                None => quota,
            }
        };
        account.quota = Some(next);
        save_accounts(&stored)?;
        let active = stored.active_id.clone();
        let saved = stored
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
            .ok_or_else(|| AppError::Config("Kiro 账号不存在".into()))?;
        Ok(public_of(&saved, &active))
    }

    pub fn clear_cooldowns(&self) -> AppResult<()> {
        let mut stored = self.lock();
        for account in &mut stored.accounts {
            account.cooldown_until_ms = 0;
        }
        save_accounts(&stored)
    }

    pub fn reload_http_client(&self) {
        if let Ok(mut client) = self.client.lock() {
            *client = build_blocking_client(30);
        }
    }

    pub fn http(&self) -> Client {
        self.client
            .lock()
            .map(|client| client.clone())
            .unwrap_or_else(|_| build_blocking_client(30))
    }
}

fn public_of(account: &KiroAccount, active_id: &str) -> KiroAccountPublic {
    KiroAccountPublic {
        id: account.id.clone(),
        label: account.label.clone(),
        auth_method: account.auth_method.clone(),
        provider: account.provider.clone(),
        region: account.region.clone(),
        disabled: account.disabled,
        disable_reason: account.disable_reason.clone(),
        has_refresh_token: !account.refresh_token.trim().is_empty(),
        active: account.id == active_id,
        quota: account.quota.clone(),
    }
}

pub fn list_accounts() -> AppResult<Vec<KiroAccountPublic>> {
    store().list_public()
}

pub fn import_accounts_json(raw: &str) -> AppResult<usize> {
    store().import_json(raw)
}

pub fn remove_account(id: &str) -> AppResult<()> {
    store().remove(id)
}

fn load_accounts() -> AppResult<StoredAccounts> {
    let path = accounts_path();
    if !path.exists() {
        return Ok(StoredAccounts::default());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(StoredAccounts::default());
    }
    Ok(serde_json::from_str(&raw)?)
}

fn save_accounts(stored: &StoredAccounts) -> AppResult<()> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(stored)?;
    fs::write(path, raw)?;
    Ok(())
}

pub(crate) fn account_from_import(value: &serde_json::Value) -> AppResult<KiroAccount> {
    let refresh = string_field(value, &["refreshToken", "refresh_token"]);
    let access = string_field(value, &["accessToken", "access_token"]);
    if refresh.is_empty() && access.is_empty() {
        return Err(AppError::Config("Kiro 凭据缺少 refreshToken".into()));
    }
    let mut account = KiroAccount {
        id: string_field(value, &["id"]),
        label: string_field(value, &["email", "label", "subscriptionTitle"]),
        auth_method: string_field(value, &["authMethod", "auth_method"]),
        provider: string_field(value, &["provider"]),
        refresh_token: refresh,
        access_token: access,
        expires_at: nonempty(string_field(value, &["expiresAt", "expires_at"])),
        client_id: string_field(value, &["clientId", "client_id"]),
        client_secret: string_field(value, &["clientSecret", "client_secret"]),
        profile_arn: string_field(value, &["profileArn", "profile_arn"]),
        machine_id: string_field(value, &["machineId", "machine_id"]),
        region: {
            let region = string_field(value, &["authRegion", "region", "apiRegion"]);
            if region.is_empty() { DEFAULT_REGION.to_string() } else { region }
        },
        disabled: value.get("disabled").and_then(|item| item.as_bool()).unwrap_or(false),
        disable_reason: String::new(),
        cooldown_until_ms: 0,
        quota: None,
    };
    normalize_account(&mut account)?;
    Ok(account)
}

fn normalize_account(account: &mut KiroAccount) -> AppResult<()> {
    if account.refresh_token.trim().is_empty() && account.access_token.trim().is_empty() {
        return Err(AppError::Config("Kiro 凭据缺少 refreshToken".into()));
    }
    let method = account.auth_method.trim().to_ascii_lowercase();
    account.auth_method = match method.as_str() {
        "social" => "social".to_string(),
        "idc" | "builder-id" | "builder_id" | "iam" => "idc".to_string(),
        "" if account.provider.eq_ignore_ascii_case("github")
            || account.provider.eq_ignore_ascii_case("google") =>
        {
            "social".to_string()
        }
        "" => "idc".to_string(),
        other => other.to_string(),
    };
    if account.auth_method == "idc" && account.provider.trim().is_empty() {
        account.provider = "BuilderId".to_string();
    }
    if account.auth_method == "idc"
        && (account.client_id.trim().is_empty() || account.client_secret.trim().is_empty())
        && account.refresh_token.trim().is_empty()
    {
        return Err(AppError::Config("Builder ID 凭据需要 clientId 和 clientSecret".into()));
    }
    if account.profile_arn.trim().is_empty() {
        account.profile_arn = default_profile_arn(account).to_string();
    }
    if account.machine_id.trim().is_empty() {
        account.machine_id = derive_machine_id(&account.refresh_token, &account.client_id);
    }
    if account.id.trim().is_empty() {
        account.id = format!("kiro_{}", &account.machine_id[..12]);
    }
    if account.label.trim().is_empty() {
        account.label = format!("{} {}", account.provider, &account.id[account.id.len().saturating_sub(6)..]);
    }
    if account.region.trim().is_empty() {
        account.region = DEFAULT_REGION.to_string();
    }
    Ok(())
}

pub fn default_profile_arn(account: &KiroAccount) -> &'static str {
    if account.auth_method == "social"
        || account.provider.eq_ignore_ascii_case("github")
        || account.provider.eq_ignore_ascii_case("google")
    {
        SOCIAL_PROFILE_ARN
    } else {
        BUILDER_ID_PROFILE_ARN
    }
}

/// Streaming calls must send the Builder ID placeholder when no enterprise profile exists.
pub fn streaming_profile_arn(account: &KiroAccount) -> String {
    let arn = account.profile_arn.trim();
    if arn.is_empty() {
        default_profile_arn(account).to_string()
    } else {
        arn.to_string()
    }
}

pub fn is_placeholder_profile_arn(arn: &str) -> bool {
    arn == BUILDER_ID_PROFILE_ARN
}

fn derive_machine_id(refresh_token: &str, client_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ai-switcher-kiro");
    hasher.update(refresh_token.as_bytes());
    hasher.update(client_id.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn nonempty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn access_token_fresh(account: &KiroAccount) -> bool {
    let token = account.access_token.trim();
    if token.is_empty() {
        return false;
    }
    let Some(expires) = account.expires_at.as_deref() else {
        return true;
    };
    let Ok(expires) = DateTime::parse_from_rfc3339(expires) else {
        return true;
    };
    expires.timestamp() > Utc::now().timestamp() + 60
}

pub fn expires_after(seconds: i64) -> String {
    (Utc::now() + chrono::Duration::seconds(seconds.max(0))).to_rfc3339()
}

pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builder_id_without_profile_uses_placeholder() {
        let account = account_from_import(&json!({
            "refreshToken": "rt-1",
            "authMethod": "builder-id",
            "clientId": "cid",
            "clientSecret": "sec"
        }))
        .unwrap();
        assert_eq!(account.auth_method, "idc");
        assert_eq!(account.provider, "BuilderId");
        assert_eq!(streaming_profile_arn(&account), BUILDER_ID_PROFILE_ARN);
        assert!(is_placeholder_profile_arn(&account.profile_arn));
    }

    #[test]
    fn social_import_uses_social_arn() {
        let account = account_from_import(&json!({
            "refreshToken": "rt-social",
            "provider": "Google"
        }))
        .unwrap();
        assert_eq!(account.auth_method, "social");
        assert_eq!(account.profile_arn, SOCIAL_PROFILE_ARN);
    }
}
