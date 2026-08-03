//! Claude Code installation discovery, version reporting and anchored updates.

use serde::Serialize;
use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

const NPM_PACKAGE: &str = "@anthropic-ai/claude-code";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeVersionInfo {
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
    pub wsl_distro: Option<String>,
}

#[derive(Debug, Clone)]
enum Probe {
    Found(Installation),
    Broken(Installation, String),
    NotFound(String),
}

#[derive(Debug, Clone)]
struct Installation {
    path: String,
    version: Option<String>,
    source: String,
    environment: String,
    wsl_distro: Option<String>,
}

fn install_command() -> String {
    #[cfg(windows)]
    {
        format!(
            "irm https://claude.ai/install.ps1 | iex ; if (-not $?) {{ npm i -g {NPM_PACKAGE}@latest }}"
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "curl -fsSL https://claude.ai/install.sh -o /tmp/claude-install.sh && bash /tmp/claude-install.sh || npm i -g {NPM_PACKAGE}@latest"
        )
    }
}

fn map_cli_install_error(detail: &str) -> String {
    crate::commands::node_runtime::map_npm_install_error(detail)
}

fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|c: char| {
            !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+'
        });
        (token
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
            && token.contains('.'))
            .then(|| token.to_string())
    })
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

fn output_detail(output: &Output) -> String {
    let stderr = decode_output(&output.stderr).trim().to_string();
    let stdout = decode_output(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    } else if !stdout.is_empty() {
        stdout.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
    } else {
        format!("claude --version exited with {}", output.status)
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && path.is_dir() && seen.insert(path.clone()) {
        paths.push(path);
    }
}

fn push_env_dir(
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    key: &str,
    child: Option<&str>,
) {
    if let Some(value) = std::env::var_os(key) {
        let mut path = PathBuf::from(value);
        if let Some(child) = child {
            path.push(child);
        }
        push_unique(paths, seen, path);
    }
}

fn push_child_dirs(
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &Path,
) {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten().take(100) {
            let path = entry.path();
            push_unique(paths, seen, path.clone());
            push_unique(paths, seen, path.join("bin"));
        }
    }
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let home = dirs::home_dir().unwrap_or_default();

    push_unique(&mut paths, &mut seen, home.join(".local").join("bin"));
    push_env_dir(&mut paths, &mut seen, "PNPM_HOME", None);
    push_env_dir(&mut paths, &mut seen, "VOLTA_HOME", Some("bin"));
    push_env_dir(&mut paths, &mut seen, "NVM_SYMLINK", None);
    push_env_dir(&mut paths, &mut seen, "FNM_MULTISHELL_PATH", None);
    push_env_dir(&mut paths, &mut seen, "SCOOP", Some("shims"));
    push_env_dir(&mut paths, &mut seen, "SCOOP_GLOBAL", Some("shims"));

    if let Some(app_data) = dirs::data_dir() {
        push_unique(&mut paths, &mut seen, app_data.join("npm"));
        let nvm = app_data.join("nvm");
        push_unique(&mut paths, &mut seen, nvm.clone());
        push_child_dirs(&mut paths, &mut seen, &nvm);
    }
    if let Some(local_data) = dirs::data_local_dir() {
        push_unique(&mut paths, &mut seen, local_data.join("pnpm"));
        push_unique(&mut paths, &mut seen, local_data.join("Volta").join("bin"));
        push_unique(&mut paths, &mut seen, local_data.join("Yarn").join("bin"));
        let fnm = local_data.join("fnm_multishells");
        push_child_dirs(&mut paths, &mut seen, &fnm);
    }
    if let Some(nvm_home) = std::env::var_os("NVM_HOME").map(PathBuf::from) {
        push_unique(&mut paths, &mut seen, nvm_home.clone());
        push_child_dirs(&mut paths, &mut seen, &nvm_home);
    }

    // fnm-managed Node bins (Ubuntu/Linux GUI PATH usually omits these).
    for dir in crate::commands::node_runtime::fnm_node_bin_dirs() {
        push_unique(&mut paths, &mut seen, dir);
    }

    // nvm version bins under ~/.nvm/versions/node/*/bin
    let nvm_versions = home.join(".nvm").join("versions").join("node");
    if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
        for entry in entries.flatten().take(64) {
            push_unique(&mut paths, &mut seen, entry.path().join("bin"));
        }
    }

    push_unique(&mut paths, &mut seen, home.join("scoop").join("shims"));
    push_unique(
        &mut paths,
        &mut seen,
        PathBuf::from(r"C:\Program Files\nodejs"),
    );
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    push_unique(
        &mut paths,
        &mut seen,
        program_data.join("scoop").join("shims"),
    );

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            push_unique(&mut paths, &mut seen, dir);
        }
    }
    paths
}

