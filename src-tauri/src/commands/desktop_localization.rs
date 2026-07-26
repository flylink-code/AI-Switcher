//! Windows Claude Desktop zh-CN resource-mode localization.
//!
//! Third-party packs are treated as data only. No script or executable from the
//! selected directory is ever launched.

use crate::config::{atomic_write, get_app_config_dir, get_backup_dir, read_json_file, write_json_file};
use crate::database::dao::settings::{get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::store::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PACK_SETTING_KEY: &str = "desktop_localization_pack_path";
const LOCALIZATION_LOCALE: &str = "zh-CN";
const MAX_PACK_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_HARDCODED_REPLACEMENTS: usize = 5_000;
const BACKUPS_TO_KEEP: usize = 3;

const FRONTEND_FILE: &str = "frontend-zh-CN.json";
const DESKTOP_FILE: &str = "desktop-zh-CN.json";
const STATSIG_FILE: &str = "statsig-zh-CN.json";
const HARDCODED_FILE: &str = "frontend-hardcoded-zh-CN.json";

const BASE_LANGUAGES: &str =
    "[\"en-US\",\"de-DE\",\"fr-FR\",\"ko-KR\",\"ja-JP\",\"es-419\",\"es-ES\",\"it-IT\",\"hi-IN\",\"pt-BR\",\"id-ID\"";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLocalizationStatus {
    pub platform_supported: bool,
    pub install_detected: bool,
    pub install_kind: Option<String>,
    pub install_path: Option<String>,
    pub resources_path: Option<String>,
    pub claude_version: Option<String>,
    pub multiple_installs: bool,
    pub state: String,
    pub configured_locale: Option<String>,
    pub pack_path: Option<String>,
    pub pack_valid: bool,
    pub backup_available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLocalizationPackValidation {
    pub valid: bool,
    pub pack_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLocalizationActionResult {
    pub ok: bool,
    pub changed_files: usize,
    pub message: String,
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalizationInstall {
    kind: String,
    app_path: PathBuf,
    resources_path: PathBuf,
    exe_path: PathBuf,
    version: String,
    multiple_installs: bool,
}

#[derive(Debug, Clone)]
struct ValidatedPack {
    root: PathBuf,
    resources: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalizationWorkerJob {
    action: String,
    install: LocalizationInstall,
    pack_path: Option<PathBuf>,
    locale_paths: Vec<PathBuf>,
    backup_root: PathBuf,
    result_path: PathBuf,
    log_path: PathBuf,
}

#[derive(Debug)]
struct FileChange {
    target: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalizationBackupManifest {
    version: u8,
    claude_version: String,
    install_path: PathBuf,
    created_at: i64,
    files: Vec<LocalizationBackupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalizationBackupFile {
    target: PathBuf,
    existed: bool,
    backup_file: Option<String>,
    sha256: Option<String>,
}

#[tauri::command]
pub fn get_desktop_localization_status(
    state: tauri::State<'_, AppState>,
) -> AppResult<DesktopLocalizationStatus> {
    let pack_path = state
        .db
        .with_conn(|conn| get_setting(conn, PACK_SETTING_KEY))?;
    localization_status(pack_path.as_deref())
}

#[tauri::command]
pub fn validate_desktop_localization_pack(
    path: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<DesktopLocalizationPackValidation> {
    let pack = validate_pack(Path::new(&path))?;
    let canonical = pack.root.to_string_lossy().into_owned();
    state
        .db
        .with_conn(|conn| set_setting(conn, PACK_SETTING_KEY, &canonical))?;
    Ok(DesktopLocalizationPackValidation {
        valid: true,
        pack_path: canonical,
        message: "中文资源包校验通过".to_string(),
    })
}

#[tauri::command]
pub fn select_desktop_localization_pack() -> AppResult<Option<String>> {
    #[cfg(not(windows))]
    {
        return Err(AppError::Config(
            "Claude Desktop 中文化首版仅支持 Windows".to_string(),
        ));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = concat!(
            "Add-Type -AssemblyName System.Windows.Forms;",
            "$d=New-Object System.Windows.Forms.FolderBrowserDialog;",
            "$d.Description='选择 claude-desktop-zh-cn 资源目录';",
            "$d.ShowNewFolderButton=$false;",
            "if($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK)",
            "{[Console]::OutputEncoding=[Text.Encoding]::UTF8;[Console]::Out.Write($d.SelectedPath)}"
        );
        let output = Command::new(windows_powershell())
            .args(["-NoProfile", "-STA", "-Command", script])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|error| AppError::Other(format!("无法打开目录选择器: {error}")))?;
        if !output.status.success() {
            return Err(AppError::Other("目录选择器异常退出".to_string()));
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!path.is_empty()).then_some(path))
    }
}

#[tauri::command]
pub async fn install_desktop_localization(
    pack_path: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<DesktopLocalizationActionResult> {
    let pack = validate_pack(Path::new(&pack_path))?;
    let canonical = pack.root.to_string_lossy().into_owned();
    state
        .db
        .with_conn(|conn| set_setting(conn, PACK_SETTING_KEY, &canonical))?;
    run_localization_action("install", Some(pack.root)).await
}

#[tauri::command]
pub async fn restore_desktop_localization() -> AppResult<DesktopLocalizationActionResult> {
    run_localization_action("restore", None).await
}

async fn run_localization_action(
    action: &str,
    pack_path: Option<PathBuf>,
) -> AppResult<DesktopLocalizationActionResult> {
    #[cfg(not(windows))]
    {
        let _ = (action, pack_path);
        return Err(AppError::Config(
            "Claude Desktop 中文化首版仅支持 Windows".to_string(),
        ));
    }

    #[cfg(windows)]
    {
        let install = detect_install()?.ok_or_else(|| {
            AppError::Config("未检测到可支持的 Claude Desktop 安装".to_string())
        })?;
        let jobs_dir = get_app_config_dir().join("localization-jobs");
        fs::create_dir_all(&jobs_dir)?;
        let job_id = uuid::Uuid::new_v4().simple().to_string();
        let job_path = jobs_dir.join(format!("{job_id}.json"));
        let result_path = jobs_dir.join(format!("{job_id}.result.json"));
        let log_path = jobs_dir.join(format!("{job_id}.log"));
        let job = LocalizationWorkerJob {
            action: action.to_string(),
            install,
            pack_path,
            locale_paths: claude_locale_paths(),
            backup_root: get_backup_dir().join("desktop-localization"),
            result_path: result_path.clone(),
            log_path,
        };
        let claude_executable = job.install.exe_path.clone();
        write_json_file(&job_path, &job)?;
        let worker_path = job_path.clone();
        let worker_result = tokio::task::spawn_blocking(move || {
            launch_elevated_worker(&worker_path, &result_path)
        })
        .await
        .map_err(|error| AppError::Other(format!("中文化工作进程异常结束: {error}")));
        let _ = fs::remove_file(job_path);
        restart_claude(&claude_executable);
        worker_result?
    }
}

fn localization_status(pack_path: Option<&str>) -> AppResult<DesktopLocalizationStatus> {
    #[cfg(not(windows))]
    {
        return Ok(DesktopLocalizationStatus {
            platform_supported: false,
            install_detected: false,
            install_kind: None,
            install_path: None,
            resources_path: None,
            claude_version: None,
            multiple_installs: false,
            state: "unsupported".to_string(),
            configured_locale: None,
            pack_path: pack_path.map(str::to_string),
            pack_valid: false,
            backup_available: false,
            message: "Claude Desktop 中文化首版仅支持 Windows".to_string(),
        });
    }

    #[cfg(windows)]
    {
        let install = detect_install()?;
        let pack_valid = pack_path
            .map(|path| validate_pack(Path::new(path)).is_ok())
            .unwrap_or(false);
        let configured_locale = read_configured_locale();
        let Some(install) = install else {
            return Ok(DesktopLocalizationStatus {
                platform_supported: true,
                install_detected: false,
                install_kind: None,
                install_path: None,
                resources_path: None,
                claude_version: None,
                multiple_installs: false,
                state: "notInstalled".to_string(),
                configured_locale,
                pack_path: pack_path.map(str::to_string),
                pack_valid,
                backup_available: false,
                message: "未检测到 Claude Desktop".to_string(),
            });
        };
        let frontend = install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join(FRONTEND_FILE.replace("frontend-", ""));
        let desktop = install
            .resources_path
            .join(DESKTOP_FILE.replace("desktop-", ""));
        let statsig = install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join("statsig")
            .join(STATSIG_FILE.replace("statsig-", ""));
        let present = [frontend.exists(), desktop.exists(), statsig.exists()]
            .into_iter()
            .filter(|present| *present)
            .count();
        let locale_is_zh = configured_locale.as_deref() == Some(LOCALIZATION_LOCALE);
        let localized_state = match (present, locale_is_zh) {
            (3, true) => "installed",
            (0, false) => "notInstalled",
            _ => "partial",
        };
        let backup_available =
            latest_backup_dir(&get_backup_dir().join("desktop-localization"), &install)
                .ok()
                .flatten()
                .is_some();
        Ok(DesktopLocalizationStatus {
            platform_supported: true,
            install_detected: true,
            install_kind: Some(install.kind.clone()),
            install_path: Some(install.app_path.to_string_lossy().into_owned()),
            resources_path: Some(install.resources_path.to_string_lossy().into_owned()),
            claude_version: Some(install.version.clone()),
            multiple_installs: install.multiple_installs,
            state: localized_state.to_string(),
            configured_locale,
            pack_path: pack_path.map(str::to_string),
            pack_valid,
            backup_available,
            message: match localized_state {
                "installed" => "简体中文资源已安装".to_string(),
                "partial" => "检测到不完整的中文化状态，可重新安装或恢复".to_string(),
                _ => "尚未安装简体中文资源".to_string(),
            },
        })
    }
}

fn validate_pack(path: &Path) -> AppResult<ValidatedPack> {
    let root = fs::canonicalize(path)
        .map_err(|error| AppError::Path(format!("中文资源包目录无效: {error}")))?;
    let resources_candidate = root.join("resources");
    let resources = if resources_candidate.is_dir() {
        fs::canonicalize(resources_candidate)?
    } else {
        root.clone()
    };
    for (name, expected) in [
        (FRONTEND_FILE, "object"),
        (DESKTOP_FILE, "object"),
        (STATSIG_FILE, "object"),
        (HARDCODED_FILE, "array"),
    ] {
        let file = resources.join(name);
        let canonical = fs::canonicalize(&file)
            .map_err(|_| AppError::Config(format!("中文资源包缺少文件: {name}")))?;
        if !canonical.starts_with(&resources) || !canonical.is_file() {
            return Err(AppError::Config(format!("中文资源文件越界或无效: {name}")));
        }
        let metadata = fs::metadata(&canonical)?;
        if metadata.len() == 0 || metadata.len() > MAX_PACK_FILE_BYTES {
            return Err(AppError::Config(format!("中文资源文件大小异常: {name}")));
        }
        let value: Value = serde_json::from_slice(&fs::read(&canonical)?)
            .map_err(|_| AppError::Config(format!("中文资源文件不是有效 JSON: {name}")))?;
        let shape_ok = match expected {
            "array" => value.is_array(),
            _ => value.is_object(),
        };
        if !shape_ok {
            return Err(AppError::Config(format!("中文资源文件结构无效: {name}")));
        }
    }
    Ok(ValidatedPack { root, resources })
}

#[cfg(windows)]
fn detect_install() -> AppResult<Option<LocalizationInstall>> {
    let mut installs = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let base = local_app_data.join("AnthropicClaude");
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(version) = name.strip_prefix("app-") else {
                    continue;
                };
                let version = version.to_string();
                let exe_path = path.join("Claude.exe");
                let resources_path = path.join("resources");
                if exe_path.is_file() && resources_path.is_dir() {
                    installs.push(LocalizationInstall {
                        kind: "unpackaged".to_string(),
                        app_path: path,
                        resources_path,
                        exe_path,
                        version,
                        multiple_installs: false,
                    });
                }
            }
        }
    }
    installs.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
    if let Some(mut install) = installs.into_iter().next() {
        install.multiple_installs = count_unpackaged_installs() > 1;
        return Ok(Some(install));
    }

    let script = concat!(
        "$all=@(Get-AppxPackage -Name Claude -ErrorAction SilentlyContinue | ",
        "Sort-Object Version -Descending);$p=$all|Select-Object -First 1;",
        "if($p){[Console]::Out.Write($p.InstallLocation+'|'+$p.Version+'|'+$all.Count)}"
    );
    let output = Command::new(windows_powershell())
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('|');
    let (Some(path), Some(version), Some(count)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let app_path = PathBuf::from(path);
    let resources_path = app_path.join("resources");
    if !resources_path.is_dir() {
        return Ok(None);
    }
    let exe_path = ["Claude.exe", "claude.exe", "app/Claude.exe"]
        .into_iter()
        .map(|relative| app_path.join(relative))
        .find(|candidate| candidate.is_file());
    Ok(exe_path.map(|exe_path| LocalizationInstall {
        kind: "appx".to_string(),
        app_path,
        resources_path,
        exe_path,
        version: version.to_string(),
        multiple_installs: count.parse::<usize>().unwrap_or(1) > 1,
    }))
}

#[cfg(windows)]
fn count_unpackaged_installs() -> usize {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return 0;
    };
    fs::read_dir(local_app_data.join("AnthropicClaude"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry.path().join("Claude.exe").is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("app-"))
        })
        .count()
}

#[cfg(windows)]
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(windows)]
fn claude_locale_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(app_data.join("Claude").join("config.json"));
        paths.push(app_data.join("Claude-3p").join("config.json"));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let packages = local_app_data.join("Packages");
        if let Ok(entries) = fs::read_dir(packages) {
            for entry in entries.flatten().filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("Claude_"))
            }) {
                let roaming = entry.path().join("LocalCache").join("Roaming");
                paths.push(roaming.join("Claude").join("config.json"));
                paths.push(roaming.join("Claude-3p").join("config.json"));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(not(windows))]
