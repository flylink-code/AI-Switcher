//! Trivial connectivity probe.

/// Always returns `"pong"`. Used by the frontend to confirm IPC is wired up.
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}