fn executable_candidates(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            dir.join("claude.cmd"),
            dir.join("claude.exe"),
            dir.join("claude.bat"),
            dir.join("claude"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![dir.join("claude")]
    }
}

fn infer_source(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("/.local/bin/claude") || normalized.contains("/.local/bin/codex") {
        "native".to_string()
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
    } else if normalized.contains("/scoop/") {
        "scoop".to_string()
    } else if normalized.contains("/npm/") {
        "npm".to_string()
    } else {
        "system".to_string()
    }
}

#[cfg(windows)]
fn compact_execution_path(tool_dir: &Path) -> std::ffi::OsString {
    let mut dirs = vec![tool_dir.to_path_buf(), PathBuf::from(r"C:\Program Files\nodejs")];
    if let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
        dirs.push(system_root.join("System32"));
        dirs.push(system_root);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    let mut estimated_length = 0usize;
    for path in dirs {
        if !seen.insert(path.clone()) {
            continue;
        }
        let added = path.as_os_str().to_string_lossy().len() + 1;
        if estimated_length + added > 6_000 {
            continue;
        }
        estimated_length += added;
        unique.push(path);
    }
    std::env::join_paths(unique).unwrap_or_default()
}

#[cfg(windows)]
fn run_local_tool(path: &Path) -> io::Result<Output> {
    use std::os::windows::process::CommandExt;

    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("");
    if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
        let quoted_path = path.to_string_lossy().replace('"', "\"\"");
        let command_line = format!("call \"{quoted_path}\" --version");
        let mut command = Command::new("cmd");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg(command_line)
            .env("PATH", compact_execution_path(path.parent().unwrap_or(Path::new(""))))
            .creation_flags(CREATE_NO_WINDOW);
        command.output()
    } else {
        let mut command = Command::new(path);
        command
            .arg("--version")
            .env("PATH", compact_execution_path(path.parent().unwrap_or(Path::new(""))))
            .creation_flags(CREATE_NO_WINDOW);
        command.output()
    }
}

#[cfg(not(windows))]
fn run_local_tool(path: &Path) -> io::Result<Output> {
    Command::new(path).arg("--version").output()
}

fn probe_native_installations() -> Probe {
    let mut seen = HashSet::new();
    let mut broken: Option<(Installation, String)> = None;
    let mut dirs = candidate_dirs();
    if let Some(login_path) = resolve_command_via_login_shell("claude") {
        if let Some(parent) = login_path.parent() {
            dirs.insert(0, parent.to_path_buf());
        }
    }
    for dir in dirs {
        for path in executable_candidates(&dir) {
            if !path.is_file() {
                continue;
            }
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !seen.insert(real) {
                continue;
            }
            let installation = Installation {
                path: path.display().to_string(),
                version: None,
                source: infer_source(&path),
                environment: if cfg!(windows) { "windows" } else { "native" }.to_string(),
                wsl_distro: None,
            };
            match run_local_tool(&path) {
                Ok(output) if output.status.success() => {
                    let stdout = decode_output(&output.stdout);
                    let stderr = decode_output(&output.stderr);
                    let raw = if stdout.trim().is_empty() { &stderr } else { &stdout };
                    if let Some(version) = parse_version(raw) {
                        return Probe::Found(Installation {
                            version: Some(version),
                            ..installation
                        });
                    }
                    if broken.is_none() {
                        broken = Some((installation, "Claude Code returned no version".to_string()));
                    }
                }
                Ok(output) if broken.is_none() => {
                    broken = Some((installation, output_detail(&output)));
                }
                Err(error) if broken.is_none() => {
                    broken = Some((installation, error.to_string()));
                }
                _ => {}
            }
        }
    }
    match broken {
        Some((installation, error)) => Probe::Broken(installation, error),
        None => Probe::NotFound("Claude Code executable was not found".to_string()),
    }
}