fn claude_locale_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn read_configured_locale() -> Option<String> {
    claude_locale_paths().into_iter().find_map(|path| {
        read_json_file::<Value>(&path)
            .ok()
            .flatten()
            .and_then(|value| value.get("locale")?.as_str().map(str::to_string))
    })
}

#[cfg(windows)]
fn launch_elevated_worker(
    job_path: &Path,
    result_path: &Path,
) -> AppResult<DesktopLocalizationActionResult> {
    use std::os::windows::process::CommandExt;

    let executable = std::env::current_exe()?;
    let executable_quoted = executable.to_string_lossy().replace('\'', "''");
    let encoded_job = hex::encode(job_path.to_string_lossy().as_bytes());
    let script = format!(
        "$p=Start-Process -FilePath '{executable_quoted}' \
         -ArgumentList '--desktop-localization-worker-hex','{encoded_job}' \
         -Verb RunAs -Wait -PassThru -ErrorAction Stop; exit $p.ExitCode"
    );
    let status = Command::new(windows_powershell())
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|error| AppError::Other(format!("无法请求管理员权限: {error}")))?;
    if !result_path.is_file() {
        return Err(AppError::Config(if status.success() {
            "中文化工作进程没有返回结果".to_string()
        } else {
            "管理员权限请求被取消或中文化工作进程启动失败".to_string()
        }));
    }
    let result: DesktopLocalizationActionResult =
        serde_json::from_slice(&fs::read(result_path)?)?;
    let _ = fs::remove_file(result_path);
    if result.ok {
        Ok(result)
    } else {
        Err(AppError::Config(result.message))
    }
}

