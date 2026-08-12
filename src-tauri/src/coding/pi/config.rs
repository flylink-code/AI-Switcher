//! Pi 配置文件解析与原子读写 (`settings.json`, `auth.json`, `models.json`, `AGENTS.md`)

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::config::atomic::{ensure_dir_with_context, read_json_file, write_json_file};
use crate::config::paths::get_home_dir;
use crate::error::{AppError, AppResult};

/// 获取 Pi Agent 配置根目录
/// 默认 `~/.pi/agent`，优先支持 `PI_CODING_AGENT_DIR` 环境变量
/// 兼容 WSL UNC 路径与 Windows/Unix 异构环境
pub fn get_pi_dir() -> PathBuf {
    if let Ok(custom_dir) = env::var("PI_CODING_AGENT_DIR") {
        if !custom_dir.trim().is_empty() {
            let p = PathBuf::from(&custom_dir);
            return p;
        }
    }

    let default_home = get_home_dir().join(".pi").join("agent");
    if default_home.exists() {
        return default_home;
    }

    // Windows 环境下探测是否存在 WSL 网络位置
    #[cfg(windows)]
    {
        for wsl_prefix in ["\\\\wsl$\\Ubuntu\\home", "\\\\wsl.localhost\\Ubuntu\\home"] {
            let base = PathBuf::from(wsl_prefix);
            if base.exists() {
                if let Ok(entries) = fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        let candidate = entry.path().join(".pi").join("agent");
                        if candidate.exists() {
                            return candidate;
                        }
                    }
                }
            }
        }
    }

    default_home
}

pub fn get_pi_settings_path() -> PathBuf {
    get_pi_dir().join("settings.json")
}

pub fn get_pi_auth_path() -> PathBuf {
    get_pi_dir().join("auth.json")
}

pub fn get_pi_models_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

pub fn get_pi_global_agents_path() -> PathBuf {
    get_pi_dir().join("AGENTS.md")
}

fn pi_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_pi_config() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    pi_config_lock()
        .lock()
        .map_err(|error| AppError::Config(format!("Pi 配置锁已中毒: {error}")))
}

/// Pi settings.json 的 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiSettingsDto {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub default_thinking_level: Option<String>, // off / minimal / low / medium / high / xhigh / max
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 读取 `settings.json`
pub fn read_pi_settings() -> AppResult<Value> {
    let _guard = lock_pi_config()?;
    let path = get_pi_settings_path();
    let val = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));
    Ok(val)
}

/// 写入 `settings.json`（保留现有未知字段）
pub fn update_pi_settings(
    default_provider: Option<String>,
    default_model: Option<String>,
    default_thinking_level: Option<String>,
    extra_patch: Option<Value>,
) -> AppResult<Value> {
    let _guard = lock_pi_config()?;
    let path = get_pi_settings_path();
    let mut current = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));

    if !current.is_object() {
        current = json!({});
    }

    let obj = current.as_object_mut().unwrap();

    if let Some(dp) = default_provider {
        if dp.is_empty() {
            obj.remove("defaultProvider");
        } else {
            obj.insert("defaultProvider".to_string(), Value::String(dp));
        }
    }

    if let Some(dm) = default_model {
        if dm.is_empty() {
            obj.remove("defaultModel");
        } else {
            obj.insert("defaultModel".to_string(), Value::String(dm));
        }
    }

    if let Some(dtl) = default_thinking_level {
        if dtl.is_empty() {
            obj.remove("defaultThinkingLevel");
        } else {
            obj.insert("defaultThinkingLevel".to_string(), Value::String(dtl));
        }
    }

    if let Some(patch) = extra_patch {
        if let Some(patch_obj) = patch.as_object() {
            for (k, v) in patch_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    }

    if let Some(parent) = path.parent() {
        ensure_dir_with_context(parent)?;
    }
    write_json_file(&path, &current)?;
    Ok(current)
}

/// 读取 `auth.json`
pub fn read_pi_auth() -> AppResult<Value> {
    let _guard = lock_pi_config()?;
    let path = get_pi_auth_path();
    let val = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));
    Ok(val)
}

