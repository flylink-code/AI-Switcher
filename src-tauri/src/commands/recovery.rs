//! Provider configuration backup inspection and recovery.

use std::fs;

use serde::Serialize;
use serde_json::Value;

use crate::backup::{backup_file_named, DEFAULT_BACKUP_KEEP};
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
}

#[tauri::command]
pub fn list_config_backups(target: ProviderTarget) -> AppResult<Vec<ConfigBackup>> {
    let dir = get_backup_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut backups = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_backup_for_target(target, &name) {
                return None;
            }
            let created_at = entry.metadata().ok()?.modified().ok()?
                .duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
            Some(ConfigBackup { name, created_at })
        })
        .collect::<Vec<_>>();
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

#[tauri::command]
pub fn preview_config_backup(target: ProviderTarget, name: String) -> AppResult<String> {
    let path = backup_path(target, &name)?;
    let value: Value = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| AppError::Config("该备份不是可预览的 JSON 配置".to_string()))?;
    Ok(serde_json::to_string_pretty(&redact(value))?)
}

#[tauri::command]
pub async fn restore_config_backup(
    target: ProviderTarget,
    name: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let source = backup_path(target, &name)?;
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
        }
        Ok(())
    })?;
    Ok(())
}

fn is_backup_for_target(target: ProviderTarget, name: &str) -> bool {
    name.ends_with(".bak") && match target {
        ProviderTarget::ClaudeCode => name.starts_with("settings.json_"),
        ProviderTarget::ClaudeDesktop => name.starts_with("_meta.json_") || name.starts_with("claude-switcher.json_"),
    }
}

fn backup_path(target: ProviderTarget, name: &str) -> AppResult<std::path::PathBuf> {
    if name.contains('/') || name.contains('\\') || !is_backup_for_target(target, name) {
        return Err(AppError::Config("无效的配置备份标识".to_string()));
    }
    let path = get_backup_dir().join(name);
    if !path.is_file() {
        return Err(AppError::Config("配置备份不存在".to_string()));
    }
    Ok(path)
}

fn destination_for_backup(target: ProviderTarget, name: &str) -> AppResult<std::path::PathBuf> {
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
