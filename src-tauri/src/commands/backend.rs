//! Trivial connectivity probe.

/// Always returns `"pong"`. Used by the frontend to confirm IPC is wired up.
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

/// `windows` | `macos` | `linux`: the OS this build runs on.
#[tauri::command]
pub fn get_app_platform() -> String {
    std::env::consts::OS.to_string()
}
