//! Configuration directory discovery and safe file IO.
//!
//! Submodules are `pub` so later phases can call into them directly; the curated
//! re-exports below cover the helpers currently wired into commands.

pub mod atomic;
pub mod claude_desktop;
pub mod paths;

// P0-wired re-exports (used by commands).
#[allow(unused_imports)]
pub use claude_desktop::{detect_claude_desktop, ClaudeDesktopPaths};
pub use paths::{
    get_app_config_dir, get_app_db_path, get_backup_dir, get_claude_config_dir,
    get_claude_json_path, get_claude_settings_path, get_home_dir,
};

// Foundational helpers (used from P1+). Re-exported for convenience; the
// `#[allow(unused)]` keeps the crate warning-clean during scaffolding.
#[allow(unused_imports)]
pub use atomic::{atomic_write, read_json_file, write_json_file};
