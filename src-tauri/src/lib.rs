//! Library entry point for the Tauri app. `main.rs` is a thin binary wrapper.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod backup;
mod commands;
mod config;
mod database;
mod error;
mod mcp;
mod mcp_registry;
mod prompts;
mod provider;
mod proxy;
mod secrets;
mod session_manager;
mod skills;
mod store;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::commands::{
    check_app_update, install_app_update,
    activate_prompt, backup_now, export_library_backup, preview_library_backup, create_provider, delete_mcp_server, delete_prompt,
    delete_provider, delete_skill, check_skill_update, check_skill_updates, discover_provider_models, discover_provider_models_input,
    download_desktop_localization_pack, export_providers, get_autostart_config, get_data_root,
    get_autostart_enabled, get_current_provider, get_db_info, get_paths,
    get_cached_provider_models, get_desktop_localization_status, get_proxy_failover_enabled, get_proxy_status, import_live_config, import_live_prompt, import_mcp_servers, import_providers_json,
    list_config_backups, preview_config_backup, restore_config_backup,
    install_desktop_localization, install_github_repository_skills, install_github_skill, install_zip_skill,
    install_mcp_registry_server,
    get_localization_hub_status, install_claude_code_localization, install_editor_localization_helper,
    get_skill_repository, get_skill_repository_snapshot, list_github_repository_skills, refresh_github_repository_skills, set_skill_repository, update_github_skills, list_mcp_servers, list_prompts,
    list_providers, list_skills, ping, read_live_prompt, read_prompt, reorder_providers,
    search_mcp_registry,
    report_frontend_performance, report_frontend_startup, save_mcp_server, save_model_pricing, save_prompt, set_autostart_config, set_autostart_enabled, set_proxy_failover_enabled, set_proxy_port,
    set_skill_enabled, start_proxy, stop_proxy, switch_provider, switch_to_official, test_provider_connection, test_provider_input,
    toggle_mcp_server, update_provider, delete_model_pricing, get_usage_dashboard,
    export_model_pricing_xlsx, get_log_maintenance_policy, get_pricing_catalog, import_model_pricing_xlsx, list_model_pricing, list_proxy_request_logs_cmd, maintain_proxy_logs,
    preview_model_pricing_xlsx, preview_proxy_log_maintenance, restore_desktop_localization, save_log_maintenance_policy,
    select_desktop_localization_pack,
    validate_desktop_localization_pack, get_claude_code_version, run_claude_code_update,
    backup_claude_code_sessions, export_claude_code_session, export_claude_code_sessions,
    import_claude_code_session, load_session_messages,
    list_trashed_claude_code_sessions, restore_trashed_claude_code_session, scan_sessions, search_session_contents,
    trash_claude_code_session,
    delete_sync_target, discover_wsl_distributions, list_sync_targets, preview_sync, push_sync_archive, save_sync_target,
    set_app_language, get_update_mirror_settings, set_update_mirror_settings,
    restart_app,
    dismiss_onboarding_tip, get_close_behavior, get_dismissed_onboarding_tips, migrate_data_root,
    resolve_close_request, restore_onboarding_tips, set_close_behavior,
};
use crate::error::AppError;
use crate::proxy::ProxyManager;
use crate::store::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `generate_context!` and plugin construction happen before `Builder::run`
    // can return an error.  In a Windows release build those panics otherwise
    // look like a silent exit because there is no console window.
    std::panic::set_hook(Box::new(|panic_info| {
        report_startup_failure(&format!("Startup panic: {panic_info}"));
    }));

    let runtime_log_dir = config::get_app_config_dir().join("logs");
    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .targets([Target::new(TargetKind::Folder {
                    path: runtime_log_dir,
                    file_name: Some("runtime.log".to_string()),
                })])
                .rotation_strategy(RotationStrategy::KeepSome(5))
                .max_file_size(1_000_000)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            ping,
            check_app_update,
            install_app_update,
            get_update_mirror_settings,
            set_update_mirror_settings,
            get_paths,
            get_db_info,
            get_data_root,
            migrate_data_root,
            backup_now,
            export_library_backup,
            preview_library_backup,
            get_desktop_localization_status,
            get_localization_hub_status,
            download_desktop_localization_pack,
            validate_desktop_localization_pack,
            select_desktop_localization_pack,
            install_desktop_localization,
            restore_desktop_localization,
            install_claude_code_localization,
            install_editor_localization_helper,
            list_providers,
            get_current_provider,
            create_provider,
            update_provider,
            delete_provider,
            switch_provider,
            switch_to_official,
            test_provider_connection,
            discover_provider_models,
            get_cached_provider_models,
            test_provider_input,
            discover_provider_models_input,
            reorder_providers,
            import_live_config,
            export_providers,
            import_providers_json,
            list_config_backups,
            preview_config_backup,
            restore_config_backup,
            list_mcp_servers,
            save_mcp_server,
            delete_mcp_server,
            toggle_mcp_server,
            import_mcp_servers,
            search_mcp_registry,
            install_mcp_registry_server,
            list_prompts,
            read_prompt,
            save_prompt,
            delete_prompt,
            activate_prompt,
            read_live_prompt,
            import_live_prompt,
            get_proxy_status,
            get_proxy_failover_enabled,
            start_proxy,
            stop_proxy,
            set_proxy_port,
            set_proxy_failover_enabled,
            list_skills,
            get_skill_repository,
            get_skill_repository_snapshot,
            set_skill_repository,
            list_github_repository_skills,
            refresh_github_repository_skills,
            install_github_repository_skills,
            install_github_skill,
            install_zip_skill,
            set_skill_enabled,
            delete_skill,
            check_skill_update,
            check_skill_updates,
            update_github_skills,
            get_autostart_enabled,
            set_autostart_enabled,
            get_autostart_config,
            set_autostart_config,
            report_frontend_performance,
            report_frontend_startup,
            get_usage_dashboard,
            list_model_pricing,
            export_model_pricing_xlsx,
            preview_model_pricing_xlsx,
            import_model_pricing_xlsx,
            get_pricing_catalog,
            save_model_pricing,
            delete_model_pricing,
            maintain_proxy_logs,
            get_log_maintenance_policy,
            save_log_maintenance_policy,
            preview_proxy_log_maintenance,
            list_proxy_request_logs_cmd,
            get_claude_code_version,
            run_claude_code_update,
            scan_sessions,
            search_session_contents,
            load_session_messages,
            export_claude_code_session,
            backup_claude_code_sessions,
            export_claude_code_sessions,
            import_claude_code_session,
            trash_claude_code_session,
            restore_trashed_claude_code_session,
            list_trashed_claude_code_sessions,
            list_sync_targets,
            save_sync_target,
            delete_sync_target,
            discover_wsl_distributions,
            preview_sync,
            push_sync_archive,
            set_app_language,
            restart_app,
            get_dismissed_onboarding_tips,
            dismiss_onboarding_tip,
            restore_onboarding_tips,
            get_close_behavior,
            set_close_behavior,
            resolve_close_request,
        ]);
    let builder = add_single_instance(builder);
    if let Err(error) = builder.run(tauri::generate_context!()) {
        report_startup_failure(&error.to_string());
    }
}