fn collect_child_output(mut child: Child, status: ExitStatus) -> io::Result<Output> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)?;
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return collect_child_output(child, status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let status = child.wait()?;
            let mut output = collect_child_output(child, status)?;
            output.stderr.extend_from_slice(b"\nWSL probe timed out");
            return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
fn configure_hidden(command: &mut Command) {
    crate::process_util::apply_no_window(command);
}

#[cfg(not(windows))]
fn configure_hidden(_command: &mut Command) {}

#[cfg(windows)]
fn probe_wsl_distro(distro: Option<&str>) -> Probe {
    let script =
        "p=\"$(command -v claude 2>/dev/null)\" || exit 127; printf '%s\\n' \"$p\"; \"$p\" --version";
    let mut command = Command::new("wsl.exe");
    if let Some(distro) = distro {
        command.args(["-d", distro]);
    }
    command.args(["--", "sh", "-lc", script]);
    configure_hidden(&mut command);
    let output = match run_with_timeout(&mut command, Duration::from_secs(5)) {
        Ok(output) => output,
        Err(error) => return Probe::NotFound(error.to_string()),
    };
    let stdout = decode_output(&output.stdout);
    let mut lines = stdout.lines();
    let path = lines.next().unwrap_or("").trim().to_string();
    let version_text = lines.collect::<Vec<_>>().join(" ");
    let installation = Installation {
        path,
        version: parse_version(&version_text),
        source: "wsl".to_string(),
        environment: "wsl".to_string(),
        wsl_distro: distro.map(str::to_string),
    };
    if output.status.success() && installation.version.is_some() {
        Probe::Found(installation)
    } else if output.status.code() != Some(127) && !installation.path.is_empty() {
        Probe::Broken(installation, output_detail(&output))
    } else {
        Probe::NotFound(output_detail(&output))
    }
}

#[cfg(windows)]
fn list_wsl_distros() -> Vec<String> {
    let mut command = Command::new("wsl.exe");
    command.args(["--list", "--quiet"]);
    configure_hidden(&mut command);
    let Ok(output) = run_with_timeout(&mut command, Duration::from_secs(3)) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    decode_output(&output.stdout)
        .lines()
        .map(|line| line.trim_matches('\0').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(windows)]
fn probe_wsl() -> Probe {
    match probe_wsl_distro(None) {
        found @ Probe::Found(_) | found @ Probe::Broken(_, _) => return found,
        Probe::NotFound(_) => {}
    }
    for distro in list_wsl_distros() {
        match probe_wsl_distro(Some(&distro)) {
            found @ Probe::Found(_) | found @ Probe::Broken(_, _) => return found,
            Probe::NotFound(_) => {}
        }
    }
    Probe::NotFound("Claude Code was not found in Windows or WSL".to_string())
}

fn probe_installation() -> Probe {
    match probe_native_installations() {
        found @ Probe::Found(_) | found @ Probe::Broken(_, _) => found,
        Probe::NotFound(native_error) => {
            #[cfg(windows)]
            {
                let wsl = probe_wsl();
                if matches!(wsl, Probe::NotFound(_)) {
                    Probe::NotFound(native_error)
                } else {
                    wsl
                }
            }
            #[cfg(not(windows))]
            {
                Probe::NotFound(native_error)
            }
        }
    }
}

async fn fetch_npm_latest() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let url = format!("https://registry.npmjs.org/{NPM_PACKAGE}/latest");
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .split(|c| c == '.' || c == '-')
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn update_available(current: &str, latest: &str) -> bool {
    current != latest && version_parts(latest) > version_parts(current)
}

fn find_command_near(name: &str, installation: &Installation) -> PathBuf {
    let bin = PathBuf::from(&installation.path);
    let mut dirs = bin.parent().map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
    #[cfg(windows)]
    dirs.push(PathBuf::from(r"C:\Program Files\nodejs"));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    for dir in dirs {
        #[cfg(windows)]
        {
            for suffix in ["cmd", "exe", "bat"] {
                let candidate = dir.join(format!("{name}.{suffix}"));
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    resolve_command_via_login_shell(name).unwrap_or_else(|| PathBuf::from(name))
}

/// GUI apps on Linux often lack nvm/fnm PATH; resolve via a login shell.
fn resolve_command_via_login_shell(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = name;
        None
    }
    #[cfg(not(windows))]
    {
        let script = format!("command -v {}", shell_single_quote(name));
        let output = Command::new("sh").args(["-lc", &script]).output().ok()?;
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
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn update_command_for(installation: Option<&Installation>) -> String {
    let Some(installation) = installation else {
        return install_command();
    };
    if installation.environment == "wsl" {
        let prefix = installation
            .wsl_distro
            .as_deref()
            .map(|distro| format!("wsl -d {} -- ", quoted(distro)))
            .unwrap_or_else(|| "wsl -- ".to_string());
        return format!(
            "{prefix}sh -lc 'claude update || npm i -g {NPM_PACKAGE}@latest'"
        );
    }
    match installation.source.as_str() {
        "native" => format!("{} update", quoted(&installation.path)),
        "pnpm" => format!(
            "{} add -g {NPM_PACKAGE}@latest",
            quoted(&find_command_near("pnpm", installation).display().to_string())
        ),
        "volta" => format!(
            "{} install {NPM_PACKAGE}@latest",
            quoted(&find_command_near("volta", installation).display().to_string())
        ),
        _ => {
            let npm = find_command_near("npm", installation);
            format!("{} i -g {NPM_PACKAGE}@latest", quoted(&npm.display().to_string()))
        }
    }
}

#[tauri::command]
pub async fn get_claude_code_version(
    include_latest: Option<bool>,
) -> AppResult<ClaudeCodeVersionInfo> {
    let probe_task = tokio::task::spawn_blocking(probe_installation);
    let (probe_result, latest_version) = if include_latest.unwrap_or(true) {
        let (probe_result, latest_version) = tokio::join!(probe_task, fetch_npm_latest());
        (probe_result, latest_version)
    } else {
        (probe_task.await, None)
    };
    let probe = probe_result
        .map_err(|error| AppError::Other(format!("Claude Code version probe failed: {error}")))?;

    let (installation, error, installed_but_broken) = match &probe {
        Probe::Found(installation) => (Some(installation), None, false),
        Probe::Broken(installation, error) => (Some(installation), Some(error.clone()), true),
        Probe::NotFound(error) => (None, Some(error.clone()), false),
    };
    let current_version = installation.and_then(|value| value.version.clone());
    let has_update = current_version
        .as_deref()
        .zip(latest_version.as_deref())
        .is_some_and(|(current, latest)| update_available(current, latest));

    Ok(ClaudeCodeVersionInfo {
        installed: installation.is_some(),
        current_version,
        latest_version,
        update_available: has_update,
        install_command: install_command(),
        update_command: update_command_for(installation),
        error,
        executable_path: installation.map(|value| value.path.clone()),
        source: installation.map(|value| value.source.clone()),
        environment: installation
            .map(|value| value.environment.clone())
            .unwrap_or_else(|| if cfg!(windows) { "windows" } else { "native" }.to_string()),
        installed_but_broken,
        wsl_distro: installation.and_then(|value| value.wsl_distro.clone()),
    })
}

#[cfg(windows)]
fn run_windows_command(program: &Path, args: &[&str]) -> io::Result<Output> {
    use std::os::windows::process::CommandExt;

    let extension = program.extension().and_then(|value| value.to_str()).unwrap_or("");
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
        command.output()
    } else {
        let mut command = Command::new(program);
        command.args(args).creation_flags(CREATE_NO_WINDOW);
        command.output()
    }
}

fn run_anchored_update(installation: &Installation) -> io::Result<Output> {
    if installation.environment == "wsl" {
        let mut command = Command::new("wsl.exe");
        if let Some(distro) = &installation.wsl_distro {
            command.args(["-d", distro]);
        }
        command.args([
            "--",
            "sh",
            "-lc",
            "claude update || npm i -g @anthropic-ai/claude-code@latest",
        ]);
        configure_hidden(&mut command);
        return command.output();
    }

    match installation.source.as_str() {
        "native" => {
            #[cfg(windows)]
            {
                run_windows_command(Path::new(&installation.path), &["update"])
            }
            #[cfg(not(windows))]
            {
                Command::new(&installation.path).arg("update").output()
            }
        }
        source => {
            let (program, args) = match source {
                "pnpm" => (
                    find_command_near("pnpm", installation),
                    vec!["add", "-g", "@anthropic-ai/claude-code@latest"],
                ),
                "volta" => (
                    find_command_near("volta", installation),
                    vec!["install", "@anthropic-ai/claude-code@latest"],
                ),
                _ => {
                    if let Ok(runtime) = crate::commands::node_runtime::require_node_for_npm() {
                        let npm = find_command_near("npm", installation);
                        let npm = if npm.is_file() {
                            npm
                        } else {
                            runtime.npm_path.clone()
                        };
                        return crate::commands::node_runtime::run_anchored_npm_global_install(
                            &npm,
                            &runtime.node_path,
                            "@anthropic-ai/claude-code@latest",
                        );
                    }
                    (
                        find_command_near("npm", installation),
                        vec!["i", "-g", "@anthropic-ai/claude-code@latest"],
                    )
                }
            };
            #[cfg(windows)]
            {
                run_windows_command(&program, &args)
            }
            #[cfg(not(windows))]
            {
                run_unix_command_with_login_path(&program, &args, Path::new(&installation.path))
            }
        }
    }
}

#[cfg(not(windows))]
fn run_unix_command_with_login_path(
    program: &Path,
    args: &[&str],
    tool_path: &Path,
) -> io::Result<Output> {
    let tool_dir = tool_path
        .parent()
        .unwrap_or_else(|| Path::new("/usr/bin"))
        .display()
        .to_string();
    let mut pieces = Vec::with_capacity(args.len() + 1);
    pieces.push(shell_single_quote(&program.display().to_string()));
    for arg in args {
        pieces.push(shell_single_quote(arg));
    }
    let script = format!(
        "export PATH={}:\"$PATH\"; {}",
        shell_single_quote(&tool_dir),
        pieces.join(" ")
    );
    Command::new("sh").args(["-lc", &script]).output()
}

fn download_to_temp(url: &str, filename: &str) -> AppResult<PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
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
    let bytes = response
        .bytes()
        .map_err(|error| AppError::Other(format!("Download read failed ({url}): {error}")))?;
    let path = std::env::temp_dir().join(format!(
        "{}-{}-{}",
        filename.trim_end_matches(".sh").trim_end_matches(".ps1"),
        std::process::id(),
        filename
    ));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(not(windows))]
fn run_claude_native_install_unix() -> AppResult<Output> {
    let script = download_to_temp("https://claude.ai/install.sh", "claude-install.sh")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    let output = Command::new("bash")
        .arg(script.as_os_str())
        .output()
        .map_err(|error| AppError::Other(format!("无法执行 Claude 安装脚本: {error}")))?;
    let _ = std::fs::remove_file(&script);
    Ok(output)
}

#[cfg(windows)]
fn run_claude_native_install_windows() -> AppResult<Output> {
    use std::os::windows::process::CommandExt;

    let script = download_to_temp("https://claude.ai/install.ps1", "claude-install.ps1")?;
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
        ])
        .creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| AppError::Other(format!("无法执行 Claude 安装脚本: {error}")))?;
    let _ = std::fs::remove_file(&script);
    Ok(output)
}

fn run_claude_npm_install() -> AppResult<Output> {
    let runtime = crate::commands::node_runtime::require_node_for_npm()?;
    let output = crate::commands::node_runtime::run_anchored_npm_global_install(
        &runtime.npm_path,
        &runtime.node_path,
        "@anthropic-ai/claude-code@latest",
    )
    .map_err(|error| AppError::Other(format!("无法执行 npm 安装: {error}")))?;
    Ok(ensure_npm_cli_after_install(
        &runtime.node_path,
        &runtime.npm_path,
        "claude",
        output,
    ))
}

fn output_failed(detail: &str) -> Output {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: detail.as_bytes().to_vec(),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        Output {
            status: ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: detail.as_bytes().to_vec(),
        }
    }
}