/// Called by `main.rs` before Tauri starts. Returns true when this process was
/// launched solely as the elevated localization worker.
pub fn run_worker_from_args() -> bool {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() != Some("--desktop-localization-worker-hex") {
        return false;
    }
    let result = args
        .next()
        .ok_or_else(|| AppError::Config("中文化工作任务参数缺失".to_string()))
        .and_then(|encoded| {
            let bytes = hex::decode(encoded)
                .map_err(|_| AppError::Config("中文化工作任务参数无效".to_string()))?;
            let path = String::from_utf8(bytes)
                .map_err(|_| AppError::Config("中文化工作任务路径无效".to_string()))?;
            run_worker_job(Path::new(&path))
        });
    if let Err(error) = result {
        log::error!("Claude Desktop localization worker failed: {error}");
        std::process::exit(1);
    }
    true
}

fn run_worker_job(job_path: &Path) -> AppResult<()> {
    let job: LocalizationWorkerJob = serde_json::from_slice(&fs::read(job_path)?)?;
    validate_worker_job(job_path, &job)?;
    let _mutex = LocalizationMutex::acquire()?;
    let result = perform_worker_action(&job);
    let action_result = match result {
        Ok(changed_files) => DesktopLocalizationActionResult {
            ok: true,
            changed_files,
            message: if job.action == "restore" {
                "Claude Desktop 已恢复为中文化前状态".to_string()
            } else {
                "Claude Desktop 简体中文资源安装完成".to_string()
            },
            log_path: Some(job.log_path.to_string_lossy().into_owned()),
        },
        Err(error) => DesktopLocalizationActionResult {
            ok: false,
            changed_files: 0,
            message: error.to_string(),
            log_path: Some(job.log_path.to_string_lossy().into_owned()),
        },
    };
    if let Some(parent) = job.log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &job.log_path,
        format!(
            "{}\r\n{}\r\n",
            Utc::now().to_rfc3339(),
            action_result.message
        ),
    )?;
    write_json_file(&job.result_path, &action_result)?;
    if action_result.ok {
        Ok(())
    } else {
        Err(AppError::Config(action_result.message))
    }
}