pub fn run_localization_worker_if_requested() -> bool {
    commands::desktop_localization::run_worker_from_args()
}

/// Release binaries do not have a console window. Preserve startup failures in
/// a user-accessible file instead of silently terminating.
fn report_startup_failure(error: &str) {
    let directory = config::get_app_config_dir();
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join("startup-error.log");
    let report = format!(
        "AI-Switcher failed to start.\n\n{error}\n\nSee: {}\n",
        path.display()
    );
    let _ = std::fs::write(&path, &report);

    eprintln!("{report}");
}

/// Single-instance guard + DB init + tray. Windows/macOS/Linux only.
#[cfg(desktop)]
fn add_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        if argv.iter().any(|arg| arg == "--autostart") {
            return;
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }))
}

#[cfg(not(desktop))]
fn add_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let setup_started = std::time::Instant::now();
    // Ensure the data directory exists before opening the DB.
    let app_config_dir = config::get_app_config_dir();
    std::fs::create_dir_all(&app_config_dir)?;
    std::fs::create_dir_all(config::get_backup_dir())?;
    log::info!("AI-Switcher starting; data directory: {}", app_config_dir.display());

    // Initialize storage.
    let db = std::sync::Arc::new(database::Database::init().map_err(box_app_error)?);
    log::info!(
        "Database initialization completed: duration_ms={}",
        setup_started.elapsed().as_millis()
    );
    if let Err(error) = commands::system::migrate_autostart_registration(app.handle(), &db) {
        log::warn!("开机自启注册迁移失败: {error}");
    }

    // First-run seeding + live-config import. Non-fatal: a seeding failure should
    // not block the app, only log.
    if let Err(e) = db.with_conn(|conn| database::seed::run_seed(conn)) {
        log::error!("供应商初始化/导入失败: {e}");
    }

    let initial_proxy_status = commands::proxy::initial_proxy_statuses(&db);
    let (proxy_lifecycle_tx, proxy_lifecycle_rx) =
        tokio::sync::mpsc::unbounded_channel();
    app.manage(AppState {
        db: Arc::clone(&db),
        proxy: tokio::sync::Mutex::new(ProxyManager::new(
            Arc::clone(&db),
            proxy_lifecycle_tx,
        )),
        proxy_status: tokio::sync::RwLock::new(initial_proxy_status),
    });
    commands::proxy::spawn_proxy_lifecycle_listener(
        app.handle().clone(),
        proxy_lifecycle_rx,
    );

    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let background_started = std::time::Instant::now();
        // Let WebView paint the shell before touching legacy Desktop files.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let state = app_handle.state::<AppState>();
        let repair_started = std::time::Instant::now();
        if let Err(error) = commands::providers::repair_current_desktop_profile(&state).await {
            log::error!("Claude Desktop managed profile migration failed: {error}");
        }
        if let Err(error) = commands::providers::repair_current_code_model_fields(&state).await {
            log::error!("Claude Code model-field migration failed: {error}");
        }
        log::info!(
            "启动配置修复完成: duration_ms={}",
            repair_started.elapsed().as_millis()
        );
        let proxy_started = std::time::Instant::now();
        commands::proxy::ensure_runtime_proxies(&app_handle, &state).await;
        log::info!(
            "启动代理检查完成: duration_ms={}, background_total_ms={}",
            proxy_started.elapsed().as_millis(),
            background_started.elapsed().as_millis()
        );
    });

    // System tray.
    if let Err(e) = tray::build_tray(app.handle()) {
        log::error!("托盘初始化失败: {e}");
    }

    if !commands::system::is_silent_autostart(&db) {
        if let Some(window) = app.get_webview_window("main") {
            window.show().ok();
        }
    } else {
        log::info!("开机自启采用静默模式，主窗口保持隐藏");
    }
    log::info!(
        "Tauri setup completed: duration_ms={}",
        setup_started.elapsed().as_millis()
    );

    Ok(())
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let app = window.app_handle();
        let state = app.state::<AppState>();
        let behavior = commands::system::read_close_behavior(&state.db)
            .unwrap_or(commands::system::CloseBehavior::Ask);
        match behavior {
            commands::system::CloseBehavior::Ask => {
                if let Err(error) = window.emit("close-choice-requested", ()) {
                    log::error!("发送关闭选择事件失败: {error}");
                    window.hide().ok();
                }
            }
            commands::system::CloseBehavior::Tray => {
                window.hide().ok();
            }
            commands::system::CloseBehavior::Quit => {
                app.exit(0);
            }
        }
    }
}

/// Promote an [`AppError`] into a boxed error for the setup signature.
fn box_app_error(e: AppError) -> Box<dyn std::error::Error> {
    Box::<dyn std::error::Error>::from(e.to_string())
}