/// 写入 `auth.json`（合并保存，保留已有供应商和其他未知顶级扩展字段）
pub fn save_pi_auth(auth_val: Value) -> AppResult<()> {
    let _guard = lock_pi_config()?;
    let path = get_pi_auth_path();
    let mut current = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));
    if !current.is_object() {
        current = json!({});
    }

    if let (Some(cur_obj), Some(new_obj)) = (current.as_object_mut(), auth_val.as_object()) {
        for (k, v) in new_obj {
            cur_obj.insert(k.clone(), v.clone());
        }
    }

    if let Some(parent) = path.parent() {
        ensure_dir_with_context(parent)?;
    }
    write_json_file(&path, &current)?;
    Ok(())
}

/// 读取 `models.json`
pub fn read_pi_models() -> AppResult<Value> {
    let _guard = lock_pi_config()?;
    let path = get_pi_models_path();
    let val = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));
    Ok(val)
}

/// 写入 `models.json`（合并保存，保留未知顶级扩展字段如 `packages` / `extensionSettings`）
pub fn save_pi_models(models_val: Value) -> AppResult<()> {
    let _guard = lock_pi_config()?;
    let path = get_pi_models_path();
    let mut current = read_json_file::<Value>(&path)?.unwrap_or_else(|| json!({}));
    if !current.is_object() {
        current = json!({});
    }

    if let (Some(cur_obj), Some(new_obj)) = (current.as_object_mut(), models_val.as_object()) {
        for (k, v) in new_obj {
            cur_obj.insert(k.clone(), v.clone());
        }
    }

    if let Some(parent) = path.parent() {
        ensure_dir_with_context(parent)?;
    }
    write_json_file(&path, &current)?;
    Ok(())
}

/// 读取全局 Prompt (`~/.pi/agent/AGENTS.md`)
pub fn read_global_agents_md() -> AppResult<String> {
    let path = get_pi_global_agents_path();
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&path).map_err(|e| AppError::Io(format!("读取全局 AGENTS.md 失败: {e}")))
}

/// 保存全局 Prompt (`~/.pi/agent/AGENTS.md`)
pub fn save_global_agents_md(content: &str) -> AppResult<()> {
    let path = get_pi_global_agents_path();
    if let Some(parent) = path.parent() {
        ensure_dir_with_context(parent)?;
    }
    fs::write(&path, content).map_err(|e| AppError::Io(format!("保存全局 AGENTS.md 失败: {e}")))
}

/// 读取工作区 Prompt（检测项目下的 `AGENTS.md` 或 `SYSTEM.md`）
pub fn read_workspace_prompt(workspace_dir: &str) -> AppResult<Option<(String, String)>> {
    let ws_path = Path::new(workspace_dir);
    let agents_path = ws_path.join("AGENTS.md");
    if agents_path.exists() {
        let content = fs::read_to_string(&agents_path)
            .map_err(|e| AppError::Io(format!("读取工作区 AGENTS.md 失败: {e}")))?;
        return Ok(Some(("AGENTS.md".to_string(), content)));
    }
    let system_path = ws_path.join("SYSTEM.md");
    if system_path.exists() {
        let content = fs::read_to_string(&system_path)
            .map_err(|e| AppError::Io(format!("读取工作区 SYSTEM.md 失败: {e}")))?;
        return Ok(Some(("SYSTEM.md".to_string(), content)));
    }
    Ok(None)
}

/// 保存工作区 Prompt
pub fn save_workspace_prompt(workspace_dir: &str, file_name: &str, content: &str) -> AppResult<()> {
    let file_name = if file_name.eq_ignore_ascii_case("system.md") {
        "SYSTEM.md"
    } else {
        "AGENTS.md"
    };
    let path = Path::new(workspace_dir).join(file_name);
    fs::write(&path, content).map_err(|e| AppError::Io(format!("保存工作区 {file_name} 失败: {e}")))
}
