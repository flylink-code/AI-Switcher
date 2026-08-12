//! Pi CLI 路径与版本探测 (`@earendil-works/pi-coding-agent`)

use serde::Serialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AppResult;
use crate::process_util::apply_no_window;

const PI_NPM_PACKAGE: &str = "@earendil-works/pi-coding-agent";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCliVersionInfo {
    pub installed: bool,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub executable_path: Option<String>,
    pub install_command: String,
    pub update_command: String,
    pub error: Option<String>,
}

/// 解析 CLI 版本号（寻找像 x.y.z 的数字版本串）
fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let clean = token.trim_matches(|c: char| {
            !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+'
        });
        if clean.chars().next().is_some_and(|c| c.is_ascii_digit()) && clean.contains('.') {
            Some(clean.to_string())
        } else {
            None
        }
    })
}

/// 收集候选 Pi 可执行文件路径
fn get_pi_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. 系统 PATH 中通过 `where pi` / `which pi`
    #[cfg(windows)]
    {
        let mut cmd = Command::new("where");
        cmd.arg("pi");
        apply_no_window(&mut cmd);
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        candidates.push(p);
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("which");
        cmd.arg("pi");
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let p = PathBuf::from(stdout.trim());
                if p.is_file() {
                    candidates.push(p);
                }
            }
        }
    }

    // 2. 探查常规 npm / bun 全局 bin 路径
    if let Ok(user_profile) = env::var("USERPROFILE") {
        let base = PathBuf::from(user_profile);
        // Bun global bin
        candidates.push(base.join(".bun").join("bin").join("pi.exe"));
        candidates.push(base.join(".bun").join("bin").join("pi.cmd"));
        candidates.push(base.join(".bun").join("bin").join("pi"));
    }

    if let Ok(app_data) = env::var("APPDATA") {
        let base = PathBuf::from(app_data);
        // npm global bin on Windows
        candidates.push(base.join("npm").join("pi.cmd"));
        candidates.push(base.join("npm").join("pi.exe"));
        candidates.push(base.join("npm").join("pi"));
    }

    if let Ok(home) = env::var("HOME") {
        let base = PathBuf::from(home);
        candidates.push(base.join(".bun").join("bin").join("pi"));
        candidates.push(base.join(".nvm").join("versions").join("node")); // 通用参考
        candidates.push(PathBuf::from("/usr/local/bin/pi"));
        candidates.push(PathBuf::from("/usr/bin/pi"));
    }

    candidates
}

/// 执行探测 Pi CLI 路径和版本
pub fn detect_pi_cli_sync() -> PiCliVersionInfo {
    let install_cmd = format!("npm install -g {PI_NPM_PACKAGE}@latest");
    let update_cmd = format!("npm install -g {PI_NPM_PACKAGE}@latest");

    let candidates = get_pi_candidates();
    let mut found_path: Option<PathBuf> = None;
    let mut found_version: Option<String> = None;
    let mut last_err: Option<String> = None;

    for path in candidates {
        if !path.exists() || !path.is_file() {
            continue;
        }

        let mut cmd = Command::new(&path);
        cmd.arg("--version");
        apply_no_window(&mut cmd);

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let combined = format!("{stdout}\n{stderr}");

                if let Some(ver) = parse_version(&combined) {
                    found_path = Some(path);
                    found_version = Some(ver);
                    break;
                } else if output.status.success() && !stdout.is_empty() {
                    found_path = Some(path);
                    found_version = Some(stdout);
                    break;
                }
            }
            Err(e) => {
                last_err = Some(format!("执行 {} --version 失败: {}", path.display(), e));
            }
        }
    }

    if let Some(path) = found_path {
        PiCliVersionInfo {
            installed: true,
            current_version: found_version,
            latest_version: None,
            executable_path: Some(path.to_string_lossy().to_string()),
            install_command: install_cmd,
            update_command: update_cmd,
            error: None,
        }
    } else {
        PiCliVersionInfo {
            installed: false,
            current_version: None,
            latest_version: None,
            executable_path: None,
            install_command: install_cmd,
            update_command: update_cmd,
            error: last_err.or_else(|| Some("未检测到 Pi CLI (pi)".to_string())),
        }
    }
}
