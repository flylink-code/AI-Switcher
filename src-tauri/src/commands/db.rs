//! Database introspection command.

use serde::Serialize;

use crate::database::dao::count_providers;
use crate::database::schema::SCHEMA_VERSION;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::store::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbInfo {
    pub path: String,
    pub schema_version: u32,
    pub provider_count: i64,
}

#[tauri::command]
pub fn get_db_info(state: tauri::State<'_, AppState>) -> AppResult<DbInfo> {
    let provider_count = state.db.with_conn(|conn| {
        Ok(count_providers(conn, ProviderTarget::ClaudeCode)?
            + count_providers(conn, ProviderTarget::ClaudeDesktop)?)
    })?;
    Ok(DbInfo {
        path: crate::config::paths::get_app_db_path()
            .to_string_lossy()
            .into_owned(),
        schema_version: SCHEMA_VERSION,
        provider_count,
    })
}

/// 导出回滚库供退出新版本后恢复；不会降级运行中的库或改写 Agent 配置。
#[tauri::command]
pub async fn rollback_v34(
    destination: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let path = std::path::PathBuf::from(destination.trim());
    if !path.is_absolute() {
        return Err(AppError::Config("回滚导出必须使用绝对路径".into()));
    }
    let db = std::sync::Arc::clone(&state.db);
    crate::process_util::spawn_blocking_result(move || {
        db.export_rollback_v34(&path)?;
        Ok(path.to_string_lossy().into_owned())
    }).await
}