fn validate_worker_job(job_path: &Path, job: &LocalizationWorkerJob) -> AppResult<()> {
    let jobs_dir = fs::canonicalize(get_app_config_dir().join("localization-jobs"))
        .map_err(|_| AppError::Config("中文化任务目录无效".to_string()))?;
    let canonical_job = fs::canonicalize(job_path)
        .map_err(|_| AppError::Config("中文化任务文件无效".to_string()))?;
    if canonical_job.parent() != Some(jobs_dir.as_path()) {
        return Err(AppError::Config("中文化任务文件越界".to_string()));
    }
    for path in [&job.result_path, &job.log_path] {
        let parent = path
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok());
        if parent.as_deref() != Some(jobs_dir.as_path()) {
            return Err(AppError::Config("中文化任务输出路径越界".to_string()));
        }
    }
    if job.backup_root != get_backup_dir().join("desktop-localization") {
        return Err(AppError::Config("中文化备份路径无效".to_string()));
    }
    if !matches!(job.action.as_str(), "install" | "restore") {
        return Err(AppError::Config("中文化任务操作无效".to_string()));
    }
    #[cfg(windows)]
    {
        let detected = detect_install()?.ok_or_else(|| {
            AppError::Config("提升后未检测到 Claude Desktop 安装".to_string())
        })?;
        if detected.app_path != job.install.app_path
            || detected.resources_path != job.install.resources_path
            || detected.exe_path != job.install.exe_path
            || detected.version != job.install.version
        {
            return Err(AppError::Config(
                "Claude Desktop 安装在权限确认期间发生变化".to_string(),
            ));
        }
        let mut expected_locale_paths = claude_locale_paths();
        let mut supplied_locale_paths = job.locale_paths.clone();
        expected_locale_paths.sort();
        supplied_locale_paths.sort();
        if supplied_locale_paths != expected_locale_paths {
            return Err(AppError::Config("Claude locale 目标路径无效".to_string()));
        }
    }
    if job.action == "install" {
        let pack_path = job
            .pack_path
            .as_ref()
            .ok_or_else(|| AppError::Config("中文资源包路径缺失".to_string()))?;
        validate_pack(pack_path)?;
    }
    Ok(())
}

