//! Probe and ensure a Node.js runtime (≥22) via fnm for CLI installs.

use serde::Serialize;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use crate::error::{AppError, AppResult};

const MINIMUM_NODE_MAJOR: u64 = 22;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Node.js dist mirror used by fnm (same host as the CSDN Ubuntu install guide).
pub const FNM_NODE_DIST_MIRROR: &str = "https://npmmirror.com/mirrors/node";
/// npm registry mirror for global CLI installs.
pub const NPM_REGISTRY_MIRROR: &str = "https://registry.npmmirror.com";
const DEFAULT_GITHUB_MIRROR_BASE: &str = "https://gh-proxy.com/";
#[cfg(not(windows))]
const FNM_RELEASE_ZIP_LINUX: &str =
    "https://github.com/Schniz/fnm/releases/latest/download/fnm-linux.zip";
#[cfg(not(windows))]
const FNM_RELEASE_ZIP_LINUX_ARM64: &str =
    "https://github.com/Schniz/fnm/releases/latest/download/fnm-arm64.zip";
#[cfg(windows)]
const FNM_RELEASE_ZIP_WINDOWS: &str =
    "https://github.com/Schniz/fnm/releases/latest/download/fnm-windows.zip";
#[cfg(not(windows))]
const FNM_INSTALL_SCRIPT: &str = "https://fnm.vercel.app/install";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRuntimeStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub meets_minimum: bool,
    pub npm_path: Option<String>,
    pub node_path: Option<String>,
    pub source: String,
    pub fnm_installed: bool,
    pub install_hint: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedNodeRuntime {
    pub node_path: PathBuf,
    pub npm_path: PathBuf,
}

fn decode_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).any(|byte| *byte == 0) {
        let utf16 = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&utf16)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(not(windows))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub fn parse_node_version(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_start_matches('v').trim();
    let token = trimmed
        .split_whitespace()
        .next()?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+');
    if token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        && token.contains('.')
    {
        Some(token.to_string())
    } else {
        None
    }
}

pub fn version_meets_minimum(version: &str, minimum_major: u64) -> bool {
    let major = version
        .split(|c| c == '.' || c == '-')
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    major >= minimum_major
}

pub fn map_npm_install_error(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("npm: not found")
        || lower.contains("'npm' is not recognized")
        || lower.contains("npm.cmd' is not recognized")
        || (lower.contains("cannot find path") && lower.contains("npm"))
    {
        return format!(
            "NODE_RUNTIME_MISSING: npm was not found. Install Node.js ≥{MINIMUM_NODE_MAJOR} via fnm first. Detail: {detail}"
        );
    }
    if lower.contains("ebadengine") || lower.contains("engine \"node\"") {
        return format!(
            "NODE_RUNTIME_TOO_OLD: Node.js ≥{MINIMUM_NODE_MAJOR} is required. Detail: {detail}"
        );
    }
    if lower.contains("econnrefused")
        || lower.contains("etimedout")
        || lower.contains("enetunreach")
        || lower.contains("network")
        || lower.contains("getaddrinfo")
        || lower.contains("certificate")
        || (lower.contains("fetch failed") && lower.contains("registry"))
        || lower.contains("npm err! code eai_again")
    {
        return format!(
            "NODE_NETWORK_OR_REGISTRY: npm/Node download failed (network or registry). Detail: {detail}"
        );
    }
    detail.to_string()
}

/// Build candidate URLs for a GitHub asset: direct first, then configured/default mirrors.
pub fn github_download_candidates(direct_url: &str, mirror_base: Option<&str>) -> Vec<String> {
    let mut urls = vec![direct_url.to_string()];
    let mut bases = Vec::new();
    if let Some(base) = mirror_base.map(str::trim).filter(|value| !value.is_empty()) {
        let normalized = if base.ends_with('/') {
            base.to_string()
        } else {
            format!("{base}/")
        };
        bases.push(normalized);
    }
    if !bases.iter().any(|base| base == DEFAULT_GITHUB_MIRROR_BASE) {
        bases.push(DEFAULT_GITHUB_MIRROR_BASE.to_string());
    }
    for base in bases {
        let mirrored = format!("{base}{direct_url}");
        if !urls.iter().any(|url| url == &mirrored) {
            urls.push(mirrored);
        }
    }
    urls
}

