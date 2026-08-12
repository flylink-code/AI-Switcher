//! Pi CLI 专属 Tauri 命令处理器

use serde_json::Value;
use std::process::Command;

use crate::coding::pi::config::{
    read_global_agents_md, read_pi_auth, read_pi_models, read_pi_settings, read_workspace_prompt,
    save_global_agents_md, save_pi_auth as save_pi_auth_fn, save_pi_models as save_pi_models_fn,
    save_workspace_prompt, update_pi_settings as update_pi_settings_fn,
};
use crate::coding::pi::detector::{detect_pi_cli_sync, PiCliVersionInfo};
use crate::coding::pi::session::{
    read_pi_session_file_content, scan_pi_sessions_sync, PiSessionItem,
};
use crate::error::{AppError, AppResult};
use crate::process_util::apply_no_window;

#[tauri::command]
pub async fn detect_pi_cli() -> AppResult<PiCliVersionInfo> {
    Ok(detect_pi_cli_sync())
}

#[tauri::command]
pub async fn install_pi_cli() -> AppResult<String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "npm", "install", "-g", "@earendil-works/pi-coding-agent@latest"]);
        c
    } else {
        let mut c = Command::new("npm");
        c.args(["install", "-g", "@earendil-works/pi-coding-agent@latest"]);
        c
    };
    apply_no_window(&mut cmd);

    let output = cmd
        .output()
        .map_err(|e| AppError::Io(format!("执行 npm install -g 失败: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("安装成功！\n{stdout}"))
    } else {
        Err(AppError::Config(format!(
            "安装失败:\n{stderr}\n{stdout}"
        )))
    }
}

#[tauri::command]
pub async fn get_pi_settings() -> AppResult<Value> {
    read_pi_settings()
}

#[tauri::command]
pub async fn update_pi_settings(
    default_provider: Option<String>,
    default_model: Option<String>,
    default_thinking_level: Option<String>,
    extra_patch: Option<Value>,
) -> AppResult<Value> {
    update_pi_settings_fn(default_provider, default_model, default_thinking_level, extra_patch)
}

#[tauri::command]
pub async fn get_pi_auth() -> AppResult<Value> {
    read_pi_auth()
}

#[tauri::command]
pub async fn save_pi_auth(auth_val: Value) -> AppResult<()> {
    save_pi_auth_fn(auth_val)
}

#[tauri::command]
pub async fn get_pi_models() -> AppResult<Value> {
    read_pi_models()
}

#[tauri::command]
pub async fn save_pi_models(models_val: Value) -> AppResult<()> {
    save_pi_models_fn(models_val)
}

#[tauri::command]
pub async fn get_global_pi_agents_md() -> AppResult<String> {
    read_global_agents_md()
}

#[tauri::command]
pub async fn save_global_pi_agents_md(content: String) -> AppResult<()> {
    save_global_agents_md(&content)
}

#[tauri::command]
pub async fn get_workspace_pi_prompt(
    workspace_dir: String,
) -> AppResult<Option<(String, String)>> {
    read_workspace_prompt(&workspace_dir)
}

#[tauri::command]
pub async fn save_workspace_pi_prompt(
    workspace_dir: String,
    file_name: String,
    content: String,
) -> AppResult<()> {
    save_workspace_prompt(&workspace_dir, &file_name, &content)
}

#[tauri::command]
pub async fn list_pi_sessions() -> AppResult<Vec<PiSessionItem>> {
    scan_pi_sessions_sync()
}

#[tauri::command]
pub async fn read_pi_session_detail(file_path: String) -> AppResult<String> {
    read_pi_session_file_content(&file_path)
}
