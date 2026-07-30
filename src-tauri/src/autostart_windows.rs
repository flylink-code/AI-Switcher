//! Windows launch-at-login registration via HKCU Run + StartupApproved.

#![cfg(windows)]

use std::env;
use std::io;
use std::path::Path;

use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::{RegKey, RegValue};

use crate::error::{AppError, AppResult};

/// Stable Task Manager / Run key name (matches `productName`).
pub const AUTOSTART_APP_NAME: &str = "AI-Switcher";

const RUN_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_SUBKEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Legacy Run / StartupApproved value names from earlier product branding.
const LEGACY_APP_NAMES: &[&str] = &["Claude Switcher", "claude-switcher"];

/// Task Manager "enabled" blob: status 0x02 + eight zero timestamp bytes.
const APPROVED_ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartRegistration {
    pub registry_name: String,
    pub command: Option<String>,
    pub enabled: bool,
    pub task_manager_disabled: bool,
}

pub fn registration_status() -> AppResult<AutostartRegistration> {
    let command = read_run_value(AUTOSTART_APP_NAME)?;
    let approved = read_approved_bytes(AUTOSTART_APP_NAME)?;
    let task_manager_disabled = approved
        .as_ref()
        .map(|bytes| !approved_means_enabled(bytes))
        .unwrap_or(false);
    let enabled = command.is_some() && !task_manager_disabled;
    Ok(AutostartRegistration {
        registry_name: AUTOSTART_APP_NAME.to_string(),
        command,
        enabled,
        task_manager_disabled,
    })
}

pub fn enable() -> AppResult<()> {
    cleanup_legacy_names()?;
    let command = build_autostart_command()?;
    write_run_value(AUTOSTART_APP_NAME, &command)?;
    write_approved_enabled(AUTOSTART_APP_NAME)?;
    let status = registration_status()?;
    if !status.enabled {
        return Err(AppError::Other(
            "开机自启已写入注册表，但仍被任务管理器禁用或校验失败，请在任务管理器中启用 AI-Switcher 后重试"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn disable() -> AppResult<()> {
    delete_run_value(AUTOSTART_APP_NAME)?;
    delete_approved_value(AUTOSTART_APP_NAME)?;
    cleanup_legacy_names()?;
    Ok(())
}

pub fn cleanup_legacy_names() -> AppResult<()> {
    for name in LEGACY_APP_NAMES {
        delete_run_value(name)?;
        delete_approved_value(name)?;
    }
    Ok(())
}

pub fn build_autostart_command() -> AppResult<String> {
    let exe = current_exe_path()?;
    Ok(format_quoted_command(&exe, &["--autostart"]))
}

/// Quote the executable path and append args. The exe path is always quoted.
pub fn format_quoted_command(exe: &str, args: &[&str]) -> String {
    let mut parts = Vec::with_capacity(1 + args.len());
    parts.push(format!("\"{}\"", exe.replace('"', "\\\"")));
    for arg in args {
        if arg.chars().any(|c| c.is_whitespace() || c == '"') {
            parts.push(format!("\"{}\"", arg.replace('"', "\\\"")));
        } else {
            parts.push((*arg).to_string());
        }
    }
    parts.join(" ")
}

fn current_exe_path() -> AppResult<String> {
    let path = env::current_exe().map_err(|e| AppError::Other(format!("读取程序路径失败: {e}")))?;
    Ok(normalize_exe_display(&path))
}

fn normalize_exe_display(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

fn read_run_value(name: &str) -> AppResult<Option<String>> {
    let key = hkcu()
        .open_subkey_with_flags(RUN_SUBKEY, KEY_READ)
        .map_err(map_reg_err)?;
    match key.get_value::<String, _>(name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(map_reg_err(error)),
    }
}

fn write_run_value(name: &str, command: &str) -> AppResult<()> {
    let (key, _) = hkcu().create_subkey(RUN_SUBKEY).map_err(map_reg_err)?;
    key.set_value(name, &command).map_err(map_reg_err)?;
    Ok(())
}

fn delete_run_value(name: &str) -> AppResult<()> {
    let key = match hkcu().open_subkey_with_flags(RUN_SUBKEY, KEY_SET_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(map_reg_err(error)),
    };
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_reg_err(error)),
    }
}

fn read_approved_bytes(name: &str) -> AppResult<Option<Vec<u8>>> {
    let key = match hkcu().open_subkey_with_flags(APPROVED_SUBKEY, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_reg_err(error)),
    };
    match key.get_raw_value(name) {
        Ok(value) => Ok(Some(value.bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(map_reg_err(error)),
    }
}

fn write_approved_enabled(name: &str) -> AppResult<()> {
    let (key, _) = hkcu().create_subkey(APPROVED_SUBKEY).map_err(map_reg_err)?;
    key.set_raw_value(
        name,
        &RegValue {
            bytes: APPROVED_ENABLED.to_vec(),
            vtype: RegType::REG_BINARY,
        },
    )
    .map_err(map_reg_err)?;
    Ok(())
}

fn delete_approved_value(name: &str) -> AppResult<()> {
    let key = match hkcu().open_subkey_with_flags(APPROVED_SUBKEY, KEY_SET_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(map_reg_err(error)),
    };
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_reg_err(error)),
    }
}

fn approved_means_enabled(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return true;
    }
    // Match auto-launch: last eight bytes all zero ⇒ enabled (or never stamped disabled).
    bytes.iter().rev().take(8).all(|b| *b == 0)
}

fn map_reg_err(error: io::Error) -> AppError {
    AppError::Other(format!("读写开机自启注册表失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_with_spaces() {
        let command =
            format_quoted_command(r"C:\Program Files\AI-Switcher\AISwitcher.exe", &["--autostart"]);
        assert_eq!(
            command,
            r#""C:\Program Files\AI-Switcher\AISwitcher.exe" --autostart"#
        );
    }

    #[test]
    fn quotes_simple_paths_for_run_key_safety() {
        let command =
            format_quoted_command(r"F:\software\AI-Switcher\AISwitcher.exe", &["--autostart"]);
        assert_eq!(
            command,
            r#""F:\software\AI-Switcher\AISwitcher.exe" --autostart"#
        );
    }

    #[test]
    fn approved_disabled_blob_is_detected() {
        let disabled = [0x03, 0, 0, 0, 0x5b, 0x01, 0x5d, 0x32, 0x3a, 0x1f, 0xdd, 0x01];
        assert!(!approved_means_enabled(&disabled));
        assert!(approved_means_enabled(&APPROVED_ENABLED));
    }

    #[test]
    fn live_enable_disable_roundtrip_cleans_legacy_names() {
        let _ = cleanup_legacy_names();
        enable().expect("enable autostart");
        let status = registration_status().expect("status after enable");
        assert!(status.enabled, "{status:?}");
        assert_eq!(status.registry_name, AUTOSTART_APP_NAME);
        assert!(
            status
                .command
                .as_deref()
                .is_some_and(|command| command.contains("--autostart") && command.starts_with('"')),
            "{status:?}"
        );
        assert!(!status.task_manager_disabled);
        assert!(read_run_value("Claude Switcher").unwrap().is_none());
        assert!(read_approved_bytes("Claude Switcher").unwrap().is_none());

        disable().expect("disable autostart");
        let status = registration_status().expect("status after disable");
        assert!(!status.enabled, "{status:?}");
        assert!(status.command.is_none());
        assert!(read_run_value(AUTOSTART_APP_NAME).unwrap().is_none());
        assert!(read_approved_bytes(AUTOSTART_APP_NAME).unwrap().is_none());
    }
}
