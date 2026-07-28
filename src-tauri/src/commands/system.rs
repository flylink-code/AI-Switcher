//! Commands for launch-at-login settings.

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_autostart::ManagerExt;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const AUTOSTART_MODE_KEY: &str = "autostart_launch_mode";
const AUTOSTART_ARGS_MIGRATED_KEY: &str = "autostart_args_migrated_v1";
const APP_LANGUAGE_KEY: &str = "app.language";
const CLOSE_BEHAVIOR_KEY: &str = "app.close_behavior";
const DISMISSED_ONBOARDING_TIPS_KEY: &str = "ui.dismissed_onboarding_tips";
const UPDATE_MIRROR_SETTINGS_KEY: &str = "app.update_mirror_settings";
const DEFAULT_UPDATE_MIRROR_BASE: &str = "https://gh-proxy.com/";
const ONBOARDING_TIP_KEYS: &[&str] = &[
    "proxy",
    "mcp",
    "prompts",
    "skills",
    "sessions",
    "usage",
    "localization",
    "environment",
    "about",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutostartMode {
    Off,
    Silent,
    Window,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    Ask,
    Tray,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMirrorSettings {
    pub use_mirror: bool,
    pub mirror_base: String,
}

impl CloseBehavior {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Tray => "tray",
            Self::Quit => "quit",
        }
    }
}

pub(crate) fn read_close_behavior(db: &Database) -> AppResult<CloseBehavior> {
    Ok(db
        .with_conn(|conn| get_setting(conn, CLOSE_BEHAVIOR_KEY))?
        .and_then(|value| match value.as_str() {
            "ask" => Some(CloseBehavior::Ask),
            "tray" => Some(CloseBehavior::Tray),
            "quit" => Some(CloseBehavior::Quit),
            _ => None,
        })
        .unwrap_or(CloseBehavior::Ask))
}

#[tauri::command]
pub fn get_close_behavior(state: tauri::State<'_, AppState>) -> AppResult<CloseBehavior> {
    read_close_behavior(&state.db)
}

#[tauri::command]
pub fn set_close_behavior(
    behavior: CloseBehavior,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    set_setting_value(&state.db, CLOSE_BEHAVIOR_KEY, behavior.as_str())
}

#[tauri::command]
pub fn resolve_close_request(
    action: CloseBehavior,
    remember: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    if action == CloseBehavior::Ask {
        return Err(AppError::Config("关闭操作必须为系统托盘或直接退出".to_string()));
    }
    if remember {
        set_setting_value(&state.db, CLOSE_BEHAVIOR_KEY, action.as_str())?;
    }
    match action {
        CloseBehavior::Tray => {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| AppError::Tauri("找不到主窗口".to_string()))?;
            window
                .hide()
                .map_err(|error| AppError::Tauri(format!("隐藏主窗口失败: {error}")))
        }
        CloseBehavior::Quit => {
            app.exit(0);
            Ok(())
        }
        CloseBehavior::Ask => unreachable!(),
    }
}

pub(crate) fn read_app_language(db: &Database) -> AppResult<String> {
    Ok(db
        .with_conn(|conn| get_setting(conn, APP_LANGUAGE_KEY))?
        .filter(|value| value == "zh-CN" || value == "en-US")
        .unwrap_or_else(|| "zh-CN".to_string()))
}

#[tauri::command]
pub fn set_app_language(
    language: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    validate_app_language(&language)?;
    set_setting_value(&state.db, APP_LANGUAGE_KEY, &language)?;
    crate::tray::refresh_tray_menu(&app, &language)
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.request_restart();
}

#[tauri::command]
pub fn get_update_mirror_settings(
    state: tauri::State<'_, AppState>,
) -> AppResult<UpdateMirrorSettings> {
    let configured = state.db.with_conn(|conn| get_setting(conn, UPDATE_MIRROR_SETTINGS_KEY))?
        .and_then(|value| serde_json::from_str::<UpdateMirrorSettings>(&value).ok());
    Ok(configured.unwrap_or_else(|| default_update_mirror_settings(&state.db)))
}

#[tauri::command]
pub fn set_update_mirror_settings(
    settings: UpdateMirrorSettings,
    state: tauri::State<'_, AppState>,
) -> AppResult<UpdateMirrorSettings> {
    let settings = normalize_update_mirror_settings(settings)?;
    set_setting_value(
        &state.db,
        UPDATE_MIRROR_SETTINGS_KEY,
        &serde_json::to_string(&settings)?,
    )?;
    Ok(settings)
}

