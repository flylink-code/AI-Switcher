//! System tray menus and provider quick switching.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::store::AppState;

const TRAY_ID: &str = "main-tray";
const CODE_PROVIDER_PREFIX: &str = "code-provider:";
const DESKTOP_PROVIDER_PREFIX: &str = "desktop-provider:";
const CODE_OFFICIAL_ID: &str = "code-provider:official";
const DESKTOP_OFFICIAL_ID: &str = "desktop-provider:official";
const PROFILE_PREFIX: &str = "profile:";

/// Build and attach the tray icon. Provider entries are generated once at app
/// startup; selecting an entry applies the same configuration as the UI switch.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> AppResult<()> {
    let state = app.state::<AppState>();
    let language = crate::commands::system::read_app_language(&state.db)?;
    let menu = create_tray_menu(app, &language)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("missing icon"))
        .tooltip("AI-Switcher")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "show" => show_main_window(app),
                "quit" => app.exit(0),
                CODE_OFFICIAL_ID => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_to_official(app, ProviderTarget::ClaudeCode)) {
                        log::error!("托盘切换 Claude Code 官方登录失败: {e}");
                    }
                }
                DESKTOP_OFFICIAL_ID => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_to_official(app, ProviderTarget::ClaudeDesktop)) {
                        log::error!("托盘切换 Claude Desktop 官方登录失败: {e}");
                    }
                }
                _ if id.starts_with(CODE_PROVIDER_PREFIX) => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_provider(app, &id[CODE_PROVIDER_PREFIX.len()..], ProviderTarget::ClaudeCode)) {
                        log::error!("托盘切换 Claude Code 供应商失败: {e}");
                    }
                }
                _ if id.starts_with(DESKTOP_PROVIDER_PREFIX) => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_provider(app, &id[DESKTOP_PROVIDER_PREFIX.len()..], ProviderTarget::ClaudeDesktop)) {
                        log::error!("托盘切换 Claude Desktop 供应商失败: {e}");
                    }
                }
                _ if id.starts_with(PROFILE_PREFIX) => {
                    if let Err(e) = tauri::async_runtime::block_on(apply_profile_from_tray(app, &id[PROFILE_PREFIX.len()..])) {
                        log::error!("托盘应用配置快照失败: {e}");
                    }
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    Ok(())
}

pub fn refresh_tray_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> AppResult<()> {
    let menu = create_tray_menu(app, language)?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| AppError::Tauri("找不到系统托盘图标".to_string()))?;
    tray.set_menu(Some(menu))
        .map_err(|error| AppError::Tauri(format!("更新托盘菜单失败: {error}")))
}

fn create_tray_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> AppResult<Menu<R>> {
    let labels = tray_labels(language);
    let code_menu =
        build_provider_menu(app, ProviderTarget::ClaudeCode, "Claude Code", labels.official)?;
    let desktop_menu = build_provider_menu(
        app,
        ProviderTarget::ClaudeDesktop,
        "Claude Desktop",
        labels.official,
    )?;
    let profiles_menu = build_profiles_menu(app, labels.projects)?;
    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let separator =
        PredefinedMenuItem::separator(app).map_err(|e| AppError::Tauri(e.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    Menu::with_items(app, &[&show, &code_menu, &desktop_menu, &profiles_menu, &separator, &quit])
        .map_err(|e| AppError::Tauri(e.to_string()))
}

#[derive(Debug, PartialEq, Eq)]
struct TrayLabels {
    show: &'static str,
    official: &'static str,
    projects: &'static str,
    quit: &'static str,
}

fn tray_labels(language: &str) -> TrayLabels {
    if language == "en-US" {
        TrayLabels {
            show: "Open AI-Switcher",
            official: "Official login",
            projects: "Projects",
            quit: "Quit",
        }
    } else {
        TrayLabels {
            show: "打开 AI-Switcher",
            official: "官方登录",
            projects: "项目",
            quit: "退出",
        }
    }
}

fn build_provider_menu<R: Runtime>(
    app: &AppHandle<R>,
    target: ProviderTarget,
    label: &str,
    official_label: &str,
) -> AppResult<Submenu<R>> {
    let state = app.state::<AppState>();
    let providers = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    let (prefix, official_id) = match target {
        ProviderTarget::ClaudeCode => (CODE_PROVIDER_PREFIX, CODE_OFFICIAL_ID),
        ProviderTarget::ClaudeDesktop => (DESKTOP_PROVIDER_PREFIX, DESKTOP_OFFICIAL_ID),
        ProviderTarget::Codex => return Err(AppError::Config("Codex 不显示在 Claude 供应商托盘菜单中".to_string())),
    };
    let official = MenuItem::with_id(app, official_id, official_label, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let mut provider_items = Vec::new();
    for provider in providers {
        let item_label = if provider.is_current { format!("✓ {}", provider.name) } else { provider.name };
        provider_items.push(
            MenuItem::with_id(app, format!("{prefix}{}", provider.id), item_label, true, None::<&str>)
                .map_err(|e| AppError::Tauri(e.to_string()))?,
        );
    }
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<R>> = vec![&official];
    items.extend(provider_items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<R>));
    Submenu::with_items(app, label, true, &items)
        .map_err(|e| AppError::Tauri(e.to_string()))
}

fn build_profiles_menu<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> AppResult<Submenu<R>> {
    let state = app.state::<AppState>();
    let profiles = state.db.with_conn(dao::profiles::list_profiles)?;
    let current_id = state
        .db
        .with_conn(dao::profiles::get_current_profile_id)?;
    let mut items: Vec<MenuItem<R>> = Vec::new();
    for profile in profiles {
        let item_label = if current_id.as_deref() == Some(profile.id.as_str()) {
            format!("✓ {}", profile.name)
        } else {
            profile.name
        };
        items.push(
            MenuItem::with_id(
                app,
                format!("{PROFILE_PREFIX}{}", profile.id),
                item_label,
                true,
                None::<&str>,
            )
            .map_err(|e| AppError::Tauri(e.to_string()))?,
        );
    }
    if items.is_empty() {
        items.push(
            MenuItem::with_id(app, "profiles-empty", "—", false, None::<&str>)
                .map_err(|e| AppError::Tauri(e.to_string()))?,
        );
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
        items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<R>).collect();
    Submenu::with_items(app, label, true, &refs)
        .map_err(|e| AppError::Tauri(e.to_string()))
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

async fn switch_provider<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    target: ProviderTarget,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    let provider =
        crate::commands::providers::switch_provider_for_target(id, target, &state).await?;
    crate::commands::providers::schedule_provider_health_check(
        app.clone(),
        provider.provider,
        std::sync::Arc::clone(&state.db),
    );
    Ok(())
}

async fn switch_to_official<R: Runtime>(app: &AppHandle<R>, target: ProviderTarget) -> AppResult<()> {
    let state = app.state::<AppState>();
    crate::commands::providers::switch_to_official_for_target(target, &state).await
}

async fn apply_profile_from_tray<R: Runtime>(app: &AppHandle<R>, id: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    let result =
        crate::commands::profiles::apply_profile_for_id(id, true, app, &state).await?;
    if !result.warnings.is_empty() {
        log::warn!(
            "配置快照 {} 已应用，但有 {} 条警告",
            result.profile.name,
            result.warnings.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_follow_the_selected_language() {
        assert_eq!(tray_labels("zh-CN").quit, "退出");
        assert_eq!(tray_labels("en-US").quit, "Quit");
        assert_eq!(tray_labels("unsupported"), tray_labels("zh-CN"));
    }
}
