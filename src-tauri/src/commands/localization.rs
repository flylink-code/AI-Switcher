//! Unified localization status and safe installers for Claude Code editors.
//!
//! Claude Desktop resource localization remains in `desktop_localization`. This
//! module only installs the public Claude Code plugin and the VS Code/Cursor
//! patch helper; it never runs third-party patch scripts or edits extensions.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::backup::{backup_file_named, DEFAULT_BACKUP_KEEP};
use crate::commands::tools::get_claude_code_version;
use crate::config::{get_app_config_dir, get_claude_settings_path, read_json_file, write_json_file};
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const CLAUDE_PLUGIN_ID: &str = "claude-code-zh-cn@claude-code-zh-cn";
const CLAUDE_MARKETPLACE_REPOSITORY: &str = "https://github.com/taekchef/claude-code-zh-cn";
const CLAUDE_CODE_LOCALIZATION_REPOSITORY: &str = "taekchef/claude-code-zh-cn";
const EDITOR_LOCALIZATION_REPOSITORY: &str = "shanjiancaofu/claude-code-vscode-zh-cn";
const DESKTOP_LOCALIZATION_REPOSITORY: &str = "javaht/claude-desktop-zh-cn";
const MAX_GITHUB_RELEASE_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_HELPER_VSIX_BYTES: u64 = 20 * 1024 * 1024;
const PATCH_HELPER_EXTENSION: &str = "shanjiancaofu.claude-code-zh-cn-patch-helper";
const CLAUDE_EXTENSION_PREFIX: &str = "anthropic.claude-code-";
const PATCH_HELPER_PREFIX: &str = "shanjiancaofu.claude-code-zh-cn-patch-helper-";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationHubStatus {
    pub claude_code: ClaudeCodeLocalizationStatus,
    pub editors: Vec<EditorLocalizationStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationUpstreamStatus {
    pub checked_at: i64,
    pub claude_code: LocalizationUpstreamRelease,
    pub editor: LocalizationUpstreamRelease,
    pub desktop: LocalizationUpstreamRelease,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationUpstreamRelease {
    pub repository: String,
    pub available: bool,
    pub version: Option<String>,
    pub published_at: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeLocalizationStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub executable_path: Option<String>,
    pub plugin_version: Option<String>,
    pub plugin_enabled: bool,
    pub settings_configured: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLocalizationStatus {
    pub id: String,
    pub label: String,
    pub editor_detected: bool,
    pub editor_cli_path: Option<String>,
    pub claude_extension_path: Option<String>,
    pub helper_installed: bool,
    pub helper_version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
struct EditorDefinition {
    id: &'static str,
    label: &'static str,
    extension_dir: PathBuf,
    command_name: &'static str,
    cli_candidates: Vec<PathBuf>,
}

#[tauri::command]
pub async fn get_localization_hub_status() -> AppResult<LocalizationHubStatus> {
    let code_info = get_claude_code_version(Some(false)).await?;
    let settings = read_json_file::<Value>(&get_claude_settings_path())?.unwrap_or(Value::Object(Map::new()));
    let plugin_enabled = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get(CLAUDE_PLUGIN_ID))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let settings_configured = settings.get("language").and_then(Value::as_str) == Some("Chinese")
        && settings.get("spinnerTipsEnabled").and_then(Value::as_bool) == Some(true);
    let spinner_verbs_invalid = settings.get("spinnerVerbs").is_some_and(Value::is_array);
    let plugin_version = installed_zh_cn_plugin_version();
    let claude_code = ClaudeCodeLocalizationStatus {
        installed: code_info.installed,
        version: code_info.current_version,
        executable_path: code_info.executable_path,
        plugin_version: plugin_version.clone(),
        plugin_enabled,
        settings_configured,
        message: if !code_info.installed {
            "未检测到 Claude Code".to_string()
        } else if spinner_verbs_invalid {
            "settings.json 中 spinnerVerbs 格式无效（写成了数组），会导致 Claude Code 整份设置失效；请重新执行「安装中文」以自动修复".to_string()
        } else if plugin_enabled && plugin_version.is_some() && settings_configured {
            format!(
                "中文插件 {} 与基础设置已启用",
                plugin_version.as_deref().unwrap_or("")
            )
        } else if plugin_enabled || plugin_version.is_some() {
            "中文插件已安装，可更新或补全基础设置".to_string()
        } else {
            "可安装中文插件并启用基础中文设置".to_string()
        },
    };
    Ok(LocalizationHubStatus {
        claude_code,
        editors: editor_definitions().into_iter().map(editor_status).collect(),
    })
}

/// Check public upstream releases on demand. This never downloads a pack or
/// changes local installation state; an unavailable upstream is returned as a
/// visible status so the localization page keeps working offline.
#[tauri::command]
pub async fn check_localization_upstream(
    state: tauri::State<'_, AppState>,
) -> AppResult<LocalizationUpstreamStatus> {
    let extra_hosts = helper_mirror_hosts(&state);
    let extra_hosts_for_redirect = extra_hosts.clone();
    let settings = crate::commands::system::get_update_mirror_settings(state.clone())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("AI-Switcher localization-upstream-check")
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 8 {
                attempt.error("中文化上游检查重定向次数过多")
            } else if helper_download_url_allowed(attempt.url(), &extra_hosts_for_redirect) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| AppError::Other(format!("创建中文化上游检查客户端失败: {error}")))?;
    let (claude_code, editor, desktop) = tokio::join!(
        fetch_latest_release(&client, CLAUDE_CODE_LOCALIZATION_REPOSITORY, &settings, &extra_hosts),
        fetch_latest_release(&client, EDITOR_LOCALIZATION_REPOSITORY, &settings, &extra_hosts),
        fetch_latest_release(&client, DESKTOP_LOCALIZATION_REPOSITORY, &settings, &extra_hosts),
    );
    Ok(LocalizationUpstreamStatus {
        checked_at: chrono::Utc::now().timestamp_millis(),
        claude_code: claude_code.unwrap_or_else(|error| {
            upstream_release_unavailable(CLAUDE_CODE_LOCALIZATION_REPOSITORY, error)
        }),
        editor: editor.unwrap_or_else(|error| {
            upstream_release_unavailable(EDITOR_LOCALIZATION_REPOSITORY, error)
        }),
        desktop: desktop.unwrap_or_else(|error| {
            upstream_release_unavailable(DESKTOP_LOCALIZATION_REPOSITORY, error)
        }),
    })
}

async fn fetch_latest_release(
    client: &reqwest::Client,
    repository: &str,
    settings: &crate::commands::system::UpdateMirrorSettings,
    extra_hosts: &[String],
) -> AppResult<LocalizationUpstreamRelease> {
    let release = fetch_github_release_json(client, repository, settings, extra_hosts).await?;
    let version = normalize_release_tag(&release.tag_name)
        .ok_or_else(|| AppError::Config(format!("{repository} 返回了无效的 Release 标签")))?;
    Ok(LocalizationUpstreamRelease {
        repository: repository.to_string(),
        available: true,
        version: Some(version.clone()),
        published_at: release.published_at,
        message: format!("已检测到线上最新版 {version}"),
    })
}

fn github_latest_release_url(repository: &str) -> String {
    format!("https://api.github.com/repos/{repository}/releases/latest")
}

async fn fetch_github_release_json(
    client: &reqwest::Client,
    repository: &str,
    settings: &crate::commands::system::UpdateMirrorSettings,
    extra_hosts: &[String],
) -> AppResult<GitHubReleaseResponse> {
    let urls = github_asset_download_urls(&github_latest_release_url(repository), settings);
    let mut last_error = None;
    for url in urls {
        match fetch_github_release_json_from_url(client, repository, &url, extra_hosts).await {
            Ok(release) => return Ok(release),
            Err(error) => {
                log::warn!("github latest release failed for {url}: {error}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::Other(format!("连接 {repository} 失败"))
    }))
}

async fn fetch_github_release_json_from_url(
    client: &reqwest::Client,
    repository: &str,
    url: &str,
    extra_hosts: &[String],
) -> AppResult<GitHubReleaseResponse> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| AppError::Config(format!("{repository} 上游地址无效: {error}")))?;
    if !helper_download_url_allowed(&parsed, extra_hosts) {
        return Err(AppError::Config(format!(
            "{repository} 上游地址不受信任: {parsed}"
        )));
    }
    let response = client
        .get(parsed)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| AppError::Other(format!("连接 {repository} 失败: {error}")))?;
    if !helper_download_url_allowed(response.url(), extra_hosts) {
        return Err(AppError::Config(format!(
            "{repository} 被重定向到不受信任的地址: {}",
            response.url()
        )));
    }
    if !response.status().is_success() {
        return Err(AppError::Other(format!(
            "{repository} 返回 HTTP {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_GITHUB_RELEASE_METADATA_BYTES)
    {
        return Err(AppError::Config(format!(
            "{repository} Release 元数据超过 1 MB 限制"
        )));
    }
    let body = read_limited_body(response, MAX_GITHUB_RELEASE_METADATA_BYTES)
        .await
        .map_err(|error| AppError::Other(format!("读取 {repository} Release 元数据失败: {error}")))?;
    serde_json::from_slice(&body)
        .map_err(|error| AppError::Config(format!("{repository} Release 元数据无效: {error}")))
}

fn normalize_release_tag(tag: &str) -> Option<String> {
    let version = tag.trim().trim_start_matches('v');
    (!version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !version.contains(".."))
    .then(|| version.to_string())
}

fn upstream_release_unavailable(repository: &str, error: AppError) -> LocalizationUpstreamRelease {
    LocalizationUpstreamRelease {
        repository: repository.to_string(),
        available: false,
        version: None,
        published_at: None,
        message: format!("线上检查失败：{error}"),
    }
}

#[tauri::command]
pub async fn install_claude_code_localization() -> AppResult<String> {
    let executable = require_native_claude_executable().await?;
    run_claude_plugin_command(&executable, ["plugin", "marketplace", "add", "--scope", "user", CLAUDE_MARKETPLACE_REPOSITORY])?;
    run_claude_plugin_command(&executable, ["plugin", "install", CLAUDE_PLUGIN_ID, "--scope", "user"])?;
    merge_claude_code_chinese_settings()?;
    Ok("Claude Code 中文插件已安装，并已启用中文基础设置".to_string())
}

#[tauri::command]
pub async fn update_claude_code_localization() -> AppResult<String> {
    let executable = require_native_claude_executable().await?;
    if installed_zh_cn_plugin_version().is_some() || zh_cn_plugin_enabled() {
        crate::claude_plugins::update_plugin(Path::new(&executable), CLAUDE_PLUGIN_ID)?;
        merge_claude_code_chinese_settings()?;
        let version = installed_zh_cn_plugin_version().unwrap_or_else(|| "latest".to_string());
        return Ok(format!("Claude Code 中文插件已更新到 {version}"));
    }
    install_claude_code_localization().await
}

#[tauri::command]
pub async fn uninstall_claude_code_localization() -> AppResult<String> {
    let executable = require_native_claude_executable().await?;
    crate::claude_plugins::uninstall_plugin(Path::new(&executable), CLAUDE_PLUGIN_ID)?;
    remove_chinese_language_if_set()?;
    Ok("Claude Code 中文插件已卸载".to_string())
}

#[tauri::command]
pub async fn install_editor_localization_helper(
    editor: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    install_or_update_editor_helper(editor, true, state).await
}

#[tauri::command]
pub async fn update_editor_localization_helper(
    editor: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    install_or_update_editor_helper(editor, false, state).await
}

#[tauri::command]
pub fn uninstall_editor_localization_helper(editor: String) -> AppResult<String> {
    let status = editor_status(require_editor_definition(&editor)?);
    let cli = require_editor_cli(&status)?;
    let output = run_command(Path::new(&cli), &["--uninstall-extension", PATCH_HELPER_EXTENSION])?;
    if output.status.success() {
        return Ok(format!("{} 中文补丁助手已卸载；请重载编辑器窗口", status.label));
    }
    Err(AppError::Other(format!(
        "卸载中文补丁助手失败: {}",
        command_detail(&output)
    )))
}

async fn install_or_update_editor_helper(
    editor: String,
    require_official: bool,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let status = editor_status(require_editor_definition(&editor)?);
    if require_official && status.claude_extension_path.is_none() {
        return Err(AppError::Config(format!(
            "{} 未检测到 Claude Code for VS Code 扩展",
            status.label
        )));
    }
    let cli = PathBuf::from(require_editor_cli(&status)?);
    // Cursor (and Open VSX) does not list this helper; VS Code Marketplace often does.
    // Try the gallery first for VS Code, then always fall back to the GitHub .vsix.
    if editor == "vscode" && install_helper_from_gallery(&cli).is_ok() {
        return Ok(format!(
            "{} 中文补丁助手已安装；请在编辑器命令面板运行 Apply Patch 并重载窗口",
            status.label
        ));
    }
    let vsix = download_helper_vsix(&state).await?;
    install_helper_from_vsix(&cli, &vsix)?;
    Ok(format!(
        "{} 中文补丁助手已从 GitHub 安装；请在编辑器命令面板运行 Apply Patch 并重载窗口",
        status.label
    ))
}

fn merge_claude_code_chinese_settings() -> AppResult<()> {
    let path = get_claude_settings_path();
    if path.is_file() {
        backup_file_named(&path, "claude-code-zh-cn-settings", DEFAULT_BACKUP_KEEP)?;
    }
    let mut settings = read_json_file::<Value>(&path)?.unwrap_or(Value::Object(Map::new()));
    let object = settings.as_object_mut()
        .ok_or_else(|| AppError::Config("Claude Code settings.json 必须是 JSON 对象".to_string()))?;
    object.insert("language".to_string(), Value::String("Chinese".to_string()));
    object.insert("spinnerTipsEnabled".to_string(), Value::Bool(true));
    normalize_spinner_verbs(object);
    write_json_file(&path, &settings)
}

fn remove_chinese_language_if_set() -> AppResult<()> {
    let path = get_claude_settings_path();
    if !path.is_file() {
        return Ok(());
    }
    backup_file_named(&path, "claude-code-zh-cn-settings", DEFAULT_BACKUP_KEEP)?;
    let mut settings = read_json_file::<Value>(&path)?.unwrap_or(Value::Object(Map::new()));
    let Some(object) = settings.as_object_mut() else {
        return Ok(());
    };
    if remove_chinese_language_key(object) {
        write_json_file(&path, &settings)?;
    }
    Ok(())
}

fn remove_chinese_language_key(settings: &mut Map<String, Value>) -> bool {
    if settings.get("language").and_then(Value::as_str) == Some("Chinese") {
        settings.remove("language");
        true
    } else {
        false
    }
}

fn zh_cn_plugin_enabled() -> bool {
    read_json_file::<Value>(&get_claude_settings_path())
        .ok()
        .flatten()
        .and_then(|settings| {
            settings
                .get("enabledPlugins")
                .and_then(Value::as_object)
                .and_then(|plugins| plugins.get(CLAUDE_PLUGIN_ID))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

fn installed_zh_cn_plugin_version() -> Option<String> {
    crate::claude_plugins::list_plugins_snapshot()
        .ok()?
        .plugins
        .into_iter()
        .find(|plugin| plugin.plugin_id == CLAUDE_PLUGIN_ID && plugin.installed)
        .and_then(|plugin| plugin.version)
}

async fn require_native_claude_executable() -> AppResult<String> {
    let info = get_claude_code_version(Some(false)).await?;
    let executable = info.executable_path
        .ok_or_else(|| AppError::Config("未检测到 Claude Code，无法管理中文插件".to_string()))?;
    if info.environment == "wsl" {
        return Err(AppError::Config("请在 WSL 终端中管理 Claude Code 中文插件".to_string()));
    }
    Ok(executable)
}

fn require_editor_definition(editor: &str) -> AppResult<EditorDefinition> {
    editor_definitions()
        .into_iter()
        .find(|definition| definition.id == editor)
        .ok_or_else(|| AppError::Config("不支持的编辑器".to_string()))
}

fn require_editor_cli(status: &EditorLocalizationStatus) -> AppResult<String> {
    status.editor_cli_path.clone().ok_or_else(|| {
        AppError::Config(format!("未找到 {} 命令行工具，无法管理补丁助手", status.label))
    })
}

/// Claude Code expects `spinnerVerbs` as `{ mode, verbs }`. Some Chinese tip packs
/// write a bare string array, which makes the whole settings file fail validation
/// ("Expected object, but received array") and disables permission rules.
fn normalize_spinner_verbs(settings: &mut Map<String, Value>) {
    let Some(Value::Array(verbs)) = settings.get("spinnerVerbs").cloned() else {
        return;
    };
    let verbs: Vec<Value> = verbs
        .into_iter()
        .filter(|value| value.is_string())
        .collect();
    let mut object = Map::new();
    object.insert("mode".to_string(), Value::String("replace".to_string()));
    object.insert("verbs".to_string(), Value::Array(verbs));
    settings.insert("spinnerVerbs".to_string(), Value::Object(object));
}

fn run_claude_plugin_command<const N: usize>(executable: &str, args: [&str; N]) -> AppResult<()> {
    let output = run_command(Path::new(executable), &args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::Other(format!("Claude Code 插件命令失败: {}", command_detail(&output))))
}

fn install_helper_from_gallery(cli: &Path) -> AppResult<()> {
    let output = run_command(cli, &["--install-extension", PATCH_HELPER_EXTENSION, "--force"])?;
    if output.status.success() && !gallery_extension_missing(&output) {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "从编辑器市场安装中文补丁助手失败: {}",
        command_detail(&output)
    )))
}

fn install_helper_from_vsix(cli: &Path, vsix: &Path) -> AppResult<()> {
    let vsix = vsix.to_string_lossy();
    let output = run_command(cli, &["--install-extension", vsix.as_ref(), "--force"])?;
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "安装中文补丁助手失败: {}",
        command_detail(&output)
    )))
}

fn gallery_extension_missing(output: &std::process::Output) -> bool {
    let detail = command_detail(output).to_ascii_lowercase();
    detail.contains("not found") || detail.contains("failed installing")
}

async fn download_helper_vsix(state: &tauri::State<'_, AppState>) -> AppResult<PathBuf> {
    let extra_hosts = helper_mirror_hosts(state);
    let extra_hosts_for_redirect = extra_hosts.clone();
    let settings = crate::commands::system::get_update_mirror_settings(state.clone())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .user_agent("AI-Switcher localization-helper-vsix")
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 8 {
                attempt.error("中文补丁助手下载重定向次数过多")
            } else if helper_download_url_allowed(attempt.url(), &extra_hosts_for_redirect) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| AppError::Other(format!("创建中文补丁助手下载客户端失败: {error}")))?;
    let release = fetch_github_release_json(
        &client,
        EDITOR_LOCALIZATION_REPOSITORY,
        &settings,
        &extra_hosts,
    )
    .await?;
    let version = normalize_release_tag(&release.tag_name)
        .ok_or_else(|| AppError::Config("补丁助手仓库返回了无效的 Release 标签".to_string()))?;
    let asset = pick_helper_vsix_asset(&release.assets).ok_or_else(|| {
        AppError::Config(format!(
            "{EDITOR_LOCALIZATION_REPOSITORY} latest 没有可用的 .vsix 资源"
        ))
    })?;
    let download_url = reqwest::Url::parse(&asset.browser_download_url)
        .map_err(|error| AppError::Config(format!("补丁助手下载地址无效: {error}")))?;
    if !helper_download_url_allowed(&download_url, &extra_hosts) {
        return Err(AppError::Config(format!(
            "补丁助手下载地址不受信任: {download_url}"
        )));
    }
    let settings = crate::commands::system::get_update_mirror_settings(state.clone())?;
    let urls = github_asset_download_urls(&asset.browser_download_url, &settings);
    let bytes = download_vsix_with_fallbacks(&client, &urls, &extra_hosts).await?;
    let dir = get_app_config_dir().join("localization").join("editor-helper");
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("claude-code-zh-cn-patch-helper-{version}.vsix"));
    let stage = dir.join(format!(".download-{}.vsix", uuid::Uuid::new_v4().simple()));
    if let Err(error) = fs::write(&stage, &bytes) {
        let _ = fs::remove_file(&stage);
        return Err(error.into());
    }
    if target.exists() {
        fs::remove_file(&target)?;
    }
    if let Err(error) = fs::rename(&stage, &target) {
        let _ = fs::remove_file(&stage);
        return Err(error.into());
    }
    Ok(target)
}

