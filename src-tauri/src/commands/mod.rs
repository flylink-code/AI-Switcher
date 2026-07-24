//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod backup;
pub mod db;
pub mod paths;
pub mod providers;

pub use backend::ping;
pub use backup::backup_now;
pub use db::get_db_info;
pub use paths::get_paths;
pub use providers::{
    create_provider, delete_provider, get_current_provider, import_live_config,
    list_presets, list_providers, reorder_providers, switch_provider, switch_to_official,
    update_provider,
};
