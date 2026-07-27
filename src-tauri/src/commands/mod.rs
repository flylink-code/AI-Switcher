//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod backup;
pub mod db;
pub mod desktop_localization;
pub mod mcp;
pub mod paths;
pub mod prompts;
pub mod recovery;
pub mod providers;
pub mod proxy;
pub mod skills;
pub mod system;
pub mod tools;
pub mod usage;

pub use backend::ping;
pub use backup::backup_now;
pub use db::get_db_info;
pub use desktop_localization::{
    download_desktop_localization_pack, get_desktop_localization_status,
    install_desktop_localization,
    restore_desktop_localization, select_desktop_localization_pack,
    validate_desktop_localization_pack,
};
pub use mcp::{
    delete_mcp_server, import_mcp_servers, list_mcp_servers, save_mcp_server,
    toggle_mcp_server,
};
pub use paths::get_paths;
pub use prompts::{
    activate_prompt, delete_prompt, import_live_prompt, list_prompts, read_live_prompt,
    read_prompt, save_prompt,
};
pub use recovery::{list_config_backups, preview_config_backup, restore_config_backup};
pub use providers::{
    create_provider, delete_provider, discover_provider_models, discover_provider_models_input, export_providers,
    get_cached_provider_models, get_current_provider, import_live_config, import_providers_json,
    list_providers, reorder_providers, switch_provider, switch_to_official,
    test_provider_connection, test_provider_input, update_provider,
};
pub use proxy::{get_proxy_status, set_proxy_port, start_proxy, stop_proxy};
pub use skills::{
    delete_skill, install_github_skill, install_zip_skill, list_skills, set_skill_enabled,
};
pub use system::{
    get_autostart_config, get_autostart_enabled, report_frontend_performance,
    report_frontend_startup,
    set_autostart_config, set_autostart_enabled,
};
pub use tools::{get_claude_code_version, run_claude_code_update};
pub use usage::{
    delete_model_pricing, get_log_maintenance_policy, get_usage_dashboard, list_model_pricing,
    list_proxy_request_logs_cmd, maintain_proxy_logs, preview_proxy_log_maintenance,
    save_log_maintenance_policy, save_model_pricing,
};
