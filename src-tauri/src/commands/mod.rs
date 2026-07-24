//! Tauri command handlers exposed to the frontend.

pub mod backend;
pub mod backup;
pub mod db;
pub mod paths;

pub use backend::ping;
pub use backup::backup_now;
pub use db::get_db_info;
pub use paths::get_paths;
