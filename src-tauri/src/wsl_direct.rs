//! WSL Direct: detect whether Claude/Codex configs should be copied into a distro.

use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::config::paths::{get_claude_settings_path, get_codex_config_path, get_home_dir};
use crate::error::{AppError, AppResult};
use crate::process_util::apply_no_window;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslRuntimeStatus {
    pub location: String,
    pub distro: Option<String>,
    pub linux_home: Option<String>,
    pub skip_copy: bool,
    pub claude_unc: Option<String>,
    pub codex_unc: Option<String>,
}

pub fn detect_runtime() -> WslRuntimeStatus {
    if cfg!(not(windows)) {
        return WslRuntimeStatus {
            location: if is_wsl_proc() {
                "wsl_direct".into()
            } else {
                "native".into()
            },
            distro: None,
            linux_home: None,
            skip_copy: is_wsl_proc(),
            claude_unc: None,
            codex_unc: None,
        };
    }
    let distro = default_distro();
    let linux_home = distro.as_deref().and_then(linux_home_for);
    let unc_root = distro.as_deref().and_then(|name| {
        linux_home
            .as_deref()
            .map(|home| format!(r"\\wsl$\{}\{}", name, home.trim_start_matches('/').replace('/', "\\")))
    });
    WslRuntimeStatus {
        location: if distro.is_some() { "wsl_unc".into() } else { "windows".into() },
        distro,
        linux_home,
        skip_copy: false,
        claude_unc: unc_root.as_ref().map(|root| format!("{root}\\.claude\\settings.json")),
        codex_unc: unc_root.as_ref().map(|root| format!("{root}\\.codex\\config.toml")),
    }
}

pub fn sync_claude_codex_files() -> AppResult<WslRuntimeStatus> {
    let status = detect_runtime();
    if status.skip_copy {
        return Ok(status);
    }
    if let Some(dest) = status.claude_unc.as_deref() {
        copy_if_exists(&get_claude_settings_path(), Path::new(dest))?;
    }
    if let Some(dest) = status.codex_unc.as_deref() {
        copy_if_exists(&get_codex_config_path(), Path::new(dest))?;
    }
    Ok(status)
}

fn copy_if_exists(src: &Path, dest: &Path) -> AppResult<()> {
    if !src.is_file() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::Config(format!("无法创建 WSL 目标目录 {}: {error}", parent.display()))
        })?;
    }
    std::fs::copy(src, dest).map_err(|error| {
        AppError::Config(format!("同步到 {} 失败: {error}", dest.display()))
    })?;
    Ok(())
}

fn is_wsl_proc() -> bool {
    Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").is_file()
        || std::env::var("WSL_DISTRO_NAME").is_ok()
}

fn default_distro() -> Option<String> {
    let mut command = Command::new("wsl.exe");
    apply_no_window(&mut command);
    let output = command.args(["-l", "-q"]).output().ok()?;
    let text = String::from_utf16_lossy(
        &output
            .stdout
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>(),
    );
    let ascii = String::from_utf8_lossy(&output.stdout);
    let combined = if text.trim().is_empty() { ascii.into_owned() } else { text };
    combined
        .lines()
        .map(|line| line.trim().trim_matches('\u{0}').to_string())
        .find(|line| !line.is_empty())
}

fn linux_home_for(distro: &str) -> Option<String> {
    let mut command = Command::new("wsl.exe");
    apply_no_window(&mut command);
    let output = command
        .args(["-d", distro, "--exec", "printenv", "HOME"])
        .output()
        .ok()?;
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.starts_with('/') {
        Some(home)
    } else {
        get_home_dir()
            .file_name()
            .map(|name| format!("/home/{}", name.to_string_lossy()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_runtime_returns_status() {
        let status = detect_runtime();
        assert!(!status.location.is_empty());
    }
}
