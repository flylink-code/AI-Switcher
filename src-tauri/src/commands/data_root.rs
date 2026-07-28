//! Managed application-library migration. Claude's live files remain under
//! their official locations; only AI-Switcher-owned data is moved.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::paths::{configured_data_root, get_legacy_app_config_dir, write_data_root_config};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRootInfo {
    pub active_path: String,
    pub legacy_path: String,
    pub migrated: bool,
    pub restart_required: bool,
}

#[tauri::command]
pub fn get_data_root() -> DataRootInfo {
    let legacy = get_legacy_app_config_dir();
    let active = configured_data_root().unwrap_or_else(|| legacy.clone());
    DataRootInfo {
        active_path: active.to_string_lossy().into_owned(),
        legacy_path: legacy.to_string_lossy().into_owned(),
        migrated: active != legacy,
        restart_required: false,
    }
}

/// Copy the legacy AI-Switcher data directory into an empty target and make it
/// the active root for the next process launch. Existing data is never removed.
#[tauri::command]
pub fn migrate_data_root(target_path: String) -> AppResult<DataRootInfo> {
    let source = get_legacy_app_config_dir();
    let target = normalize_target(&target_path)?;
    if target == source {
        return Err(AppError::Config("所选目录已经是当前资料库目录".to_string()));
    }
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err(AppError::Config("资料库目录不能嵌套在当前资料库中".to_string()));
    }
    if target.exists() && fs::read_dir(&target)?.next().is_some() {
        return Err(AppError::Config("资料库目标目录必须为空，避免覆盖已有文件".to_string()));
    }
    fs::create_dir_all(&target)?;
    if source.exists() {
        copy_directory(&source, &target)?;
    }
    write_data_root_config(&target)?;
    Ok(DataRootInfo {
        active_path: target.to_string_lossy().into_owned(),
        legacy_path: source.to_string_lossy().into_owned(),
        migrated: true,
        restart_required: true,
    })
}

fn normalize_target(value: &str) -> AppResult<PathBuf> {
    let raw = PathBuf::from(value.trim());
    if !raw.is_absolute() {
        return Err(AppError::Config("资料库目录必须是绝对路径".to_string()));
    }
    fs::create_dir_all(&raw)?;
    raw.canonicalize().map_err(Into::into)
}

fn copy_directory(source: &Path, target: &Path) -> AppResult<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(AppError::Config(format!("资料库不支持迁移符号链接: {}", source_path.display())));
        }
        if kind.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory(&source_path, &target_path)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_library_paths_are_rejected() {
        assert!(normalize_target("relative/library").is_err());
    }
}
