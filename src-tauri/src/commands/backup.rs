//! Backup trigger command.

use crate::backup::{
    backup_file, export_library_backup as export_library,
    find_latest_library_archive, preview_library_backup as preview_library,
    restore_library_backup as restore_library, LibraryArchivePreview, LibraryBackupInfo,
    LibraryRestoreResult, DEFAULT_BACKUP_KEEP,
};
use crate::config::paths::get_app_db_path;
use crate::error::{AppError, AppResult};
use crate::store::AppState;

/// Back up the app database now, returning the created backup path (or a message
/// if the source was missing).
#[tauri::command]
pub fn backup_now() -> AppResult<String> {
    let src = get_app_db_path();
    // Checkpoint WAL so the backup copy reflects the latest committed data.
    // (No-op if the DB has no WAL; safe to ignore the result.)
    if let Ok(conn) = rusqlite::Connection::open(&src) {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    match backup_file(&src, DEFAULT_BACKUP_KEEP)? {
        Some(p) => Ok(p.to_string_lossy().into_owned()),
        None => Err(AppError::Config(format!(
            "数据库文件不存在，无法备份: {}",
            src.display()
        ))),
    }
}

/// Export a versioned, portable managed-library ZIP.
/// Optional `destination_dir` writes the ZIP into that directory.
/// Optional `include_credentials` embeds resolved API keys (opt-in only).
#[tauri::command]
pub fn export_library_backup(
    destination_dir: Option<String>,
    include_credentials: Option<bool>,
) -> AppResult<LibraryBackupInfo> {
    let destination = destination_dir
        .as_deref()
        .map(std::path::Path::new)
        .filter(|path| !path.as_os_str().is_empty());
    export_library(destination, include_credentials.unwrap_or(false))
}

/// Verify a portable library ZIP before any restore workflow is allowed to
/// stage it.  This command never extracts or changes local files.
#[tauri::command]
pub fn preview_library_backup(archive_path: String) -> AppResult<LibraryArchivePreview> {
    preview_library(std::path::Path::new(&archive_path))
}

/// Replace the local managed library from a verified portable ZIP.
/// Requires an application restart before the restored database is used.
#[tauri::command]
pub fn restore_library_backup(
    archive_path: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<LibraryRestoreResult> {
    restore_library(std::path::Path::new(&archive_path), &state.db)
}

/// Resolve the newest `library-*.zip` inside a directory (for sync incoming folders).
#[tauri::command]
pub fn find_latest_library_archive_cmd(directory: String) -> AppResult<String> {
    Ok(find_latest_library_archive(std::path::Path::new(&directory))?
        .to_string_lossy()
        .into_owned())
}

const WEBDAV_URL_KEY: &str = "webdav_url";
const WEBDAV_USER_KEY: &str = "webdav_username";
const WEBDAV_PATH_KEY: &str = "webdav_remote_path";
const WEBDAV_PASSWORD_KEY: &str = "webdav_password";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSettings {
    pub url: String,
    pub username: String,
    pub remote_path: String,
    pub password_set: bool,
}

fn load_webdav_settings(state: &AppState) -> AppResult<(WebDavSettings, String)> {
    use crate::database::dao::settings::get_setting;
    state.db.with_conn(|conn| {
        let url = get_setting(conn, WEBDAV_URL_KEY)?.unwrap_or_default();
        let username = get_setting(conn, WEBDAV_USER_KEY)?.unwrap_or_default();
        let remote_path = get_setting(conn, WEBDAV_PATH_KEY)?.unwrap_or_else(|| "/library.zip".into());
        let password = get_setting(conn, WEBDAV_PASSWORD_KEY)?.unwrap_or_default();
        Ok((
            WebDavSettings {
                url,
                username,
                remote_path,
                password_set: !password.is_empty(),
            },
            password,
        ))
    })
}

#[tauri::command]
pub fn get_webdav_settings(state: tauri::State<'_, AppState>) -> AppResult<WebDavSettings> {
    Ok(load_webdav_settings(&state)?.0)
}

#[tauri::command]
pub fn set_webdav_settings(
    url: String,
    username: String,
    remote_path: String,
    password: Option<String>,
    state: tauri::State<'_, AppState>,
) -> AppResult<WebDavSettings> {
    use crate::database::dao::settings::set_setting;
    state.db.with_conn(|conn| {
        set_setting(conn, WEBDAV_URL_KEY, url.trim())?;
        set_setting(conn, WEBDAV_USER_KEY, username.trim())?;
        let path = if remote_path.trim().is_empty() {
            "/library.zip"
        } else {
            remote_path.trim()
        };
        set_setting(conn, WEBDAV_PATH_KEY, path)?;
        if let Some(password) = password {
            set_setting(conn, WEBDAV_PASSWORD_KEY, password.trim())?;
        }
        Ok(())
    })?;
    Ok(load_webdav_settings(&state)?.0)
}

fn webdav_client() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|error| AppError::Network(error.to_string()))
}

fn join_webdav_url(base: &str, path: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    format!("{base}{path}")
}

#[tauri::command]
pub fn upload_library_to_webdav(
    include_credentials: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let (settings, password) = load_webdav_settings(&state)?;
    if settings.url.trim().is_empty() {
        return Err(AppError::Config("WebDAV 地址为空".into()));
    }
    let info = export_library(None, include_credentials.unwrap_or(false))?;
    let bytes = std::fs::read(&info.archive_path)
        .map_err(|error| AppError::Io(error.to_string()))?;
    let url = join_webdav_url(&settings.url, &settings.remote_path);
    let mut request = webdav_client()?.put(&url).body(bytes);
    if !settings.username.is_empty() {
        request = request.basic_auth(&settings.username, Some(&password));
    }
    let response = request.send().map_err(|error| AppError::Network(error.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Network(format!("WebDAV 上传失败: HTTP {}", response.status())));
    }
    Ok(url)
}

#[tauri::command]
pub fn restore_library_from_webdav(state: tauri::State<'_, AppState>) -> AppResult<LibraryRestoreResult> {
    let (settings, password) = load_webdav_settings(&state)?;
    if settings.url.trim().is_empty() {
        return Err(AppError::Config("WebDAV 地址为空".into()));
    }
    // Capture settings before the restored SQLite overwrites them.
    let captured = (settings.clone(), password.clone());
    let url = join_webdav_url(&settings.url, &settings.remote_path);
    let mut request = webdav_client()?.get(&url);
    if !settings.username.is_empty() {
        request = request.basic_auth(&settings.username, Some(&password));
    }
    let response = request.send().map_err(|error| AppError::Network(error.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Network(format!("WebDAV 下载失败: HTTP {}", response.status())));
    }
    let bytes = response.bytes().map_err(|error| AppError::Network(error.to_string()))?;
    let temp = std::env::temp_dir().join("ai-switcher-webdav-restore.zip");
    std::fs::write(&temp, &bytes).map_err(|error| AppError::Io(error.to_string()))?;
    let restored = restore_library(&temp, &state.db)?;
    use crate::database::dao::settings::set_setting;
    let _ = state.db.with_conn(|conn| {
        set_setting(conn, WEBDAV_URL_KEY, captured.0.url.trim())?;
        set_setting(conn, WEBDAV_USER_KEY, captured.0.username.trim())?;
        set_setting(conn, WEBDAV_PATH_KEY, captured.0.remote_path.trim())?;
        set_setting(conn, WEBDAV_PASSWORD_KEY, captured.1.trim())?;
        Ok(())
    });
    Ok(restored)
}
