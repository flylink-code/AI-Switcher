//! Library entry point for the Tauri app. `main.rs` is a thin binary wrapper.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod backup;
mod agents;
mod catalog;
mod antigravity;
mod claude_plugins;
mod coding;
mod codex_oauth;
mod codex_plugins;
mod commands;
mod config;
mod database;
mod deeplink;
mod error;
mod log_redact;
mod mcp;
mod mcp_oauth;
mod mcp_registry;
mod process_util;
mod prompts;
mod runtime_status;
mod provider;
mod proxy;
mod secrets;
mod session_manager;
mod skills;
mod store;
mod system_proxy;
mod tray;
mod usage;
mod usage_events;
mod wsl_direct;

#[cfg(windows)]
mod autostart_windows;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::commands::{
    check_app_update, install_app_update,
    get_codex_auth_status, get_codex_web_search_mode, add_codex_plugin_marketplace,
    list_codex_plugin_marketplaces, list_codex_plugin_catalog, list_codex_plugins,
    remove_codex_plugin_marketplace, set_codex_plugin_enabled, set_codex_web_search_mode,
    uninstall_codex_plugin, install_codex_plugin, update_codex_plugin,
    upgrade_codex_plugin_marketplace, check_codex_plugin_update, check_codex_plugin_updates,
    add_claude_plugin_marketplace, list_claude_plugin_marketplaces, list_claude_plugins,
    remove_claude_plugin_marketplace, set_claude_plugin_enabled, uninstall_claude_plugin,
    install_claude_plugin, list_claude_plugin_catalog, update_claude_plugin,
    update_claude_plugin_marketplace, check_claude_plugin_update, check_claude_plugin_updates,
    sync_codex_session_providers,
    activate_prompt, backup_now, export_library_backup, find_latest_library_archive_cmd, get_webdav_settings, preview_library_backup, restore_library_backup, restore_library_from_webdav, set_webdav_settings, upload_library_to_webdav, copy_provider_to_target, create_provider, delete_mcp_server, delete_prompt,
    delete_provider, delete_skill, check_skill_update, check_skill_updates, discover_provider_models, discover_provider_models_input,
    delete_agent, install_zip_agent, list_agents, save_agent, set_agent_enabled,
    ensure_antigravity_provider, get_antigravity_defaults, get_antigravity_gateway_status,
    get_antigravity_pool_warning, get_antigravity_recommended_account,
    import_antigravity_accounts, list_antigravity_accounts, list_antigravity_models,
    refresh_antigravity_account_quota, refresh_antigravity_quotas, remove_antigravity_account,
    set_antigravity_active_account, set_antigravity_gateway_api_key, set_antigravity_gateway_port,
    get_antigravity_limiter_settings, set_antigravity_limiter_settings,
    get_antigravity_fast_path_settings, set_antigravity_fast_path_settings,
    set_antigravity_outbound_proxy, start_antigravity_gateway, start_antigravity_oauth_login,
    stop_antigravity_gateway,
    download_desktop_localization_pack, export_providers, get_autostart_config, get_data_root,
    get_autostart_enabled, get_current_provider, get_gateway_catalog_enabled,
    get_gateway_catalog_subagent, get_gateway_catalog_hide_official, get_db_info, get_paths,
    get_cached_provider_models, get_desktop_localization_status, get_proxy_failover_enabled,
    get_proxy_retryable_status_codes, get_proxy_streaming_idle_timeout_secs, get_proxy_status,
    get_managed_apps_runtime_status, import_live_config, import_live_prompt, import_mcp_servers, import_providers_json,
    list_config_backups, preview_config_backup, restore_config_backup,
    build_provider_deeplink, build_mcp_deeplink, build_skill_deeplink, confirm_import_preview, preview_import_text,
    install_desktop_localization, install_github_repository_skills, install_github_skill, install_zip_skill,
    install_mcp_registry_server, get_mcp_desktop_conflict_status, get_mcp_oauth_status, clear_mcp_oauth,
    get_localization_hub_status, install_claude_code_localization, install_editor_localization_helper,
    get_skill_repository, get_skill_repository_snapshot, list_skill_repositories, add_skill_repository, remove_skill_repository, ignore_unmanaged_skill, list_github_repository_skills, refresh_github_repository_skills, register_unmanaged_skill, scan_unmanaged_skills, set_skill_repository, update_github_skills, list_mcp_servers, list_prompts,
    list_providers, list_skills, ping, read_live_prompt, read_prompt, rename_prompt, reorder_mcp_servers, reorder_providers,
    search_mcp_registry,
    report_frontend_performance, report_frontend_startup, save_mcp_server, save_model_pricing, save_prompt, set_autostart_config, set_autostart_enabled, set_gateway_catalog_enabled, set_gateway_catalog_subagent, set_gateway_catalog_hide_official, list_gateway_catalog_models, set_proxy_failover_enabled, set_proxy_retryable_status_codes, set_proxy_streaming_idle_timeout_secs, set_proxy_port,
    set_skill_enabled, start_proxy, stop_proxy, switch_provider, switch_to_official, speedtest_provider_endpoint, test_provider_connection, test_provider_input, batch_diagnose_providers, quarantine_failed_providers,
    toggle_mcp_server, update_provider, delete_model_pricing, get_usage_dashboard,
    export_model_pricing_xlsx, get_log_maintenance_policy, get_pricing_catalog, import_model_pricing_xlsx, list_model_pricing, list_proxy_request_logs_cmd, maintain_proxy_logs,
    preview_model_pricing_xlsx, preview_proxy_log_maintenance, rebuild_codex_session_usage_cmd, restore_desktop_localization, save_log_maintenance_policy,
    sync_codex_session_usage_cmd, sync_claude_code_session_usage_cmd, rebuild_claude_code_session_usage_cmd,
    sync_opencode_session_usage_cmd, rebuild_opencode_session_usage_cmd,
    sync_pi_session_usage_cmd, rebuild_pi_session_usage_cmd,
    sync_dsh_session_usage_cmd, rebuild_dsh_session_usage_cmd,
    select_desktop_localization_pack,
    validate_desktop_localization_pack, get_claude_code_version, get_codex_cli_version,
    get_node_runtime_status, ensure_node_runtime_via_fnm,
    run_claude_code_update, run_codex_cli_update, run_environment_doctor,
    get_opencode_cli_version, run_opencode_cli_update, get_opencode_desktop_status,
    get_dsh_cli_version, run_dsh_cli_update, start_dsh_web,
    repair_doctor_check, repair_environment_visibility,
    backup_claude_code_sessions, export_claude_code_session, export_claude_code_sessions,
    import_claude_code_session, load_session_messages,
    list_trashed_claude_code_sessions, restore_trashed_claude_code_session, scan_sessions, search_session_contents,
    trash_claude_code_session,
    backup_sessions, export_session, export_session_markdown, export_sessions, import_session, list_trashed_sessions,
    restore_trashed_session, trash_session,
    delete_sync_target, discover_wsl_distributions, get_wsl_runtime_status, list_sync_targets, preview_sync, push_sync_archive, save_sync_target, sync_wsl_direct,
    set_app_language, get_update_mirror_settings, set_update_mirror_settings,
    restart_app,
    dismiss_onboarding_tip, get_close_behavior, get_dismissed_onboarding_tips, migrate_data_root,
    resolve_close_request, restore_onboarding_tips, set_close_behavior,
    apply_profile, create_workspace_profile, delete_workspace_profile, get_current_profile_id,
    list_profiles, update_workspace_profile,
    ensure_codex_oauth_provider, list_codex_oauth_accounts, poll_codex_oauth_login,
    remove_codex_oauth_account, set_default_codex_oauth_account, start_codex_oauth_login,
    detect_pi_cli, get_global_pi_agents_md, get_pi_auth, get_pi_models, get_pi_settings,
    get_workspace_pi_prompt, install_pi_cli, list_pi_prompt_templates, list_pi_sessions, read_pi_prompt_template, read_pi_session_detail,
    save_global_pi_agents_md, save_pi_auth, save_pi_models, save_pi_prompt_template, save_workspace_pi_prompt, delete_pi_prompt_template,
    update_pi_settings,
};
use crate::error::AppError;
use crate::proxy::ProxyManager;
use crate::store::AppState;

