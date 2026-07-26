// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if claude_switcher_lib::run_localization_worker_if_requested() {
        return;
    }
    claude_switcher_lib::run()
}
