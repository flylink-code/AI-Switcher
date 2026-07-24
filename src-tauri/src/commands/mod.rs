//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod backup;
pub mod db;
pub mod mcp;
pub mod paths;
pub mod prompts;
pub mod providers;
pub mod proxy;
pub mod skills;
pub mod system;
pub mod usage;

pub use backend::ping;
pub use backup::backup_now;
pub use db::get_db_info;
pub use mcp::{
    delete_mcp_server, import_mcp_servers, list_mcp_servers, save_mcp_server,
    toggle_mcp_server,
};
pub use paths::get_paths;
pub use prompts::{
    activate_prompt, delete_prompt, import_live_prompt, list_prompts, read_live_prompt,
    read_prompt, save_prompt,
};
pub use providers::{
    create_provider, delete_provider, get_current_provider, import_live_config,
    list_providers, reorder_providers, switch_provider, switch_to_official, update_provider,
};
pub use proxy::{get_proxy_status, set_proxy_port, start_proxy, stop_proxy};
pub use skills::{
    delete_skill, install_github_skill, install_zip_skill, list_skills, set_skill_enabled,
};
pub use system::{get_autostart_enabled, set_autostart_enabled};
pub use usage::{
    delete_model_pricing, get_usage_dashboard, list_model_pricing, save_model_pricing,
};