fn configure_hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

fn run_output(mut command: Command) -> io::Result<Output> {
    configure_hidden(&mut command);
    command.output()
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn fnm_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        dirs.push(home.join(".fnm"));
        dirs.push(home.join(".local").join("share").join("fnm"));
        #[cfg(target_os = "macos")]
        dirs.push(home.join("Library").join("Application Support").join("fnm"));
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data).join("fnm"));
    }
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("fnm"));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("fnm"));
        }
    }
    dirs
}

fn fnm_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("fnm");
        }
    }
    if let Some(home) = home_dir() {
        return home.join(".local").join("share").join("fnm");
    }
    PathBuf::from(".fnm")
}

fn looks_like_fnm_path(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase();
    text.contains("fnm")
}

fn push_bin_dir(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() || !path.is_dir() || !seen.insert(path.clone()) {
        return;
    }
    dirs.push(path);
}

/// Bin directories for fnm-managed Node installs.
///
/// GUI apps on Linux/macOS often lack `fnm env` on PATH, so global npm CLIs
/// that land under `node-versions/*/installation/bin` are invisible unless we
/// scan these roots explicitly.
pub fn fnm_node_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    for root in fnm_candidate_dirs() {
        push_bin_dir(
            &mut dirs,
            &mut seen,
            root.join("aliases").join("default").join("bin"),
        );
        if let Ok(entries) = std::fs::read_dir(root.join("aliases")) {
            for entry in entries.flatten().take(64) {
                push_bin_dir(&mut dirs, &mut seen, entry.path().join("bin"));
            }
        }
        if let Ok(entries) = std::fs::read_dir(root.join("node-versions")) {
            for entry in entries.flatten().take(96) {
                let version_root = entry.path();
                push_bin_dir(
                    &mut dirs,
                    &mut seen,
                    version_root.join("installation").join("bin"),
                );
                push_bin_dir(&mut dirs, &mut seen, version_root.join("bin"));
            }
        }
    }

    if let Some(fnm) = find_fnm_binary() {
        if let Some((node, _, _, _)) = probe_fnm_default_node(&fnm) {
            if let Some(parent) = node.parent() {
                push_bin_dir(&mut dirs, &mut seen, parent.to_path_buf());
            }
        }
    }

    dirs
}

fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            dir.join(format!("{name}.exe")),
            dir.join(format!("{name}.cmd")),
            dir.join(format!("{name}.bat")),
            dir.join(name),
        ]
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        vec![dir.join(name)]
    }
}