const SESSION_USAGE_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

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
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("AI-Switcher")
                .args(["--autostart"])
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            ping,
            detect_pi_cli,
            install_pi_cli,
            get_pi_settings,
            update_pi_settings,
            get_pi_auth,
            save_pi_auth,
            get_pi_models,
            save_pi_models,
            get_global_pi_agents_md,
            save_global_pi_agents_md,
            get_workspace_pi_prompt,
            save_workspace_pi_prompt,
            list_pi_prompt_templates,
            read_pi_prompt_template,
            save_pi_prompt_template,
            delete_pi_prompt_template,
            list_pi_sessions,
            read_pi_session_detail,
            get_codex_auth_status,
            get_codex_web_search_mode,
            set_codex_web_search_mode,
            list_codex_plugins,
            set_codex_plugin_enabled,
            list_codex_plugin_marketplaces,
            list_codex_plugin_catalog,
            add_codex_plugin_marketplace,
            remove_codex_plugin_marketplace,
            upgrade_codex_plugin_marketplace,
            uninstall_codex_plugin,
            install_codex_plugin,
            update_codex_plugin,
            check_codex_plugin_update,
            check_codex_plugin_updates,
            list_claude_plugins,
            set_claude_plugin_enabled,
            list_claude_plugin_marketplaces,
            add_claude_plugin_marketplace,
            remove_claude_plugin_marketplace,
            update_claude_plugin_marketplace,
            uninstall_claude_plugin,
            install_claude_plugin,
            update_claude_plugin,
            check_claude_plugin_update,
            check_claude_plugin_updates,
            list_claude_plugin_catalog,
            sync_codex_session_providers,
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
            find_latest_library_archive_cmd,
            preview_library_backup,
            restore_library_backup,
            get_webdav_settings,
            set_webdav_settings,
            upload_library_to_webdav,
            restore_library_from_webdav,
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
            get_gateway_catalog_enabled,
            set_gateway_catalog_enabled,
            get_gateway_catalog_subagent,
            set_gateway_catalog_subagent,
            get_gateway_catalog_hide_official,
            set_gateway_catalog_hide_official,
            list_gateway_catalog_models,
            copy_provider_to_target,
            create_provider,
            update_provider,
            delete_provider,
            switch_provider,
            switch_to_official,
            test_provider_connection,
            test_provider_input,
            batch_diagnose_providers,
            quarantine_failed_providers,
            speedtest_provider_endpoint,
            discover_provider_models,
            get_cached_provider_models,
            test_provider_input,
            discover_provider_models_input,
            reorder_providers,
            import_live_config,
            export_providers,
            import_providers_json,
            preview_import_text,
            confirm_import_preview,
            build_provider_deeplink,
            build_mcp_deeplink,
            build_skill_deeplink,
            start_codex_oauth_login,
            poll_codex_oauth_login,
            list_codex_oauth_accounts,
            remove_codex_oauth_account,
            set_default_codex_oauth_account,
            ensure_codex_oauth_provider,
            list_config_backups,
            preview_config_backup,
            restore_config_backup,
            list_mcp_servers,
            save_mcp_server,
            delete_mcp_server,
            toggle_mcp_server,
            reorder_mcp_servers,
            import_mcp_servers,
            search_mcp_registry,
            install_mcp_registry_server,
            get_mcp_oauth_status,
            clear_mcp_oauth,
            get_mcp_desktop_conflict_status,
            list_prompts,
            read_prompt,
            save_prompt,
            rename_prompt,
            delete_prompt,
            activate_prompt,
            read_live_prompt,
            import_live_prompt,
            get_proxy_status,
            get_managed_apps_runtime_status,
            get_proxy_failover_enabled,
            get_proxy_retryable_status_codes,
            get_proxy_streaming_idle_timeout_secs,
            start_proxy,
            stop_proxy,
            set_proxy_port,
            set_proxy_failover_enabled,
            set_proxy_retryable_status_codes,
            set_proxy_streaming_idle_timeout_secs,
            list_antigravity_accounts,
            list_antigravity_models,
            import_antigravity_accounts,
            remove_antigravity_account,
            set_antigravity_active_account,
            get_antigravity_gateway_status,
            set_antigravity_gateway_port,
            set_antigravity_gateway_api_key,
            set_antigravity_outbound_proxy,
            get_antigravity_limiter_settings,
            set_antigravity_limiter_settings,
            get_antigravity_fast_path_settings,
            set_antigravity_fast_path_settings,
            start_antigravity_gateway,
            start_antigravity_oauth_login,
            stop_antigravity_gateway,
            refresh_antigravity_account_quota,
            refresh_antigravity_quotas,
            ensure_antigravity_provider,
            get_antigravity_defaults,
            get_antigravity_pool_warning,
            get_antigravity_recommended_account,
            list_skills,
            list_agents,
            save_agent,
            set_agent_enabled,
            delete_agent,
            install_zip_agent,
            list_skill_repositories,
            add_skill_repository,
            remove_skill_repository,
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
            scan_unmanaged_skills,
            register_unmanaged_skill,
            ignore_unmanaged_skill,
            get_autostart_enabled,
            set_autostart_enabled,
            get_autostart_config,
            set_autostart_config,
            report_frontend_performance,
            report_frontend_startup,
            get_usage_dashboard,
            sync_codex_session_usage_cmd,
            rebuild_codex_session_usage_cmd,
            sync_claude_code_session_usage_cmd,
            rebuild_claude_code_session_usage_cmd,
            sync_opencode_session_usage_cmd,
            rebuild_opencode_session_usage_cmd,
            sync_pi_session_usage_cmd,
            rebuild_pi_session_usage_cmd,
            sync_dsh_session_usage_cmd,
            rebuild_dsh_session_usage_cmd,
            sync_dsh_session_usage_cmd,
            rebuild_dsh_session_usage_cmd,
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
            get_codex_cli_version,
            run_codex_cli_update,
            get_opencode_cli_version,
            run_opencode_cli_update,
            get_opencode_desktop_status,
            get_dsh_cli_version,
            run_dsh_cli_update,
            start_dsh_web,
            get_node_runtime_status,
            ensure_node_runtime_via_fnm,
            run_environment_doctor,
            repair_doctor_check,
            repair_environment_visibility,
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
            export_session,
            export_session_markdown,
            backup_sessions,
            export_sessions,
            import_session,
            trash_session,
            restore_trashed_session,
            list_trashed_sessions,
            list_sync_targets,
            save_sync_target,
            delete_sync_target,
            discover_wsl_distributions,
            get_wsl_runtime_status,
            sync_wsl_direct,
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
            list_profiles,
            get_current_profile_id,
            create_workspace_profile,
            update_workspace_profile,
            delete_workspace_profile,
            apply_profile,
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
        if let Some(url) = argv.iter().find(|arg| commands::deeplink::looks_like_deeplink(arg)) {
            commands::deeplink::emit_deeplink_url(app, url);
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
    usage_events::init(app.handle().clone());

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
    crate::antigravity::gateway::init_gateway(Arc::clone(&db));
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
        if let Err(error) = commands::providers::repair_codex_managed_proxy_endpoint(&state).await {
            log::error!("Codex managed proxy endpoint repair failed: {error}");
        }
        if let Err(error) = commands::providers::sync_opencode_providers_to_live(&state) {
            log::warn!("启动时同步 OpenCode 供应商失败: {error}");
        }
        if let Err(error) = commands::providers::sync_pi_providers_to_live(&state) {
            log::warn!("启动时同步 Pi 供应商失败: {error}");
        }
        if let Err(error) = commands::providers::sync_dsh_providers_to_live(&state) {
            log::warn!("启动时同步 DeepSeek Harness 供应商失败: {error}");
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
        crate::antigravity::gateway::restore_gateway_if_enabled().await;
        // Post-update NSIS relaunch can still leave ports busy after the first
        // pass; recover Codex routing + proxy binding a few seconds later.
        let recover_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let state = recover_handle.state::<AppState>();
            commands::proxy::recover_runtime_after_relaunch(&recover_handle, &state).await;
            log::info!("启动后二次恢复检查完成");
        });
    });

    spawn_codex_session_usage_sync(Arc::clone(&db));
    spawn_claude_code_session_usage_sync(Arc::clone(&db));
    spawn_opencode_session_usage_sync(Arc::clone(&db));
    spawn_pi_session_usage_sync(Arc::clone(&db));
    spawn_dsh_session_usage_sync(Arc::clone(&db));
    spawn_antigravity_quota_refresh(app.handle().clone());

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

    #[cfg(desktop)]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        if let Err(error) = app.deep_link().register_all() {
            log::warn!("Deep Link 协议注册失败: {error}");
        }
        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                commands::deeplink::emit_deeplink_url(&handle, &url.to_string());
            }
        });
    }

    log::info!(
        "Tauri setup completed: duration_ms={}",
        setup_started.elapsed().as_millis()
    );

    Ok(())
}