fn merge_install_failures(primary: &str, secondary: &str) -> String {
    format!("{primary}\n---\n{secondary}")
}

/// Hard-verify a freshly installed npm global CLI via absolute paths.
/// Soft checks previously allowed npm exit 0 + missing bin (common on Ubuntu GUI/fnm).
fn verify_npm_cli_near_node(node: &Path, npm: &Path, cli_name: &str) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(node_dir) = node.parent() {
        candidates.push(node_dir.join(cli_name));
        candidates.push(node_dir.join(format!("{cli_name}.cmd")));
        candidates.push(node_dir.join(format!("{cli_name}.exe")));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("bin").join(cli_name));
    }
    for dir in crate::commands::node_runtime::fnm_node_bin_dirs() {
        candidates.push(dir.join(cli_name));
        candidates.push(dir.join(format!("{cli_name}.cmd")));
        candidates.push(dir.join(format!("{cli_name}.exe")));
    }
    if let Ok(bin_output) =
        crate::commands::node_runtime::run_anchored_npm(npm, node, &["bin", "-g"])
    {
        if bin_output.status.success() {
            let bin_dir = decode_output(&bin_output.stdout).trim().to_string();
            if !bin_dir.is_empty() {
                let dir = PathBuf::from(&bin_dir);
                candidates.push(dir.join(cli_name));
                candidates.push(dir.join(format!("{cli_name}.cmd")));
                candidates.push(dir.join(format!("{cli_name}.exe")));
            }
        }
    }
    if let Some(login) = resolve_command_via_login_shell(cli_name) {
        candidates.push(login);
    }

    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.clone()) || !candidate.is_file() {
            continue;
        }
        match run_local_tool(&candidate) {
            Ok(output) if output.status.success() => return Ok(candidate),
            _ => continue,
        }
    }
    Err(format!(
        "npm reported success but `{cli_name}` was not found next to Node ({}) or under fnm/npm global bins. Check that the Node bin directory is writable and retry.",
        node.display()
    ))
}

