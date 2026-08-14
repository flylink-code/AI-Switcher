//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod agents;
pub mod antigravity;
pub mod claude_plugins;
pub mod codex;
pub mod codex_plugins;
pub mod codex_oauth;
pub mod app_update;
pub mod backup;
pub mod db;
pub mod data_root;
pub mod deeplink;
pub mod doctor;
pub mod desktop_localization;
pub mod localization;
pub mod mcp;
pub mod paths;
pub mod profiles;
pub mod prompts;
pub mod recovery;
pub mod runtime_status;
pub mod sessions;
pub mod providers;
pub mod proxy;
pub mod skills;
pub mod system;
pub mod sync;
pub mod node_runtime;
pub mod pi;
pub mod tools;
pub mod usage;

pub use backend::ping;
pub use agents::{
    delete_agent, install_zip_agent, list_agents, save_agent, set_agent_enabled,
};
pub use antigravity::{
    ensure_antigravity_provider, get_antigravity_defaults, get_antigravity_gateway_status,
    import_antigravity_accounts, list_antigravity_accounts, list_antigravity_models,
    refresh_antigravity_account_quota, refresh_antigravity_quotas, remove_antigravity_account,
    set_antigravity_active_account, set_antigravity_gateway_api_key, set_antigravity_gateway_port,
    set_antigravity_outbound_proxy, start_antigravity_gateway,
    start_antigravity_oauth_login, stop_antigravity_gateway,
};
pub use claude_plugins::{
    add_claude_plugin_marketplace, check_claude_plugin_update, check_claude_plugin_updates,
    install_claude_plugin, list_claude_plugin_catalog, list_claude_plugin_marketplaces,
    list_claude_plugins, remove_claude_plugin_marketplace, set_claude_plugin_enabled,
    uninstall_claude_plugin, update_claude_plugin, update_claude_plugin_marketplace,
};
pub use codex::{
    get_codex_auth_status, get_codex_web_search_mode, set_codex_web_search_mode,
    sync_codex_session_providers,
};
pub use codex_plugins::{
    add_codex_plugin_marketplace, check_codex_plugin_update, check_codex_plugin_updates,
    install_codex_plugin, list_codex_plugin_catalog, list_codex_plugin_marketplaces,
    list_codex_plugins, remove_codex_plugin_marketplace, set_codex_plugin_enabled,
    uninstall_codex_plugin, update_codex_plugin, upgrade_codex_plugin_marketplace,
};
pub use codex_oauth::{
    ensure_codex_oauth_provider, list_codex_oauth_accounts, poll_codex_oauth_login,
    remove_codex_oauth_account, set_default_codex_oauth_account, start_codex_oauth_login,
};
pub use app_update::{check_app_update, install_app_update};
pub use backup::{
    backup_now, export_library_backup, find_latest_library_archive_cmd, preview_library_backup,
    restore_library_backup,
};
pub use db::get_db_info;
pub use data_root::{get_data_root, migrate_data_root};
pub use deeplink::{
    build_mcp_deeplink, build_provider_deeplink, build_skill_deeplink, confirm_import_preview, preview_import_text,
};
pub use doctor::{
    repair_doctor_check, repair_environment_visibility, run_environment_doctor,
};
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
    clear_mcp_oauth, delete_mcp_server, get_mcp_desktop_conflict_status, get_mcp_oauth_status,
    import_mcp_servers, install_mcp_registry_server, list_mcp_servers, reorder_mcp_servers,
    save_mcp_server, search_mcp_registry, toggle_mcp_server,
};
pub use paths::get_paths;
pub use profiles::{
    apply_profile, apply_profile_for_id, create_workspace_profile, delete_workspace_profile,
    get_current_profile_id, list_profiles, update_workspace_profile, ApplyProfileResult,
};
pub use prompts::{
    activate_prompt, delete_prompt, import_live_prompt, list_prompts, read_live_prompt,
    read_prompt, rename_prompt, save_prompt,
};
pub use recovery::{list_config_backups, preview_config_backup, restore_config_backup};
pub use runtime_status::get_managed_apps_runtime_status;
pub use sessions::{
    backup_claude_code_sessions, export_claude_code_session, export_claude_code_sessions,
    import_claude_code_session, load_session_messages,
    list_trashed_claude_code_sessions, restore_trashed_claude_code_session, scan_sessions, search_session_contents,
    trash_claude_code_session,
    backup_sessions, export_session, export_session_markdown, export_sessions, import_session, list_trashed_sessions,
    restore_trashed_session, trash_session,
};
pub use providers::{
    batch_diagnose_providers, copy_provider_to_target, create_provider, delete_provider, discover_provider_models, discover_provider_models_input, export_providers,
    get_cached_provider_models, get_current_provider, import_live_config, import_providers_json,
    list_providers, quarantine_failed_providers, reorder_providers, speedtest_provider_endpoint, switch_provider, switch_to_official,
    test_provider_connection, test_provider_input, update_provider,
};
pub use proxy::{
    get_proxy_failover_enabled, get_proxy_retryable_status_codes, get_proxy_streaming_idle_timeout_secs,
    get_proxy_status, set_proxy_failover_enabled, set_proxy_retryable_status_codes,
    set_proxy_streaming_idle_timeout_secs, set_proxy_port, start_proxy, stop_proxy,
};
pub use skills::{
    add_skill_repository, check_skill_update, check_skill_updates, delete_skill, get_skill_repository,
    get_skill_repository_snapshot, ignore_unmanaged_skill, install_github_repository_skills,
    install_github_skill, install_zip_skill, list_github_repository_skills, list_skill_repositories,
    list_skills, refresh_github_repository_skills, register_unmanaged_skill, remove_skill_repository,
    scan_unmanaged_skills, set_skill_enabled, set_skill_repository, update_github_skills,
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
pub use node_runtime::{ensure_node_runtime_via_fnm, get_node_runtime_status};
pub use pi::{
    delete_pi_prompt_template, detect_pi_cli, get_global_pi_agents_md, get_pi_auth, get_pi_models, get_pi_settings,
    get_workspace_pi_prompt, install_pi_cli, list_pi_prompt_templates, list_pi_sessions, read_pi_prompt_template, read_pi_session_detail,
    save_global_pi_agents_md, save_pi_auth, save_pi_models, save_pi_prompt_template, save_workspace_pi_prompt,
    update_pi_settings,
};
pub use tools::{
    get_claude_code_version, get_codex_cli_version, run_claude_code_update, run_codex_cli_update,
    get_opencode_cli_version, run_opencode_cli_update, get_opencode_desktop_status,
    get_dsh_cli_version, run_dsh_cli_update, start_dsh_web, DshCliVersionInfo,
};
pub use usage::{
    delete_model_pricing, export_model_pricing_xlsx, get_log_maintenance_policy, get_pricing_catalog, get_usage_dashboard, import_model_pricing_xlsx, list_model_pricing,
    list_proxy_request_logs_cmd, maintain_proxy_logs, preview_proxy_log_maintenance,
    rebuild_codex_session_usage_cmd, sync_codex_session_usage_cmd,
    rebuild_claude_code_session_usage_cmd, sync_claude_code_session_usage_cmd,
    rebuild_opencode_session_usage_cmd, sync_opencode_session_usage_cmd,
    rebuild_pi_session_usage_cmd, sync_pi_session_usage_cmd,
    preview_model_pricing_xlsx, save_log_maintenance_policy, save_model_pricing,
};
