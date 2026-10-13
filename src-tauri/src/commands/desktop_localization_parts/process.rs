// Claude 进程停止校验、AppX 重启、动作收尾。由 desktop_localization.rs `include!`。
//
// 纯函数（解析 PID、推导 AppX 包族、挑选 AUMID、拼启动参数、收尾拼消息）都不启动或结束任何进程，
// 单测只覆盖这些。真正的进程操作只在 Windows 上由下面带 cfg 的函数执行。

/// 重启结果。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartOutcome {
    ok: bool,
    /// `appx` | `exe` | `none`
    method: &'static str,
    detail: String,
    skipped: bool,
}

impl RestartOutcome {
    fn ok(method: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: true,
            method,
            detail: detail.into(),
            skipped: false,
        }
    }

    fn failed(method: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            method,
            detail: detail.into(),
            skipped: false,
        }
    }

    fn skipped() -> Self {
        Self {
            ok: false,
            method: "none",
            detail: String::new(),
            skipped: true,
        }
    }
}

/// 把 PowerShell 输出解析成 PID 列表，忽略空行和垃圾行。
fn parse_pid_list(output: &str) -> Vec<u32> {
    output
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .collect()
}

/// 从 AppX 安装目录名 `Claude_<版本>_<架构>__<发布者ID>` 推出包族名 `Claude_<发布者ID>`。
fn appx_package_family(app_path: &Path) -> Option<String> {
    let name = app_path.file_name()?.to_str()?;
    let (head, publisher) = name.split_once("__")?;
    let package = head.split('_').next().filter(|part| !part.is_empty())?;
    if publisher.is_empty() || !publisher.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    if !package.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("{package}_{publisher}"))
}

fn is_safe_app_user_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'!' | b'-'))
}

/// 从 `Get-StartApps` 的 AppID 列表里挑 Claude 的 AUMID：先按包族精确匹配，再退到 `*Claude_*!*`。
fn select_appx_app_id(lines: &[String], family: Option<&str>) -> Option<String> {
    let candidates = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| is_safe_app_user_model_id(line) && line.contains('!'))
        .collect::<Vec<_>>();
    if let Some(family) = family {
        let prefix = format!("{}!", family.to_ascii_lowercase());
        if let Some(found) = candidates
            .iter()
            .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        {
            return Some((*found).to_string());
        }
    }
    candidates
        .iter()
        .find(|line| line.starts_with("Claude_"))
        .map(|line| (*line).to_string())
}

/// `explorer.exe` 的启动参数；AUMID 含非常规字符时拒绝。
fn appx_launch_argument(app_id: &str) -> Option<String> {
    is_safe_app_user_model_id(app_id).then(|| format!("shell:AppsFolder\\{app_id}"))
}

/// 结束 Claude 进程并确认真的停了。返回是否确实关闭过进程。
/// 仅限定到当前安装路径下的 Claude.exe，避免误杀 Claude Code CLI 的 claude.exe。
fn stop_claude_verified(install: &LocalizationInstall, log: &JobLog) -> AppResult<bool> {
    #[cfg(windows)]
    {
        let mut killed_any = false;
        for attempt in 1..=5 {
            let pids = list_claude_pids(&install.app_path)?;
            if pids.is_empty() {
                if attempt > 1 || killed_any {
                    log.line("Claude Desktop 进程已全部停止");
                }
                return Ok(killed_any);
            }
            log.line(&format!("第 {attempt} 轮：结束 Claude 进程 {pids:?}"));
            for pid in &pids {
                let mut command = Command::new(windows_system_executable("taskkill.exe"));
                command.args(["/PID", &pid.to_string(), "/T", "/F"]);
                hide_console_window(&mut command);
                let _ = command.status();
            }
            killed_any = true;
            std::thread::sleep(Duration::from_millis(600));
        }
        let remaining = list_claude_pids(&install.app_path)?;
        if remaining.is_empty() {
            log.line("Claude Desktop 进程已全部停止");
            return Ok(killed_any);
        }
        Err(AppError::Config(format!(
            "无法关闭 Claude Desktop（PID {remaining:?} 仍在运行），请手动退出后重试，未修改任何文件"
        )))
    }
    #[cfg(not(windows))]
    {
        let _ = (install, log);
        Ok(false)
    }
}