fn find_fnm_binary() -> Option<PathBuf> {
    if let Some(path) = resolve_command_on_path("fnm") {
        return Some(path);
    }
    for dir in fnm_candidate_dirs() {
        for candidate in executable_candidates(&dir, "fnm") {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_command_on_path(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut command = Command::new("where");
        command.arg(name);
        let output = run_output(command).ok()?;
        if !output.status.success() {
            return None;
        }
        let first = decode_output(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())?
            .to_string();
        Some(PathBuf::from(first))
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = resolve_via_login_shell(name) {
            return Some(path);
        }
        let output = run_output({
            let mut command = Command::new("sh");
            command.args(["-c", &format!("command -v {}", shell_single_quote(name))]);
            command
        })
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = decode_output(&output.stdout).trim().to_string();
        if path.is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        }
    }
}

#[cfg(not(windows))]
fn resolve_via_login_shell(name: &str) -> Option<PathBuf> {
    let script = format!("command -v {}", shell_single_quote(name));
    let output = run_output({
        let mut command = Command::new("sh");
        command.args(["-lc", &script]);
        command
    })
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = decode_output(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn read_node_version(node: &Path) -> Option<String> {
    let output = run_tool_version(node, &["--version"]).ok()?;
    if !output.status.success() {
        return None;
    }
    parse_node_version(&decode_output(&output.stdout))
        .or_else(|| parse_node_version(&decode_output(&output.stderr)))
}

fn run_tool_version(program: &Path, args: &[&str]) -> io::Result<Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let extension = program
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if extension.eq_ignore_ascii_case("cmd")
            || extension.eq_ignore_ascii_case("bat")
            || extension.is_empty()
        {
            let mut command_line = format!("call {}", quoted(&program.display().to_string()));
            for arg in args {
                command_line.push(' ');
                command_line.push_str(&quoted(arg));
            }
            let mut command = Command::new("cmd");
            command
                .args(["/D", "/S", "/C"])
                .raw_arg(command_line)
                .creation_flags(CREATE_NO_WINDOW);
            return command.output();
        }
        let mut command = Command::new(program);
        command.args(args).creation_flags(CREATE_NO_WINDOW);
        command.output()
    }
    #[cfg(not(windows))]
    {
        Command::new(program).args(args).output()
    }
}

fn sibling_npm(node: &Path) -> Option<PathBuf> {
    let dir = node.parent()?;
    for candidate in executable_candidates(dir, "npm") {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn probe_from_node(node: PathBuf, source_hint: &str) -> Option<(PathBuf, PathBuf, String, String)> {
    let version = read_node_version(&node)?;
    let npm = sibling_npm(&node).or_else(|| resolve_command_on_path("npm"))?;
    let source = if source_hint == "fnm" || looks_like_fnm_path(&node) {
        "fnm".to_string()
    } else {
        "system".to_string()
    };
    Some((node, npm, version, source))
}

fn probe_fnm_default_node(fnm: &Path) -> Option<(PathBuf, PathBuf, String, String)> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let command_line = format!(
            "for /f \"delims=\" %i in ('{} env --shell cmd') do @call %i & where node",
            quoted(&fnm.display().to_string())
        );
        let mut command = Command::new("cmd");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg(command_line)
            .creation_flags(CREATE_NO_WINDOW);
        let path_output = command.output().ok()?;
        if !path_output.status.success() {
            return None;
        }
        let node_path = decode_output(&path_output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && Path::new(line).is_file())
            .map(PathBuf::from)?;
        return probe_from_node(node_path, "fnm");
    }

    #[cfg(not(windows))]
    {
        let output = run_output({
            let mut command = Command::new(fnm);
            command.args(["env", "--shell", "bash"]);
            command
        })
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let env_script = decode_output(&output.stdout);
        let script = format!("{env_script}\ncommand -v node");
        let path_output = run_output({
            let mut command = Command::new("sh");
            command.args(["-lc", &script]);
            command
        })
        .ok()?;
        if !path_output.status.success() {
            return None;
        }
        let node_path = decode_output(&path_output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.contains('='))
            .map(PathBuf::from)?;
        probe_from_node(node_path, "fnm")
    }
}

fn common_system_node_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        dirs.push(PathBuf::from(r"C:\Program Files\nodejs"));
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs").join("nodejs"));
        }
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(home.join("n").join("bin"));
            dirs.push(home.join(".nvm").join("current").join("bin"));
        }
    }
    dirs
}

fn probe_system_node() -> Option<(PathBuf, PathBuf, String, String)> {
    if let Some(node) = resolve_command_on_path("node") {
        if let Some(result) = probe_from_node(node, "system") {
            return Some(result);
        }
    }
    for dir in common_system_node_dirs() {
        for candidate in executable_candidates(&dir, "node") {
            if candidate.is_file() {
                if let Some(result) = probe_from_node(candidate, "system") {
                    return Some(result);
                }
            }
        }
    }
    None
}

