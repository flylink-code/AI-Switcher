//! System tray menus and provider quick switching.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime,
};

use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::{ProtocolType, ProviderTarget};
use crate::store::AppState;

const TRAY_ID: &str = "main-tray";
const CODE_PROVIDER_PREFIX: &str = "code-provider:";
const CODE_GATEWAY_ID: &str = "code-provider:gateway";
const CODE_OFFICIAL_ID: &str = "code-provider:official";

const CODEX_PROVIDER_PREFIX: &str = "codex-provider:";
const CODEX_GATEWAY_ID: &str = "codex-provider:gateway";
const CODEX_OFFICIAL_ID: &str = "codex-provider:official";

const T2_NAV_PREFIX: &str = "t2-nav:";
const PROFILE_PREFIX: &str = "profile:";

/// Build and attach the tray icon. Provider entries are generated once at app
/// startup; selecting an entry applies the same configuration as the UI switch.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> AppResult<()> {
    let state = app.state::<AppState>();
    let language = crate::commands::system::read_app_language(&state.db)?;
    let menu = create_tray_menu(app, &language)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| AppError::Config("缺少托盘图标资源".into()))?,
        )
        .tooltip("AI-Switcher")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "show" => show_main_window(app),
                "quit" => app.exit(0),
                CODE_GATEWAY_ID => {
                    let state = app.state::<AppState>();
                    if let Err(error) = tauri::async_runtime::block_on(
                        crate::commands::providers::set_agent_gateway_for_target(
                            ProviderTarget::ClaudeCode,
                            Some(app),
                            &state,
                        ),
                    ) {
                        log::error!("托盘切换 Claude Code 智能网关失败: {error}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                CODEX_GATEWAY_ID => {
                    let state = app.state::<AppState>();
                    if let Err(error) = tauri::async_runtime::block_on(
                        crate::commands::providers::set_agent_gateway_for_target(
                            ProviderTarget::Codex,
                            Some(app),
                            &state,
                        ),
                    ) {
                        log::error!("托盘切换 Codex 智能网关失败: {error}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                CODE_OFFICIAL_ID => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_to_official(
                        app,
                        ProviderTarget::ClaudeCode,
                    )) {
                        log::error!("托盘切换 Claude Code 官方登录失败: {e}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                CODEX_OFFICIAL_ID => {
                    if let Err(e) = tauri::async_runtime::block_on(switch_to_official(
                        app,
                        ProviderTarget::Codex,
                    )) {
                        log::error!("托盘切换 Codex 官方登录失败: {e}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                _ if id.starts_with(CODE_PROVIDER_PREFIX) => {
                    let provider_id = &id[CODE_PROVIDER_PREFIX.len()..];
                    if let Err(e) = tauri::async_runtime::block_on(switch_provider(
                        app,
                        provider_id,
                        ProviderTarget::ClaudeCode,
                    )) {
                        log::error!("托盘切换 Claude Code 供应商失败: {e}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                _ if id.starts_with(CODEX_PROVIDER_PREFIX) => {
                    let provider_id = &id[CODEX_PROVIDER_PREFIX.len()..];
                    if let Err(e) = tauri::async_runtime::block_on(switch_provider(
                        app,
                        provider_id,
                        ProviderTarget::Codex,
                    )) {
                        log::error!("托盘切换 Codex 供应商失败: {e}");
                    } else {
                        refresh_tray_on_switch(app);
                    }
                }
                _ if id.starts_with(T2_NAV_PREFIX) => {
                    let target = &id[T2_NAV_PREFIX.len()..];
                    navigate_to_providers(app, target);
                }
                _ if id.starts_with(PROFILE_PREFIX) => {
                    let profile_id = &id[PROFILE_PREFIX.len()..];
                    if let Err(e) = tauri::async_runtime::block_on(apply_profile_from_tray(
                        app,
                        profile_id,
                    )) {
                        log::error!("托盘应用配置快照失败: {e}");
                    } else {
                        refresh_tray_on_switch(app);
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

fn refresh_tray_on_switch<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    match crate::commands::system::read_app_language(&state.db) {
        Ok(language) => {
            if let Err(error) = refresh_tray_menu(app, &language) {
                log::warn!("托盘切换后刷新菜单失败: {error}");
            }
        }
        Err(error) => {
            log::warn!("托盘切换后读取语言失败: {error}");
        }
    }
}

fn create_tray_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> AppResult<Menu<R>> {
    let labels = tray_labels(language);
    let code_menu =
        build_provider_menu(app, ProviderTarget::ClaudeCode, "Claude Code", labels.official)?;
    let codex_menu =
        build_provider_menu(app, ProviderTarget::Codex, "Codex", labels.official)?;
    let opencode_menu =
        build_t2_menu(app, ProviderTarget::OpenCode, "OpenCode", labels.manage_providers)?;
    let pi_menu =
        build_t2_menu(app, ProviderTarget::Pi, "Pi", labels.manage_providers)?;
    let cline_menu =
        build_t2_menu(app, ProviderTarget::Cline, "Cline", labels.manage_providers)?;
    let profiles_menu = build_profiles_menu(app, labels.projects)?;

    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let sep1 =
        PredefinedMenuItem::separator(app).map_err(|e| AppError::Tauri(e.to_string()))?;
    let sep2 =
        PredefinedMenuItem::separator(app).map_err(|e| AppError::Tauri(e.to_string()))?;
    let sep3 =
        PredefinedMenuItem::separator(app).map_err(|e| AppError::Tauri(e.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    Menu::with_items(
        app,
        &[
            &show,
            &sep1,
            &code_menu,
            &codex_menu,
            &opencode_menu,
            &pi_menu,
            &cline_menu,
            &sep2,
            &profiles_menu,
            &sep3,
            &quit,
        ],
    )
    .map_err(|e| AppError::Tauri(e.to_string()))
}

#[derive(Debug, PartialEq, Eq)]
struct TrayLabels {
    show: &'static str,
    official: &'static str,
    projects: &'static str,
    quit: &'static str,
    manage_providers: &'static str,
}

fn tray_labels(language: &str) -> TrayLabels {
    if language == "en-US" {
        TrayLabels {
            show: "Open AI-Switcher",
            official: "Official login",
            projects: "Projects",
            quit: "Quit",
            manage_providers: "Manage Providers...",
        }
    } else {
        TrayLabels {
            show: "打开 AI-Switcher",
            official: "官方登录",
            projects: "项目",
            quit: "退出",
            manage_providers: "前往供应商页...",
        }
    }
}

pub(crate) fn is_upstream_valid_for_direct_target(
    target: ProviderTarget,
    protocol: ProtocolType,
    is_codex_oauth: bool,
) -> bool {
    if is_codex_oauth {
        return false;
    }
    match target {
        ProviderTarget::ClaudeCode => protocol == ProtocolType::Anthropic,
        ProviderTarget::Codex => matches!(
            protocol,
            ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses
        ),
        _ => false,
    }
}

fn target_provider_prefix_and_ids(
    target: ProviderTarget,
) -> AppResult<(&'static str, &'static str, &'static str)> {
    match target {
        ProviderTarget::ClaudeCode => Ok((CODE_PROVIDER_PREFIX, CODE_OFFICIAL_ID, CODE_GATEWAY_ID)),
        ProviderTarget::Codex => Ok((CODEX_PROVIDER_PREFIX, CODEX_OFFICIAL_ID, CODEX_GATEWAY_ID)),
        ProviderTarget::ClaudeDesktop | ProviderTarget::Dsh => Err(AppError::Config(
            "Claude Desktop 与 DSH 在产品面隐藏，不显示在托盘菜单中".to_string(),
        )),
        ProviderTarget::OpenCode | ProviderTarget::Pi | ProviderTarget::Cline => {
            Err(AppError::Config(
                "T2 Agent 仅提供跳转供应商页，不在此构建独立供应商切换菜单".to_string(),
            ))
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
    let (prefix, official_id, gateway_id) = target_provider_prefix_and_ids(target)?;
    let (binding, providers) = state.db.with_read_conn(|conn| {
        let binding = crate::database::dao::gateway::binding_for_target(conn, target)?;
        let mut providers = crate::database::dao::gateway::list_upstream_providers(conn, true)?;
        providers.retain(|provider| {
            is_upstream_valid_for_direct_target(
                target,
                provider.protocol_type,
                provider.is_codex_oauth(),
            )
        });
        for provider in &mut providers {
            provider.is_current = binding
                .as_ref()
                .is_some_and(|b| b.mode == "direct" && b.direct_upstream_id == provider.id);
        }
        Ok((binding, providers))
    })?;

    let is_gateway = binding.as_ref().is_some_and(|b| b.mode == "gateway");
    let is_official = binding.is_none();

    let official_text = if is_official {
        format!("✓ {official_label}")
    } else {
        official_label.to_string()
    };
    let official = MenuItem::with_id(app, official_id, official_text, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    let gateway_label = if crate::commands::system::read_app_language(&state.db)? == "en-US" {
        "Smart Gateway"
    } else {
        "智能网关"
    };
    let gateway_text = if is_gateway {
        format!("✓ {gateway_label}")
    } else {
        gateway_label.to_string()
    };
    let gateway = MenuItem::with_id(app, gateway_id, gateway_text, true, None::<&str>)
        .map_err(|error| AppError::Tauri(error.to_string()))?;

    let mut provider_items = Vec::new();
    for provider in &providers {
        let item_label = if provider.is_current {
            format!("✓ {}", provider.name)
        } else {
            provider.name.clone()
        };
        provider_items.push(
            MenuItem::with_id(
                app,
                format!("{prefix}{}", provider.id),
                item_label,
                true,
                None::<&str>,
            )
            .map_err(|e| AppError::Tauri(e.to_string()))?,
        );
    }

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<R>> = vec![&official, &gateway];
    items.extend(provider_items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<R>));
    Submenu::with_items(app, label, true, &items)
        .map_err(|e| AppError::Tauri(e.to_string()))
}

fn build_t2_menu<R: Runtime>(
    app: &AppHandle<R>,
    target: ProviderTarget,
    label: &str,
    action_label: &str,
) -> AppResult<Submenu<R>> {
    let item_id = format!("{T2_NAV_PREFIX}{}", target.as_str());
    let item = MenuItem::with_id(app, item_id, action_label, true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let items: [&dyn tauri::menu::IsMenuItem<R>; 1] = [&item];
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

fn navigate_to_providers<R: Runtime>(app: &AppHandle<R>, target: &str) {
    show_main_window(app);
    let _ = app.emit("navigate", "providers");
    let _ = app.emit("open-providers", target);
}

async fn switch_provider<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    target: ProviderTarget,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    if target == ProviderTarget::ClaudeCode || target == ProviderTarget::Codex {
        crate::commands::providers::set_agent_direct_for_target(target, id, Some(app), &state)
            .await?;
        return Ok(());
    }
    let provider =
        crate::commands::providers::switch_provider_for_target(id, target, Some(app), &state)
            .await?;
    crate::commands::providers::schedule_provider_health_check(
        app.clone(),
        provider.provider,
        std::sync::Arc::clone(&state.db),
    );
    Ok(())
}

async fn switch_to_official<R: Runtime>(app: &AppHandle<R>, target: ProviderTarget) -> AppResult<()> {
    let state = app.state::<AppState>();
    crate::commands::providers::switch_to_official_for_target(target, Some(app), &state).await
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
        assert_eq!(tray_labels("zh-CN").manage_providers, "前往供应商页...");
        assert_eq!(tray_labels("en-US").quit, "Quit");
        assert_eq!(tray_labels("en-US").manage_providers, "Manage Providers...");
        assert_eq!(tray_labels("unsupported"), tray_labels("zh-CN"));
    }

    #[test]
    fn upstream_validation_for_direct_target_enforces_agent_boundaries() {
        // Claude Code: Anthropic only, no Codex OAuth
        assert!(is_upstream_valid_for_direct_target(
            ProviderTarget::ClaudeCode,
            ProtocolType::Anthropic,
            false,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::ClaudeCode,
            ProtocolType::OpenAiChat,
            false,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::ClaudeCode,
            ProtocolType::OpenAiResponses,
            false,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::ClaudeCode,
            ProtocolType::Anthropic,
            true,
        ));

        // Codex: OpenAI protocols only, no Anthropic, no Codex OAuth
        assert!(is_upstream_valid_for_direct_target(
            ProviderTarget::Codex,
            ProtocolType::OpenAiChat,
            false,
        ));
        assert!(is_upstream_valid_for_direct_target(
            ProviderTarget::Codex,
            ProtocolType::OpenAiResponses,
            false,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::Codex,
            ProtocolType::Anthropic,
            false,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::Codex,
            ProtocolType::OpenAiChat,
            true,
        ));
        assert!(!is_upstream_valid_for_direct_target(
            ProviderTarget::Codex,
            ProtocolType::OpenAiResponses,
            true,
        ));

        // T2 and T3 targets are never valid direct targets for the provider menu
        for target in [
            ProviderTarget::ClaudeDesktop,
            ProviderTarget::Dsh,
            ProviderTarget::OpenCode,
            ProviderTarget::Pi,
            ProviderTarget::Cline,
        ] {
            assert!(!is_upstream_valid_for_direct_target(
                target,
                ProtocolType::Anthropic,
                false
            ));
            assert!(!is_upstream_valid_for_direct_target(
                target,
                ProtocolType::OpenAiChat,
                false
            ));
        }
    }

    #[test]
    fn target_provider_prefix_and_ids_distinguishes_t1_and_rejects_others() {
        let (code_prefix, code_off, code_gw) =
            target_provider_prefix_and_ids(ProviderTarget::ClaudeCode).expect("Claude Code is T1");
        assert_eq!(code_prefix, CODE_PROVIDER_PREFIX);
        assert_eq!(code_off, CODE_OFFICIAL_ID);
        assert_eq!(code_gw, CODE_GATEWAY_ID);

        let (codex_prefix, codex_off, codex_gw) =
            target_provider_prefix_and_ids(ProviderTarget::Codex).expect("Codex is T1");
        assert_eq!(codex_prefix, CODEX_PROVIDER_PREFIX);
        assert_eq!(codex_off, CODEX_OFFICIAL_ID);
        assert_eq!(codex_gw, CODEX_GATEWAY_ID);

        // Retired T3 targets are rejected
        assert!(target_provider_prefix_and_ids(ProviderTarget::ClaudeDesktop).is_err());
        assert!(target_provider_prefix_and_ids(ProviderTarget::Dsh).is_err());

        // T2 targets only provide navigation, no direct provider menu
        assert!(target_provider_prefix_and_ids(ProviderTarget::OpenCode).is_err());
        assert!(target_provider_prefix_and_ids(ProviderTarget::Pi).is_err());
        assert!(target_provider_prefix_and_ids(ProviderTarget::Cline).is_err());
    }

    #[test]
    fn menu_id_prefixes_do_not_collide() {
        assert!(!CODEX_PROVIDER_PREFIX.starts_with(CODE_PROVIDER_PREFIX));
        assert!(!CODE_PROVIDER_PREFIX.starts_with(CODEX_PROVIDER_PREFIX));
        assert!(!T2_NAV_PREFIX.starts_with(CODE_PROVIDER_PREFIX));
        assert!(!T2_NAV_PREFIX.starts_with(CODEX_PROVIDER_PREFIX));
        assert!(!PROFILE_PREFIX.starts_with(CODE_PROVIDER_PREFIX));
        assert!(!PROFILE_PREFIX.starts_with(CODEX_PROVIDER_PREFIX));

        assert!(CODE_GATEWAY_ID.starts_with(CODE_PROVIDER_PREFIX));
        assert!(CODE_OFFICIAL_ID.starts_with(CODE_PROVIDER_PREFIX));
        assert!(CODEX_GATEWAY_ID.starts_with(CODEX_PROVIDER_PREFIX));
        assert!(CODEX_OFFICIAL_ID.starts_with(CODEX_PROVIDER_PREFIX));
    }

    #[test]
    fn t2_navigation_ids_format_correctly() {
        let opencode_id = format!("{T2_NAV_PREFIX}{}", ProviderTarget::OpenCode.as_str());
        let pi_id = format!("{T2_NAV_PREFIX}{}", ProviderTarget::Pi.as_str());
        let cline_id = format!("{T2_NAV_PREFIX}{}", ProviderTarget::Cline.as_str());

        assert_eq!(opencode_id, "t2-nav:opencode");
        assert_eq!(pi_id, "t2-nav:pi");
        assert_eq!(cline_id, "t2-nav:cline");

        assert_eq!(&opencode_id[T2_NAV_PREFIX.len()..], "opencode");
        assert_eq!(&pi_id[T2_NAV_PREFIX.len()..], "pi");
        assert_eq!(&cline_id[T2_NAV_PREFIX.len()..], "cline");
    }
}
