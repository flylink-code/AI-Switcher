//! Data-access helpers. Kept minimal for P0 — full CRUD lands in P1+.

pub mod providers;
pub mod settings;

pub use providers::count_providers;
#[allow(unused_imports)]
pub use settings::{get_setting, set_setting};
