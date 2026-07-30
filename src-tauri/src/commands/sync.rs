//! Safe synchronization target configuration and local-only previews.
//!
//! Target records never contain a remote password, private key, API key, or
//! Claude login state. A user must inspect a preview before a future push is
//! allowed to execute.

use std::fs;
use std::io::copy;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::atomic::{read_json_file, write_json_file};
use crate::config::paths::get_app_config_dir;
use crate::error::{AppError, AppResult};
use crate::process_util::apply_no_window;

const SYNC_TARGETS_VERSION: u8 = 1;
const SYNC_TARGETS_FILE: &str = "sync-targets.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncTargetKind { Wsl, Ssh }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncItem { ProviderPresets, Mcp, Prompts, Skills, SessionArchives }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathMapping { pub windows_path: String, pub remote_path: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTarget {
    pub id: String,
    pub name: String,
    pub kind: SyncTargetKind,
    #[serde(default)] pub wsl_distribution: Option<String>,
    #[serde(default)] pub ssh_host: Option<String>,
    #[serde(default)] pub ssh_port: Option<u16>,
    pub remote_root: String,
    #[serde(default)] pub path_mappings: Vec<PathMapping>,
    #[serde(default)] pub items: Vec<SyncItem>,
    #[serde(default)] pub last_synced_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncTargetsConfig { version: u8, targets: Vec<SyncTarget> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPreviewChange {
    pub item: SyncItem,
    pub source_path: String,
    pub remote_path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPreview {
    pub target: SyncTarget,
    pub changes: Vec<SyncPreviewChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushResult {
    pub target_id: String,
    pub archive_path: String,
    pub remote_path: String,
    pub bytes: u64,
}

fn config_path() -> PathBuf { get_app_config_dir().join(SYNC_TARGETS_FILE) }

fn load_targets() -> AppResult<SyncTargetsConfig> {
    Ok(read_json_file(&config_path())?.unwrap_or(SyncTargetsConfig {
        version: SYNC_TARGETS_VERSION, targets: Vec::new(),
    }))
}

fn validate_target(target: &SyncTarget) -> AppResult<()> {
    if target.name.trim().is_empty() { return Err(AppError::Config("同步目标名称不能为空".to_string())); }
    if !is_safe_posix_absolute_path(&target.remote_root) {
        return Err(AppError::Path("远端资料库路径必须是 POSIX 绝对路径，且不能包含 '..' 或反斜杠".to_string()));
    }
    match target.kind {
        SyncTargetKind::Wsl if target.wsl_distribution.as_deref().unwrap_or_default().trim().is_empty() => Err(AppError::Config("WSL 目标必须选择发行版".to_string())),
        SyncTargetKind::Ssh if target.ssh_host.as_deref().unwrap_or_default().trim().is_empty() => Err(AppError::Config("SSH 目标必须填写主机名".to_string())),
        _ => Ok(()),
    }
}

fn is_safe_posix_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && !value.contains('\r')
        && !value.contains('\n')
        && !value.split('/').any(|part| part == "..")
}

#[tauri::command]
pub fn list_sync_targets() -> AppResult<Vec<SyncTarget>> { Ok(load_targets()?.targets) }

#[tauri::command]
pub fn save_sync_target(mut target: SyncTarget) -> AppResult<SyncTarget> {
    target.id = if target.id.trim().is_empty() { Uuid::new_v4().to_string() } else { target.id };
    target.name = target.name.trim().to_string();
    target.remote_root = if target.remote_root == "/" {
        "/".to_string()
    } else {
        target.remote_root.trim_end_matches('/').to_string()
    };
    validate_target(&target)?;
    let mut config = load_targets()?;
    if let Some(existing) = config.targets.iter_mut().find(|item| item.id == target.id) { *existing = target.clone(); }
    else { config.targets.push(target.clone()); }
    config.version = SYNC_TARGETS_VERSION;
    write_json_file(&config_path(), &config)?;
    Ok(target)
}

#[tauri::command]
pub fn delete_sync_target(id: String) -> AppResult<()> {
    let mut config = load_targets()?;
    let original = config.targets.len();
    config.targets.retain(|target| target.id != id);
    if config.targets.len() == original { return Err(AppError::Config("未找到同步目标".to_string())); }
    write_json_file(&config_path(), &config)
}

#[tauri::command]
pub async fn discover_wsl_distributions() -> AppResult<Vec<String>> {
    tokio::task::spawn_blocking(|| {
        #[cfg(windows)]
        {
            let mut command = Command::new("wsl.exe");
            command.args(["--list", "--quiet"]);
            apply_no_window(&mut command);
            let output = command.output()
                .map_err(|error| AppError::Other(format!("无法调用 WSL: {error}")))?;
            if !output.status.success() { return Err(AppError::Config("WSL 未安装或没有可用发行版".to_string())); }
            Ok(String::from_utf8_lossy(&output.stdout).lines()
                .map(|line| line.trim_matches('\0').trim().to_string())
                .filter(|line| !line.is_empty()).collect())
        }
        #[cfg(not(windows))]
        { Ok(Vec::new()) }
    }).await.map_err(|error| AppError::Other(format!("WSL 检测任务异常结束: {error}")))?
}

