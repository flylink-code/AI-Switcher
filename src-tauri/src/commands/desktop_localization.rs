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
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use zip::ZipArchive;

const PACK_SETTING_KEY: &str = "desktop_localization_pack_path";
const LOCALIZATION_LOCALE: &str = "zh-CN";
const MAX_PACK_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PACK_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PACK_EXTRACTED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_GITHUB_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_HARDCODED_REPLACEMENTS: usize = 5_000;
const BACKUPS_TO_KEEP: usize = 3;
const UPSTREAM_REPOSITORY: &str = "javaht/claude-desktop-zh-cn";
const PACK_INFO_FILE: &str = "pack-info.json";
const MANIFEST_FILE: &str = "manifest.json";
const RELEASE_FILE: &str = "release.json";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    pub detection_source: String,
    pub checked_at: i64,
    pub diagnostics: Vec<String>,
    pub install_path: Option<String>,
    pub resources_path: Option<String>,
    pub claude_version: Option<String>,
    pub multiple_installs: bool,
    pub state: String,
    pub configured_locale: Option<String>,
    pub pack_path: Option<String>,
    pub pack_valid: bool,
    pub pack_source: Option<String>,
    pub pack_version: Option<String>,
    pub pack_revision: Option<String>,
    pub pack_fetched_at: Option<i64>,
    pub backup_available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLocalizationPackInfo {
    pub source: String,
    pub version: Option<String>,
    pub revision: Option<String>,
    pub fetched_at: Option<i64>,
    pub pack_path: String,
    pub valid: bool,
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
struct InstallDetection {
    install: Option<LocalizationInstall>,
    diagnostics: Vec<String>,
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
pub async fn download_desktop_localization_pack(
    state: tauri::State<'_, AppState>,
) -> AppResult<DesktopLocalizationPackInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("Claude-Switcher desktop-localization")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("中文资源下载重定向次数过多")
            } else if upstream_url_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| AppError::Other(format!("创建中文资源下载客户端失败: {error}")))?;
    let revision = fetch_upstream_release_tag(&client).await?;
    let packs_root = get_app_config_dir().join("localization").join("packs");
    fs::create_dir_all(&packs_root)?;
    let target = packs_root.join(safe_component(&revision));
    let stage = packs_root.join(format!(
        ".download-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(stage.join("resources"))?;

    let result = download_pack_to_stage(&client, &revision, &stage, &target).await;
    let info = match result {
        Ok(info) => info,
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
    };

    if let Err(error) = promote_downloaded_pack(&stage, &target) {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    state.db.with_conn(|conn| {
        set_setting(
            conn,
            PACK_SETTING_KEY,
            &target.to_string_lossy(),
        )
    })?;
    Ok(info)
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
        let script = concat!(
            "Add-Type -AssemblyName System.Windows.Forms;",
            "$d=New-Object System.Windows.Forms.FolderBrowserDialog;",
            "$d.Description='选择 claude-desktop-zh-cn 资源目录';",
            "$d.ShowNewFolderButton=$false;",
            "if($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK)",
            "{[Console]::OutputEncoding=[Text.Encoding]::UTF8;[Console]::Out.Write($d.SelectedPath)}"
        );
        let mut command = Command::new(windows_powershell());
        command.args(["-NoProfile", "-STA", "-Command", script]);
        hide_console_window(&mut command);
        let output = command
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

#[tauri::command]
pub async fn update_desktop_localization(
    state: tauri::State<'_, AppState>,
) -> AppResult<DesktopLocalizationActionResult> {
    let was_installed = {
        let pack_path = state
            .db
            .with_conn(|conn| get_setting(conn, PACK_SETTING_KEY))?;
        localization_status(pack_path.as_deref())?.state == "installed"
    };
    let info = download_desktop_localization_pack(state.clone()).await?;
    if was_installed {
        return install_desktop_localization(info.pack_path, state).await;
    }
    Ok(DesktopLocalizationActionResult {
        ok: true,
        changed_files: 0,
        message: format!(
            "已下载最新中文资源 {}（尚未写入 Desktop）",
            info.version.unwrap_or_else(|| "latest".to_string())
        ),
        log_path: None,
    })
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

#[derive(Debug, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
}

async fn fetch_upstream_release_tag(client: &reqwest::Client) -> AppResult<String> {
    let url = format!("https://api.github.com/repos/{UPSTREAM_REPOSITORY}/releases/latest");
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| AppError::Other(format!("连接中文资源仓库失败: {error}")))?;
    validate_upstream_url(response.url())?;
    if !response.status().is_success() {
        return Err(github_http_error(
            "读取中文资源最新 Release",
            response.status(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_GITHUB_METADATA_BYTES)
    {
        return Err(AppError::Config(
            "中文资源版本响应超过 1 MB 限制".to_string(),
        ));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| AppError::Other(format!("读取中文资源版本失败: {error}")))?;
    if body.len() as u64 > MAX_GITHUB_METADATA_BYTES {
        return Err(AppError::Config(
            "中文资源版本响应超过 1 MB 限制".to_string(),
        ));
    }
    let payload: GitHubReleaseResponse = serde_json::from_slice(&body)
        .map_err(|error| AppError::Config(format!("中文资源版本响应无效: {error}")))?;
    let tag = payload.tag_name.trim().trim_start_matches('v').to_string();
    if !is_valid_upstream_revision(&tag) {
        return Err(AppError::Config(
            "中文资源仓库返回了无效的 Release 标签".to_string(),
        ));
    }
    Ok(tag)
}

fn is_valid_upstream_revision(revision: &str) -> bool {
    if revision.is_empty() || revision.len() > 64 {
        return false;
    }
    if revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    // GitHub Release tags such as 1.4.6 (and older commit-folder caches).
    revision
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !revision.contains("..")
}

async fn download_pack_to_stage(
    client: &reqwest::Client,
    revision: &str,
    stage: &Path,
    final_target: &Path,
) -> AppResult<DesktopLocalizationPackInfo> {
    let url = format!(
        "https://api.github.com/repos/{UPSTREAM_REPOSITORY}/zipball/{revision}"
    );
    let mut response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| AppError::Other(format!("下载中文资源失败: {error}")))?;
    validate_upstream_url(response.url())?;
    if !response.status().is_success() {
        return Err(github_http_error("下载中文资源", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_PACK_ARCHIVE_BYTES)
    {
        return Err(AppError::Config(
            "中文资源仓库压缩包超过 20 MB 限制".to_string(),
        ));
    }

    let mut archive_bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::Other(format!("读取中文资源下载内容失败: {error}")))?
    {
        if archive_bytes.len() as u64 + chunk.len() as u64 > MAX_PACK_ARCHIVE_BYTES {
            return Err(AppError::Config(
                "中文资源仓库压缩包超过 20 MB 限制".to_string(),
            ));
        }
        archive_bytes.extend_from_slice(&chunk);
    }

    extract_pack_archive(&archive_bytes, stage)?;
    let version = validate_downloaded_metadata(stage)?;
    validate_pack(stage)?;
    let info = DesktopLocalizationPackInfo {
        source: "github".to_string(),
        version: Some(version),
        revision: Some(revision.to_string()),
        fetched_at: Some(Utc::now().timestamp_millis()),
        pack_path: final_target.to_string_lossy().into_owned(),
        valid: true,
    };
    write_json_file(&stage.join(PACK_INFO_FILE), &info)?;
    Ok(info)
}

fn validate_upstream_url(url: &reqwest::Url) -> AppResult<()> {
    if !upstream_url_allowed(url) {
        return Err(AppError::Config(format!(
            "中文资源下载被重定向到不受信任的地址: {url}"
        )));
    }
    Ok(())
}

fn upstream_url_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("api.github.com" | "github.com" | "codeload.github.com")
        )
}

fn github_http_error(action: &str, status: reqwest::StatusCode) -> AppError {
    let detail = match status.as_u16() {
        403 | 429 => "，GitHub 可能已限流，请稍后重试",
        404 => "，上游仓库或 Release 不存在",
        _ => "",
    };
    AppError::Config(format!("{action}失败（HTTP {status}{detail}）"))
}

fn extract_pack_archive(bytes: &[u8], stage: &Path) -> AppResult<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::Config(format!("中文资源 ZIP 无效: {error}")))?;
    let required = [
        FRONTEND_FILE,
        HARDCODED_FILE,
        DESKTOP_FILE,
        STATSIG_FILE,
        MANIFEST_FILE,
        RELEASE_FILE,
    ];
    let resources = stage.join("resources");
    let mut found = HashSet::new();
    let mut extracted_bytes = 0u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Config(format!("读取中文资源 ZIP 条目失败: {error}")))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| AppError::Config("中文资源 ZIP 包含不安全路径".to_string()))?;
        let Some(name) = enclosed.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let in_resources = enclosed
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("resources");
        if !in_resources || !required.contains(&name) {
            continue;
        }
        if entry.is_dir()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::Config(format!(
                "中文资源 ZIP 条目类型无效: {name}"
            )));
        }
        if !found.insert(name.to_string()) {
            return Err(AppError::Config(format!(
                "中文资源 ZIP 包含重复文件: {name}"
            )));
        }
        if entry.size() == 0 || entry.size() > MAX_PACK_FILE_BYTES {
            return Err(AppError::Config(format!(
                "中文资源 ZIP 文件大小异常: {name}"
            )));
        }
        extracted_bytes = extracted_bytes.saturating_add(entry.size());
        if extracted_bytes > MAX_PACK_EXTRACTED_BYTES {
            return Err(AppError::Config(
                "中文资源 ZIP 解压内容超过 32 MB 限制".to_string(),
            ));
        }
        let mut content = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut content)
            .map_err(|error| AppError::Other(format!("解压中文资源 {name} 失败: {error}")))?;
        if content.len() as u64 != entry.size() {
            return Err(AppError::Config(format!(
                "中文资源 ZIP 文件长度不一致: {name}"
            )));
        }
        atomic_write(&resources.join(name), &content)?;
    }

    for name in required {
        if !found.contains(name) {
            return Err(AppError::Config(format!(
                "中文资源 ZIP 缺少文件: {name}"
            )));
        }
    }
    Ok(())
}

