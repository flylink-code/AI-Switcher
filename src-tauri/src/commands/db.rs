//! Database introspection command.

use serde::Serialize;

use crate::database::dao::count_providers;
use crate::database::schema::SCHEMA_VERSION;
use crate::error::AppResult;
use crate::store::AppState;

#[derive(Debug, Serialize)]
pub struct DbInfo {
    pub path: String,
    pub schema_version: u32,
    pub provider_count: i64,
}

#[tauri::command]
pub fn get_db_info(state: tauri::State<'_, AppState>) -> AppResult<DbInfo> {
    let provider_count = state.db.with_conn(|conn| count_providers(conn))?;
    Ok(DbInfo {
        path: crate::config::paths::get_app_db_path()
            .to_string_lossy()
            .into_owned(),
        schema_version: SCHEMA_VERSION,
        provider_count,
    })
}
