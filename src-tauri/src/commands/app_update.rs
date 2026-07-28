//! User-configurable, signature-verified application update checks.

use serde::Serialize;
use tauri_plugin_updater::UpdaterExt;
use url::Url;

use crate::commands::system::{get_update_mirror_settings, UpdateMirrorSettings};
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const DIRECT_MANIFEST_URL: &str = "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest.json";
const MIRROR_MANIFEST_PATH: &str = "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest-mirror.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub version: String,
}

#[tauri::command]
pub async fn check_app_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<Option<AppUpdateInfo>> {
    let settings = get_update_mirror_settings(state)?;
    let update = configured_updater(&app, &settings)?.check().await
        .map_err(|error| AppError::Other(format!("检查应用更新失败: {error}")))?;
    Ok(update.map(|update| AppUpdateInfo { version: update.version }))
}

#[tauri::command]
pub async fn install_app_update(
    version: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let settings = get_update_mirror_settings(state)?;
    let update = configured_updater(&app, &settings)?.check().await
        .map_err(|error| AppError::Other(format!("重新检查应用更新失败: {error}")))?
        .ok_or_else(|| AppError::Config("当前没有可安装的应用更新".to_string()))?;
    if update.version != version {
        return Err(AppError::Config("可安装版本已变化，请重新检查更新".to_string()));
    }
    update.download_and_install(|_, _| {}, || {}).await
        .map_err(|error| AppError::Other(format!("下载或安装应用更新失败: {error}")))
}

fn configured_updater(
    app: &tauri::AppHandle,
    settings: &UpdateMirrorSettings,
) -> AppResult<tauri_plugin_updater::Updater> {
    let endpoints = update_manifest_endpoints(settings)?;
    app.updater_builder()
        .endpoints(endpoints)
        .map_err(|error| AppError::Config(format!("更新端点配置无效: {error}")))?
        .build()
        .map_err(|error| AppError::Other(format!("创建更新器失败: {error}")))
}

pub(crate) fn update_manifest_endpoints(settings: &UpdateMirrorSettings) -> AppResult<Vec<Url>> {
    let direct = Url::parse(DIRECT_MANIFEST_URL)
        .map_err(|error| AppError::Other(format!("内置 GitHub 更新地址无效: {error}")))?;
    if !settings.use_mirror {
        return Ok(vec![direct]);
    }
    let base = Url::parse(&settings.mirror_base)
        .map_err(|error| AppError::Config(format!("GitHub 镜像地址无效: {error}")))?;
    if base.scheme() != "https" || base.host_str().is_none() {
        return Err(AppError::Config("GitHub 镜像地址必须为 HTTPS URL".to_string()));
    }
    let mirror = Url::parse(&format!("{}{}", settings.mirror_base, MIRROR_MANIFEST_PATH))
        .map_err(|error| AppError::Config(format!("镜像更新地址无效: {error}")))?;
    Ok(vec![mirror, direct])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_endpoints_fall_back_to_direct_github() {
        let endpoints = update_manifest_endpoints(&UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://gh-proxy.com/".to_string(),
        }).unwrap();
        assert_eq!(endpoints.len(), 2);
        assert!(endpoints[0].as_str().starts_with("https://gh-proxy.com/https://github.com/"));
        assert_eq!(endpoints[1].as_str(), DIRECT_MANIFEST_URL);
    }

    #[test]
    fn direct_only_has_no_mirror_endpoint() {
        let endpoints = update_manifest_endpoints(&UpdateMirrorSettings {
            use_mirror: false,
            mirror_base: "https://gh-proxy.com/".to_string(),
        }).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].as_str(), DIRECT_MANIFEST_URL);
    }
}