fn ensure_npm_cli_after_install(node: &Path, npm: &Path, cli_name: &str, output: Output) -> Output {
    if !output.status.success() {
        return output;
    }
    match verify_npm_cli_near_node(node, npm, cli_name) {
        Ok(_) => output,
        Err(detail) => {
            let mut merged = output_detail(&output);
            if !merged.is_empty() {
                merged.push('\n');
            }
            merged.push_str(&detail);
            output_failed(&merged)
        }
    }
}

fn run_claude_install_or_update() -> AppResult<Output> {
    let probe = probe_installation();
    match probe {
        Probe::Found(installation) | Probe::Broken(installation, _) => {
            if installation.source == "native" || installation.environment == "wsl" {
                return run_anchored_update(&installation)
                    .map_err(|error| AppError::Other(format!("无法执行更新命令: {error}")));
            }
            // npm/pnpm/volta paths require a usable Node runtime.
            let _ = crate::commands::node_runtime::require_node_for_npm()?;
            run_anchored_update(&installation)
                .map_err(|error| AppError::Other(format!("无法执行更新命令: {error}")))
        }
        Probe::NotFound(_) => {
            // Prefer npm when Node≥22 is available (more reliable behind mirrors than install.sh).
            if crate::commands::node_runtime::require_node_for_npm().is_ok() {
                match run_claude_npm_install() {
                    Ok(output) if output.status.success() => {
                        return Ok(output);
                    }
                    Ok(npm_output) => {
                        #[cfg(windows)]
                        let native = run_claude_native_install_windows();
                        #[cfg(not(windows))]
                        let native = run_claude_native_install_unix();
                        return match native {
                            Ok(native_output) if native_output.status.success() => Ok(native_output),
                            Ok(native_output) => Ok(output_failed(&merge_install_failures(
                                &output_detail(&npm_output),
                                &output_detail(&native_output),
                            ))),
                            Err(native_error) => Ok(output_failed(&merge_install_failures(
                                &output_detail(&npm_output),
                                &native_error.to_string(),
                            ))),
                        };
                    }
                    Err(npm_error) => {
                        #[cfg(windows)]
                        let native = run_claude_native_install_windows();
                        #[cfg(not(windows))]
                        let native = run_claude_native_install_unix();
                        return match native {
                            Ok(native_output) if native_output.status.success() => Ok(native_output),
                            Ok(native_output) => Ok(output_failed(&merge_install_failures(
                                &npm_error.to_string(),
                                &output_detail(&native_output),
                            ))),
                            Err(native_error) => Ok(output_failed(&merge_install_failures(
                                &npm_error.to_string(),
                                &native_error.to_string(),
                            ))),
                        };
                    }
                }
            }

            #[cfg(windows)]
            {
                match run_claude_native_install_windows() {
                    Ok(output) if output.status.success() => Ok(output),
                    Ok(native_output) => match run_claude_npm_install() {
                        Ok(npm_output) if npm_output.status.success() => Ok(npm_output),
                        Ok(npm_output) => Ok(output_failed(&merge_install_failures(
                            &output_detail(&native_output),
                            &output_detail(&npm_output),
                        ))),
                        Err(npm_error) => Ok(output_failed(&merge_install_failures(
                            &output_detail(&native_output),
                            &npm_error.to_string(),
                        ))),
                    },
                    Err(native_error) => match run_claude_npm_install() {
                        Ok(npm_output) if npm_output.status.success() => Ok(npm_output),
                        Ok(npm_output) => Ok(output_failed(&merge_install_failures(
                            &native_error.to_string(),
                            &output_detail(&npm_output),
                        ))),
                        Err(npm_error) => Ok(output_failed(&merge_install_failures(
                            &native_error.to_string(),
                            &npm_error.to_string(),
                        ))),
                    },
                }
            }
            #[cfg(not(windows))]
            {
                match run_claude_native_install_unix() {
                    Ok(output) if output.status.success() => Ok(output),
                    Ok(native_output) => match run_claude_npm_install() {
                        Ok(npm_output) if npm_output.status.success() => Ok(npm_output),
                        Ok(npm_output) => Ok(output_failed(&merge_install_failures(
                            &output_detail(&native_output),
                            &output_detail(&npm_output),
                        ))),
                        Err(npm_error) => Ok(output_failed(&merge_install_failures(
                            &output_detail(&native_output),
                            &npm_error.to_string(),
                        ))),
                    },
                    Err(native_error) => match run_claude_npm_install() {
                        Ok(npm_output) if npm_output.status.success() => Ok(npm_output),
                        Ok(npm_output) => Ok(output_failed(&merge_install_failures(
                            &native_error.to_string(),
                            &output_detail(&npm_output),
                        ))),
                        Err(npm_error) => Ok(output_failed(&merge_install_failures(
                            &native_error.to_string(),
                            &npm_error.to_string(),
                        ))),
                    },
                }
            }
        }
    }
}

