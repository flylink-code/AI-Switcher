//! Pi CLI 路径与版本探测 (`@earendil-works/pi-coding-agent`)

use serde::Serialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process_util::apply_no_window;

const PI_NPM_PACKAGE: &str = "@earendil-works/pi-coding-agent";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCliVersionInfo {
    pub installed: bool,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub install_command: String,
    pub update_command: String,
    pub error: Option<String>,
    pub executable_path: Option<String>,
    pub source: Option<String>,
    pub environment: String,
    pub installed_but_broken: bool,
}

fn install_command() -> String {
    format!("npm install -g {PI_NPM_PACKAGE}@latest")
}

fn environment_label() -> String {
    if cfg!(windows) {
        "windows".to_string()
    } else {
        "native".to_string()
    }
}

fn infer_pi_source(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("/.bun/") || normalized.contains("/bun/") {
        "bun".to_string()
    } else if normalized.contains("/pnpm/") {
        "pnpm".to_string()
    } else if normalized.contains("/volta/") {
        "volta".to_string()
    } else if normalized.contains("/nvm/") {
        "nvm".to_string()
    } else if normalized.contains("fnm_multishell")
        || normalized.contains("/fnm/")
        || normalized.contains("\\fnm\\")
        || normalized.contains("node-versions")
    {
        "fnm".to_string()
    } else if normalized.contains("/npm/") || normalized.contains("/appdata/roaming/npm") {
        "npm".to_string()
    } else {
        "system".to_string()
    }
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

    if let Ok(user_profile) = env::var("USERPROFILE") {
        let base = PathBuf::from(user_profile);
        candidates.push(base.join(".bun").join("bin").join("pi.exe"));
        candidates.push(base.join(".bun").join("bin").join("pi"));
        if let Ok(appdata) = env::var("APPDATA") {
            let npm = PathBuf::from(appdata).join("npm");
            candidates.push(npm.join("pi.cmd"));
            candidates.push(npm.join("pi"));
        }
        candidates.push(base.join("AppData").join("Roaming").join("npm").join("pi.cmd"));
    }

    if let Ok(home) = env::var("HOME") {
        let base = PathBuf::from(home);
        candidates.push(base.join(".bun").join("bin").join("pi"));
        candidates.push(base.join(".local").join("bin").join("pi"));
    }

    candidates
}

fn empty_info(error: Option<String>) -> PiCliVersionInfo {
    let cmd = install_command();
    PiCliVersionInfo {
        installed: false,
        current_version: None,
        latest_version: None,
        update_available: false,
        install_command: cmd.clone(),
        update_command: cmd,
        error,
        executable_path: None,
        source: None,
        environment: environment_label(),
        installed_but_broken: false,
    }
}

/// 执行探测 Pi CLI 路径和版本（不含 npm latest）
pub fn detect_pi_cli_sync() -> PiCliVersionInfo {
    let cmd = install_command();
    let candidates = get_pi_candidates();
    let mut found_path: Option<PathBuf> = None;
    let mut found_version: Option<String> = None;
    let mut last_err: Option<String> = None;
    let mut broken_path: Option<PathBuf> = None;

    for path in candidates {
        if !path.exists() || !path.is_file() {
            continue;
        }

        let mut version_cmd = Command::new(&path);
        version_cmd.arg("--version");
        apply_no_window(&mut version_cmd);

        match version_cmd.output() {
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
                } else {
                    broken_path = Some(path);
                    last_err = Some(format!("pi --version 无法解析版本: {combined}"));
                }
            }
            Err(e) => {
                broken_path = Some(path.clone());
                last_err = Some(format!("执行 {} --version 失败: {e}", path.display()));
            }
        }
    }

    if let Some(path) = found_path {
        PiCliVersionInfo {
            installed: true,
            current_version: found_version,
            latest_version: None,
            update_available: false,
            install_command: cmd.clone(),
            update_command: cmd,
            error: None,
            source: Some(infer_pi_source(&path)),
            executable_path: Some(path.to_string_lossy().to_string()),
            environment: environment_label(),
            installed_but_broken: false,
        }
    } else if let Some(path) = broken_path {
        PiCliVersionInfo {
            installed: true,
            current_version: None,
            latest_version: None,
            update_available: false,
            install_command: cmd.clone(),
            update_command: cmd,
            error: last_err,
            source: Some(infer_pi_source(&path)),
            executable_path: Some(path.to_string_lossy().to_string()),
            environment: environment_label(),
            installed_but_broken: true,
        }
    } else {
        empty_info(last_err.or_else(|| Some("未检测到 Pi CLI (pi)".to_string())))
    }
}

/// `npm view <pkg> version` — best-effort, returns None on failure.
pub fn fetch_pi_npm_latest_sync() -> Option<String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "npm", "view", PI_NPM_PACKAGE, "version"]);
        c
    } else {
        let mut c = Command::new("npm");
        c.args(["view", PI_NPM_PACKAGE, "version"]);
        c
    };
    apply_no_window(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub fn apply_pi_latest(mut info: PiCliVersionInfo, latest: Option<String>) -> PiCliVersionInfo {
    info.latest_version = latest.clone();
    info.update_available = info
        .current_version
        .as_deref()
        .zip(latest.as_deref())
        .map(|(current, latest)| current != latest)
        .unwrap_or(false);
    info
}
