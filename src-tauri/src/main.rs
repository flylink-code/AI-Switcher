// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK on Wayland (common on Ubuntu 22.04+) can leave smears / ghost icons
    // when the DMABUF GPU path fails. Same workaround as Clash Verge and
    // https://v2.tauri.app/develop/debug/linux-graphics/
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    if claude_switcher_lib::run_localization_worker_if_requested() {
        return;
    }
    claude_switcher_lib::run()
}