/// Background Codex session JSONL → DB sync: first run after ~8s, then every 30s.
/// Skips overlapping runs so a slow sync cannot stack with the next tick.
/// Each file uses a short DB lock so UI queries can interleave.
fn spawn_codex_session_usage_sync(db: Arc<database::Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        loop {
            let db = Arc::clone(&db);
            let sync_result = tokio::task::spawn_blocking(move || {
                usage::session_usage_codex::try_sync_codex_session_usage_db(&db)
            })
            .await;
            match sync_result {
                Ok(Ok(result)) => {
                    if result.inserted_rows > 0 {
                        usage_events::notify_log_recorded();
                        log::info!(
                            "Codex session usage sync: scanned={}, inserted={}, skipped={}",
                            result.scanned_files,
                            result.inserted_rows,
                            result.skipped_rows
                        );
                    } else {
                        log::debug!(
                            "Codex session usage sync: scanned={}, message={}",
                            result.scanned_files,
                            result.message
                        );
                    }
                }
                Ok(Err(error)) => {
                    log::warn!("Codex session usage sync failed: {error}");
                }
                Err(error) => {
                    log::warn!("Codex session usage sync task join failed: {error}");
                }
            }
            tokio::time::sleep(SESSION_USAGE_SYNC_INTERVAL).await;
        }
    });
}