#[cfg(windows)]
struct LocalizationMutex(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl LocalizationMutex {
    fn acquire() -> AppResult<Self> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name: Vec<u16> = std::ffi::OsStr::new("Global\\ClaudeSwitcherDesktopLocalization")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(AppError::Other("无法创建中文化互斥锁".to_string()));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err(AppError::Config(
                "另一个 Claude Desktop 中文化任务正在运行".to_string(),
            ));
        }
        Ok(Self(handle))
    }
}

fn perform_worker_action(job: &LocalizationWorkerJob) -> AppResult<usize> {
    stop_claude();
    let result = (|| {
        if job.install.kind == "appx" {
            ensure_write_access(&job.install.resources_path)?;
        }
        if job.action == "restore" {
            restore_latest_backup(&job.backup_root, &job.install, &job.locale_paths)
        } else if job.action == "install" {
            perform_install(job)
        } else {
            Err(AppError::Config("未知的中文化操作".to_string()))
        }
    })();
    result
}

fn perform_install(job: &LocalizationWorkerJob) -> AppResult<usize> {
    let pack_path = job
        .pack_path
        .as_ref()
        .ok_or_else(|| AppError::Config("中文资源包路径缺失".to_string()))?;
    let pack = validate_pack(pack_path)?;

    let installed_frontend = job
        .install
        .resources_path
        .join("ion-dist")
        .join("i18n")
        .join("zh-CN.json");
    if installed_frontend.exists() {
        if latest_backup_dir(&job.backup_root, &job.install)?.is_some() {
            restore_latest_backup(&job.backup_root, &job.install, &job.locale_paths)?;
        }
    }

    let changes = collect_install_changes(&job.install, &pack, &job.locale_paths)?;
    if changes.is_empty() {
        return Err(AppError::Config(
            "没有生成任何中文化文件修改".to_string(),
        ));
    }
    let backup_dir = create_backup(&job.backup_root, &job.install, &changes)?;
    for change in &changes {
        if let Err(error) = atomic_write(&change.target, &change.bytes) {
            let rollback_error =
                restore_backup_dir(&backup_dir, &job.install, &job.locale_paths).err();
            return Err(AppError::Other(match rollback_error {
                Some(rollback) => format!("安装失败且自动回滚失败：{error}；{rollback}"),
                None => format!("安装失败，已自动回滚：{error}"),
            }));
        }
    }
    if let Err(error) = prune_backups(&job.backup_root, &job.install) {
        log::warn!("轮换中文化备份失败（安装已完成）: {error}");
    }
    Ok(changes.len())
}

