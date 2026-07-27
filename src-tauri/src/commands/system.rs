//! Commands for launch-at-login settings.

use serde::{Deserialize, Serialize};
use tauri_plugin_autostart::ManagerExt;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const AUTOSTART_MODE_KEY: &str = "autostart_launch_mode";
const AUTOSTART_ARGS_MIGRATED_KEY: &str = "autostart_args_migrated_v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutostartMode {
    Off,
    Silent,
    Window,
}

impl AutostartMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Silent => "silent",
            Self::Window => "window",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartConfig {
    pub enabled: bool,
    pub mode: AutostartMode,
}

#[tauri::command]
pub fn get_autostart_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<AutostartConfig> {
    let enabled = autostart_enabled(&app)?;
    let stored = read_mode(&state.db)?;
    Ok(AutostartConfig {
        enabled,
        mode: if enabled {
            stored.filter(|mode| *mode != AutostartMode::Off).unwrap_or(AutostartMode::Window)
        } else {
            AutostartMode::Off
        },
    })
}

#[tauri::command]
pub fn set_autostart_config(
    mode: AutostartMode,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    set_setting_value(&state.db, AUTOSTART_MODE_KEY, mode.as_str())?;
    set_autostart_registration(&app, mode != AutostartMode::Off)
}

#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> AppResult<bool> {
    autostart_enabled(&app)
}

fn autostart_enabled(app: &tauri::AppHandle) -> AppResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| AppError::Other(format!("读取开机自启状态失败: {e}")))
}

#[tauri::command]
pub fn set_autostart_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let mode = if enabled {
        read_mode(&state.db)?
            .filter(|mode| *mode != AutostartMode::Off)
            .unwrap_or(AutostartMode::Window)
    } else {
        AutostartMode::Off
    };
    set_setting_value(&state.db, AUTOSTART_MODE_KEY, mode.as_str())?;
    set_autostart_registration(&app, enabled)
}

fn set_autostart_registration(app: &tauri::AppHandle, enabled: bool) -> AppResult<()> {
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

fn read_mode(db: &Database) -> AppResult<Option<AutostartMode>> {
    Ok(db
        .with_conn(|conn| get_setting(conn, AUTOSTART_MODE_KEY))?
        .and_then(|value| match value.as_str() {
            "off" => Some(AutostartMode::Off),
            "silent" => Some(AutostartMode::Silent),
            "window" => Some(AutostartMode::Window),
            _ => None,
        }))
}

fn set_setting_value(db: &Database, key: &str, value: &str) -> AppResult<()> {
    db.with_conn(|conn| set_setting(conn, key, value))
}

pub fn is_silent_autostart(db: &Database) -> bool {
    should_launch_silently(
        std::env::args().any(|arg| arg == "--autostart"),
        read_mode(db).ok().flatten(),
    )
}

fn should_launch_silently(from_autostart: bool, mode: Option<AutostartMode>) -> bool {
    from_autostart && mode == Some(AutostartMode::Silent)
}

pub fn migrate_autostart_registration(
    app: &tauri::AppHandle,
    db: &Database,
) -> AppResult<()> {
    if !autostart_enabled(app)? {
        return Ok(());
    }
    let migrated = db
        .with_conn(|conn| get_setting(conn, AUTOSTART_ARGS_MIGRATED_KEY))?
        .as_deref()
        == Some("true");
    if migrated {
        return Ok(());
    }
    if read_mode(db)?.is_none() {
        set_setting_value(db, AUTOSTART_MODE_KEY, AutostartMode::Window.as_str())?;
    }
    set_autostart_registration(app, false)?;
    set_autostart_registration(app, true)?;
    set_setting_value(db, AUTOSTART_ARGS_MIGRATED_KEY, "true")
}

#[tauri::command]
pub fn report_frontend_startup(
    duration_ms: u64,
    reason: String,
    failures: Vec<String>,
) {
    let safe_reason: String = reason
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
        .take(32)
        .collect();
    let safe_failures = failures
        .into_iter()
        .take(32)
        .map(|failure| {
            failure
                .chars()
                .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
                .take(64)
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    log::info!(
        "前端启动预热完成: duration_ms={duration_ms}, reason={safe_reason}, failures={:?}",
        safe_failures
    );
}

#[tauri::command]
pub fn report_frontend_performance(kind: String, name: String, duration_ms: u64) {
    let safe_kind = sanitize_performance_label(&kind, 32);
    let safe_name = sanitize_performance_label(&name, 64);
    log::info!(
        "前端性能阶段: kind={safe_kind}, name={safe_name}, duration_ms={duration_ms}"
    );
}

fn sanitize_performance_label(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(*character, '_' | '-')
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_modes_use_stable_wire_values() {
        assert_eq!(serde_json::to_string(&AutostartMode::Off).unwrap(), "\"off\"");
        assert_eq!(
            serde_json::to_string(&AutostartMode::Silent).unwrap(),
            "\"silent\""
        );
        assert_eq!(
            serde_json::to_string(&AutostartMode::Window).unwrap(),
            "\"window\""
        );
    }

    #[test]
    fn only_silent_autostart_hides_the_main_window() {
        assert!(should_launch_silently(true, Some(AutostartMode::Silent)));
        assert!(!should_launch_silently(false, Some(AutostartMode::Silent)));
        assert!(!should_launch_silently(true, Some(AutostartMode::Window)));
        assert!(!should_launch_silently(true, Some(AutostartMode::Off)));
        assert!(!should_launch_silently(true, None));
    }

    #[test]
    fn performance_labels_drop_sensitive_punctuation() {
        assert_eq!(
            sanitize_performance_label("proxy/status?api_key=secret", 64),
            "proxystatusapi_keysecret"
        );
        assert_eq!(sanitize_performance_label("page-module", 8), "page-mod");
    }
}