#[tauri::command]
pub async fn run_claude_code_update() -> AppResult<String> {
    let result = tokio::task::spawn_blocking(run_claude_install_or_update)
        .await
        .map_err(|error| AppError::Other(format!("更新任务异常结束: {error}")))?;

    let output = result?;
    if output.status.success() {
        let stdout = decode_output(&output.stdout).trim().to_string();
        Ok(if stdout.is_empty() {
            "Claude Code 更新完成".to_string()
        } else {
            stdout
        })
    } else {
        Err(AppError::Config(map_cli_install_error(&output_detail(
            &output,
        ))))
    }
}

// ---- Codex CLI --------------------------------------------------------------

const CODEX_NPM_PACKAGE: &str = "@openai/codex";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliVersionInfo {
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

fn codex_install_command() -> String {
    format!("npm i -g {CODEX_NPM_PACKAGE}@latest")
}

fn codex_executable_candidates(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            dir.join("codex.cmd"),
            dir.join("codex.exe"),
            dir.join("codex.bat"),
            dir.join("codex"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![dir.join("codex")]
    }
}

fn probe_codex_installation() -> Probe {
    let mut seen = HashSet::new();
    let mut broken: Option<(Installation, String)> = None;
    let mut dirs = candidate_dirs();
    if let Some(login_path) = resolve_command_via_login_shell("codex") {
        if let Some(parent) = login_path.parent() {
            dirs.insert(0, parent.to_path_buf());
        }
    }
    for dir in dirs {
        for path in codex_executable_candidates(&dir) {
            if !path.is_file() {
                continue;
            }
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !seen.insert(real) {
                continue;
            }
            let installation = Installation {
                path: path.display().to_string(),
                version: None,
                source: infer_source(&path),
                environment: if cfg!(windows) { "windows" } else { "native" }.to_string(),
                wsl_distro: None,
            };
            match run_local_tool(&path) {
                Ok(output) if output.status.success() => {
                    let stdout = decode_output(&output.stdout);
                    let stderr = decode_output(&output.stderr);
                    let raw = if stdout.trim().is_empty() { &stderr } else { &stdout };
                    if let Some(version) = parse_version(raw) {
                        return Probe::Found(Installation {
                            version: Some(version),
                            ..installation
                        });
                    }
                    if broken.is_none() {
                        broken = Some((installation, "Codex CLI returned no version".to_string()));
                    }
                }
                Ok(output) if broken.is_none() => {
                    broken = Some((installation, output_detail(&output)));
                }
                Err(error) if broken.is_none() => {
                    broken = Some((installation, error.to_string()));
                }
                _ => {}
            }
        }
    }
    match broken {
        Some((installation, error)) => Probe::Broken(installation, error),
        None => Probe::NotFound("Codex CLI executable was not found".to_string()),
    }
}

async fn fetch_codex_npm_latest() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let url = format!("https://registry.npmjs.org/{CODEX_NPM_PACKAGE}/latest");
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn codex_update_command_for(installation: Option<&Installation>) -> String {
    let Some(installation) = installation else {
        return codex_install_command();
    };
    let npm = find_command_near("npm", installation);
    format!(
        "{} i -g {CODEX_NPM_PACKAGE}@latest",
        quoted(&npm.display().to_string())
    )
}

fn run_codex_npm_install() -> AppResult<Output> {
    let runtime = crate::commands::node_runtime::require_node_for_npm()?;
    let output = crate::commands::node_runtime::run_anchored_npm_global_install(
        &runtime.npm_path,
        &runtime.node_path,
        "@openai/codex@latest",
    )
    .map_err(|error| AppError::Other(format!("无法执行 npm 安装: {error}")))?;
    Ok(ensure_npm_cli_after_install(
        &runtime.node_path,
        &runtime.npm_path,
        "codex",
        output,
    ))
}

fn run_codex_install_or_update() -> AppResult<Output> {
    // Codex always depends on Node/npm.
    let runtime = crate::commands::node_runtime::require_node_for_npm()?;
    let probe = probe_codex_installation();
    let output = match probe {
        Probe::Found(installation) | Probe::Broken(installation, _) => {
            let program = find_command_near("npm", &installation);
            let npm = if program.is_file() {
                program
            } else {
                runtime.npm_path.clone()
            };
            let output = crate::commands::node_runtime::run_anchored_npm_global_install(
                &npm,
                &runtime.node_path,
                "@openai/codex@latest",
            )
            .map_err(|error| AppError::Other(format!("无法执行更新命令: {error}")))?;
            ensure_npm_cli_after_install(&runtime.node_path, &npm, "codex", output)
        }
        Probe::NotFound(_) => run_codex_npm_install()?,
    };
    Ok(output)
}

#[tauri::command]
pub async fn get_codex_cli_version(include_latest: Option<bool>) -> AppResult<CodexCliVersionInfo> {
    let probe_task = tokio::task::spawn_blocking(probe_codex_installation);
    let (probe_result, latest_version) = if include_latest.unwrap_or(true) {
        let (probe_result, latest_version) = tokio::join!(probe_task, fetch_codex_npm_latest());
        (probe_result, latest_version)
    } else {
        (probe_task.await, None)
    };
    let probe = probe_result
        .map_err(|error| AppError::Other(format!("Codex CLI version probe failed: {error}")))?;

    let (installation, error, installed_but_broken) = match &probe {
        Probe::Found(installation) => (Some(installation), None, false),
        Probe::Broken(installation, error) => (Some(installation), Some(error.clone()), true),
        Probe::NotFound(error) => (None, Some(error.clone()), false),
    };
    let current_version = installation.and_then(|value| value.version.clone());
    let has_update = current_version
        .as_deref()
        .zip(latest_version.as_deref())
        .is_some_and(|(current, latest)| update_available(current, latest));

    Ok(CodexCliVersionInfo {
        installed: installation.is_some(),
        current_version,
        latest_version,
        update_available: has_update,
        install_command: codex_install_command(),
        update_command: codex_update_command_for(installation),
        error,
        executable_path: installation.map(|value| value.path.clone()),
        source: installation.map(|value| value.source.clone()),
        environment: installation
            .map(|value| value.environment.clone())
            .unwrap_or_else(|| if cfg!(windows) { "windows" } else { "native" }.to_string()),
        installed_but_broken,
    })
}

#[tauri::command]
pub async fn run_codex_cli_update() -> AppResult<String> {
    let result = tokio::task::spawn_blocking(run_codex_install_or_update)
        .await
        .map_err(|error| AppError::Other(format!("更新任务异常结束: {error}")))?;

    let output = result?;
    if output.status.success() {
        let stdout = decode_output(&output.stdout).trim().to_string();
        Ok(if stdout.is_empty() {
            "Codex CLI 更新完成".to_string()
        } else {
            stdout
        })
    } else {
        Err(AppError::Config(map_cli_install_error(&output_detail(
            &output,
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_claude_version_outputs() {
        assert_eq!(parse_version("2.1.146 (Claude Code)"), Some("2.1.146".into()));
        assert_eq!(parse_version("claude 1.0.20"), Some("1.0.20".into()));
        assert_eq!(parse_version("no version"), None);
    }

    #[test]
    fn identifies_common_install_sources() {
        assert_eq!(
            infer_source(Path::new(r"C:\Users\me\AppData\Roaming\npm\claude.cmd")),
            "npm"
        );
        assert_eq!(
            infer_source(Path::new(r"C:\Users\me\AppData\Local\pnpm\claude.cmd")),
            "pnpm"
        );
        assert_eq!(
            infer_source(Path::new(r"C:\Users\me\.local\bin\claude.exe")),
            "native"
        );
        assert_eq!(
            infer_source(Path::new(
                "/home/me/.local/share/fnm/node-versions/v22.14.0/installation/bin/claude"
            )),
            "fnm"
        );
        assert_eq!(
            infer_source(Path::new("/home/me/.local/bin/codex")),
            "native"
        );
    }

    #[test]
    fn compares_semver_like_versions() {
        assert!(update_available("2.1.9", "2.2.0"));
        assert!(!update_available("2.2.0", "2.2.0"));
        assert!(!update_available("2.3.0", "2.2.0"));
    }

    #[test]
    fn native_install_uses_self_update() {
        let installation = Installation {
            path: r"C:\Users\me\.local\bin\claude.exe".into(),
            version: Some("2.1.146".into()),
            source: "native".into(),
            environment: "windows".into(),
            wsl_distro: None,
        };
        assert_eq!(
            update_command_for(Some(&installation)),
            r#""C:\Users\me\.local\bin\claude.exe" update"#
        );
    }

    #[test]
    fn wsl_install_updates_inside_its_distro() {
        let installation = Installation {
            path: "/home/me/.local/bin/claude".into(),
            version: Some("2.1.146".into()),
            source: "wsl".into(),
            environment: "wsl".into(),
            wsl_distro: Some("Ubuntu-24.04".into()),
        };
        let command = update_command_for(Some(&installation));
        assert!(command.starts_with(r#"wsl -d "Ubuntu-24.04" -- "#));
        assert!(command.contains("claude update"));
    }

    #[test]
    fn codex_install_command_targets_openai_package() {
        assert!(codex_install_command().contains("@openai/codex"));
    }

    #[test]
    fn maps_npm_not_found_for_cli_install() {
        let mapped = map_cli_install_error("sh: 1: npm: not found");
        assert!(mapped.starts_with("NODE_RUNTIME_MISSING:"));
    }
}