fn collect_install_changes(
    install: &LocalizationInstall,
    pack: &ValidatedPack,
    locale_paths: &[PathBuf],
) -> AppResult<Vec<FileChange>> {
    let mut changes = Vec::new();
    for (source, target) in [
        (
            pack.resources.join(FRONTEND_FILE),
            install
                .resources_path
                .join("ion-dist")
                .join("i18n")
                .join("zh-CN.json"),
        ),
        (
            pack.resources.join(DESKTOP_FILE),
            install.resources_path.join("zh-CN.json"),
        ),
        (
            pack.resources.join(STATSIG_FILE),
            install
                .resources_path
                .join("ion-dist")
                .join("i18n")
                .join("statsig")
                .join("zh-CN.json"),
        ),
    ] {
        changes.push(FileChange {
            target,
            bytes: fs::read(source)?,
        });
    }

    let replacements = load_hardcoded_replacements(&pack.resources.join(HARDCODED_FILE))?;
    let assets = install
        .resources_path
        .join("ion-dist")
        .join("assets")
        .join("v1");
    let js_files = fs::read_dir(&assets)
        .map_err(|_| AppError::Config(format!("未找到 Claude 前端资源目录: {}", assets.display())))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("js"))
        .collect::<Vec<_>>();
    if js_files.is_empty() {
        return Err(AppError::Config("未找到 Claude 前端 JS bundle".to_string()));
    }
    let mut language_registered = false;
    for path in js_files {
        let original = fs::read_to_string(&path)
            .map_err(|_| AppError::Config(format!("无法读取 UTF-8 前端资源: {}", path.display())))?;
        let (mut patched, registered) = register_language(&original);
        language_registered |= registered;
        apply_hardcoded_replacements(&mut patched, &replacements);
        if patched != original {
            changes.push(FileChange {
                target: path,
                bytes: patched.into_bytes(),
            });
        }
    }
    if !language_registered {
        return Err(AppError::Config(
            "未能注册 zh-CN，当前 Claude 前端结构可能已变化".to_string(),
        ));
    }

    for path in locale_paths {
        let mut value = if path.is_file() {
            read_json_file::<Value>(path)?.ok_or_else(|| {
                AppError::Config(format!("无法读取 Claude locale 配置: {}", path.display()))
            })?
        } else {
            serde_json::json!({})
        };
        let object = value.as_object_mut().ok_or_else(|| {
            AppError::Config(format!("Claude locale 配置不是 JSON 对象: {}", path.display()))
        })?;
        object.insert(
            "locale".to_string(),
            Value::String(LOCALIZATION_LOCALE.to_string()),
        );
        changes.push(FileChange {
            target: path.clone(),
            bytes: serde_json::to_vec_pretty(&value)?,
        });
    }

    let mut seen = HashSet::new();
    changes.retain(|change| seen.insert(change.target.clone()));
    Ok(changes)
}

fn register_language(text: &str) -> (String, bool) {
    let Some(start) = text.find(BASE_LANGUAGES) else {
        return (text.to_string(), false);
    };
    let tail = &text[start + BASE_LANGUAGES.len()..];
    let Some(end_offset) = tail.find(']') else {
        return (text.to_string(), false);
    };
    let end = start + BASE_LANGUAGES.len() + end_offset;
    let existing = &text[start..=end];
    if existing.contains(",\"zh-CN\"") {
        return (text.to_string(), true);
    }
    let without_chinese = existing
        .replace(",\"zh-CN\"", "")
        .replace(",\"zh-TW\"", "")
        .replace(",\"zh-HK\"", "");
    let replacement = format!("{},\"zh-CN\"]", without_chinese.trim_end_matches(']'));
    let mut result = String::with_capacity(text.len() + 10);
    result.push_str(&text[..start]);
    result.push_str(&replacement);
    result.push_str(&text[end + 1..]);
    (result, true)
}

fn load_hardcoded_replacements(path: &Path) -> AppResult<Vec<(String, String)>> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let array = value
        .as_array()
        .ok_or_else(|| AppError::Config("硬编码翻译映射必须是数组".to_string()))?;
    if array.len() > MAX_HARDCODED_REPLACEMENTS {
        return Err(AppError::Config("硬编码翻译映射条目过多".to_string()));
    }
    let structural = [
        "hour", "hours", "minute", "minutes", "second", "seconds", "day", "days",
        "week", "weeks", "month", "months", "year", "years",
    ];
    let mut replacements = Vec::new();
    for item in array {
        let pair = item
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| AppError::Config("硬编码翻译映射条目格式无效".to_string()))?;
        let source = pair[0]
            .as_str()
            .ok_or_else(|| AppError::Config("硬编码翻译源文本无效".to_string()))?;
        let target = pair[1]
            .as_str()
            .ok_or_else(|| AppError::Config("硬编码翻译目标文本无效".to_string()))?;
        if source.is_empty()
            || target.is_empty()
            || source == target
            || source.len() > 1_000
            || target.len() > 2_000
            || source.contains('\n')
            || structural.contains(&source)
            || source == "\"Search\""
        {
            continue;
        }
        replacements.push((source.to_string(), target.to_string()));
    }
    replacements.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
    Ok(replacements)
}