fn default_update_mirror_settings(db: &Database) -> UpdateMirrorSettings {
    UpdateMirrorSettings {
        use_mirror: read_app_language(db).unwrap_or_else(|_| "zh-CN".to_string()) == "zh-CN",
        mirror_base: DEFAULT_UPDATE_MIRROR_BASE.to_string(),
    }
}

fn normalize_update_mirror_settings(settings: UpdateMirrorSettings) -> AppResult<UpdateMirrorSettings> {
    let mirror_base = settings.mirror_base.trim();
    let parsed = url::Url::parse(mirror_base)
        .map_err(|error| AppError::Config(format!("GitHub 镜像地址无效: {error}")))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none()
        || !parsed.username().is_empty() || parsed.password().is_some()
        || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::Config("GitHub 镜像地址必须是无凭据、无参数的 HTTPS 前缀".to_string()));
    }
    Ok(UpdateMirrorSettings {
        use_mirror: settings.use_mirror,
        mirror_base: format!("{}/", mirror_base.trim_end_matches('/')),
    })
}

#[tauri::command]
pub fn get_dismissed_onboarding_tips(
    state: tauri::State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let value = state.db.with_conn(|conn| get_setting(conn, DISMISSED_ONBOARDING_TIPS_KEY))?;
    Ok(value
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|key| ONBOARDING_TIP_KEYS.contains(&key.as_str()))
        .collect())
}

#[tauri::command]
pub fn dismiss_onboarding_tip(
    tip_key: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    validate_onboarding_tip_key(&tip_key)?;
    let mut dismissed = get_dismissed_onboarding_tips(state.clone())?;
    if !dismissed.contains(&tip_key) {
        dismissed.push(tip_key);
        set_setting_value(
            &state.db,
            DISMISSED_ONBOARDING_TIPS_KEY,
            &serde_json::to_string(&dismissed)?,
        )?;
    }
    Ok(())
}

#[tauri::command]
pub fn restore_onboarding_tips(state: tauri::State<'_, AppState>) -> AppResult<()> {
    set_setting_value(&state.db, DISMISSED_ONBOARDING_TIPS_KEY, "[]")
}

fn validate_app_language(language: &str) -> AppResult<()> {
    if matches!(language, "zh-CN" | "en-US") {
        Ok(())
    } else {
        Err(AppError::Config(format!("不支持的界面语言: {language}")))
    }
}

fn validate_onboarding_tip_key(tip_key: &str) -> AppResult<()> {
    if ONBOARDING_TIP_KEYS.contains(&tip_key) {
        Ok(())
    } else {
        Err(AppError::Config(format!("不支持的新手提示标识: {tip_key}")))
    }
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
    fn close_behaviors_use_stable_wire_values() {
        assert_eq!(serde_json::to_string(&CloseBehavior::Ask).unwrap(), "\"ask\"");
        assert_eq!(serde_json::to_string(&CloseBehavior::Tray).unwrap(), "\"tray\"");
        assert_eq!(serde_json::to_string(&CloseBehavior::Quit).unwrap(), "\"quit\"");
    }

    #[test]
    fn onboarding_tip_keys_are_allowlisted() {
        assert!(validate_onboarding_tip_key("proxy").is_ok());
        assert!(validate_onboarding_tip_key("usage").is_ok());
        assert!(validate_onboarding_tip_key("localization").is_ok());
        assert!(validate_onboarding_tip_key("environment").is_ok());
        assert!(validate_onboarding_tip_key("about").is_ok());
        assert!(validate_onboarding_tip_key("anything-else").is_err());
    }

    #[test]
    fn app_language_validation_rejects_unknown_values() {
        assert!(validate_app_language("zh-CN").is_ok());
        assert!(validate_app_language("en-US").is_ok());
        assert!(validate_app_language("ja-JP").is_err());
    }

    #[test]
    fn update_mirror_requires_a_safe_https_prefix() {
        let normalized = normalize_update_mirror_settings(UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://gh-proxy.com".to_string(),
        }).unwrap();
        assert_eq!(normalized.mirror_base, "https://gh-proxy.com/");
        assert!(normalize_update_mirror_settings(UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "http://gh-proxy.com/".to_string(),
        }).is_err());
        assert!(normalize_update_mirror_settings(UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://user@example.com/".to_string(),
        }).is_err());
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
