//! Application state shared across Tauri commands via `app.manage`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::database::Database;
use crate::provider::ProviderTarget;
use crate::proxy::{ProxyManager, ProxyStatus};

pub struct AppState {
    pub db: Arc<Database>,
    pub proxy: Mutex<ProxyManager>,
    pub proxy_status: RwLock<HashMap<ProviderTarget, ProxyStatus>>,
}