fn apply_hardcoded_replacements(text: &mut String, replacements: &[(String, String)]) {
    for (source, target) in replacements {
        if source
            .chars()
            .any(|character| matches!(character, '\\' | '"' | '\'' | '`' | '=' | ';'))
            || source.contains("=>")
        {
            continue;
        }
        let source_json = match serde_json::to_string(source) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if text.contains(&source_json) {
            let target_json = match serde_json::to_string(target) {
                Ok(value) => value,
                Err(_) => continue,
            };
            *text = text.replace(&source_json, &target_json);
        }
    }
}

fn create_backup(
    backup_root: &Path,
    install: &LocalizationInstall,
    changes: &[FileChange],
) -> AppResult<PathBuf> {
    let version_dir = backup_root.join(safe_component(&install.version));
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%f").to_string();
    let backup_dir = version_dir.join(timestamp);
    fs::create_dir_all(&backup_dir)?;
    let mut files = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        if change.target.exists() {
            let backup_file = format!("{index:04}.bin");
            let backup_path = backup_dir.join(&backup_file);
            fs::copy(&change.target, &backup_path)?;
            files.push(LocalizationBackupFile {
                target: change.target.clone(),
                existed: true,
                sha256: Some(sha256_file(&backup_path)?),
                backup_file: Some(backup_file),
            });
        } else {
            files.push(LocalizationBackupFile {
                target: change.target.clone(),
                existed: false,
                sha256: None,
                backup_file: None,
            });
        }
    }
    let manifest = LocalizationBackupManifest {
        version: 1,
        claude_version: install.version.clone(),
        install_path: install.app_path.clone(),
        created_at: Utc::now().timestamp_millis(),
        files,
    };
    write_json_file(&backup_dir.join("manifest.json"), &manifest)?;
    Ok(backup_dir)
}

