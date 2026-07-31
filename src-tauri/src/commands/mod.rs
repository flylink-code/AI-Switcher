//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod codex;
pub mod app_update;
pub mod backup;
pub mod db;
pub mod data_root;
pub mod desktop_localization;
pub mod localization;
pub mod mcp;
pub mod paths;
pub mod prompts;
pub mod recovery;
pub mod sessions;
pub mod providers;
pub mod proxy;
pub mod skills;
pub mod system;
pub mod sync;
pub mod tools;
pub mod usage;

pub use backend::ping;
pub use codex::{get_codex_auth_status, sync_codex_session_providers};
pub use app_update::{check_app_update, install_app_update};
pub use backup::{
    backup_now, export_library_backup, find_latest_library_archive_cmd, preview_library_backup,
    restore_library_backup,
};
pub use db::get_db_info;
pub use data_root::{get_data_root, migrate_data_root};
pub use desktop_localization::{
    download_desktop_localization_pack, get_desktop_localization_status,
    install_desktop_localization,
    restore_desktop_localization, select_desktop_localization_pack,
    validate_desktop_localization_pack,
};
pub use localization::{
    get_localization_hub_status, install_claude_code_localization,
    install_editor_localization_helper,
};
pub use mcp::{
    delete_mcp_server, import_mcp_servers, install_mcp_registry_server, list_mcp_servers,
    save_mcp_server, search_mcp_registry, toggle_mcp_server,
};
pub use paths::get_paths;
pub use prompts::{
    activate_prompt, delete_prompt, import_live_prompt, list_prompts, read_live_prompt,
    read_prompt, save_prompt,
};
pub use recovery::{list_config_backups, preview_config_backup, restore_config_backup};
pub use sessions::{
    backup_claude_code_sessions, export_claude_code_session, export_claude_code_sessions,
    import_claude_code_session, load_session_messages,
    list_trashed_claude_code_sessions, restore_trashed_claude_code_session, scan_sessions, search_session_contents,
    trash_claude_code_session,
    backup_sessions, export_session, export_sessions, import_session, list_trashed_sessions,
    restore_trashed_session, trash_session,
};
pub use providers::{
    create_provider, delete_provider, discover_provider_models, discover_provider_models_input, export_providers,
    get_cached_provider_models, get_current_provider, import_live_config, import_providers_json,
    list_providers, reorder_providers, switch_provider, switch_to_official,
    test_provider_connection, test_provider_input, update_provider,
};
pub use proxy::{
    get_proxy_failover_enabled, get_proxy_retryable_status_codes, get_proxy_streaming_idle_timeout_secs,
    get_proxy_status, set_proxy_failover_enabled, set_proxy_retryable_status_codes,
    set_proxy_streaming_idle_timeout_secs, set_proxy_port, start_proxy, stop_proxy,
};
pub use skills::{
    check_skill_update, check_skill_updates, delete_skill, get_skill_repository, get_skill_repository_snapshot,
    install_github_repository_skills, install_github_skill, install_zip_skill, list_github_repository_skills,
    list_skills, refresh_github_repository_skills, set_skill_enabled, set_skill_repository, update_github_skills,
};
pub use system::{
    dismiss_onboarding_tip, get_autostart_config, get_autostart_enabled, get_close_behavior,
    get_dismissed_onboarding_tips,
    report_frontend_performance, report_frontend_startup, resolve_close_request,
    restart_app, restore_onboarding_tips, set_app_language, set_autostart_config, set_autostart_enabled,
    set_close_behavior, get_update_mirror_settings, set_update_mirror_settings,
};
pub use sync::{
    delete_sync_target, discover_wsl_distributions, list_sync_targets, preview_sync,
    push_sync_archive, save_sync_target,
};
pub use tools::{
    get_claude_code_version, get_codex_cli_version, run_claude_code_update, run_codex_cli_update,
};
pub use usage::{
    delete_model_pricing, export_model_pricing_xlsx, get_log_maintenance_policy, get_pricing_catalog, get_usage_dashboard, import_model_pricing_xlsx, list_model_pricing,
    list_proxy_request_logs_cmd, maintain_proxy_logs, preview_proxy_log_maintenance,
    rebuild_codex_session_usage_cmd, sync_codex_session_usage_cmd,
    preview_model_pricing_xlsx, save_log_maintenance_policy, save_model_pricing,
};
