//! System tray menus and provider quick switching.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::config::{claude_code, claude_desktop};
use crate::database::dao;
use crate::database::dao::settings::get_setting;
use crate::error::{AppError, AppResult};
use crate::provider::{ProviderTarget, ProtocolType};
use crate::store::AppState;

const CODE_PROVIDER_PREFIX: &str = "code-provider:";
const DESKTOP_PROVIDER_PREFIX: &str = "desktop-provider:";
const CODE_OFFICIAL_ID: &str = "code-provider:official";
const DESKTOP_OFFICIAL_ID: &str = "desktop-provider:official";

/// Build and attach the tray icon. Provider entries are generated once at app
/// startup; selecting an entry applies the same configuration as the UI switch.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> AppResult<()> {
    let code_menu = build_provider_menu(app, ProviderTarget::ClaudeCode, "Claude Code")?;
    let desktop_menu = build_provider_menu(app, ProviderTarget::ClaudeDesktop, "Claude Desktop")?;
    let show = MenuItem::with_id(app, "show", "Claude Switcher", true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let separator = PredefinedMenuItem::separator(app).map_err(|e| AppError::Tauri(e.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let menu = Menu::with_items(app, &[&show, &code_menu, &desktop_menu, &separator, &quit])
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("missing icon"))
        .tooltip("Claude Switcher")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "show" => show_main_window(app),
                "quit" => app.exit(0),
                CODE_OFFICIAL_ID => {
                    if let Err(e) = switch_to_official(app, ProviderTarget::ClaudeCode) {
                        log::error!("托盘切换 Claude Code 官方登录失败: {e}");
                    }
                }
                DESKTOP_OFFICIAL_ID => {
                    if let Err(e) = switch_to_official(app, ProviderTarget::ClaudeDesktop) {
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
                _ => {}
            }
        })
        .build(app)
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    Ok(())
}

fn build_provider_menu<R: Runtime>(
    app: &AppHandle<R>,
    target: ProviderTarget,
    label: &str,
) -> AppResult<Submenu<R>> {
    let state = app.state::<AppState>();
    let providers = state.db.with_conn(|conn| dao::list_providers(conn, target))?;
    let (prefix, official_id) = match target {
        ProviderTarget::ClaudeCode => (CODE_PROVIDER_PREFIX, CODE_OFFICIAL_ID),
        ProviderTarget::ClaudeDesktop => (DESKTOP_PROVIDER_PREFIX, DESKTOP_OFFICIAL_ID),
    };
    let official = MenuItem::with_id(app, official_id, "Official login", true, None::<&str>)
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
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    if provider.target_app != target {
        return Err(AppError::Config("供应商不属于此应用".to_string()));
    }
    let port = state.db.with_conn(|conn| get_setting(conn, "proxy_port"))?
        .and_then(|value| value.parse().ok())
        .unwrap_or(15821);
    match target {
        ProviderTarget::ClaudeCode if provider.protocol_type == ProtocolType::Proxy => {
            state.proxy.lock().await.start(port, ProviderTarget::ClaudeCode).await?;
            claude_code::apply_provider_to_settings_via_proxy(&provider, port)?;
        }
        ProviderTarget::ClaudeCode => claude_code::apply_provider_to_settings(&provider)?,
        ProviderTarget::ClaudeDesktop => {
            if provider.protocol_type == ProtocolType::Proxy {
                state.proxy.lock().await.start(port, ProviderTarget::ClaudeDesktop).await?;
            }
            claude_desktop::apply_provider(&provider, port)?;
        }
    }
    state.db.with_conn(|conn| dao::set_current_provider(conn, id))?;
    Ok(())
}

fn switch_to_official<R: Runtime>(app: &AppHandle<R>, target: ProviderTarget) -> AppResult<()> {
    match target {
        ProviderTarget::ClaudeCode => claude_code::clear_provider_from_settings()?,
        ProviderTarget::ClaudeDesktop => claude_desktop::clear_provider()?,
    }
    app.state::<AppState>().db.with_conn(|conn| dao::clear_current_provider(conn, target))
}
