//! Application state shared across Tauri commands via `app.manage`.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::database::Database;
use crate::proxy::ProxyManager;

pub struct AppState {
    pub db: Arc<Database>,
    pub proxy: Mutex<ProxyManager>,
}
