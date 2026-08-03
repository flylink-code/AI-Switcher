//! Environment diagnostics for Claude Code and Codex configuration.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use toml_edit::DocumentMut;

use crate::commands::tools::{
    get_claude_code_version, get_codex_cli_version, run_claude_doctor_output,
};
use crate::config::{
    get_claude_settings_path, get_codex_auth_path, get_codex_config_dir, get_codex_config_path,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

#[tauri::command]
pub async fn run_environment_doctor() -> AppResult<DoctorReport> {
    let mut checks = Vec::new();

    let claude_info = get_claude_code_version(Some(false)).await?;
    checks.push(DoctorCheck {
        id: "claude_cli".into(),
        label: "Claude Code CLI".into(),
        ok: claude_info.installed && !claude_info.installed_but_broken,
        detail: match (
            claude_info.installed,
            claude_info.current_version.as_deref(),
            claude_info.executable_path.as_deref(),
            claude_info.error.as_deref(),
        ) {
            (true, Some(version), Some(path), _) => format!("已安装 {version} @ {path}"),
            (true, Some(version), None, _) => format!("已安装 {version}"),
            (true, None, Some(path), _) => format!("已检测到可执行文件: {path}"),
            (_, _, _, Some(error)) => error.to_string(),
            _ => "未检测到 Claude Code".into(),
        },
    });

    checks.push(match run_claude_doctor_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}").trim().to_string();
            let detail = truncate(&combined, 1200);
            DoctorCheck {
                id: "claude_doctor".into(),
                label: "claude doctor".into(),
                ok: output.status.success(),
                detail: if detail.is_empty() {
                    if output.status.success() {
                        "通过".into()
                    } else {
                        format!("退出码 {:?}", output.status.code())
                    }
                } else {
                    detail
                },
            }
        }
        Err(error) => DoctorCheck {
            id: "claude_doctor".into(),
            label: "claude doctor".into(),
            ok: false,
            detail: error.to_string(),
        },
    });

    checks.push(check_claude_settings());
    checks.push(check_codex_config());
    checks.push(check_codex_auth());
    checks.push(check_codex_model_catalog());

    let codex_info = get_codex_cli_version(Some(false)).await?;
    checks.push(DoctorCheck {
        id: "codex_cli".into(),
        label: "Codex CLI".into(),
        ok: codex_info.installed && !codex_info.installed_but_broken,
        detail: match (
            codex_info.installed,
            codex_info.current_version.as_deref(),
            codex_info.executable_path.as_deref(),
            codex_info.error.as_deref(),
        ) {
            (true, Some(version), Some(path), _) => format!("已安装 {version} @ {path}"),
            (true, Some(version), None, _) => format!("已安装 {version}"),
            (true, None, Some(path), _) => format!("已检测到可执行文件: {path}"),
            (_, _, _, Some(error)) => error.to_string(),
            _ => "未检测到 Codex CLI".into(),
        },
    });

    Ok(DoctorReport { checks })
}

fn check_claude_settings() -> DoctorCheck {
    let path = get_claude_settings_path();
    if !path.is_file() {
        return DoctorCheck {
            id: "claude_settings".into(),
            label: "Claude Code settings.json".into(),
            ok: true,
            detail: "文件不存在（使用默认设置）".into(),
        };
    }
    match fs::read_to_string(&path).and_then(|text| {
        serde_json::from_str::<Value>(&text).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })
    }) {
        Ok(Value::Object(map)) => {
            let spinner_ok = map
                .get("spinnerVerbs")
                .map(|value| !value.is_array())
                .unwrap_or(true);
            if spinner_ok {
                DoctorCheck {
                    id: "claude_settings".into(),
                    label: "Claude Code settings.json".into(),
                    ok: true,
                    detail: format!("JSON 对象有效 @ {}", path.display()),
                }
            } else {
                DoctorCheck {
                    id: "claude_settings".into(),
                    label: "Claude Code settings.json".into(),
                    ok: false,
                    detail: "spinnerVerbs 写成了数组，会导致整份设置失效；请执行「安装中文」或改为 {mode, verbs} 对象".into(),
                }
            }
        }
        Ok(_) => DoctorCheck {
            id: "claude_settings".into(),
            label: "Claude Code settings.json".into(),
            ok: false,
            detail: "根节点必须是 JSON 对象".into(),
        },
        Err(error) => DoctorCheck {
            id: "claude_settings".into(),
            label: "Claude Code settings.json".into(),
            ok: false,
            detail: format!("解析失败: {error}"),
        },
    }
}

fn check_codex_config() -> DoctorCheck {
    let path = get_codex_config_path();
    if !path.is_file() {
        return DoctorCheck {
            id: "codex_config".into(),
            label: "Codex config.toml".into(),
            ok: false,
            detail: format!("未找到 {}", path.display()),
        };
    }
    match fs::read_to_string(&path).and_then(|text| {
        text.parse::<DocumentMut>()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
    }) {
        Ok(doc) => {
            let provider = doc
                .get("model_provider")
                .and_then(|item| item.as_str())
                .unwrap_or("(unset)");
            DoctorCheck {
                id: "codex_config".into(),
                label: "Codex config.toml".into(),
                ok: true,
                detail: format!("TOML 有效；model_provider={provider}"),
            }
        }
        Err(error) => DoctorCheck {
            id: "codex_config".into(),
            label: "Codex config.toml".into(),
            ok: false,
            detail: format!("解析失败: {error}"),
        },
    }
}

fn check_codex_auth() -> DoctorCheck {
    let path = get_codex_auth_path();
    let ok = path.is_file();
    DoctorCheck {
        id: "codex_auth".into(),
        label: "Codex auth.json".into(),
        ok,
        detail: if ok {
            format!("已检测到登录文件 @ {}", path.display())
        } else {
            "未检测到 auth.json；官方 Codex 需先 `codex login`".into()
        },
    }
}

fn check_codex_model_catalog() -> DoctorCheck {
    let path = get_codex_config_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return DoctorCheck {
            id: "codex_catalog".into(),
            label: "Codex model catalog".into(),
            ok: true,
            detail: "无 config.toml，跳过".into(),
        };
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return DoctorCheck {
            id: "codex_catalog".into(),
            label: "Codex model catalog".into(),
            ok: false,
            detail: "config.toml 无效，无法读取 model_catalog_json".into(),
        };
    };
    let Some(relative) = doc.get("model_catalog_json").and_then(|item| item.as_str()) else {
        return DoctorCheck {
            id: "codex_catalog".into(),
            label: "Codex model catalog".into(),
            ok: true,
            detail: "未设置 model_catalog_json".into(),
        };
    };
    let catalog_path = {
        let candidate = Path::new(relative);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            get_codex_config_dir().join(candidate)
        }
    };
    let ok = catalog_path.is_file();
    DoctorCheck {
        id: "codex_catalog".into(),
        label: "Codex model catalog".into(),
        ok,
        detail: if ok {
            format!("目录文件存在: {}", catalog_path.display())
        } else {
            format!("缺少目录文件: {}", catalog_path.display())
        },
    }
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let shortened: String = trimmed.chars().take(max).collect();
    format!("{shortened}…")
}
