//! Application state shared across Tauri commands via `app.manage`.

use crate::database::Database;

pub struct AppState {
    pub db: Database,
}
