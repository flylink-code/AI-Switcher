//! Commands for launch-at-login settings.

use tauri_plugin_autostart::ManagerExt;

use crate::error::{AppError, AppResult};

#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> AppResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| AppError::Other(format!("读取开机自启状态失败: {e}")))
}

#[tauri::command]
pub fn set_autostart_enabled(enabled: bool, app: tauri::AppHandle) -> AppResult<()> {
    let autostart = app.autolaunch();
    if enabled {
        autostart
            .enable()
            .map_err(|e| AppError::Other(format!("启用开机自启失败: {e}")))?;
    } else {
        autostart
            .disable()
            .map_err(|e| AppError::Other(format!("关闭开机自启失败: {e}")))?;
    }
    Ok(())
}