#[tauri::command]
pub fn preview_sync(target_id: String) -> AppResult<SyncPreview> {
    let target = load_targets()?.targets.into_iter().find(|target| target.id == target_id)
        .ok_or_else(|| AppError::Config("未找到同步目标".to_string()))?;
    validate_target(&target)?;
    let root = get_app_config_dir();
    let mut changes = Vec::new();
    let add = |item: SyncItem, source: PathBuf, remote_name: &str, changes: &mut Vec<SyncPreviewChange>| {
        if source.exists() { changes.push(SyncPreviewChange { item, source_path: source.to_string_lossy().into_owned(), remote_path: format!("{}/{}", target.remote_root, remote_name), status: "create_or_replace".to_string() }); }
    };
    for item in &target.items {
        match item {
            SyncItem::ProviderPresets | SyncItem::Mcp | SyncItem::Prompts => add(item.clone(), root.join("app.db"), "app.db", &mut changes),
            SyncItem::Skills => add(item.clone(), crate::config::paths::get_claude_skills_dir(), "skills", &mut changes),
            SyncItem::SessionArchives => add(item.clone(), root.join("session-archives"), "session-archives", &mut changes),
        }
    }
    Ok(SyncPreview { target, changes, warnings: vec![
        "预览只列出 Windows 端应用管理数据；不会读取或传递 API Key、Claude 登录状态、远程密码或私钥。".to_string(),
        "执行同步前仍需显式确认；SSH 将使用系统 SSH Agent 或用户选择的密钥并校验主机指纹。".to_string(),
    ]})
}

/// Push a portable, sanitized library archive into the target's dedicated
/// `incoming/` directory. This is intentionally non-destructive: it never
/// replaces the remote active configuration or merges conflicts. A remote user
/// can inspect and import the archive explicitly after transfer.
#[tauri::command]
pub fn push_sync_archive(target_id: String) -> AppResult<SyncPushResult> {
    let mut config = load_targets()?;
    let index = config.targets.iter().position(|target| target.id == target_id)
        .ok_or_else(|| AppError::Config("未找到同步目标".to_string()))?;
    let target = config.targets[index].clone();
    validate_target(&target)?;
    let archive = crate::backup::export_library_backup()?;
    let archive_path = PathBuf::from(&archive.archive_path);
    let filename = archive_path.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Path("同步归档文件名无效".to_string()))?;
    if !filename.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')) {
        return Err(AppError::Path("同步归档文件名不安全".to_string()));
    }
    let remote_path = format!("{}/incoming/{filename}", target.remote_root.trim_end_matches('/'));
    stream_to_target(&target, &archive_path, &remote_path)?;
    config.targets[index].last_synced_at = Some(chrono::Utc::now().timestamp_millis());
    write_json_file(&config_path(), &config)?;
    Ok(SyncPushResult {
        target_id,
        archive_path: archive.archive_path,
        remote_path,
        bytes: fs::metadata(archive_path)?.len(),
    })
}

fn stream_to_target(target: &SyncTarget, archive: &Path, remote_path: &str) -> AppResult<()> {
    let remote_parent = Path::new(remote_path).parent()
        .ok_or_else(|| AppError::Path("远端同步路径无效".to_string()))?
        .to_string_lossy().replace('\\', "/");
    let script = format!(
        "set -eu; mkdir -p -- {}; tmp={}.part; cat > \"$tmp\"; mv -- \"$tmp\" {}",
        shell_quote(&remote_parent),
        shell_quote(remote_path),
        shell_quote(remote_path),
    );
    let mut command = match target.kind {
        SyncTargetKind::Wsl => {
            let distro = target.wsl_distribution.as_deref().unwrap_or_default();
            let mut command = Command::new("wsl.exe");
            command.args(["--distribution", distro, "--", "sh", "-lc", &script]);
            apply_no_window(&mut command);
            command
        }
        SyncTargetKind::Ssh => {
            let host = target.ssh_host.as_deref().unwrap_or_default();
            if !safe_ssh_host(host) {
                return Err(AppError::Config("SSH 主机名包含不支持的字符".to_string()));
            }
            let port = target.ssh_port.unwrap_or(22).to_string();
            let mut command = Command::new("ssh");
            command.args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-p", &port, host, "sh", "-lc", &script]);
            apply_no_window(&mut command);
            command
        }
    };
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|error| AppError::Other(format!("无法启动同步传输: {error}")))?;
    let mut input = child.stdin.take().ok_or_else(|| AppError::Other("无法打开同步传输输入流".to_string()))?;
    let mut file = fs::File::open(archive)?;
    copy(&mut file, &mut input).map_err(|error| AppError::Other(format!("写入同步归档失败: {error}")))?;
    drop(input);
    let output = child.wait_with_output().map_err(|error| AppError::Other(format!("等待同步传输失败: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Config(format!("同步传输失败: {}", if detail.is_empty() { "远端命令返回错误" } else { &detail })));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
}

fn safe_ssh_host(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '@')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_validation_rejects_relative_remote_root() {
        let target = SyncTarget { id: "id".to_string(), name: "WSL".to_string(), kind: SyncTargetKind::Wsl, wsl_distribution: Some("Ubuntu".to_string()), ssh_host: None, ssh_port: None, remote_root: "relative".to_string(), path_mappings: Vec::new(), items: vec![SyncItem::Skills], last_synced_at: None };
        assert!(validate_target(&target).is_err());
    }

    #[test]
    fn target_validation_rejects_windows_remote_root() {
        let target = SyncTarget { id: "id".to_string(), name: "WSL".to_string(), kind: SyncTargetKind::Wsl, wsl_distribution: Some("Ubuntu".to_string()), ssh_host: None, ssh_port: None, remote_root: "C:\\Users\\admin".to_string(), path_mappings: Vec::new(), items: vec![SyncItem::Skills], last_synced_at: None };
        assert!(validate_target(&target).is_err());
    }

    #[test]
    fn shell_quote_keeps_single_quote_inside_one_argument() {
        assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\\"'\\\"'b'");
    }
}