fn validate_downloaded_metadata(stage: &Path) -> AppResult<String> {
    let resources = stage.join("resources");
    let manifest: Value = read_json_file(&resources.join(MANIFEST_FILE))?
        .ok_or_else(|| AppError::Config("中文资源缺少 manifest.json".to_string()))?;
    if manifest.get("language").and_then(Value::as_str) != Some(LOCALIZATION_LOCALE) {
        return Err(AppError::Config(
            "中文资源 manifest.json 不是 zh-CN 资源".to_string(),
        ));
    }
    let release: Value = read_json_file(&resources.join(RELEASE_FILE))?
        .ok_or_else(|| AppError::Config("中文资源缺少 release.json".to_string()))?;
    if release.get("repo").and_then(Value::as_str) != Some(UPSTREAM_REPOSITORY) {
        return Err(AppError::Config(
            "中文资源 release.json 的仓库来源不匹配".to_string(),
        ));
    }
    release
        .get("release")
        .and_then(Value::as_str)
        .filter(|version| !version.trim().is_empty() && version.len() <= 64)
        .map(str::to_string)
        .ok_or_else(|| AppError::Config("中文资源 release.json 缺少有效版本".to_string()))
}

fn promote_downloaded_pack(stage: &Path, target: &Path) -> AppResult<()> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::Path("中文资源缓存目标目录无效".to_string()))?;
    let previous = parent.join(format!(
        ".previous-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let had_previous = target.exists();
    if had_previous {
        fs::rename(target, &previous).map_err(|error| {
            AppError::Other(format!("无法暂存旧中文资源缓存: {error}"))
        })?;
    }
    if let Err(error) = fs::rename(stage, target) {
        if had_previous {
            let _ = fs::rename(&previous, target);
        }
        return Err(AppError::Other(format!(
            "无法启用新中文资源缓存: {error}"
        )));
    }
    if had_previous {
        if let Err(error) = fs::remove_dir_all(&previous) {
            log::warn!("无法清理旧中文资源缓存 {}: {error}", previous.display());
        }
    }
    Ok(())
}

fn pack_info(pack_path: Option<&str>, valid: bool) -> Option<DesktopLocalizationPackInfo> {
    let path = Path::new(pack_path?);
    let local_info = || DesktopLocalizationPackInfo {
        source: "local".to_string(),
        version: None,
        revision: None,
        fetched_at: None,
        pack_path: path.to_string_lossy().into_owned(),
        valid,
    };
    let metadata_path = if path.join(PACK_INFO_FILE).is_file() {
        path.join(PACK_INFO_FILE)
    } else if path
        .parent()
        .is_some_and(|parent| parent.join(PACK_INFO_FILE).is_file())
    {
        path.parent()?.join(PACK_INFO_FILE)
    } else {
        return Some(local_info());
    };
    let Some(mut info): Option<DesktopLocalizationPackInfo> =
        read_json_file(&metadata_path).ok().flatten()
    else {
        return Some(local_info());
    };
    let github_revision_valid = info.source == "github"
        && info
            .revision
            .as_ref()
            .is_some_and(|revision| is_valid_upstream_revision(revision));
    if !github_revision_valid {
        return Some(local_info());
    }
    info.pack_path = path.to_string_lossy().into_owned();
    info.valid = valid;
    Some(info)
}

fn localization_status(pack_path: Option<&str>) -> AppResult<DesktopLocalizationStatus> {
    let checked_at = Utc::now().timestamp_millis();
    let pack_valid = pack_path
        .map(|path| validate_pack(Path::new(path)).is_ok())
        .unwrap_or(false);
    let current_pack = pack_info(pack_path, pack_valid);
    #[cfg(not(windows))]
    {
        return Ok(DesktopLocalizationStatus {
            platform_supported: false,
            install_detected: false,
            install_kind: None,
            detection_source: "unsupported".to_string(),
            checked_at,
            diagnostics: vec!["当前平台不是 Windows".to_string()],
            install_path: None,
            resources_path: None,
            claude_version: None,
            multiple_installs: false,
            state: "unsupported".to_string(),
            configured_locale: None,
            pack_path: pack_path.map(str::to_string),
            pack_valid,
            pack_source: current_pack.as_ref().map(|pack| pack.source.clone()),
            pack_version: current_pack.as_ref().and_then(|pack| pack.version.clone()),
            pack_revision: current_pack.as_ref().and_then(|pack| pack.revision.clone()),
            pack_fetched_at: current_pack.as_ref().and_then(|pack| pack.fetched_at),
            backup_available: false,
            message: "Claude Desktop 中文化首版仅支持 Windows".to_string(),
        });
    }

    #[cfg(windows)]
    {
        let detection = detect_install_with_diagnostics()?;
        let install = detection.install;
        let configured_locale = read_configured_locale();
        let Some(install) = install else {
            return Ok(DesktopLocalizationStatus {
                platform_supported: true,
                install_detected: false,
                install_kind: None,
                detection_source: "none".to_string(),
                checked_at,
                diagnostics: detection.diagnostics,
                install_path: None,
                resources_path: None,
                claude_version: None,
                multiple_installs: false,
                state: "notInstalled".to_string(),
                configured_locale,
                pack_path: pack_path.map(str::to_string),
                pack_valid,
                pack_source: current_pack.as_ref().map(|pack| pack.source.clone()),
                pack_version: current_pack.as_ref().and_then(|pack| pack.version.clone()),
                pack_revision: current_pack.as_ref().and_then(|pack| pack.revision.clone()),
                pack_fetched_at: current_pack.as_ref().and_then(|pack| pack.fetched_at),
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
            detection_source: install.kind.clone(),
            checked_at,
            diagnostics: detection.diagnostics,
            install_path: Some(install.app_path.to_string_lossy().into_owned()),
            resources_path: Some(install.resources_path.to_string_lossy().into_owned()),
            claude_version: Some(install.version.clone()),
            multiple_installs: install.multiple_installs,
            state: localized_state.to_string(),
            configured_locale,
            pack_path: pack_path.map(str::to_string),
            pack_valid,
            pack_source: current_pack.as_ref().map(|pack| pack.source.clone()),
            pack_version: current_pack.as_ref().and_then(|pack| pack.version.clone()),
            pack_revision: current_pack.as_ref().and_then(|pack| pack.revision.clone()),
            pack_fetched_at: current_pack.as_ref().and_then(|pack| pack.fetched_at),
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
    Ok(detect_install_with_diagnostics()?.install)
}

#[cfg(windows)]
fn detect_install_with_diagnostics() -> AppResult<InstallDetection> {
    let mut installs = Vec::new();
    let mut diagnostics = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let base = local_app_data.join("AnthropicClaude");
        diagnostics.push(format!("检查 unpackaged 安装: {}", base.display()));
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
        diagnostics.push(format!(
            "使用 unpackaged 安装: {}",
            install.app_path.display()
        ));
        return Ok(InstallDetection {
            install: Some(install),
            diagnostics,
        });
    }

    diagnostics.push("检查 AppX 包: Claude".to_string());
    let script = concat!(
        "$all=@(Get-AppxPackage -Name Claude -ErrorAction SilentlyContinue | ",
        "Sort-Object Version -Descending);$p=$all|Select-Object -First 1;",
        "if($p){[Console]::Out.Write($p.InstallLocation+'|'+$p.Version+'|'+$all.Count)}"
    );
    let mut command = Command::new(windows_powershell());
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    hide_console_window(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        diagnostics.push(format!(
            "AppX 查询失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return Ok(InstallDetection {
            install: None,
            diagnostics,
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('|');
    let (Some(path), Some(version), Some(count)) = (parts.next(), parts.next(), parts.next()) else {
        diagnostics.push("未找到 Claude AppX 包".to_string());
        return Ok(InstallDetection {
            install: None,
            diagnostics,
        });
    };
    let app_path = PathBuf::from(path);
    let install = install_from_appx_path(
        app_path.clone(),
        version,
        count.parse::<usize>().unwrap_or(1) > 1,
    );
    diagnostics.push(match &install {
        Some(install) => format!(
            "使用 AppX 安装: {}；资源目录: {}",
            install.app_path.display(),
            install.resources_path.display()
        ),
        None => format!(
            "Claude AppX 存在，但未找到受支持的可执行文件或资源目录: {}",
            app_path.display()
        ),
    });
    Ok(InstallDetection {
        install,
        diagnostics,
    })
}

fn install_from_appx_path(
    app_path: PathBuf,
    version: &str,
    multiple_installs: bool,
) -> Option<LocalizationInstall> {
    let resources_path = [
        app_path.join("app").join("resources"),
        app_path.join("resources"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_dir())?;
    let exe_path = [
        app_path.join("app").join("Claude.exe"),
        app_path.join("app").join("claude.exe"),
        app_path.join("Claude.exe"),
        app_path.join("claude.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())?;
    Some(LocalizationInstall {
        kind: "appx".to_string(),
        app_path,
        resources_path,
        exe_path,
        version: version.to_string(),
        multiple_installs,
    })
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
    let executable = std::env::current_exe()?;
    let executable_quoted = executable.to_string_lossy().replace('\'', "''");
    let encoded_job = hex::encode(job_path.to_string_lossy().as_bytes());
    let script = format!(
        "$p=Start-Process -FilePath '{executable_quoted}' \
         -ArgumentList '--desktop-localization-worker-hex','{encoded_job}' \
         -Verb RunAs -Wait -PassThru -ErrorAction Stop; exit $p.ExitCode"
    );
    let mut command = Command::new(windows_powershell());
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    hide_console_window(&mut command);
    let status = command
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
    let mut applied_targets = HashSet::new();
    for change in &changes {
        if let Err(error) = atomic_write(&change.target, &change.bytes) {
            let install_error = format!(
                "写入中文化文件失败 {}: {error}",
                change.target.display()
            );
            if applied_targets.is_empty() {
                return Err(AppError::Other(format!(
                    "安装失败，尚未写入任何文件：{install_error}"
                )));
            }
            let rollback_error = restore_backup_targets(
                &backup_dir,
                &job.install,
                &job.locale_paths,
                &applied_targets,
            )
            .err();
            return Err(AppError::Other(match rollback_error {
                Some(rollback) => {
                    format!("安装失败且自动回滚失败：{install_error}；{rollback}")
                }
                None => format!("安装失败，已自动回滚：{install_error}"),
            }));
        }
        applied_targets.insert(change.target.clone());
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
    let result = (|| {
        let mut files = Vec::with_capacity(changes.len());
        for (index, change) in changes.iter().enumerate() {
            if change.target.exists() {
                let backup_file = format!("{index:04}.bin");
                let backup_path = backup_dir.join(&backup_file);
                copy_backup_contents(&change.target, &backup_path)?;
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
        Ok(backup_dir.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&backup_dir);
    }
    result
}

fn copy_backup_contents(source: &Path, destination: &Path) -> AppResult<()> {
    let contents = fs::read(source).map_err(|error| {
        AppError::Other(format!(
            "读取 Claude 原文件用于备份失败 {}: {error}",
            source.display()
        ))
    })?;
    atomic_write(destination, &contents).map_err(|error| {
        AppError::Other(format!(
            "写入中文化备份失败 {}: {error}",
            destination.display()
        ))
    })
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
    restore_backup_dir_filtered(backup_dir, install, locale_paths, None)
}

fn restore_backup_targets(
    backup_dir: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
    targets: &HashSet<PathBuf>,
) -> AppResult<usize> {
    restore_backup_dir_filtered(backup_dir, install, locale_paths, Some(targets))
}

fn restore_backup_dir_filtered(
    backup_dir: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
    targets: Option<&HashSet<PathBuf>>,
) -> AppResult<usize> {
    let manifest: LocalizationBackupManifest =
        serde_json::from_slice(&fs::read(backup_dir.join("manifest.json"))?)?;
    validate_backup_manifest(&manifest, install)?;
    let mut restored = 0;
    for file in &manifest.files {
        if targets.is_some_and(|targets| !targets.contains(&file.target)) {
            continue;
        }
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
        restored += 1;
    }
    Ok(restored)
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
        let mut takeown = Command::new(windows_system_executable("takeown.exe"));
        takeown
            .args(["/F"])
            .arg(path)
            .args(["/A", "/R", "/D", "Y"]);
        hide_console_window(&mut takeown);
        let takeown_output = takeown.output()?;
        if !takeown_output.status.success() {
            return Err(AppError::Config(format!(
                "无法接管 Claude Desktop 资源目录: {}",
                command_failure_summary(&takeown_output)
            )));
        }

        // Grant the built-in Administrators group by SID so this works on every
        // Windows display language. The localization worker itself is elevated.
        let mut icacls = Command::new(windows_system_executable("icacls.exe"));
        icacls
            .arg(path)
            .args([
                "/grant",
                "*S-1-5-32-544:(OI)(CI)F",
                "/T",
                "/C",
                "/Q",
            ]);
        hide_console_window(&mut icacls);
        let icacls_output = icacls.output()?;
        if !icacls_output.status.success() {
            return Err(AppError::Config(format!(
                "无法授予 Claude Desktop 资源目录写入权限: {}",
                command_failure_summary(&icacls_output)
            )));
        }
        verify_write_access(path)?;
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}

#[cfg(windows)]
fn verify_write_access(resources: &Path) -> AppResult<()> {
    for directory in [
        resources.to_path_buf(),
        resources.join("ion-dist").join("i18n"),
        resources
            .join("ion-dist")
            .join("i18n")
            .join("statsig"),
        resources
            .join("ion-dist")
            .join("assets")
            .join("v1"),
    ] {
        if !directory.is_dir() {
            continue;
        }
        let probe = directory.join(format!(
            ".claude-switcher-write-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .map_err(|error| {
                AppError::Config(format!(
                    "Claude Desktop 目录仍不可写 {}: {error}",
                    directory.display()
                ))
            })?;
        drop(file);
        fs::remove_file(&probe).map_err(|error| {
            AppError::Other(format!(
                "无法清理 Claude Desktop 写入探针 {}: {error}",
                probe.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn command_failure_summary(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = format!("{} {}", stdout.trim(), stderr.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if summary.is_empty() {
        format!("exit {}", output.status)
    } else {
        summary.chars().take(800).collect()
    }
}

fn stop_claude() {
    #[cfg(windows)]
    {
        let mut command = Command::new(windows_system_executable("taskkill.exe"));
        command.args(["/IM", "Claude.exe", "/T", "/F"]);
        hide_console_window(&mut command);
        let _ = command.status();
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

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn pack_archive(extra_entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for (name, content) in [
            ("source/resources/frontend-zh-CN.json", b"{}".as_slice()),
            (
                "source/resources/frontend-hardcoded-zh-CN.json",
                b"[]".as_slice(),
            ),
            ("source/resources/desktop-zh-CN.json", b"{}".as_slice()),
            ("source/resources/statsig-zh-CN.json", b"{}".as_slice()),
            (
                "source/resources/manifest.json",
                br#"{"language":"zh-CN"}"#.as_slice(),
            ),
            (
                "source/resources/release.json",
                br#"{"repo":"javaht/claude-desktop-zh-cn","release":"1.2.3"}"#.as_slice(),
            ),
        ]
        .into_iter()
        .chain(extra_entries.iter().copied())
        {
            writer.start_file(name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

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
    fn upstream_revision_accepts_release_tags_and_commit_shas() {
        assert!(is_valid_upstream_revision("1.4.3"));
        assert!(is_valid_upstream_revision(
            "9cbc72bd4d937d2ef398f80f5ee90732eb93d533"
        ));
        assert!(!is_valid_upstream_revision("../evil"));
        assert!(!is_valid_upstream_revision("1.4.3/../../x"));
        assert!(!is_valid_upstream_revision(""));
    }

    #[test]
    fn appx_layout_prefers_nested_app_resources() {
        let root = tempdir().unwrap();
        let app_path = root.path().join("Claude_AppX");
        fs::create_dir_all(app_path.join("app").join("resources")).unwrap();
        fs::create_dir_all(app_path.join("resources")).unwrap();
        fs::write(app_path.join("app").join("claude.exe"), b"exe").unwrap();

        let install = install_from_appx_path(app_path.clone(), "1.24012.9.0", false).unwrap();

        assert_eq!(install.resources_path, app_path.join("app").join("resources"));
        assert_eq!(install.exe_path, app_path.join("app").join("Claude.exe"));
    }

    #[test]
    fn appx_layout_keeps_legacy_root_resources_compatible() {
        let root = tempdir().unwrap();
        let app_path = root.path().join("Claude_Legacy");
        fs::create_dir_all(app_path.join("resources")).unwrap();
        fs::write(app_path.join("Claude.exe"), b"exe").unwrap();

        let install = install_from_appx_path(app_path.clone(), "1.0.0", true).unwrap();

        assert_eq!(install.resources_path, app_path.join("resources"));
        assert!(install.multiple_installs);
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

    #[test]
    fn rollback_restores_only_files_written_before_failure() {
        let root = tempdir().unwrap();
        let app_path = root.path().join("Claude");
        let resources_path = app_path.join("resources");
        let assets_path = resources_path.join("ion-dist").join("assets").join("v1");
        fs::create_dir_all(&assets_path).unwrap();
        let first = assets_path.join("first.js");
        let second = assets_path.join("second.js");
        fs::write(&first, b"first-original").unwrap();
        fs::write(&second, b"second-original").unwrap();
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
                target: first.clone(),
                bytes: b"first-patched".to_vec(),
            },
            FileChange {
                target: second.clone(),
                bytes: b"second-patched".to_vec(),
            },
        ];
        let backup = create_backup(&root.path().join("backups"), &install, &changes).unwrap();
        fs::write(&first, b"first-patched").unwrap();
        fs::write(&second, b"second-external-change").unwrap();
        let targets = HashSet::from([first.clone()]);

        let restored =
            restore_backup_targets(&backup, &install, &[], &targets).unwrap();

        assert_eq!(restored, 1);
        assert_eq!(fs::read(first).unwrap(), b"first-original");
        assert_eq!(fs::read(second).unwrap(), b"second-external-change");
    }

    #[test]
    fn backup_copy_writes_contents_without_source_file_attributes() {
        let root = tempdir().unwrap();
        let source = root.path().join("encrypted-source.bin");
        let destination = root.path().join("backup").join("0000.bin");
        fs::write(&source, b"original").unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions).unwrap();

        copy_backup_contents(&source, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert!(!fs::metadata(&destination).unwrap().permissions().readonly());
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(source, permissions).unwrap();
    }

    #[test]
    fn failed_backup_removes_incomplete_backup_directory() {
        let root = tempdir().unwrap();
        let app_path = root.path().join("Claude");
        let existing_file = app_path.join("resources").join("bundle.js");
        let invalid_source = app_path.join("resources").join("directory-target");
        fs::create_dir_all(existing_file.parent().unwrap()).unwrap();
        fs::write(&existing_file, b"original").unwrap();
        fs::create_dir_all(&invalid_source).unwrap();
        let install = LocalizationInstall {
            kind: "appx".to_string(),
            app_path,
            resources_path: root.path().join("Claude").join("resources"),
            exe_path: root.path().join("Claude.exe"),
            version: "1.2.3".to_string(),
            multiple_installs: false,
        };
        let changes = vec![
            FileChange {
                target: existing_file,
                bytes: b"patched".to_vec(),
            },
            FileChange {
                target: invalid_source,
                bytes: b"invalid".to_vec(),
            },
        ];
        let backup_root = root.path().join("backups");

        assert!(create_backup(&backup_root, &install, &changes).is_err());

        let version_dir = backup_root.join("1.2.3");
        let leftovers = fs::read_dir(version_dir).unwrap().count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn downloaded_archive_extracts_only_allowlisted_resource_files() {
        let root = tempdir().unwrap();
        let archive = pack_archive(&[
            ("source/scripts/install_windows.ps1", b"Write-Host unsafe"),
            ("source/install-windows.bat", b"powershell.exe"),
        ]);

        extract_pack_archive(&archive, root.path()).unwrap();

        assert!(root.path().join("resources").join(FRONTEND_FILE).is_file());
        assert!(!root.path().join("scripts").exists());
        assert!(!root.path().join("install-windows.bat").exists());
        assert_eq!(validate_downloaded_metadata(root.path()).unwrap(), "1.2.3");
        validate_pack(root.path()).unwrap();
    }

    #[test]
    fn downloaded_archive_rejects_unsafe_paths() {
        let root = tempdir().unwrap();
        let archive = pack_archive(&[("../resources/ignored.json", b"{}")]);

        let error = extract_pack_archive(&archive, root.path()).unwrap_err();

        assert!(error.to_string().contains("不安全路径"));
    }

    #[test]
    fn downloaded_archive_requires_every_metadata_file() {
        let root = tempdir().unwrap();
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(
                "source/resources/frontend-zh-CN.json",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"{}").unwrap();
        let archive = writer.finish().unwrap().into_inner();

        let error = extract_pack_archive(&archive, root.path()).unwrap_err();

        assert!(error.to_string().contains("缺少文件"));
    }

    #[test]
    fn downloaded_metadata_rejects_wrong_repository() {
        let root = tempdir().unwrap();
        let archive = pack_archive(&[]);
        extract_pack_archive(&archive, root.path()).unwrap();
        fs::write(
            root.path().join("resources").join(RELEASE_FILE),
            br#"{"repo":"attacker/repository","release":"1.2.3"}"#,
        )
        .unwrap();

        let error = validate_downloaded_metadata(root.path()).unwrap_err();

        assert!(error.to_string().contains("仓库来源不匹配"));
    }

    #[test]
    fn downloaded_pack_promotion_replaces_existing_cache() {
        let root = tempdir().unwrap();
        let stage = root.path().join(".stage");
        let target = root.path().join("revision");
        fs::create_dir_all(&stage).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(stage.join("new"), b"new").unwrap();
        fs::write(target.join("old"), b"old").unwrap();

        promote_downloaded_pack(&stage, &target).unwrap();

        assert!(target.join("new").is_file());
        assert!(!target.join("old").exists());
        assert!(!stage.exists());
    }

    #[test]
    fn upstream_download_allows_only_expected_https_hosts() {
        assert!(upstream_url_allowed(
            &reqwest::Url::parse("https://api.github.com/repos/example").unwrap()
        ));
        assert!(upstream_url_allowed(
            &reqwest::Url::parse("https://codeload.github.com/example/archive.zip").unwrap()
        ));
        assert!(!upstream_url_allowed(
            &reqwest::Url::parse("http://api.github.com/repos/example").unwrap()
        ));
        assert!(!upstream_url_allowed(
            &reqwest::Url::parse("https://github.com.attacker.invalid/archive.zip").unwrap()
        ));
    }

    #[test]
    fn invalid_download_metadata_falls_back_to_local_source() {
        let root = tempdir().unwrap();
        fs::write(root.path().join(PACK_INFO_FILE), b"{\"source\":\"github\"}").unwrap();

        let info = pack_info(root.path().to_str(), false).unwrap();

        assert_eq!(info.source, "local");
        assert!(!info.valid);
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