/// Background Claude Code project JSONL → DB sync: first run after ~12s, then every 30s.
fn spawn_claude_code_session_usage_sync(db: Arc<database::Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
        loop {
            let db = Arc::clone(&db);
            let sync_result = tokio::task::spawn_blocking(move || {
                usage::session_usage_claude_code::try_sync_claude_code_session_usage_db(&db)
            })
            .await;
            match sync_result {
                Ok(Ok(result)) => {
                    if result.inserted_rows > 0 {
                        usage_events::notify_log_recorded();
                        log::info!(
                            "Claude Code session usage sync: scanned={}, inserted={}, skipped={}",
                            result.scanned_files,
                            result.inserted_rows,
                            result.skipped_rows
                        );
                    } else {
                        log::debug!(
                            "Claude Code session usage sync: scanned={}, message={}",
                            result.scanned_files,
                            result.message
                        );
                    }
                }
                Ok(Err(error)) => {
                    log::warn!("Claude Code session usage sync failed: {error}");
                }
                Err(error) => {
                    log::warn!("Claude Code session usage sync task join failed: {error}");
                }
            }
            tokio::time::sleep(SESSION_USAGE_SYNC_INTERVAL).await;
        }
    });
}

/// Background OpenCode opencode.db → DB sync: first run after ~16s, then every 30s.
fn spawn_opencode_session_usage_sync(db: Arc<database::Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(16)).await;
        loop {
            let db = Arc::clone(&db);
            let sync_result = tokio::task::spawn_blocking(move || {
                usage::session_usage_opencode::try_sync_opencode_session_usage_db(&db)
            })
            .await;
            match sync_result {
                Ok(Ok(result)) => {
                    if result.inserted_rows > 0 {
                        usage_events::notify_log_recorded();
                        log::info!(
                            "OpenCode session usage sync: scanned={}, inserted={}, skipped={}",
                            result.scanned_sessions,
                            result.inserted_rows,
                            result.skipped_rows
                        );
                    } else {
                        log::debug!(
                            "OpenCode session usage sync: scanned={}, message={}",
                            result.scanned_sessions,
                            result.message
                        );
                    }
                }
                Ok(Err(error)) => {
                    log::warn!("OpenCode session usage sync failed: {error}");
                }
                Err(error) => {
                    log::warn!("OpenCode session usage sync task join failed: {error}");
                }
            }
            tokio::time::sleep(SESSION_USAGE_SYNC_INTERVAL).await;
        }
    });
}