fn status_from_probe(
    probe: Option<(PathBuf, PathBuf, String, String)>,
    fnm_installed: bool,
) -> NodeRuntimeStatus {
    match probe {
        Some((node, npm, version, source)) => {
            let meets = version_meets_minimum(&version, MINIMUM_NODE_MAJOR);
            let hint = if meets {
                format!("Node.js {version} is ready.")
            } else {
                format!(
                    "Node.js {version} is below the required major version {MINIMUM_NODE_MAJOR}. Install Node via fnm."
                )
            };
            NodeRuntimeStatus {
                installed: true,
                version: Some(version),
                meets_minimum: meets,
                npm_path: Some(npm.display().to_string()),
                node_path: Some(node.display().to_string()),
                source,
                fnm_installed,
                install_hint: hint,
            }
        }
        None => NodeRuntimeStatus {
            installed: false,
            version: None,
            meets_minimum: false,
            npm_path: None,
            node_path: None,
            source: "none".to_string(),
            fnm_installed,
            install_hint: format!(
                "Node.js ≥{MINIMUM_NODE_MAJOR} was not found. Install it with fnm from the About page."
            ),
        },
    }
}

pub fn probe_node_runtime() -> NodeRuntimeStatus {
    let fnm = find_fnm_binary();
    let fnm_installed = fnm.is_some();

    if let Some(fnm_path) = &fnm {
        if let Some(probe) = probe_fnm_default_node(fnm_path) {
            let status = status_from_probe(Some(probe), true);
            if status.meets_minimum {
                return status;
            }
            // Prefer a meeting system node over an old fnm default.
            if let Some(system) = probe_system_node() {
                let system_status = status_from_probe(Some(system), true);
                if system_status.meets_minimum {
                    return system_status;
                }
            }
            return status;
        }
    }

    status_from_probe(probe_system_node(), fnm_installed)
}

pub fn require_node_for_npm() -> AppResult<ResolvedNodeRuntime> {
    let status = probe_node_runtime();
    if !status.meets_minimum {
        return Err(AppError::Config(status.install_hint.clone()));
    }
    let node_path = status
        .node_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Config(status.install_hint.clone()))?;
    let npm_path = status
        .npm_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Config(status.install_hint.clone()))?;
    Ok(ResolvedNodeRuntime {
        node_path,
        npm_path,
    })
}

/// Strip IDE/sandbox npm env that confuses modern npm (e.g. Cursor's `npm_config_devdir`).
fn scrub_npm_env(command: &mut Command) {
    for key in [
        "npm_config_devdir",
        "NPM_CONFIG_DEVDIR",
        // Cursor sandbox cache redirect — keep user global prefix, avoid polluted installs.
        "npm_config_cache",
        "NPM_CONFIG_CACHE",
    ] {
        command.env_remove(key);
    }
}

/// Run `npm` with the Node binary directory prepended to PATH (GUI-safe).
pub fn run_anchored_npm(npm: &Path, node: &Path, args: &[&str]) -> io::Result<Output> {
    let node_dir = node
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .display()
        .to_string();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command_line = format!(
            "set \"PATH={};%PATH%\" && call {}",
            node_dir.replace('"', ""),
            quoted(&npm.display().to_string())
        );
        for arg in args {
            command_line.push(' ');
            command_line.push_str(&quoted(arg));
        }
        let mut command = Command::new("cmd");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg(command_line)
            .creation_flags(CREATE_NO_WINDOW);
        scrub_npm_env(&mut command);
        return command.output();
    }

    #[cfg(not(windows))]
    {
        let mut pieces = Vec::with_capacity(args.len() + 1);
        pieces.push(shell_single_quote(&npm.display().to_string()));
        for arg in args {
            pieces.push(shell_single_quote(arg));
        }
        let script = format!(
            "export PATH={}:\"$PATH\"; {}",
            shell_single_quote(&node_dir),
            pieces.join(" ")
        );
        let mut command = Command::new("sh");
        command.args(["-lc", &script]);
        scrub_npm_env(&mut command);
        command.output()
    }
}

