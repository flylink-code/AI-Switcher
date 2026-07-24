//! API-key storage backed only by the operating-system credential manager.
//!
//! The SQLite `providers.api_key` column contains a `kr://<provider-id>` marker;
//! plaintext credentials never cross the database or Tauri IPC boundary.

use crate::error::{AppError, AppResult};

/// Service name under which provider keys are filed in the OS credential store.
pub const KEYRING_SERVICE: &str = "com.claude-switcher.provider";
/// Prefix marking a stored value as a credential-store reference.
pub const KEYRING_REF_PREFIX: &str = "kr://";

pub fn keyring_ref(provider_id: &str) -> String {
    format!("{KEYRING_REF_PREFIX}{provider_id}")
}

pub fn is_keyring_ref(value: &str) -> bool {
    value.starts_with(KEYRING_REF_PREFIX)
}

fn entry(account: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| AppError::Config(format!("系统凭据库不可用: {e}")))
}

/// Verify write access before accepting a secret. There is intentionally no
/// memory/local-file fallback: an API key must survive an app restart.
pub fn ensure_available() -> AppResult<()> {
    let probe = entry("__claude_switcher_probe__")?;
    probe
        .set_password("")
        .map_err(|e| AppError::Config(format!("系统凭据库不可用，无法安全保存 API Key: {e}")))?;
    let _ = probe.delete_credential();
    Ok(())
}

pub fn store_key(account: &str, secret: &str) -> AppResult<()> {
    ensure_available()?;
    entry(account)?
        .set_password(secret)
        .map_err(|e| AppError::Config(format!("写入系统凭据库失败: {e}")))
}

pub fn load_key(account: &str) -> AppResult<Option<String>> {
    match entry(account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Config(format!("读取系统凭据库失败: {e}"))),
    }
}

pub fn delete_key(account: &str) -> AppResult<()> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Config(format!("删除系统凭据库条目失败: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_helpers_round_trip() {
        let reference = keyring_ref("p_abc123");
        assert!(is_keyring_ref(&reference));
        assert_eq!(reference, "kr://p_abc123");
        assert!(!is_keyring_ref("sk-plainsecret"));
    }
}
