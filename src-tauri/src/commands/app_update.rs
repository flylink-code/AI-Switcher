//! User-configurable, signature-verified application update checks.

use std::time::Duration;

use semver::Version;
use serde::Serialize;
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;
use url::Url;

use crate::commands::system::{get_update_mirror_settings, UpdateMirrorSettings};
use crate::error::{AppError, AppResult};
use crate::store::AppState;

const DIRECT_MANIFEST_URL: &str =
    "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest.json";
const MIRROR_MANIFEST_PATH: &str =
    "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest-mirror.json";

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
    let current = app.package_info().version.clone();

    // Prefer a lightweight version probe so "already latest" does not depend on
    // Tauri resolving platform install URLs (it still calls get_urls even when
    // remote <= current, and missing latest.json becomes ReleaseNotFound).
    match fetch_remote_release_version(&settings).await {
        Ok(remote) if remote <= current => return Ok(None),
        Ok(remote) => {
            return match configured_updater(&app, &settings)?.check().await {
                Ok(Some(update)) => Ok(Some(AppUpdateInfo {
                    version: update.version,
                })),
                Ok(None) => Ok(None),
                Err(error) => Err(AppError::Other(format!(
                    "发现新版本 {remote}，但暂时无法获取安装包，请稍后重试（{error}）"
                ))),
            };
        }
        Err(probe_error) => {
            log::warn!("update version probe failed: {probe_error}");
        }
    }

    match configured_updater(&app, &settings)?.check().await {
        Ok(update) => Ok(update.map(|update| AppUpdateInfo {
            version: update.version,
        })),
        Err(error) => {
            let detail = error.to_string();
            if updater_error_means_no_actionable_update(&detail) {
                // Manifest missing / platform package absent / release still publishing:
                // treat as "no update" instead of a scary fetch error when we cannot
                // prove a newer installable build exists.
                log::warn!("update check returned no actionable package: {detail}");
                Ok(None)
            } else {
                Err(AppError::Other(format!("检查应用更新失败: {detail}")))
            }
        }
    }
}

#[tauri::command]
pub async fn install_app_update(
    version: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    let settings = get_update_mirror_settings(state)?;
    let update = configured_updater(&app, &settings)?
        .check()
        .await
        .map_err(|error| AppError::Other(format!("重新检查应用更新失败: {error}")))?
        .ok_or_else(|| AppError::Config("当前没有可安装的应用更新".to_string()))?;
    if update.version != version {
        return Err(AppError::Config(
            "可安装版本已变化，请重新检查更新".to_string(),
        ));
    }
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| AppError::Other(format!("下载或安装应用更新失败: {error}")))
}

fn configured_updater(
    app: &tauri::AppHandle,
    settings: &UpdateMirrorSettings,
) -> AppResult<tauri_plugin_updater::Updater> {
    let endpoints = update_manifest_endpoints(settings)?;
    let app_for_exit = app.clone();
    app.updater_builder()
        .endpoints(endpoints)
        .map_err(|error| AppError::Config(format!("更新端点配置无效: {error}")))?
        // Replaces the plugin default hook: stop proxies + flush WAL before
        // Windows hard-exit so NSIS `/R` relaunch can bind ports / open DB.
        .on_before_exit(move || {
            crate::commands::proxy::prepare_for_updater_exit(&app_for_exit);
            app_for_exit.cleanup_before_exit();
        })
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
        return Err(AppError::Config(
            "GitHub 镜像地址必须为 HTTPS URL".to_string(),
        ));
    }
    let mirror = Url::parse(&format!("{}{}", settings.mirror_base, MIRROR_MANIFEST_PATH))
        .map_err(|error| AppError::Config(format!("镜像更新地址无效: {error}")))?;
    Ok(vec![mirror, direct])
}

async fn fetch_remote_release_version(settings: &UpdateMirrorSettings) -> AppResult<Version> {
    let endpoints = update_manifest_endpoints(settings)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| AppError::Other(format!("创建更新检查客户端失败: {error}")))?;

    let mut last_error = None;
    for endpoint in endpoints {
        match client.get(endpoint.clone()).send().await {
            Ok(response) if response.status().is_success() => {
                let body_text = response.text().await.map_err(|error| {
                    AppError::Other(format!("更新元数据读取失败 ({endpoint}): {error}"))
                })?;
                let body: serde_json::Value = serde_json::from_str(&body_text).map_err(|error| {
                    AppError::Other(format!("更新元数据解析失败 ({endpoint}): {error}"))
                })?;
                let Some(version_text) = body.get("version").and_then(|value| value.as_str())
                else {
                    last_error = Some(AppError::Other(format!(
                        "更新元数据缺少 version 字段 ({endpoint})"
                    )));
                    continue;
                };
                return parse_release_version(version_text);
            }
            Ok(response) => {
                last_error = Some(AppError::Other(format!(
                    "更新元数据请求失败 ({endpoint}): HTTP {}",
                    response.status()
                )));
            }
            Err(error) => {
                last_error = Some(AppError::Other(format!(
                    "更新元数据请求失败 ({endpoint}): {error}"
                )));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::Other("无法从更新通道读取版本信息".to_string())
    }))
}

fn parse_release_version(raw: &str) -> AppResult<Version> {
    let trimmed = raw.trim().trim_start_matches('v');
    Version::parse(trimmed)
        .map_err(|error| AppError::Other(format!("更新元数据 version 无效 ({raw}): {error}")))
}

fn updater_error_means_no_actionable_update(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("could not fetch a valid release json")
        || lower.contains("were found in the response `platforms`")
        || lower.contains("was not found in the response `platforms` object")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_endpoints_fall_back_to_direct_github() {
        let endpoints = update_manifest_endpoints(&UpdateMirrorSettings {
            use_mirror: true,
            mirror_base: "https://gh-proxy.com/".to_string(),
        })
        .unwrap();
        assert_eq!(endpoints.len(), 2);
        assert!(endpoints[0]
            .as_str()
            .starts_with("https://gh-proxy.com/https://github.com/"));
        assert_eq!(endpoints[1].as_str(), DIRECT_MANIFEST_URL);
    }

    #[test]
    fn direct_only_has_no_mirror_endpoint() {
        let endpoints = update_manifest_endpoints(&UpdateMirrorSettings {
            use_mirror: false,
            mirror_base: "https://gh-proxy.com/".to_string(),
        })
        .unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].as_str(), DIRECT_MANIFEST_URL);
    }

    #[test]
    fn parses_release_versions_with_optional_v_prefix() {
        assert_eq!(parse_release_version("0.7.4").unwrap().to_string(), "0.7.4");
        assert_eq!(parse_release_version("v0.7.4").unwrap().to_string(), "0.7.4");
    }

    #[test]
    fn treats_missing_manifest_as_no_actionable_update() {
        assert!(updater_error_means_no_actionable_update(
            "Could not fetch a valid release JSON from the remote"
        ));
        assert!(updater_error_means_no_actionable_update(
            "None of the fallback platforms `[\"windows-x86_64-nsis\", \"windows-x86_64\"]` were found in the response `platforms` object"
        ));
        assert!(!updater_error_means_no_actionable_update(
            "error sending request for url"
        ));
    }
}
