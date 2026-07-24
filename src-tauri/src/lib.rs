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
mod prompts;
mod provider;
mod provider_presets;
mod proxy;
mod secrets;
mod skills;
mod store;
mod tray;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use crate::commands::{
    activate_prompt, backup_now, create_provider, delete_mcp_server, delete_prompt,
    delete_provider, delete_skill, discover_provider_models, export_providers, get_autostart_enabled, get_current_provider, get_db_info, get_paths,
    get_proxy_status, import_live_config, import_live_prompt, import_mcp_servers, import_providers_json,
    list_config_backups, preview_config_backup, restore_config_backup,
    install_github_skill, install_zip_skill, list_mcp_servers, list_prompts,
    list_providers, list_skills, ping, read_live_prompt, read_prompt, reorder_providers,
    save_mcp_server, save_model_pricing, save_prompt, set_autostart_enabled, set_proxy_port,
    set_skill_enabled, start_proxy, stop_proxy, switch_provider, switch_to_official, test_provider_connection,
    toggle_mcp_server, update_provider, delete_model_pricing, get_usage_dashboard,
    list_model_pricing,
};
use crate::error::AppError;
use crate::proxy::ProxyManager;
use crate::store::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            ping,
            get_paths,
            get_db_info,
            backup_now,
            list_providers,
            get_current_provider,
            create_provider,
            update_provider,
            delete_provider,
            switch_provider,
            switch_to_official,
            test_provider_connection,
            discover_provider_models,
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
            list_prompts,
            read_prompt,
            save_prompt,
            delete_prompt,
            activate_prompt,
            read_live_prompt,
            import_live_prompt,
            get_proxy_status,
            start_proxy,
            stop_proxy,
            set_proxy_port,
            list_skills,
            install_github_skill,
            install_zip_skill,
            set_skill_enabled,
            delete_skill,
            get_autostart_enabled,
            set_autostart_enabled,
            get_usage_dashboard,
            list_model_pricing,
            save_model_pricing,
            delete_model_pricing,
        ]);
    let builder = add_single_instance(builder);
    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Single-instance guard + DB init + tray. Windows/macOS/Linux only.
#[cfg(desktop)]
fn add_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
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
    // Ensure the data directory exists before opening the DB.
    let app_config_dir = config::get_app_config_dir();
    std::fs::create_dir_all(&app_config_dir)?;
    std::fs::create_dir_all(config::get_backup_dir())?;

    // Initialize storage.
    let db = std::sync::Arc::new(database::Database::init().map_err(box_app_error)?);

    // First-run seeding + live-config import. Non-fatal: a seeding failure should
    // not block the app, only log.
    if let Err(e) = db.with_conn(|conn| database::seed::run_seed(conn)) {
        log::error!("供应商初始化/导入失败: {e}");
    }

    app.manage(AppState {
        db: Arc::clone(&db),
        proxy: tokio::sync::Mutex::new(ProxyManager::new(Arc::clone(&db))),
    });

    // System tray.
    if let Err(e) = tray::build_tray(app.handle()) {
        log::error!("托盘初始化失败: {e}");
    }

    Ok(())
}

/// Minimize-to-tray on close (kept simple for P0; a setting will toggle this in P5).
fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        window.hide().ok();
        api.prevent_close();
    }
}

/// Promote an [`AppError`] into a boxed error for the setup signature.
fn box_app_error(e: AppError) -> Box<dyn std::error::Error> {
    Box::<dyn std::error::Error>::from(e.to_string())
}
