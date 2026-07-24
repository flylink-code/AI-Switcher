//! System tray. P0 ships a minimal menu (show window + quit); the quick-switch
//! submenu that cycles providers is added in P5.

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::error::{AppError, AppResult};

/// Build and attach the tray icon. Called once during setup.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> AppResult<()> {
    let show = MenuItem::with_id(app, "show", "Claude Switcher", true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| AppError::Tauri(e.to_string()))?;
    let menu = Menu::with_items(app, &[&show, &quit]).map_err(|e| AppError::Tauri(e.to_string()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("missing icon"))
        .tooltip("Claude Switcher")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|e| AppError::Tauri(e.to_string()))?;

    Ok(())
}
