//! Environment diagnostics for Claude Code and Codex configuration.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item};

use crate::codex_plugins;
use crate::commands::tools::{
    get_claude_code_version, get_codex_cli_version, run_claude_doctor_output,
};
use crate::config::codex;
use crate::config::codex_provider_sync;
use crate::config::{
    get_claude_config_dir, get_claude_settings_path, get_codex_auth_path, get_codex_config_dir,
    get_codex_config_path, get_codex_plugins_cache_dir, read_json_file, write_json_file,
};
use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    /// When set, Environment page shows a per-check repair button.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_action: Option<String>,
}

impl DoctorCheck {
    fn new(id: impl Into<String>, label: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ok,
            detail: detail.into(),
            repair_action: None,
        }
    }

    fn with_repair(mut self, action: impl Into<String>) -> Self {
        self.repair_action = Some(action.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibilityRepairResult {
    pub codex_provider_files: i64,
    pub codex_provider_rows: i64,
    pub codex_usage_inserted: i64,
    pub claude_code_usage_inserted: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorRepairResult {
    pub id: String,
    pub message: String,
}

#[tauri::command]
pub async fn run_environment_doctor() -> AppResult<DoctorReport> {
    let mut checks = Vec::new();

    let claude_info = get_claude_code_version(Some(false)).await?;
    checks.push(DoctorCheck::new(
        "claude_cli",
        "Claude Code CLI",
        claude_info.installed && !claude_info.installed_but_broken,
        match (
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
    ));

    checks.push(match run_claude_doctor_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}").trim().to_string();
            let detail = truncate(&combined, 1200);
            DoctorCheck::new(
                "claude_doctor",
                "claude doctor",
                output.status.success(),
                if detail.is_empty() {
                    if output.status.success() {
                        "通过".into()
                    } else {
                        format!("退出码 {:?}", output.status.code())
                    }
                } else {
                    detail
                },
            )
        }
        Err(error) => DoctorCheck::new("claude_doctor", "claude doctor", false, error.to_string()),
    });

    checks.push(check_claude_settings());
    checks.push(check_claude_code_base_url());
    checks.push(check_claude_code_projects());
    checks.push(check_codex_config());
    checks.push(check_codex_auth());
    checks.push(check_codex_model_catalog());
    checks.push(check_codex_sessions());
    checks.push(check_codex_plugins());

    let codex_info = get_codex_cli_version(Some(false)).await?;
    checks.push(DoctorCheck::new(
        "codex_cli",
        "Codex CLI",
        codex_info.installed && !codex_info.installed_but_broken,
        match (
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
    ));

    Ok(DoctorReport { checks })
}

/// Sync Codex provider visibility + session usage for Codex/Claude Code.
/// Does **not** rewrite Claude Code `ANTHROPIC_BASE_URL` (no forced proxy switch).
#[tauri::command]
pub async fn repair_environment_visibility(
    state: tauri::State<'_, AppState>,
) -> AppResult<VisibilityRepairResult> {
    let provider_sync = tauri::async_runtime::spawn_blocking(|| {
        codex_provider_sync::sync_sessions_to_provider(None, None)
    })
    .await
    .map_err(|error| {
        crate::error::AppError::Database(format!("provider sync join failed: {error}"))
    })??;

    let db = Arc::clone(&state.db);
    let codex_usage = {
        let db = Arc::clone(&db);
        tauri::async_runtime::spawn_blocking(move || {
            crate::usage::session_usage_codex::sync_codex_session_usage_db_blocking(&db)
        })
        .await
        .map_err(|error| {
            crate::error::AppError::Database(format!("codex usage join failed: {error}"))
        })??
    };
    let cc_usage = tauri::async_runtime::spawn_blocking(move || {
        crate::usage::session_usage_claude_code::sync_claude_code_session_usage_db_blocking(&db)
    })
    .await
    .map_err(|error| {
        crate::error::AppError::Database(format!("claude code usage join failed: {error}"))
    })??;

    let skipped = provider_sync.skipped_locked_files.len();
    let message = format!(
        "Codex provider sync: files={}, rows={}, locked={skipped}; Codex usage +{}; Claude Code usage +{}",
        provider_sync.changed_session_files,
        provider_sync.sqlite_rows_updated,
        codex_usage.inserted_rows,
        cc_usage.inserted_rows
    );
    Ok(VisibilityRepairResult {
        codex_provider_files: provider_sync.changed_session_files as i64,
        codex_provider_rows: provider_sync.sqlite_rows_updated as i64,
        codex_usage_inserted: codex_usage.inserted_rows,
        claude_code_usage_inserted: cc_usage.inserted_rows,
        message,
    })
}

/// Repair a single doctor check when `repair_action` is present.
#[tauri::command]
pub async fn repair_doctor_check(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<DoctorRepairResult> {
    match id.as_str() {
        "claude_settings" => {
            repair_spinner_verbs()?;
            Ok(DoctorRepairResult {
                id,
                message: "已将 spinnerVerbs 规范化为 { mode, verbs } 对象".into(),
            })
        }
        "codex_catalog" => {
            let provider = state.db.with_conn(|conn| {
                dao::get_current_provider(conn, ProviderTarget::Codex)?
                    .ok_or_else(|| AppError::Config("没有当前 Codex 供应商，无法重建 model catalog".into()))
            })?;
            let extra_models = state
                .db
                .with_conn(|conn| {
                    Ok(dao::get_provider_model_cache(conn, &provider.id)?
                        .map(|cache| cache.models)
                        .unwrap_or_default())
                })
                .unwrap_or_default();
            let path = codex::repair_model_catalog_file(&provider, &extra_models)?;
            Ok(DoctorRepairResult {
                id,
                message: format!("已重建 model catalog: {path}"),
            })
        }
        other => Err(AppError::Config(format!("检查项不支持一键修复: {other}"))),
    }
}

fn repair_spinner_verbs() -> AppResult<()> {
    let path = get_claude_settings_path();
    if !path.is_file() {
        return Err(AppError::Config("settings.json 不存在".into()));
    }
    let mut settings = read_json_file::<Value>(&path)?
        .ok_or_else(|| AppError::Config("settings.json 为空".into()))?;
    let object = settings
        .as_object_mut()
        .ok_or_else(|| AppError::Config("settings.json 根节点必须是对象".into()))?;
    normalize_spinner_verbs(object);
    write_json_file(&path, &settings)
}

fn normalize_spinner_verbs(settings: &mut Map<String, Value>) {
    let Some(Value::Array(verbs)) = settings.get("spinnerVerbs").cloned() else {
        return;
    };
    let verbs: Vec<Value> = verbs.into_iter().filter(Value::is_string).collect();
    let mut object = Map::new();
    object.insert("mode".to_string(), Value::String("replace".to_string()));
    object.insert("verbs".to_string(), Value::Array(verbs));
    settings.insert("spinnerVerbs".to_string(), Value::Object(object));
}

fn check_claude_settings() -> DoctorCheck {
    let path = get_claude_settings_path();
    if !path.is_file() {
        return DoctorCheck::new(
            "claude_settings",
            "Claude Code settings.json",
            true,
            "文件不存在（使用默认设置）",
        );
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
                DoctorCheck::new(
                    "claude_settings",
                    "Claude Code settings.json",
                    true,
                    format!("JSON 对象有效 @ {}", path.display()),
                )
            } else {
                DoctorCheck::new(
                    "claude_settings",
                    "Claude Code settings.json",
                    false,
                    "spinnerVerbs 写成了数组，会导致整份设置失效；可一键修复为 {mode, verbs} 对象",
                )
                .with_repair("claude_settings")
            }
        }
        Ok(_) => DoctorCheck::new(
            "claude_settings",
            "Claude Code settings.json",
            false,
            "根节点必须是 JSON 对象",
        ),
        Err(error) => DoctorCheck::new(
            "claude_settings",
            "Claude Code settings.json",
            false,
            format!("解析失败: {error}"),
        ),
    }
}

fn check_claude_code_base_url() -> DoctorCheck {
    let path = get_claude_settings_path();
    let base = read_claude_base_url(&path);
    let via_proxy = base.as_deref().is_some_and(|url| {
        let lower = url.to_ascii_lowercase();
        lower.contains("127.0.0.1") || lower.contains("localhost")
    });
    match base {
        Some(url) if via_proxy => DoctorCheck::new(
            "claude_code_proxy",
            "Claude Code 用量路径",
            true,
            format!("ANTHROPIC_BASE_URL 指向本机代理（{url}），可实时记账"),
        ),
        Some(url) => DoctorCheck::new(
            "claude_code_proxy",
            "Claude Code 用量路径",
            true,
            format!(
                "ANTHROPIC_BASE_URL 为外部上游（{url}）：无实时代理用量；可在用量页同步 ~/.claude/projects，或切到托管供应商启用本机代理"
            ),
        ),
        None => DoctorCheck::new(
            "claude_code_proxy",
            "Claude Code 用量路径",
            true,
            "未设置 ANTHROPIC_BASE_URL（默认官方端点）",
        ),
    }
}

fn read_claude_base_url(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

fn check_claude_code_projects() -> DoctorCheck {
    let root = get_claude_config_dir().join("projects");
    if !root.is_dir() {
        return DoctorCheck::new(
            "claude_code_projects",
            "Claude Code projects",
            false,
            format!("未找到会话目录 {}", root.display()),
        );
    }
    let count = count_jsonl_files(&root, 2_000);
    DoctorCheck::new(
        "claude_code_projects",
        "Claude Code projects",
        count > 0,
        format!("发现约 {count} 个会话 JSONL @ {}", root.display()),
    )
}

fn check_codex_config() -> DoctorCheck {
    let path = get_codex_config_path();
    if !path.is_file() {
        return DoctorCheck::new(
            "codex_config",
            "Codex config.toml",
            false,
            format!("未找到 {}", path.display()),
        );
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
            DoctorCheck::new(
                "codex_config",
                "Codex config.toml",
                true,
                format!("TOML 有效；model_provider={provider}"),
            )
        }
        Err(error) => DoctorCheck::new(
            "codex_config",
            "Codex config.toml",
            false,
            format!("解析失败: {error}"),
        ),
    }
}

fn check_codex_auth() -> DoctorCheck {
    let path = get_codex_auth_path();
    let ok = path.is_file();
    DoctorCheck::new(
        "codex_auth",
        "Codex auth.json",
        ok,
        if ok {
            format!("已检测到登录文件 @ {}", path.display())
        } else {
            "未检测到 auth.json；官方 Codex 需先 `codex login`".into()
        },
    )
}

fn check_codex_model_catalog() -> DoctorCheck {
    let path = get_codex_config_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return DoctorCheck::new("codex_catalog", "Codex model catalog", true, "无 config.toml，跳过");
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return DoctorCheck::new(
            "codex_catalog",
            "Codex model catalog",
            false,
            "config.toml 无效，无法读取 model_catalog_json",
        );
    };
    let Some(relative) = doc.get("model_catalog_json").and_then(|item| item.as_str()) else {
        return DoctorCheck::new(
            "codex_catalog",
            "Codex model catalog",
            true,
            "未设置 model_catalog_json",
        );
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
    let check = DoctorCheck::new(
        "codex_catalog",
        "Codex model catalog",
        ok,
        if ok {
            format!("目录文件存在: {}", catalog_path.display())
        } else {
            format!("缺少目录文件: {}", catalog_path.display())
        },
    );
    if ok {
        check
    } else {
        check.with_repair("codex_catalog")
    }
}

fn check_codex_sessions() -> DoctorCheck {
    let root = get_codex_config_dir().join("sessions");
    if !root.is_dir() {
        return DoctorCheck::new(
            "codex_sessions",
            "Codex sessions",
            false,
            format!("未找到 {}", root.display()),
        );
    }
    let count = count_jsonl_files(&root, 2_000);
    DoctorCheck::new(
        "codex_sessions",
        "Codex sessions",
        count > 0,
        format!("发现约 {count} 个会话 JSONL @ {}", root.display()),
    )
}

fn check_codex_plugins() -> DoctorCheck {
    let cache_path = get_codex_plugins_cache_dir();
    let config_count = count_config_plugins(&get_codex_config_path());
    let cache_count = match codex_plugins::list_plugins_snapshot() {
        Ok(snap) => snap.cache_plugin_count,
        Err(_) => count_cache_plugin_dirs(&cache_path),
    };
    let ok = config_count > 0 || cache_count > 0;
    DoctorCheck::new(
        "codex_plugins",
        "Codex plugins",
        ok,
        format!(
            "config.toml plugins={config_count}；cache={cache_count} @ {}",
            cache_path.display()
        ),
    )
}

fn count_config_plugins(path: &Path) -> usize {
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return 0;
    };
    doc.get("plugins")
        .and_then(Item::as_table)
        .map(|table| table.len())
        .unwrap_or(0)
}

fn count_cache_plugin_dirs(cache_root: &Path) -> usize {
    if !cache_root.is_dir() {
        return 0;
    }
    let mut count = 0usize;
    let Ok(marketplaces) = fs::read_dir(cache_root) else {
        return 0;
    };
    for market in marketplaces.flatten() {
        if !market.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(plugins) = fs::read_dir(market.path()) else {
            continue;
        };
        for plugin in plugins.flatten() {
            if plugin.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                count += 1;
            }
        }
    }
    count
}

fn count_jsonl_files(root: &Path, limit: usize) -> usize {
    let mut count = 0usize;
    count_jsonl_files_rec(root, limit, &mut count);
    count
}

fn count_jsonl_files_rec(directory: &Path, limit: usize, count: &mut usize) {
    if *count >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if *count >= limit {
            break;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            count_jsonl_files_rec(&path, limit, count);
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name.ends_with(".jsonl") && !name.starts_with("agent-") {
            *count += 1;
        }
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