/// 当前安装路径下仍在运行的 Claude.exe 的 PID。路径通过环境变量传入，不拼进脚本。
#[cfg(windows)]
fn list_claude_pids(app_path: &Path) -> AppResult<Vec<u32>> {
    let script = concat!(
        "$root=($env:AISW_APP_PATH).TrimEnd('\\')+'\\';",
        "Get-CimInstance Win32_Process -Filter \"Name='Claude.exe'\" -ErrorAction SilentlyContinue | ",
        "Where-Object { $_.ExecutablePath -and ",
        "$_.ExecutablePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) } | ",
        "ForEach-Object { $_.ProcessId }"
    );
    let mut command = Command::new(windows_powershell());
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("AISW_APP_PATH", app_path);
    hide_console_window(&mut command);
    let output = command
        .output()
        .map_err(|error| AppError::Other(format!("无法查询 Claude 进程: {error}")))?;
    if !output.status.success() {
        return Err(AppError::Other(format!(
            "查询 Claude 进程失败: {}",
            command_failure_summary(&output)
        )));
    }
    Ok(parse_pid_list(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(windows)]
fn resolve_appx_app_id(app_path: &Path) -> Result<String, String> {
    let mut command = Command::new(windows_powershell());
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-StartApps | ForEach-Object { $_.AppID }",
    ]);
    hide_console_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法查询开始菜单应用: {error}"))?;
    if !output.status.success() {
        return Err(format!("查询开始菜单应用失败: {}", command_failure_summary(&output)));
    }
    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let family = appx_package_family(app_path);
    select_appx_app_id(&lines, family.as_deref())
        .ok_or_else(|| "开始菜单里没有找到 Claude 的应用入口".to_string())
}

/// 在应用（非提权）进程里重启 Claude。AppX 通过包标识启动，不直接执行 WindowsApps 下的 exe。
#[cfg(windows)]
fn restart_claude(install: &LocalizationInstall) -> RestartOutcome {
    let (method, launched) = if install.kind == "appx" {
        let launched = resolve_appx_app_id(&install.app_path).and_then(|app_id| {
            let argument = appx_launch_argument(&app_id)
                .ok_or_else(|| "Claude 应用入口标识包含不安全字符".to_string())?;
            let explorer = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join("explorer.exe");
            Command::new(explorer)
                .arg(argument)
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("无法通过包标识启动 Claude: {error}"))
        });
        ("appx", launched)
    } else if install.exe_path.is_file() {
        let launched = Command::new(&install.exe_path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法启动 Claude: {error}"));
        ("exe", launched)
    } else {
        ("none", Err("未找到 Claude 可执行文件".to_string()))
    };
    if let Err(detail) = launched {
        return RestartOutcome::failed(method, detail);
    }
    for _ in 0..16 {
        std::thread::sleep(Duration::from_millis(500));
        if list_claude_pids(&install.app_path).is_ok_and(|pids| !pids.is_empty()) {
            return RestartOutcome::ok(method, "已启动 Claude Desktop");
        }
    }
    RestartOutcome::failed(
        method,
        "已发出启动请求但未检测到 Claude 进程，请从开始菜单手动打开",
    )
}

#[cfg(not(windows))]
fn restart_claude(_install: &LocalizationInstall) -> RestartOutcome {
    RestartOutcome::skipped()
}

const LANGUAGE_CONFIRM_HINT: &str = "请在左下角账号菜单确认 Language 为「简体中文」";

/// 动作收尾：worker 失败原样返回且**不重启**；成功才取重启结果并写进消息。
fn finish_action(
    worker: AppResult<DesktopLocalizationActionResult>,
    action: &str,
    restart: impl FnOnce() -> RestartOutcome,
) -> AppResult<DesktopLocalizationActionResult> {
    let mut result = worker?;
    let outcome = restart();
    let hint = if action == "install" {
        format!("，{LANGUAGE_CONFIRM_HINT}")
    } else {
        String::new()
    };
    if outcome.skipped {
        result.restart = Some("skipped".to_string());
        if action == "install" {
            result.message = format!("{}。{LANGUAGE_CONFIRM_HINT}", result.message);
        }
    } else if outcome.ok {
        result.restart = Some("restarted".to_string());
        result.restart_detail = Some(outcome.detail);
        result.message = format!("{}。已重启 Claude Desktop{hint}", result.message);
    } else {
        result.message = format!(
            "{}。但自动重启失败：{}。请手动打开 Claude Desktop{hint}",
            result.message, outcome.detail
        );
        result.restart = Some("failed".to_string());
        result.restart_detail = Some(outcome.detail);
    }
    Ok(result)
}