/// Directories where `npm i -g` places CLI shims.
///
/// npm 9+ removed `npm bin`; use `npm prefix -g` (+ `/bin` on Unix). Always include the
/// well-known Windows user global dir (`%APPDATA%\\npm`) because GUI installs land there
/// when Program Files Node is not writable.
pub fn npm_global_bin_dirs(npm: &Path, node: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if path.as_os_str().is_empty() || !seen.insert(path.clone()) {
            return;
        }
        dirs.push(path);
    };

    if let Ok(output) = run_anchored_npm(npm, node, &["prefix", "-g"]) {
        if output.status.success() {
            let prefix = decode_output(&output.stdout).trim().to_string();
            if !prefix.is_empty() {
                let prefix_path = PathBuf::from(&prefix);
                push(prefix_path.clone());
                push(prefix_path.join("bin"));
            }
        }
    }

    // Legacy npm (<9) still supports `bin -g`.
    if let Ok(output) = run_anchored_npm(npm, node, &["bin", "-g"]) {
        if output.status.success() {
            let bin_dir = decode_output(&output.stdout).trim().to_string();
            if !bin_dir.is_empty() {
                push(PathBuf::from(bin_dir));
            }
        }
    }

    if let Some(app_data) = dirs::data_dir() {
        push(app_data.join("npm"));
    }
    if let Some(home) = home_dir() {
        push(home.join(".local").join("bin"));
    }

    dirs
}

/// Global package install via PATH-anchored npm, using the npmmirror registry.
pub fn run_anchored_npm_global_install(npm: &Path, node: &Path, package: &str) -> io::Result<Output> {
    run_anchored_npm(
        npm,
        node,
        &[
            "i",
            "-g",
            package,
            "--registry",
            NPM_REGISTRY_MIRROR,
            "--force",
        ],
    )
}

fn download_bytes(url: &str) -> AppResult<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| AppError::Other(format!("Failed to build HTTP client: {error}")))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| AppError::Other(format!("Download failed ({url}): {error}")))?;
    if !response.status().is_success() {
        return Err(AppError::Other(format!(
            "Download failed ({url}): HTTP {}",
            response.status()
        )));
    }
    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| AppError::Other(format!("Download read failed ({url}): {error}")))
}

fn download_bytes_with_fallbacks(urls: &[String]) -> AppResult<Vec<u8>> {
    let mut last_error = None;
    for url in urls {
        match download_bytes(url) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                log::warn!("download failed for {url}: {error}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::Other("Download failed: no candidate URLs were provided".into())
    }))
}

fn extract_zip_bytes(bytes: &[u8], dest_dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest_dir)?;
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|error| AppError::Other(format!("Invalid fnm zip: {error}")))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AppError::Other(format!("fnm zip entry error: {error}")))?;
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(enclosed);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut outfile)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if out_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "fnm" || name.starts_with("fnm"))
            {
                let mut perms = std::fs::metadata(&out_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&out_path, perms)?;
            }
        }
    }
    Ok(())
}