async fn download_vsix_with_fallbacks(
    client: &reqwest::Client,
    urls: &[String],
    extra_hosts: &[String],
) -> AppResult<Vec<u8>> {
    let mut last_error = None;
    for url in urls {
        match download_one_vsix(client, url, extra_hosts).await {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                log::warn!("helper vsix download failed for {url}: {error}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::Other("下载中文补丁助手失败：没有可用的下载地址".into())
    }))
}

async fn download_one_vsix(
    client: &reqwest::Client,
    url: &str,
    extra_hosts: &[String],
) -> AppResult<Vec<u8>> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| AppError::Config(format!("补丁助手下载地址无效: {error}")))?;
    if !helper_download_url_allowed(&parsed, extra_hosts) {
        return Err(AppError::Config(format!("补丁助手下载地址不受信任: {parsed}")));
    }
    let response = client
        .get(parsed)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|error| AppError::Other(format!("下载中文补丁助手失败: {error}")))?;
    if !helper_download_url_allowed(response.url(), extra_hosts) {
        return Err(AppError::Config(format!(
            "中文补丁助手下载被重定向到不受信任的地址: {}",
            response.url()
        )));
    }
    if !response.status().is_success() {
        return Err(AppError::Other(format!(
            "下载中文补丁助手失败（HTTP {}）",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_HELPER_VSIX_BYTES)
    {
        return Err(AppError::Config("中文补丁助手超过 20 MB 限制".to_string()));
    }
    let bytes = read_limited_body(response, MAX_HELPER_VSIX_BYTES).await?;
    if !looks_like_vsix(&bytes) {
        return Err(AppError::Config("下载的补丁助手不是有效的 .vsix 文件".to_string()));
    }
    Ok(bytes)
}

async fn read_limited_body(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::Other(error.to_string()))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(AppError::Config(format!("下载内容超过 {max_bytes} 字节限制")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn github_asset_download_urls(
    direct_url: &str,
    settings: &crate::commands::system::UpdateMirrorSettings,
) -> Vec<String> {
    let mut urls = Vec::new();
    if settings.use_mirror {
        let base = settings.mirror_base.trim();
        if !base.is_empty() {
            let mirrored = format!("{}{direct_url}", if base.ends_with('/') { base.to_string() } else { format!("{base}/") });
            urls.push(mirrored);
        }
    }
    urls.push(direct_url.to_string());
    urls.dedup();
    urls
}

fn helper_mirror_hosts(state: &tauri::State<'_, AppState>) -> Vec<String> {
    let mut hosts = vec!["gh-proxy.com".to_string()];
    if let Ok(settings) = crate::commands::system::get_update_mirror_settings(state.clone()) {
        if let Ok(parsed) = reqwest::Url::parse(&settings.mirror_base) {
            if parsed.scheme() == "https" {
                if let Some(host) = parsed.host_str() {
                    if !hosts.iter().any(|existing| existing == host) {
                        hosts.push(host.to_string());
                    }
                }
            }
        }
    }
    hosts
}

fn pick_helper_vsix_asset(assets: &[GitHubReleaseAsset]) -> Option<&GitHubReleaseAsset> {
    assets.iter().find(|asset| {
        is_safe_vsix_name(&asset.name)
            && asset.size > 0
            && asset.size <= MAX_HELPER_VSIX_BYTES
    })
}

fn is_safe_vsix_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("claude-code-zh-cn-patch-helper-")
        && lower.ends_with(".vsix")
        && !name.contains("..")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn looks_like_vsix(bytes: &[u8]) -> bool {
    bytes.len() > 4 && bytes[0] == b'P' && bytes[1] == b'K'
}

fn helper_download_url_allowed(url: &reqwest::Url, extra_hosts: &[String]) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(host, "github.com" | "api.github.com" | "codeload.github.com")
        || host == "githubusercontent.com"
        || host.ends_with(".githubusercontent.com")
        || extra_hosts.iter().any(|allowed| allowed == host)
}

fn run_command(program: &Path, args: &[&str]) -> AppResult<std::process::Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = if requires_command_shell(program) {
            // Paths like `...\Microsoft VS Code\bin\code.cmd` contain spaces.
            // Pass a single /C string so cmd.exe does not split on spaces.
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C"]).arg(format_cmd_script_line(program, args));
            command
        } else {
            let mut command = Command::new(program);
            command.args(args);
            command
        };
        command.creation_flags(CREATE_NO_WINDOW);
        command.output().map_err(|error| AppError::Other(format!("启动命令失败: {error}")))
    }
    #[cfg(not(windows))]
    {
        Command::new(program).args(args).output()
            .map_err(|error| AppError::Other(format!("启动命令失败: {error}")))
    }
}

/// Build `call "program" arg1 arg2` for cmd.exe /C.
fn format_cmd_script_line(program: &Path, args: &[&str]) -> String {
    let mut line = format!("call {}", quote_cmd_arg(&program.to_string_lossy()));
    for arg in args {
        line.push(' ');
        line.push_str(&quote_cmd_arg(arg));
    }
    line
}

fn quote_cmd_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if !value.contains([' ', '\t', '"', '&', '|', '<', '>', '^', '%']) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn requires_command_shell(program: &Path) -> bool {
    program.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
}

fn command_detail(output: &std::process::Output) -> String {
    let text = if output.stderr.is_empty() { &output.stdout } else { &output.stderr };
    let decoded = String::from_utf8_lossy(text);
    let cleaned = strip_cli_noise(&decoded);
    if cleaned.is_empty() { format!("退出码 {}", output.status) } else { cleaned }
}

fn strip_cli_noise(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.contains("DeprecationWarning")
                && !line.contains("[DEP0169]")
                && !line.contains("--trace-deprecation")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn editor_definitions() -> Vec<EditorDefinition> {
    let home = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    vec![
        EditorDefinition {
            id: "vscode",
            label: "VS Code",
            extension_dir: home.join(".vscode").join("extensions"),
            command_name: "code",
            cli_candidates: vec![
                local.join("Programs").join("Microsoft VS Code").join("bin").join("code.cmd"),
                PathBuf::from(r"C:\Program Files\Microsoft VS Code\bin\code.cmd"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd"),
            ],
        },
        EditorDefinition {
            id: "cursor",
            label: "Cursor",
            extension_dir: home.join(".cursor").join("extensions"),
            command_name: "cursor",
            cli_candidates: vec![
                local.join("Programs").join("Cursor").join("resources").join("app").join("bin").join("cursor.cmd"),
                local.join("Programs").join("cursor").join("resources").join("app").join("bin").join("cursor.cmd"),
                local.join("Programs").join("Cursor").join("resources").join("app").join("codeBin").join("cursor.cmd"),
                local.join("Programs").join("cursor").join("resources").join("app").join("codeBin").join("cursor.cmd"),
                PathBuf::from(r"C:\Program Files\Cursor\resources\app\bin\cursor.cmd"),
                PathBuf::from(r"C:\Program Files\cursor\resources\app\bin\cursor.cmd"),
            ],
        },
    ]
}

fn editor_status(definition: EditorDefinition) -> EditorLocalizationStatus {
    let claude_extension = find_extension(&definition.extension_dir, CLAUDE_EXTENSION_PREFIX);
    let helper = find_extension(&definition.extension_dir, PATCH_HELPER_PREFIX);
    let helper_version = helper.as_deref().and_then(helper_version_from_path);
    let helper_installed = helper.is_some();
    let cli = resolve_editor_cli(&definition);
    let editor_detected = cli.is_some() || definition.extension_dir.is_dir();
    let message = if claude_extension.is_none() {
        "未检测到 Claude Code for VS Code 扩展".to_string()
    } else if cli.is_none() {
        format!("已检测到 Claude Code for VS Code 扩展，但未找到 {} 命令行工具；请在编辑器命令面板安装 Shell Command，或将命令加入 PATH", definition.label)
    } else if helper_installed {
        match helper_version.as_deref() {
            Some(version) => format!("中文补丁助手 {version} 已安装；请在编辑器命令面板确认应用补丁"),
            None => "中文补丁助手已安装；请在编辑器命令面板确认应用补丁".to_string(),
        }
    } else {
        "已检测到官方扩展，可安装中文补丁助手".to_string()
    };
    EditorLocalizationStatus {
        id: definition.id.to_string(),
        label: definition.label.to_string(),
        editor_detected,
        editor_cli_path: cli.map(|path| path.to_string_lossy().to_string()),
        claude_extension_path: claude_extension.map(|path| path.to_string_lossy().to_string()),
        helper_installed,
        helper_version,
        message,
    }
}

/// Resolve an editor command from its normal installation locations first and
/// then from PATH.  Cursor commonly installs only its user-level command, so
/// treating the extension directory as proof of a CLI used to produce a
/// misleading patch-helper error.
fn resolve_editor_cli(definition: &EditorDefinition) -> Option<PathBuf> {
    definition
        .cli_candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .or_else(|| find_cli_from_install_roots(definition))
        .or_else(|| find_command_on_path(definition.command_name))
}

fn find_cli_from_install_roots(definition: &EditorDefinition) -> Option<PathBuf> {
    for root in editor_install_roots(definition.id) {
        for candidate in cli_paths_for_install_root(definition.id, &root) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn cli_paths_for_install_root(editor_id: &str, root: &Path) -> Vec<PathBuf> {
    match editor_id {
        "vscode" => vec![
            root.join("bin").join("code.cmd"),
            root.join("bin").join("code.exe"),
        ],
        "cursor" => vec![
            root.join("resources").join("app").join("bin").join("cursor.cmd"),
            root.join("resources").join("app").join("bin").join("cursor.exe"),
            root.join("resources").join("app").join("codeBin").join("cursor.cmd"),
            root.join("resources").join("app").join("codeBin").join("cursor.exe"),
        ],
        _ => Vec::new(),
    }
}

#[cfg(windows)]
fn editor_install_roots(editor_id: &str) -> Vec<PathBuf> {
    let needles: &[&str] = match editor_id {
        "vscode" => &["Visual Studio Code"],
        "cursor" => &["Cursor"],
        _ => return Vec::new(),
    };
    let mut roots = Vec::new();
    for hive in [winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER), winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)] {
        for path in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(uninstall) = hive.open_subkey(path) else {
                continue;
            };
            for subkey_name in uninstall.enum_keys().flatten() {
                let Ok(subkey) = uninstall.open_subkey(&subkey_name) else {
                    continue;
                };
                let display_name: String = subkey.get_value("DisplayName").unwrap_or_default();
                if !needles.iter().any(|needle| display_name.contains(needle)) {
                    continue;
                }
                // Avoid matching unrelated "Cursor*" products by requiring exact-ish names.
                if editor_id == "cursor" && !display_name.to_ascii_lowercase().starts_with("cursor") {
                    continue;
                }
                let location: String = subkey.get_value("InstallLocation").unwrap_or_default();
                let location = location.trim().trim_end_matches(['\\', '/']);
                if location.is_empty() {
                    continue;
                }
                let root = PathBuf::from(location);
                if root.is_dir() {
                    roots.push(root);
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(not(windows))]
fn editor_install_roots(_editor_id: &str) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn find_command_on_path(command: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    // GUI-launched apps often miss the latest user PATH; also probe registry PATH.
    if let Some(path) = find_command_in_env_path(command) {
        return Some(path);
    }
    for name in [format!("{command}.cmd"), command.to_string()] {
        let mut where_command = Command::new("where.exe");
        where_command.arg(&name).creation_flags(CREATE_NO_WINDOW);
        let Ok(output) = where_command.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        if let Some(path) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .map(PathBuf::from)
            .find(|path| {
                path.is_file()
                    && !path
                        .components()
                        .any(|part| part.as_os_str() == "WindowsApps")
            })
        {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn find_command_in_env_path(command: &str) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(windows_user_path_dirs());
    let names = [
        format!("{command}.cmd"),
        format!("{command}.exe"),
        format!("{command}.bat"),
        command.to_string(),
    ];
    for dir in dirs {
        // WindowsApps execution aliases are stubs and break --install-extension.
        if dir.components().any(|part| part.as_os_str() == "WindowsApps") {
            continue;
        }
        for name in &names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_user_path_dirs() -> Vec<PathBuf> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let Ok(env) = hkcu.open_subkey("Environment") else {
        return Vec::new();
    };
    let Ok(value) = env.get_value::<String, _>("Path") else {
        return Vec::new();
    };
    let expanded = expand_windows_env(&value);
    std::env::split_paths(&expanded).collect()
}

#[cfg(windows)]
fn expand_windows_env(value: &str) -> String {
    let mut result = value.to_string();
    // Expand common %VAR% references used in user PATH.
    for (key, replacement) in [
        ("%USERPROFILE%", dirs::home_dir().map(|p| p.to_string_lossy().into_owned())),
        ("%LOCALAPPDATA%", dirs::data_local_dir().map(|p| p.to_string_lossy().into_owned())),
        ("%APPDATA%", dirs::config_dir().map(|p| p.to_string_lossy().into_owned())),
    ] {
        if let Some(replacement) = replacement {
            result = result.replace(key, &replacement);
            result = result.replace(&key.to_ascii_lowercase(), &replacement);
        }
    }
    result
}

#[cfg(not(windows))]
fn find_command_on_path(_command: &str) -> Option<PathBuf> {
    None
}

fn find_extension(root: &Path, prefix: &str) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(root).ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(prefix) && !name.contains(".obsolete")
            })
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn helper_version_from_path(path: &Path) -> Option<String> {
    if let Some(version) = read_package_json_version(path) {
        return Some(version);
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(PATCH_HELPER_PREFIX))
        .map(|suffix| suffix.trim_end_matches("-universal").trim().to_string())
        .filter(|version| !version.is_empty())
}

fn read_package_json_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_shell_is_only_used_for_command_scripts() {
        assert!(requires_command_shell(Path::new("C:/tools/code.cmd")));
        assert!(requires_command_shell(Path::new("C:/tools/install.BAT")));
        assert!(!requires_command_shell(Path::new("C:/tools/claude.exe")));
    }

    #[test]
    fn formats_cmd_script_line_with_quoted_spaces() {
        let line = format_cmd_script_line(
            Path::new(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd"),
            &["--install-extension", "shanjiancaofu.claude-code-zh-cn-patch-helper"],
        );
        assert_eq!(
            line,
            r#"call "C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd" --install-extension shanjiancaofu.claude-code-zh-cn-patch-helper"#
        );
    }

    #[test]
    fn builds_cli_paths_under_install_root() {
        let root = Path::new(r"F:\cursor");
        let paths = cli_paths_for_install_root("cursor", root);
        assert!(paths.iter().any(|p| p.ends_with(r"resources\app\bin\cursor.cmd") || p.ends_with("resources/app/bin/cursor.cmd")));
        let vscode = cli_paths_for_install_root("vscode", Path::new(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code"));
        assert!(vscode.iter().any(|p| p.ends_with("bin\\code.cmd") || p.ends_with("bin/code.cmd")));
    }

    #[cfg(windows)]
    #[test]
    fn resolves_local_editor_cli_when_installed() {
        let editors = editor_definitions();
        let vscode = editors.iter().find(|e| e.id == "vscode").unwrap();
        let cursor = editors.iter().find(|e| e.id == "cursor").unwrap();
        // This machine has user-level VS Code under Local\Programs.
        if PathBuf::from(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd").is_file() {
            let cli = resolve_editor_cli(vscode).expect("VS Code CLI");
            assert!(cli.to_string_lossy().contains("code"));
        }
        // Portable/custom Cursor install registered via Uninstall InstallLocation.
        if PathBuf::from(r"f:\cursor\resources\app\bin\cursor.cmd").is_file() {
            let cli = resolve_editor_cli(cursor).expect("Cursor CLI");
            assert!(cli.to_string_lossy().to_ascii_lowercase().contains("cursor"));
        }
    }

    #[test]
    fn finds_only_expected_extension_prefix() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("anthropic.claude-code-2.1.183")).unwrap();
        fs::create_dir(dir.path().join("other.claude-code-9.0.0")).unwrap();
        let found = find_extension(dir.path(), CLAUDE_EXTENSION_PREFIX).unwrap();
        assert!(found.ends_with("anthropic.claude-code-2.1.183"));
    }

    #[test]
    fn normalizes_bare_spinner_verbs_array_to_object() {
        let mut settings = Map::new();
        settings.insert(
            "spinnerVerbs".to_string(),
            Value::Array(vec![
                Value::String("思考中".to_string()),
                Value::String("编写中".to_string()),
                Value::Bool(true),
            ]),
        );
        normalize_spinner_verbs(&mut settings);
        let verbs = settings
            .get("spinnerVerbs")
            .and_then(Value::as_object)
            .expect("spinnerVerbs object");
        assert_eq!(verbs.get("mode").and_then(Value::as_str), Some("replace"));
        assert_eq!(
            verbs.get("verbs").and_then(Value::as_array).map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn leaves_valid_spinner_verbs_object_untouched() {
        let mut settings = Map::new();
        settings.insert(
            "spinnerVerbs".to_string(),
            serde_json::json!({
                "mode": "append",
                "verbs": ["Pondering"]
            }),
        );
        normalize_spinner_verbs(&mut settings);
        assert_eq!(
            settings.get("spinnerVerbs").and_then(|value| value.get("mode")).and_then(Value::as_str),
            Some("append")
        );
    }

    #[test]
    fn normalize_release_tag_removes_v_prefix_and_rejects_paths() {
        assert_eq!(normalize_release_tag("v2.14.0").as_deref(), Some("2.14.0"));
        assert_eq!(normalize_release_tag("1.4.7").as_deref(), Some("1.4.7"));
        assert!(normalize_release_tag("../main").is_none());
        assert!(normalize_release_tag("").is_none());
    }

    #[test]
    fn picks_safe_helper_vsix_and_rejects_paths() {
        let assets = vec![
            GitHubReleaseAsset {
                name: "../evil.vsix".to_string(),
                browser_download_url: "https://example.com/evil.vsix".to_string(),
                size: 12,
            },
            GitHubReleaseAsset {
                name: "SHA256SUMS".to_string(),
                browser_download_url: "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/SHA256SUMS".to_string(),
                size: 108,
            },
            GitHubReleaseAsset {
                name: "claude-code-zh-cn-patch-helper-0.1.2.vsix".to_string(),
                browser_download_url: "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/claude-code-zh-cn-patch-helper-0.1.2.vsix".to_string(),
                size: 3_653_342,
            },
        ];
        let picked = pick_helper_vsix_asset(&assets).expect("vsix");
        assert_eq!(picked.name, "claude-code-zh-cn-patch-helper-0.1.2.vsix");
        assert!(is_safe_vsix_name(&picked.name));
        assert!(!is_safe_vsix_name("../claude-code-zh-cn-patch-helper-0.1.2.vsix"));
        assert!(!looks_like_vsix(b"not-a-zip"));
        assert!(looks_like_vsix(b"PK\x03\x04payload"));
    }

    #[test]
    fn helper_download_allows_github_cdn_and_configured_mirrors() {
        let extra = vec!["gh-proxy.com".to_string()];
        let github = reqwest::Url::parse(
            "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix",
        )
        .unwrap();
        let api = reqwest::Url::parse(
            "https://api.github.com/repos/shanjiancaofu/claude-code-vscode-zh-cn/releases/latest",
        )
        .unwrap();
        let cdn = reqwest::Url::parse("https://release-assets.githubusercontent.com/file").unwrap();
        let mirror = reqwest::Url::parse(
            "https://gh-proxy.com/https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix",
        )
        .unwrap();
        let blocked = reqwest::Url::parse("https://example.com/a.vsix").unwrap();
        assert!(helper_download_url_allowed(&github, &extra));
        assert!(helper_download_url_allowed(&api, &extra));
        assert!(helper_download_url_allowed(&cdn, &extra));
        assert!(helper_download_url_allowed(&mirror, &extra));
        assert!(!helper_download_url_allowed(&blocked, &extra));
    }

    #[test]
    fn github_asset_urls_use_configured_mirror_first() {
        let settings = crate::commands::system::UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://gh-proxy.com/".to_string(),
        };
        let urls = github_asset_download_urls(
            "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix",
            &settings,
        );
        assert_eq!(
            urls[0],
            "https://gh-proxy.com/https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix"
        );
        assert_eq!(
            urls[1],
            "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix"
        );

        let direct_only = crate::commands::system::UpdateMirrorSettings {
            use_mirror: false,
            mirror_base: "https://gh-proxy.com/".to_string(),
        };
        let urls = github_asset_download_urls(
            "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix",
            &direct_only,
        );
        assert_eq!(urls, vec![
            "https://github.com/shanjiancaofu/claude-code-vscode-zh-cn/releases/download/v0.1.2/a.vsix".to_string()
        ]);
    }

    #[test]
    fn github_latest_api_urls_use_configured_mirror_first() {
        let settings = crate::commands::system::UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://gh-proxy.com/".to_string(),
        };
        let direct = github_latest_release_url("shanjiancaofu/claude-code-vscode-zh-cn");
        let urls = github_asset_download_urls(&direct, &settings);
        assert_eq!(
            urls[0],
            "https://gh-proxy.com/https://api.github.com/repos/shanjiancaofu/claude-code-vscode-zh-cn/releases/latest"
        );
        assert_eq!(
            urls[1],
            "https://api.github.com/repos/shanjiancaofu/claude-code-vscode-zh-cn/releases/latest"
        );

        let direct_only = crate::commands::system::UpdateMirrorSettings {
            use_mirror: false,
            mirror_base: "https://gh-proxy.com/".to_string(),
        };
        assert_eq!(
            github_asset_download_urls(&direct, &direct_only),
            vec![direct]
        );
    }

    #[test]
    fn strips_node_deprecation_noise_from_cli_output() {
        let cleaned = strip_cli_noise(
            "(node:12852) [DEP0169] DeprecationWarning: `url.parse()` behavior is not standardized\n(Use `Cursor --trace-deprecation ...` to show where the warning was created)\nExtension 'shanjiancaofu.claude-code-zh-cn-patch-helper' not found.\nFailed Installing Extensions: shanjiancaofu.claude-code-zh-cn-patch-helper\n",
        );
        assert_eq!(
            cleaned,
            "Extension 'shanjiancaofu.claude-code-zh-cn-patch-helper' not found.\nFailed Installing Extensions: shanjiancaofu.claude-code-zh-cn-patch-helper"
        );
    }

    #[test]
    fn helper_version_prefers_package_json_then_directory_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("shanjiancaofu.claude-code-zh-cn-patch-helper-0.1.1");
        fs::create_dir(&helper).unwrap();
        assert_eq!(
            helper_version_from_path(&helper).as_deref(),
            Some("0.1.1")
        );
        fs::write(
            helper.join("package.json"),
            r#"{"name":"helper","version":"0.1.2"}"#,
        )
        .unwrap();
        assert_eq!(
            helper_version_from_path(&helper).as_deref(),
            Some("0.1.2")
        );
    }

    #[test]
    fn strips_universal_suffix_from_helper_folder() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir
            .path()
            .join("shanjiancaofu.claude-code-zh-cn-patch-helper-0.1.2-universal");
        fs::create_dir(&helper).unwrap();
        assert_eq!(
            helper_version_from_path(&helper).as_deref(),
            Some("0.1.2")
        );
    }

    #[test]
    fn removes_only_chinese_language_key() {
        let mut settings = Map::new();
        settings.insert("language".to_string(), Value::String("Chinese".to_string()));
        settings.insert("spinnerTipsEnabled".to_string(), Value::Bool(true));
        assert!(remove_chinese_language_key(&mut settings));
        assert!(settings.get("language").is_none());
        assert_eq!(
            settings.get("spinnerTipsEnabled").and_then(Value::as_bool),
            Some(true)
        );

        let mut english = Map::new();
        english.insert("language".to_string(), Value::String("English".to_string()));
        assert!(!remove_chinese_language_key(&mut english));
        assert_eq!(
            english.get("language").and_then(Value::as_str),
            Some("English")
        );
    }
}
