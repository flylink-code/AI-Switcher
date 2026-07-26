//! Returns all detected paths to the frontend (drives the Environment page).

use serde::Serialize;

use crate::config::{
    claude_desktop::detect_claude_desktop,
    get_app_config_dir, get_app_db_path, get_backup_dir, get_claude_config_dir,
    get_claude_json_path, get_claude_settings_path, get_home_dir,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsInfo {
    pub home: String,
    pub claude_config_dir: String,
    pub claude_settings_path: String,
    pub claude_json_path: String,
    pub app_config_dir: String,
    pub app_db_path: String,
    pub backup_dir: String,
    pub claude_desktop_base: Option<String>,
    pub claude_desktop_threep_base: Option<String>,
    pub claude_desktop_config_library: Option<String>,
    pub claude_desktop_meta_path: Option<String>,
    pub claude_desktop_normal_config_path: Option<String>,
    pub claude_desktop_threep_config_path: Option<String>,
}

/// Camel-case the field names for the frontend (TS interface uses camelCase).
fn s(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

#[tauri::command]
pub fn get_paths() -> PathsInfo {
    let desktop = detect_claude_desktop();
    PathsInfo {
        home: s(&get_home_dir()),
        claude_config_dir: s(&get_claude_config_dir()),
        claude_settings_path: s(&get_claude_settings_path()),
        claude_json_path: s(&get_claude_json_path()),
        app_config_dir: s(&get_app_config_dir()),
        app_db_path: s(&get_app_db_path()),
        backup_dir: s(&get_backup_dir()),
        claude_desktop_base: desktop.base.map(|p| s(&p)),
        claude_desktop_threep_base: desktop.threep_base.map(|p| s(&p)),
        claude_desktop_config_library: desktop.config_library.map(|p| s(&p)),
        claude_desktop_meta_path: desktop.meta_path.map(|p| s(&p)),
        claude_desktop_normal_config_path: desktop.normal_config_path.map(|p| s(&p)),
        claude_desktop_threep_config_path: desktop.threep_config_path.map(|p| s(&p)),
    }
}