fn restore_latest_backup(
    backup_root: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> AppResult<usize> {
    let backup_dir = latest_backup_dir(backup_root, install)?.ok_or_else(|| {
        AppError::Config(format!(
            "没有找到与 Claude Desktop {} 匹配的中文化备份",
            install.version
        ))
    })?;
    restore_backup_dir(&backup_dir, install, locale_paths)
}

fn restore_backup_dir(
    backup_dir: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> AppResult<usize> {
    let manifest: LocalizationBackupManifest =
        serde_json::from_slice(&fs::read(backup_dir.join("manifest.json"))?)?;
    validate_backup_manifest(&manifest, install)?;
    for file in &manifest.files {
        validate_restore_target(&file.target, install, locale_paths)?;
        if file.existed {
            let name = file
                .backup_file
                .as_deref()
                .ok_or_else(|| AppError::Config("中文化备份缺少文件名".to_string()))?;
            let source = backup_dir.join(name);
            let actual_sha256 = sha256_file(&source)?;
            if !source.is_file() || file.sha256.as_deref() != Some(actual_sha256.as_str()) {
                return Err(AppError::Config(
                    "中文化备份校验失败，拒绝恢复".to_string(),
                ));
            }
            atomic_write(&file.target, &fs::read(source)?)?;
        } else if file.target.exists() {
            fs::remove_file(&file.target)?;
        }
    }
    Ok(manifest.files.len())
}

fn latest_backup_dir(
    backup_root: &Path,
    install: &LocalizationInstall,
) -> AppResult<Option<PathBuf>> {
    let version_dir = backup_root.join(safe_component(&install.version));
    if !version_dir.is_dir() {
        return Ok(None);
    }
    let mut candidates = fs::read_dir(version_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for candidate in candidates {
        let manifest = serde_json::from_slice::<LocalizationBackupManifest>(&fs::read(
            candidate.join("manifest.json"),
        )?);
        if let Ok(manifest) = manifest {
            if validate_backup_manifest(&manifest, install).is_ok() {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

fn validate_backup_manifest(
    manifest: &LocalizationBackupManifest,
    install: &LocalizationInstall,
) -> AppResult<()> {
    if manifest.version != 1
        || manifest.claude_version != install.version
        || manifest.install_path != install.app_path
    {
        return Err(AppError::Config(
            "中文化备份与当前 Claude Desktop 版本不匹配".to_string(),
        ));
    }
    Ok(())
}

fn validate_restore_target(
    target: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> AppResult<()> {
    let allowed_resource_files = [
        install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join("zh-CN.json"),
        install.resources_path.join("zh-CN.json"),
        install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join("statsig")
            .join("zh-CN.json"),
    ];
    let assets_dir = install
        .resources_path
        .join("ion-dist")
        .join("assets")
        .join("v1");
    let is_frontend_js = target.parent() == Some(assets_dir.as_path())
        && target.extension().and_then(|extension| extension.to_str()) == Some("js");
    if allowed_resource_files.iter().any(|path| path == target)
        || is_frontend_js
        || locale_paths.iter().any(|path| path == target)
    {
        return Ok(());
    }
    Err(AppError::Config(format!(
        "中文化备份包含越界目标，拒绝恢复: {}",
        target.display()
    )))
}

fn prune_backups(backup_root: &Path, install: &LocalizationInstall) -> AppResult<()> {
    let version_dir = backup_root.join(safe_component(&install.version));
    let mut candidates = fs::read_dir(version_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for old in candidates.into_iter().skip(BACKUPS_TO_KEEP) {
        if let Err(error) = fs::remove_dir_all(&old) {
            log::warn!("删除过期中文化备份失败 {}: {error}", old.display());
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn safe_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn ensure_write_access(path: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let username = std::env::var("USERNAME")
            .map_err(|_| AppError::Config("无法确定 Windows 用户名".to_string()))?;
        let identity = std::env::var("USERDOMAIN")
            .ok()
            .filter(|domain| !domain.is_empty())
            .map(|domain| format!("{domain}\\{username}"))
            .unwrap_or(username);
        let grant = format!("{identity}:(OI)(CI)F");
        let output = Command::new(windows_system_executable("icacls.exe"))
            .arg(path)
            .args(["/grant", &grant, "/T", "/C", "/Q"])
            .creation_flags(0x0800_0000)
            .output()?;
        if !output.status.success() {
            return Err(AppError::Config(format!(
                "无法取得 Claude Desktop 资源目录写入权限: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}

fn stop_claude() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new(windows_system_executable("taskkill.exe"))
            .args(["/IM", "Claude.exe", "/T", "/F"])
            .creation_flags(0x0800_0000)
            .status();
    }
}

fn restart_claude(executable: &Path) {
    if executable.is_file() {
        if let Err(error) = Command::new(executable).spawn() {
            log::warn!("中文化完成后无法重启 Claude Desktop: {error}");
        }
    }
}

#[cfg(windows)]
fn windows_system_executable(name: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(name)
}

#[cfg(windows)]
fn windows_powershell() -> PathBuf {
    windows_system_executable("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn language_registration_is_idempotent() {
        let original = format!("const languages={BASE_LANGUAGES}];");
        let (patched, registered) = register_language(&original);
        assert!(registered);
        assert!(patched.contains(",\"zh-CN\"]"));
        let (again, registered_again) = register_language(&patched);
        assert!(registered_again);
        assert_eq!(again, patched);
    }

    #[test]
    fn safe_component_removes_path_characters() {
        assert_eq!(safe_component("1.2.3/../../x"), "1.2.3_.._.._x");
    }

    #[test]
    fn backup_restore_roundtrip_restores_and_removes_created_files() {
        let root = tempdir().unwrap();
        let app_path = root.path().join("Claude");
        let resources_path = app_path.join("resources");
        let assets_path = resources_path.join("ion-dist").join("assets").join("v1");
        fs::create_dir_all(&assets_path).unwrap();
        let js_path = assets_path.join("bundle.js");
        fs::write(&js_path, b"original").unwrap();
        let zh_path = resources_path
            .join("ion-dist")
            .join("i18n")
            .join("zh-CN.json");
        let locale_path = root.path().join("profile").join("Claude").join("config.json");
        let install = LocalizationInstall {
            kind: "unpackaged".to_string(),
            app_path,
            resources_path,
            exe_path: root.path().join("Claude.exe"),
            version: "1.2.3".to_string(),
            multiple_installs: false,
        };
        let changes = vec![
            FileChange {
                target: js_path.clone(),
                bytes: b"patched".to_vec(),
            },
            FileChange {
                target: zh_path.clone(),
                bytes: b"{}".to_vec(),
            },
            FileChange {
                target: locale_path.clone(),
                bytes: br#"{"locale":"zh-CN"}"#.to_vec(),
            },
        ];
        let backup_root = root.path().join("backups");
        let backup = create_backup(&backup_root, &install, &changes).unwrap();
        for change in &changes {
            atomic_write(&change.target, &change.bytes).unwrap();
        }
        restore_backup_dir(&backup, &install, &[locale_path.clone()]).unwrap();
        assert_eq!(fs::read(js_path).unwrap(), b"original");
        assert!(!zh_path.exists());
        assert!(!locale_path.exists());
    }
}


#[cfg(windows)]
impl Drop for LocalizationMutex {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct LocalizationMutex;

#[cfg(not(windows))]
impl LocalizationMutex {
    fn acquire() -> AppResult<Self> {
        Ok(Self)
    }
}
