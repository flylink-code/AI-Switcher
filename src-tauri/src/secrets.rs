//! API-key storage backed only by the operating-system credential manager.
//!
//! The SQLite `providers.api_key` column contains a `kr://<provider-id>` marker;
//! plaintext credentials never cross the database or Tauri IPC boundary.

use crate::error::{AppError, AppResult};

/// Service name under which provider keys are filed in the OS credential store.
pub const KEYRING_SERVICE: &str = "com.claude-switcher.provider";
/// 隔离测试使用独立凭据命名空间，不访问正式条目。
const TEST_KEYRING_SERVICE: &str = "com.claude-switcher.provider.test";
/// Prefix marking a stored value as a credential-store reference.
pub const KEYRING_REF_PREFIX: &str = "kr://";

pub fn keyring_ref(provider_id: &str) -> String {
    format!("{KEYRING_REF_PREFIX}{provider_id}")
}

pub fn is_keyring_ref(value: &str) -> bool {
    value.starts_with(KEYRING_REF_PREFIX)
}

fn keyring_service() -> String {
    if !crate::config::paths::test_isolation_enabled() {
        return KEYRING_SERVICE.to_string();
    }
    // 每个临时 HOME 独立；没有 HOME 的单元测试按进程隔离。
    // 同一 HOME 重启后仍可取回 Key，不把测试明文落盘。
    use sha2::{Digest, Sha256};
    let scope = std::env::var_os(crate::config::paths::TEST_HOME_ENV)
        .filter(|home| std::path::Path::new(home).is_absolute())
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            static SCOPE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            SCOPE.get_or_init(|| uuid::Uuid::new_v4().to_string()).clone()
        });
    format!("{TEST_KEYRING_SERVICE}.{:x}", Sha256::digest(scope.as_bytes()))
}

fn entry(account: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(&keyring_service(), account)
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
    #[cfg(test)]
    if let Some(secret) = test_credentials::lookup(account) {
        return Ok(Some(secret));
    }
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

/// 仅单元测试编译：显式注册的合成凭据，不作为系统凭据失败的回退。
#[cfg(test)]
pub(crate) mod test_credentials {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn keys() -> &'static Mutex<HashMap<String, String>> {
        static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        KEYS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) struct Credential(String);

    impl Credential {
        pub(crate) fn new(secret: &str) -> Self {
            let account = format!("aisw_unit_{}", uuid::Uuid::new_v4().simple());
            keys().lock().unwrap().insert(account.clone(), secret.to_string());
            Self(account)
        }

        pub(crate) fn reference(&self) -> String {
            super::keyring_ref(&self.0)
        }
    }

    impl Drop for Credential {
        fn drop(&mut self) {
            keys().lock().unwrap().remove(&self.0);
        }
    }

    pub(super) fn lookup(account: &str) -> Option<String> {
        keys().lock().unwrap().get(account).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_credentials_are_scoped_and_keep_stored_key_validation() {
        let credential = test_credentials::Credential::new("synthetic-key");
        let reference = credential.reference();
        assert_eq!(
            crate::database::dao::materialize_api_key(&reference).unwrap().as_deref(),
            Some("synthetic-key")
        );
        assert!(crate::database::dao::materialize_api_key("synthetic-key").is_err());
        drop(credential);
        assert!(test_credentials::lookup(&reference[KEYRING_REF_PREFIX.len()..]).is_none());
    }

    #[test]
    fn tests_never_use_production_keyring_service() {
        let home = tempfile::tempdir().unwrap();
        crate::config::paths::with_isolated_home(home.path(), || {
            let service = keyring_service();
            assert!(service.starts_with(&format!("{TEST_KEYRING_SERVICE}.")));
            assert_ne!(service, KEYRING_SERVICE);
            assert_eq!(service, keyring_service());
        });
    }

    #[test]
    fn ref_helpers_round_trip() {
        let reference = keyring_ref("p_abc123");
        assert!(is_keyring_ref(&reference));
        assert_eq!(reference, "kr://p_abc123");
        assert!(!is_keyring_ref("sk-plainsecret"));
    }
}