fn install_fnm_binary(github_mirror_base: Option<&str>) -> AppResult<PathBuf> {
    if let Some(existing) = find_fnm_binary() {
        return Ok(existing);
    }

    let install_dir = fnm_install_dir();
    std::fs::create_dir_all(&install_dir)?;

    #[cfg(windows)]
    {
        // Prefer winget when available (user-level).
        let winget = run_output({
            let mut command = Command::new("winget");
            command.args([
                "install",
                "-e",
                "--id",
                "Schniz.fnm",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]);
            command
        });
        if let Ok(output) = winget {
            if output.status.success() {
                if let Some(path) = find_fnm_binary() {
                    return Ok(path);
                }
            }
        }

        let urls = github_download_candidates(FNM_RELEASE_ZIP_WINDOWS, github_mirror_base);
        let bytes = download_bytes_with_fallbacks(&urls)?;
        extract_zip_bytes(&bytes, &install_dir)?;
        if let Some(path) = find_fnm_binary() {
            return Ok(path);
        }
        let direct = install_dir.join("fnm.exe");
        if direct.is_file() {
            return Ok(direct);
        }
        return Err(AppError::Config(
            "fnm was downloaded but the binary was not found under LocalAppData\\fnm.".into(),
        ));
    }

    #[cfg(not(windows))]
    {
        let zip_url = match std::env::consts::ARCH {
            "aarch64" => FNM_RELEASE_ZIP_LINUX_ARM64,
            _ => FNM_RELEASE_ZIP_LINUX,
        };
        let urls = github_download_candidates(zip_url, github_mirror_base);
        match download_bytes_with_fallbacks(&urls) {
            Ok(bytes) => {
                extract_zip_bytes(&bytes, &install_dir)?;
                if let Some(path) = find_fnm_binary() {
                    return Ok(path);
                }
                let direct = install_dir.join("fnm");
                if direct.is_file() {
                    return Ok(direct);
                }
            }
            Err(zip_error) => {
                log::warn!("fnm zip download failed, trying install script: {zip_error}");
                // Fallback: official install script to a temp file (no curl|bash).
                use std::io::Write;
                use std::process::Stdio;

                let script_bytes = download_bytes(FNM_INSTALL_SCRIPT).map_err(|script_error| {
                    AppError::Config(format!(
                        "fnm download failed (zip: {zip_error}; script: {script_error})"
                    ))
                })?;
                let temp = std::env::temp_dir().join(format!(
                    "fnm-install-{}.sh",
                    std::process::id()
                ));
                {
                    let mut file = std::fs::File::create(&temp)?;
                    file.write_all(&script_bytes)?;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&temp)?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&temp, perms)?;
                }
                let output = run_output({
                    let mut command = Command::new("bash");
                    command.args([
                        temp.to_string_lossy().as_ref(),
                        "--install-dir",
                        &install_dir.display().to_string(),
                        "--skip-shell",
                    ]);
                    command.stdin(Stdio::null());
                    command
                })?;
                let _ = std::fs::remove_file(&temp);
                if !output.status.success() {
                    return Err(AppError::Config(format!(
                        "fnm install script failed: {}",
                        decode_output(&output.stderr).trim()
                    )));
                }
            }
        }

        if let Some(path) = find_fnm_binary() {
            return Ok(path);
        }
        let direct = install_dir.join("fnm");
        if direct.is_file() {
            return Ok(direct);
        }
        Err(AppError::Config(
            "fnm installation finished but the binary was not found.".into(),
        ))
    }
}

fn run_fnm_with_env(fnm: &Path, args: &[&str], env: &[(&str, &str)]) -> AppResult<Output> {
    let output = run_output({
        let mut command = Command::new(fnm);
        command.args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        command
    })
    .map_err(|error| AppError::Other(format!("Failed to run fnm: {error}")))?;
    if !output.status.success() {
        let detail = {
            let stderr = decode_output(&output.stderr).trim().to_string();
            let stdout = decode_output(&output.stdout).trim().to_string();
            if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("fnm {:?} failed", args)
            }
        };
        return Err(AppError::Config(detail));
    }
    Ok(output)
}

fn install_node_22_via_fnm(fnm: &Path) -> AppResult<()> {
    let env = [("FNM_NODE_DIST", FNM_NODE_DIST_MIRROR)];
    let first = run_fnm_with_env(fnm, &["install", "22"], &env);
    if let Err(error) = first {
        log::warn!("fnm install 22 failed once, retrying with Node mirror: {error}");
        run_fnm_with_env(fnm, &["install", "22"], &env).map_err(|retry_error| {
            AppError::Config(format!(
                "NODE_NETWORK_OR_REGISTRY: fnm install Node 22 failed via {FNM_NODE_DIST_MIRROR}. {retry_error}"
            ))
        })?;
    }
    let _ = run_fnm_with_env(fnm, &["default", "22"], &env);
    Ok(())
}

pub fn ensure_node_runtime_via_fnm_sync_with_mirror(
    github_mirror_base: Option<&str>,
) -> AppResult<NodeRuntimeStatus> {
    let current = probe_node_runtime();
    if current.meets_minimum {
        return Ok(current);
    }

    let fnm = install_fnm_binary(github_mirror_base)?;
    install_node_22_via_fnm(&fnm)?;

    // Prefer probing through fnm env so GUI sessions see absolute node/npm paths.
    if let Some(probe) = probe_fnm_default_node(&fnm) {
        let status = status_from_probe(Some(probe), true);
        if status.meets_minimum {
            return Ok(status);
        }
    }

    let status = probe_node_runtime();
    if !status.meets_minimum {
        return Err(AppError::Config(format!(
            "fnm installed Node via mirror {FNM_NODE_DIST_MIRROR}, but the runtime still does not meet ≥{MINIMUM_NODE_MAJOR}. {}",
            status.install_hint
        )));
    }
    Ok(status)
}