/// Background Pi session JSONL → DB sync: first run after ~20s, then every 30s.
fn spawn_pi_session_usage_sync(db: Arc<database::Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            let db = Arc::clone(&db);
            let sync_result = tokio::task::spawn_blocking(move || {
                usage::session_usage_pi::try_sync_pi_session_usage_db(&db)
            })
            .await;
            match sync_result {
                Ok(Ok(result)) => {
                    if result.inserted_rows > 0 {
                        usage_events::notify_log_recorded();
                        log::info!(
                            "Pi session usage sync: scanned={}, inserted={}, skipped={}",
                            result.scanned_files,
                            result.inserted_rows,
                            result.skipped_rows
                        );
                    } else {
                        log::debug!(
                            "Pi session usage sync: scanned={}, message={}",
                            result.scanned_files,
                            result.message
                        );
                    }
                }
                Ok(Err(error)) => {
                    log::warn!("Pi session usage sync failed: {error}");
                }
                Err(error) => {
                    log::warn!("Pi session usage sync task join failed: {error}");
                }
            }
            tokio::time::sleep(SESSION_USAGE_SYNC_INTERVAL).await;
        }
    });
}

/// Background DeepSeek Harness compressed JSONL → DB sync: first run after ~25s, then every 30s.
fn spawn_dsh_session_usage_sync(db: Arc<database::Database>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        loop {
            let db = Arc::clone(&db);
            match tokio::task::spawn_blocking(move || {
                usage::session_usage_dsh::try_sync_dsh_session_usage_db(&db)
            }).await {
                Ok(Ok(result)) if result.inserted_rows > 0 => {
                    usage_events::notify_log_recorded();
                    log::info!(
                        "DeepSeek Harness session usage sync: scanned={}, inserted={}, skipped={}",
                        result.scanned_files, result.inserted_rows, result.skipped_rows
                    );
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => log::warn!("DeepSeek Harness session usage sync failed: {error}"),
                Err(error) => log::warn!("DeepSeek Harness session usage task join failed: {error}"),
            }
            tokio::time::sleep(SESSION_USAGE_SYNC_INTERVAL).await;
        }
    });
}

/// Background Antigravity quota refresh: first run after ~20s, then every 5 minutes.
fn spawn_antigravity_quota_refresh(app: tauri::AppHandle) {
    use crate::antigravity::{
        try_refresh_all_quotas, QUOTA_REFRESH_EVENT, QUOTA_REFRESH_INTERVAL_SECS,
    };

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let interval = std::time::Duration::from_secs(QUOTA_REFRESH_INTERVAL_SECS);
        loop {
            match try_refresh_all_quotas().await {
                Ok(summary) if summary.attempted > 0 => {
                    log::info!(
                        "Antigravity quota auto-refresh: attempted={}, ok={}, failed={}",
                        summary.attempted,
                        summary.succeeded,
                        summary.failed
                    );
                    if let Err(error) = app.emit(QUOTA_REFRESH_EVENT, ()) {
                        log::debug!("Antigravity quota refresh event emit failed: {error}");
                    }
                }
                Ok(_) => {}
                Err(error) => log::warn!("Antigravity quota auto-refresh failed: {error}"),
            }
            tokio::time::sleep(interval).await;
        }
    });
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
