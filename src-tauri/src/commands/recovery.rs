//! Provider configuration backup inspection and recovery.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use crate::backup::{backup_file_named, load_manifest, verify_backup, DEFAULT_BACKUP_KEEP};
use crate::config::{atomic_write, detect_claude_desktop, get_backup_dir, get_claude_settings_path};
use crate::database::dao;
use crate::database::dao::settings::set_setting;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackup {
    pub name: String,
    pub created_at: i64,
    pub verified: bool,
    pub source_name: Option<String>,
}

#[tauri::command]
pub fn list_config_backups(
    target: ProviderTarget,
    directory: Option<String>,
) -> AppResult<Vec<ConfigBackup>> {
    let dir = resolve_backup_directory(directory.as_deref())?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut backups = fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_backup_for_target(target, &name) {
                return None;
            }
            let created_at = entry.metadata().ok()?.modified().ok()?
                .duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
            let manifest = load_manifest(&entry.path()).ok().flatten();
            Some(ConfigBackup {
                name,
                created_at,
                verified: manifest.is_some(),
                source_name: manifest.map(|manifest| manifest.source_name),
            })
        })
        .collect::<Vec<_>>();
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

#[tauri::command]
pub fn preview_config_backup(
    target: ProviderTarget,
    name: String,
    directory: Option<String>,
) -> AppResult<String> {
    let path = backup_path(target, &name, directory.as_deref())?;
    verify_backup(&path)?;
    let value: Value = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| AppError::Config("该备份不是可预览的 JSON 配置".to_string()))?;
    Ok(serde_json::to_string_pretty(&redact(value))?)
}

#[tauri::command]
pub async fn restore_config_backup(
    target: ProviderTarget,
    name: String,
    directory: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let source = backup_path(target, &name, directory.as_deref())?;
    verify_backup(&source)?;
    let destination = destination_for_backup(target, &name)?;
    if destination.exists() {
        let stem = destination.file_name().and_then(|n| n.to_str()).unwrap_or("config.json");
        backup_file_named(&destination, stem, DEFAULT_BACKUP_KEEP)?;
    }
    atomic_write(&destination, &fs::read(source)?)?;
    state.db.with_conn(|conn| {
        dao::clear_current_provider(conn, target)?;
        match target {
            ProviderTarget::ClaudeCode => set_setting(conn, "p7.code_config_ownership", "")?,
            ProviderTarget::ClaudeDesktop => set_setting(conn, "p7.desktop_original_applied_id", "")?,
            ProviderTarget::Codex => set_setting(conn, "v040.codex_managed", "")?,
            ProviderTarget::OpenCode => set_setting(conn, "v131.opencode_managed", "")?,
            ProviderTarget::Pi => set_setting(conn, "v136.pi_managed", "")?,
            ProviderTarget::Dsh => set_setting(conn, "v1310.dsh_managed", "")?,
            ProviderTarget::Cline => set_setting(conn, "v1323.cline_managed", "")?,
        }
        Ok(())
    })?;
    Ok(())
}

fn resolve_backup_directory(directory: Option<&str>) -> AppResult<PathBuf> {
    match directory.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let dir = PathBuf::from(path);
            if !dir.is_dir() {
                return Err(AppError::Path(format!(
                    "备份目录不存在或不是文件夹: {}",
                    dir.display()
                )));
            }
            Ok(dir)
        }
        None => Ok(get_backup_dir()),
    }
}

fn is_backup_for_target(target: ProviderTarget, name: &str) -> bool {
    name.ends_with(".bak") && match target {
        ProviderTarget::ClaudeCode => name.starts_with("settings.json_"),
        ProviderTarget::ClaudeDesktop => name.starts_with("_meta.json_") || name.starts_with("claude-switcher.json_"),
        ProviderTarget::Codex => name.starts_with("codex-"),
        ProviderTarget::OpenCode => name.starts_with("opencode-"),
        ProviderTarget::Pi => name.starts_with("pi-"),
        ProviderTarget::Dsh => name.starts_with("dsh-"),
        ProviderTarget::Cline => name.starts_with("cline-"),
    }
}

fn backup_path(target: ProviderTarget, name: &str, directory: Option<&str>) -> AppResult<PathBuf> {
    if name.contains('/') || name.contains('\\') || !is_backup_for_target(target, name) {
        return Err(AppError::Config("无效的配置备份标识".to_string()));
    }
    let path = resolve_backup_directory(directory)?.join(name);
    if !path.is_file() {
        return Err(AppError::Config("配置备份不存在".to_string()));
    }
    Ok(path)
}

fn destination_for_backup(target: ProviderTarget, name: &str) -> AppResult<PathBuf> {
    match target {
        ProviderTarget::ClaudeCode => Ok(get_claude_settings_path()),
        ProviderTarget::ClaudeDesktop => {
            let paths = detect_claude_desktop();
            let library = paths.config_library.ok_or_else(|| AppError::Config("未检测到 Claude Desktop 配置目录".to_string()))?;
            if name.starts_with("_meta.json_") {
                paths.meta_path.ok_or_else(|| AppError::Config("未检测到 Claude Desktop _meta.json".to_string()))
            } else {
                Ok(library.join("claude-switcher.json"))
            }
        }
        ProviderTarget::Codex => Err(AppError::Config("Codex 仅支持通过“切换官方配置”恢复原始文件".to_string())),
        ProviderTarget::OpenCode => Err(AppError::Config(
            "OpenCode 仅支持通过“切换官方配置”恢复原始文件".to_string(),
        )),
        ProviderTarget::Pi => Err(AppError::Config(
            "Pi 仅支持通过“切换官方配置”恢复原始文件".to_string(),
        )),
        ProviderTarget::Dsh => Err(AppError::Config(
            "DeepSeek Harness 仅支持多供应商自动同步，无需单独恢复配置文件".to_string(),
        )),
        ProviderTarget::Cline => Err(AppError::Config(
            "Cline 仅支持多供应商自动同步，无需单独恢复配置文件".to_string(),
        )),
    }
}

fn redact(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(values.into_iter().map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let value = if lower.contains("key") || lower.contains("token") || lower.contains("authorization") {
                Value::String("***REDACTED***".to_string())
            } else { redact(value) };
            (key, value)
        }).collect()),
        Value::Array(values) => Value::Array(values.into_iter().map(redact).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn list_config_backups_reads_custom_directory() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("settings.json_1.bak"), b"{}").unwrap();
        fs::write(dir.path().join("unrelated.bak"), b"{}").unwrap();
        let listed = list_config_backups(
            ProviderTarget::ClaudeCode,
            Some(dir.path().to_string_lossy().into_owned()),
        )
        .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "settings.json_1.bak");
    }

    #[test]
    fn backup_path_rejects_traversal_names() {
        assert!(backup_path(ProviderTarget::ClaudeCode, "../settings.json_1.bak", None).is_err());
    }
}