#[tauri::command]
pub async fn get_node_runtime_status() -> AppResult<NodeRuntimeStatus> {
    tokio::task::spawn_blocking(probe_node_runtime)
        .await
        .map_err(|error| AppError::Other(format!("Node runtime probe failed: {error}")))
}

#[tauri::command]
pub async fn ensure_node_runtime_via_fnm(
    state: tauri::State<'_, crate::store::AppState>,
) -> AppResult<NodeRuntimeStatus> {
    let mirror_base = crate::commands::system::get_update_mirror_settings(state)
        .ok()
        .filter(|settings| settings.use_mirror)
        .map(|settings| settings.mirror_base);
    tokio::task::spawn_blocking(move || {
        ensure_node_runtime_via_fnm_sync_with_mirror(mirror_base.as_deref())
    })
    .await
    .map_err(|error| AppError::Other(format!("Node runtime install task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_version_strings() {
        assert_eq!(parse_node_version("v22.14.0"), Some("22.14.0".into()));
        assert_eq!(parse_node_version("22.14.0\n"), Some("22.14.0".into()));
        assert_eq!(parse_node_version("node"), None);
    }

    #[test]
    fn meets_minimum_major() {
        assert!(version_meets_minimum("22.0.0", 22));
        assert!(version_meets_minimum("23.1.0", 22));
        assert!(!version_meets_minimum("20.11.1", 22));
        assert!(!version_meets_minimum("12.22.9", 22));
    }

    #[test]
    fn maps_missing_npm_errors() {
        let mapped = map_npm_install_error("sh: 1: npm: not found");
        assert!(mapped.starts_with("NODE_RUNTIME_MISSING:"));
    }

    #[test]
    fn maps_ebadengine_errors() {
        let mapped = map_npm_install_error("npm ERR! code EBADENGINE\nnpm ERR! engine \"node\"");
        assert!(mapped.starts_with("NODE_RUNTIME_TOO_OLD:"));
    }

    #[test]
    fn maps_network_registry_errors() {
        let mapped = map_npm_install_error("npm ERR! code ETIMEDOUT\nnpm ERR! network");
        assert!(mapped.starts_with("NODE_NETWORK_OR_REGISTRY:"));
    }

    #[test]
    fn github_candidates_include_mirror() {
        let urls = github_download_candidates(
            "https://github.com/Schniz/fnm/releases/latest/download/fnm-linux.zip",
            Some("https://gh-proxy.com/"),
        );
        assert_eq!(urls[0], "https://github.com/Schniz/fnm/releases/latest/download/fnm-linux.zip");
        assert!(urls.iter().any(|url| url.starts_with("https://gh-proxy.com/https://github.com/")));
    }

    #[test]
    fn npm_global_bin_dirs_includes_user_npm_prefix() {
        let status = probe_node_runtime();
        let Some(node) = status.node_path.as_ref().map(PathBuf::from) else {
            return;
        };
        let Some(npm) = status.npm_path.as_ref().map(PathBuf::from) else {
            return;
        };
        let dirs = npm_global_bin_dirs(&npm, &node);
        assert!(
            !dirs.is_empty(),
            "expected at least one global bin dir from npm prefix / well-known paths"
        );
        // Windows user installs land in %APPDATA%\\npm even when Node is under Program Files.
        if cfg!(windows) {
            if let Some(app_data) = dirs::data_dir() {
                assert!(
                    dirs.iter().any(|dir| dir == &app_data.join("npm")),
                    "missing AppData\\npm in {dirs:?}"
                );
            }
        }
    }
}
