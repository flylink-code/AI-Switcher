//! VS Code Copilot（原生 Custom Endpoint）配置写入器。
//!
//! VS Code 的 `chatLanguageModels.json` 是顶层数组，可能含用户自己的 provider。
//! 本模块只维护名为 [`MANAGED_NAME`] 的 `customendpoint` 一项，其余条目原样保留。
//! 本机 VS Code 的 provider 级 `url` 自动发现只读取模型 `id` 且要求命中内置模型表，
//! 所以这里写显式 `models[]`（每项带 `url` 与能力），不写 provider 级 `url`。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::config::atomic::write_json_file;
use crate::config::paths::get_home_dir;
use crate::error::{AppError, AppResult};

/// 托管条目的固定名称，用于在用户其他 provider 中识别自己。
pub const MANAGED_NAME: &str = "AI-Switcher";
const VENDOR: &str = "customendpoint";
const PROVIDER_API_KEY_PLACEHOLDER: &str = "managed-via-requestHeaders";
const FILE_NAME: &str = "chatLanguageModels.json";
const BACKUP_KEEP: usize = 5;

fn lock() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|error| AppError::Config(format!("VS Code Copilot 配置锁已中毒: {error}")))
}

/// 写入 `models[]` 的一个模型。字段名与 VS Code 清单一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CopilotModel {
    pub id: String,
    pub name: String,
    pub tool_calling: bool,
    pub vision: bool,
    pub thinking: bool,
    /// `supportsReasoningEffort` 必须是字符串数组；空表示不支持推理档位。
    pub reasoning_efforts: Vec<String>,
    pub context_window: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
}

/// 本机 VS Code 与 GitHub Copilot 扩展探测结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VscodeInstallationInfo {
    pub vscode_installed: bool,
    pub vscode_version: Option<String>,
    pub vscode_path: Option<String>,
    pub copilot_extension_installed: bool,
}

/// 枚举当前系统可能的 VS Code `User` 配置目录（按优先级排序）。
///
/// Ubuntu / Linux 下兼容官方 `.deb` / APT（`~/.config/Code/User`）、Snap 沙盒版
/// （`~/snap/code/current/.config/Code/User`）、Flatpak（`~/.var/app/com.visualstudio.code/config/Code/User`）、
/// VS Code Insiders 与 VSCodium。
pub fn candidate_vscode_user_dirs() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("AISW_VSCODE_USER_DIR").filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(dir)];
    }
    let home = get_home_dir();
    let isolated = crate::config::paths::test_isolation_enabled();

    #[cfg(windows)]
    {
        let base = if isolated {
            home.join("AppData").join("Roaming")
        } else {
            std::env::var_os("APPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        };
        vec![
            base.join("Code").join("User"),
            base.join("Code - Insiders").join("User"),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        let base = home.join("Library").join("Application Support");
        vec![
            base.join("Code").join("User"),
            base.join("Code - Insiders").join("User"),
        ]
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let xdg_config = if isolated {
            home.join(".config")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
        };
        let mut dirs = vec![
            xdg_config.join("Code").join("User"),
            home.join("snap")
                .join("code")
                .join("current")
                .join(".config")
                .join("Code")
                .join("User"),
            home.join(".var")
                .join("app")
                .join("com.visualstudio.code")
                .join("config")
                .join("Code")
                .join("User"),
            xdg_config.join("Code - Insiders").join("User"),
            home.join("snap")
                .join("code-insiders")
                .join("current")
                .join(".config")
                .join("Code - Insiders")
                .join("User"),
            xdg_config.join("VSCodium").join("User"),
            home.join(".var")
                .join("app")
                .join("com.vscodium.codium")
                .join("config")
                .join("VSCodium")
                .join("User"),
        ];
        dirs.dedup();
        dirs
    }
}

/// `chatLanguageModels.json` 主路径。`AISW_VSCODE_USER_DIR` 可覆盖（测试与便携版）。
///
/// 在 Ubuntu 等多渠道环境下：优先选择已有 `chatLanguageModels.json` 的目录，
/// 其次选择已存在 `settings.json` / `User` 目录 / 父级 `Code` 目录的实例，最后回退到默认 `~/.config/Code/User`。
pub fn chat_language_models_path() -> PathBuf {
    let candidates = candidate_vscode_user_dirs();
    if let Some(dir) = candidates.iter().find(|dir| dir.join(FILE_NAME).is_file()) {
        return dir.join(FILE_NAME);
    }
    if let Some(dir) = candidates
        .iter()
        .find(|dir| dir.join("settings.json").is_file() || dir.is_dir())
    {
        return dir.join(FILE_NAME);
    }
    if let Some(dir) = candidates
        .iter()
        .find(|dir| dir.parent().is_some_and(Path::is_dir))
    {
        return dir.join(FILE_NAME);
    }
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| get_home_dir().join(".config").join("Code").join("User"))
        .join(FILE_NAME)
}

/// 枚举某个 VS Code `User` 目录下所有自定义配置档案（`User/profiles/<profile_id>`）的
/// `chatLanguageModels.json` 路径。
///
/// 在 VS Code 中：
/// - `Copilot CLI`（`agent-host-copilotcli`）运行于全局共享的 `agentHostMain` 进程，会跨窗口汇总
///   任意默认档案窗口上报的 BYOK 模型；
/// - 但 `本地（Local）` 模式由当前窗口自身的 `ILanguageModelsService` 直接读取当前窗口所属 Profile
///   下的 `User/profiles/<profile_id>/chatLanguageModels.json`。
///
/// 若只写顶层 `User/chatLanguageModels.json`，使用自定义 Profile（如 CMake / Python / Rust 等）的窗口
/// 在切换到「本地」模式时就只能看到官方默认模型，看不到网关模型。
fn profile_chat_language_models_paths_in_user_dir(user_dir: &Path) -> Vec<PathBuf> {
    let profiles_dir = user_dir.join("profiles");
    let Ok(entries) = std::fs::read_dir(&profiles_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `builtin`（如 `builtin/agents`）固定开启 `useDefaultFlags.languageModels = true`，共用顶层配置。
        if name.eq_ignore_ascii_case("builtin") || name.starts_with('.') {
            continue;
        }
        let is_valid_profile = path.join(FILE_NAME).is_file()
            || path.join("settings.json").is_file()
            || path.join("extensions.json").is_file()
            || path.join("keybindings.json").is_file()
            || path.join("globalStorage").is_dir();
        if is_valid_profile {
            out.push(path.join(FILE_NAME));
        }
    }
    out.sort();
    out
}

/// 当本机存在多个 VS Code 配置目录（如 Ubuntu 同时有 `~/.config/Code/User` 与 Snap / Insiders）
/// 或同一 `User` 目录下存在多个自定义 Profile（`User/profiles/<id>`）时，
/// 返回除 `primary` 之外其余需要同步写入或清理的 `chatLanguageModels.json` 路径。
pub fn secondary_chat_language_models_paths(primary: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(primary_user_dir) = primary.parent() {
        paths.extend(profile_chat_language_models_paths_in_user_dir(
            primary_user_dir,
        ));
    }

    let isolated = std::env::var_os("AISW_VSCODE_USER_DIR").is_some_and(|v| !v.is_empty())
        || crate::config::paths::test_isolation_enabled();
    if !isolated && primary == chat_language_models_path() {
        for dir in candidate_vscode_user_dirs() {
            if dir.is_dir() || dir.join(FILE_NAME).is_file() {
                let candidate = dir.join(FILE_NAME);
                if candidate != primary {
                    paths.push(candidate);
                }
                paths.extend(profile_chat_language_models_paths_in_user_dir(&dir));
            }
        }
    }

    paths.retain(|candidate| candidate != primary);
    paths.sort();
    paths.dedup();
    paths
}

fn read_vscode_package_version(app_dir: &Path) -> Option<String> {
    let pkg = app_dir.join("package.json");
    let raw = std::fs::read_to_string(pkg).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    val.get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 定位 VS Code 安装根目录下的 `resources/app`（兼容新版 Windows `<root>/<commit_hash>/resources/app` 结构）。
fn resolve_vscode_app_dir(root: &Path) -> Option<PathBuf> {
    let direct = root.join("resources").join("app");
    if direct.join("package.json").is_file() {
        return Some(direct);
    }
    if root.join("package.json").is_file() {
        return Some(root.to_path_buf());
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let sub = entry.path();
        if !sub.is_dir() {
            continue;
        }
        let nested = sub.join("resources").join("app");
        if nested.join("package.json").is_file() {
            return Some(nested);
        }
    }
    None
}

fn version_from_install_root(root: &Path) -> Option<String> {
    let app_dir = resolve_vscode_app_dir(root)?;
    read_vscode_package_version(&app_dir)
}

fn has_builtin_copilot_in_root(root: &Path) -> bool {
    let Some(app_dir) = resolve_vscode_app_dir(root) else {
        return false;
    };
    let ext_dir = app_dir.join("extensions");
    ext_dir.join("copilot").is_dir() || ext_dir.join("github.copilot-chat").is_dir()
}

/// 修复已确认的 VS Code Claude 本地模型视觉声明；未知构建或受完整性校验的文件不改写。
/// 仅等长替换能力标志，备份原文件后原子落盘，避免中断写入损坏 Agent Host。
fn patch_vscode_agent_host_claude_vision_in_root(root: &Path) -> AppResult<bool> {
    let Some(app_dir) = resolve_vscode_app_dir(root) else {
        return Ok(false);
    };
    const RELATIVE_PATH: &str = "vs/platform/agentHost/node/agentHostMain.js";
    let agent_host_js = app_dir.join("out").join(RELATIVE_PATH);
    if !agent_host_js.is_file() {
        return Ok(false);
    }
    let product: Value = match std::fs::read_to_string(app_dir.join("product.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
    {
        Some(product) => product,
        None => return Ok(false),
    };
    if !product
        .get("checksums")
        .is_some_and(|checksums| checksums.is_object() && checksums.get(RELATIVE_PATH).is_none())
    {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&agent_host_js)?;
    const NEEDLE: &str = "return{provider:i,id:r.value,name:r.displayName,supportsVision:!1,...t?{configSchema:t}:{}}}";
    const REPLACEMENT: &str = "return{provider:i,id:r.value,name:r.displayName,supportsVision:!0,...t?{configSchema:t}:{}}}";
    if !content.contains("vscode_claude_code") || content.matches(NEEDLE).count() != 1 {
        return Ok(false);
    }

    let backup = agent_host_js.with_extension("js.aisw-claude-vision.bak");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // 原文件可能已被 VS Code 更新，不能复用另一构建的备份。
            if std::fs::read(&backup)? != content.as_bytes() {
                return Err(AppError::Config(
                    "Claude 图片兼容补丁备份与当前 VS Code 构建不一致，已跳过".into(),
                ));
            }
        }
        Err(error) => return Err(error.into()),
    }
    let patched = content.replacen(NEEDLE, REPLACEMENT, 1);
    crate::config::atomic::atomic_write(&agent_host_js, patched.as_bytes())?;
    log::info!("已修复 Claude Agent SDK 图片能力声明，请完全重启 VS Code 后生效");
    Ok(true)
}

fn has_copilot_extension_in(ext_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(ext_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if !path.is_dir() {
            return false;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let lower = name.to_ascii_lowercase();
        !lower.contains(".obsolete")
            && (lower.starts_with("github.copilot-chat-") || lower.starts_with("github.copilot-"))
    })
}

/// 探测本机 VS Code 安装状态、版本与 GitHub Copilot 扩展。
/// 只读本地文件与目录元数据，不拉起 Electron 进程，避免 Ubuntu GUI 下闪烁或阻塞。
pub fn detect_vscode_installation() -> VscodeInstallationInfo {
    let home = get_home_dir();
    let isolated = crate::config::paths::test_isolation_enabled();

    #[cfg(any(windows, target_os = "macos"))]
    let ext_dirs = [
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
    ];
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let ext_dirs = [
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
        home.join("snap")
            .join("code")
            .join("current")
            .join(".vscode")
            .join("extensions"),
        home.join(".var")
            .join("app")
            .join("com.visualstudio.code")
            .join("data")
            .join("vscode")
            .join("extensions"),
        home.join("snap")
            .join("code-insiders")
            .join("current")
            .join(".vscode-insiders")
            .join("extensions"),
        home.join(".vscode-oss").join("extensions"),
    ];
    let mut copilot_extension_installed = ext_dirs.iter().any(|dir| has_copilot_extension_in(dir));

    if isolated {
        let user_dir_exists = candidate_vscode_user_dirs()
            .iter()
            .any(|dir| dir.is_dir() || dir.parent().is_some_and(Path::is_dir));
        let ext_dir_exists = ext_dirs.iter().any(|dir| dir.is_dir());
        let installed = user_dir_exists || ext_dir_exists || copilot_extension_installed;
        return VscodeInstallationInfo {
            vscode_installed: installed,
            vscode_version: None,
            vscode_path: None,
            copilot_extension_installed,
        };
    }

    let mut install_roots: Vec<PathBuf> = Vec::new();
    let mut cli_candidates: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            install_roots.push(local.join("Programs").join("Microsoft VS Code"));
            install_roots.push(local.join("Programs").join("Microsoft VS Code Insiders"));
            cli_candidates.push(
                local
                    .join("Programs")
                    .join("Microsoft VS Code")
                    .join("bin")
                    .join("code.cmd"),
            );
            cli_candidates.push(
                local
                    .join("Programs")
                    .join("Microsoft VS Code Insiders")
                    .join("bin")
                    .join("code-insiders.cmd"),
            );
        }
        install_roots.push(PathBuf::from(r"C:\Program Files\Microsoft VS Code"));
        install_roots.push(PathBuf::from(r"C:\Program Files (x86)\Microsoft VS Code"));
        cli_candidates.push(PathBuf::from(
            r"C:\Program Files\Microsoft VS Code\bin\code.cmd",
        ));
        cli_candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        install_roots.push(PathBuf::from(
            "/Applications/Visual Studio Code.app/Contents",
        ));
        install_roots.push(PathBuf::from(
            "/Applications/Visual Studio Code - Insiders.app/Contents",
        ));
        cli_candidates.push(PathBuf::from(
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
        ));
        cli_candidates.push(PathBuf::from("/usr/local/bin/code"));
        cli_candidates.push(PathBuf::from("/opt/homebrew/bin/code"));
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        install_roots.extend([
            PathBuf::from("/usr/share/code"),
            PathBuf::from("/snap/code/current/usr/share/code"),
            PathBuf::from("/var/lib/snapd/snap/code/current/usr/share/code"),
            PathBuf::from("/opt/vscode"),
            PathBuf::from("/usr/local/share/code"),
            PathBuf::from("/usr/share/code-insiders"),
            PathBuf::from("/snap/code-insiders/current/usr/share/code-insiders"),
            PathBuf::from("/usr/share/codium"),
        ]);
        cli_candidates.extend([
            PathBuf::from("/usr/bin/code"),
            PathBuf::from("/snap/bin/code"),
            PathBuf::from("/var/lib/snapd/snap/bin/code"),
            PathBuf::from("/usr/share/code/bin/code"),
            PathBuf::from("/usr/local/bin/code"),
            PathBuf::from("/opt/vscode/bin/code"),
            PathBuf::from("/var/lib/flatpak/exports/bin/com.visualstudio.code"),
            home.join(".local")
                .join("share")
                .join("flatpak")
                .join("exports")
                .join("bin")
                .join("com.visualstudio.code"),
            PathBuf::from("/usr/bin/code-insiders"),
            PathBuf::from("/snap/bin/code-insiders"),
            PathBuf::from("/usr/share/code-insiders/bin/code-insiders"),
            PathBuf::from("/usr/bin/codium"),
            PathBuf::from("/snap/bin/codium"),
        ]);
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            #[cfg(windows)]
            {
                if dir.components().any(|c| c.as_os_str() == "WindowsApps") {
                    continue;
                }
                cli_candidates.push(dir.join("code.cmd"));
                cli_candidates.push(dir.join("code-insiders.cmd"));
            }
            #[cfg(not(windows))]
            {
                cli_candidates.push(dir.join("code"));
                cli_candidates.push(dir.join("code-insiders"));
                cli_candidates.push(dir.join("codium"));
            }
        }
    }

    let found_cli = cli_candidates.into_iter().find(|p| p.is_file());
    if let Some(ref cli) = found_cli {
        if let Ok(real) = std::fs::canonicalize(cli) {
            if let Some(bin_dir) = real.parent() {
                if let Some(root) = bin_dir.parent() {
                    install_roots.insert(0, root.to_path_buf());
                }
            }
        }
        if let Some(bin_dir) = cli.parent() {
            if let Some(root) = bin_dir.parent() {
                install_roots.insert(0, root.to_path_buf());
            }
        }
    }

    let mut found_version = None;
    let mut found_root = None;
    for root in &install_roots {
        if root.is_dir() {
            if !copilot_extension_installed && has_builtin_copilot_in_root(root) {
                copilot_extension_installed = true;
            }
            if found_root.is_none() {
                found_root = Some(root.clone());
            }
            if let Some(ver) = version_from_install_root(root) {
                found_version = Some(ver);
                found_root = Some(root.clone());
                break;
            }
        }
    }

    let user_dir_exists = candidate_vscode_user_dirs()
        .iter()
        .any(|dir| dir.is_dir() || dir.parent().is_some_and(Path::is_dir));
    let ext_dir_exists = ext_dirs.iter().any(|dir| dir.is_dir());
    let vscode_path = found_cli
        .or(found_root)
        .map(|p| p.to_string_lossy().into_owned());
    let vscode_installed =
        vscode_path.is_some() || user_dir_exists || ext_dir_exists || copilot_extension_installed;

    VscodeInstallationInfo {
        vscode_installed,
        vscode_version: found_version,
        vscode_path,
        copilot_extension_installed,
    }
}

/// 读取顶层数组。文件不存在为空数组；无法解析或根不是数组时报错，不静默重建。
fn read_entries(path: &Path) -> AppResult<Vec<Value>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(AppError::Io(format!("读取 {FILE_NAME} 失败: {err}"))),
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = json5::from_str(&raw)
        .map_err(|e| AppError::Config(format!("无法解析 {}: {e}", path.display())))?;
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(AppError::Config(format!(
            "{FILE_NAME} 根节点必须是数组: {}",
            path.display()
        ))),
    }
}

fn is_managed(entry: &Value) -> bool {
    entry.get("name").and_then(Value::as_str) == Some(MANAGED_NAME)
        && entry.get("vendor").and_then(Value::as_str) == Some(VENDOR)
}

fn model_value(model: &CopilotModel, base_url: &str, api_key: &str) -> Value {
    let mut value = Map::new();
    value.insert("id".into(), json!(model.id));
    value.insert("name".into(), json!(model.name));
    value.insert("url".into(), json!(base_url));
    value.insert("apiType".into(), json!("responses"));
    // VS Code 1.141+ 把 provider 级 apiKey 当作 SecretStorage 引用解析，外部写入的
    // 明文会变成空值。模型级 x-api-key 会被 Custom Endpoint 原样发送，并抑制空的
    // `Authorization: Bearer`，因此用它携带独立的网关绑定 token。
    value.insert("requestHeaders".into(), json!({ "x-api-key": api_key }));
    value.insert("toolCalling".into(), json!(model.tool_calling));
    value.insert("vision".into(), json!(model.vision));
    if model.thinking {
        value.insert("thinking".into(), json!(true));
    }
    if !model.reasoning_efforts.is_empty() {
        value.insert(
            "supportsReasoningEffort".into(),
            json!(model.reasoning_efforts),
        );
    }
    value.insert("contextWindow".into(), json!(model.context_window));
    value.insert("maxInputTokens".into(), json!(model.max_input_tokens));
    value.insert("maxOutputTokens".into(), json!(model.max_output_tokens));
    Value::Object(value)
}

/// 构造托管条目。`api_key` 是仅对本机网关有效的绑定 token。
///
/// provider 级 `apiKey` 写固定占位符以满足配置 schema；真实请求只通过各模型的
/// `requestHeaders.x-api-key` 携带 token，避免被 VS Code 的 SecretStorage 引用解析吞掉。
pub fn managed_entry(base_url: &str, api_key: &str, models: &[CopilotModel]) -> Value {
    json!({
        "name": MANAGED_NAME,
        "vendor": VENDOR,
        "apiKey": PROVIDER_API_KEY_PLACEHOLDER,
        "apiType": "responses",
        "models": models.iter().map(|m| model_value(m, base_url, api_key)).collect::<Vec<_>>(),
    })
}

/// 从 `chatLanguageModels.json` 的 `agent-host-copilotcli` 或 `AI-Switcher` 的 `settings` 中提取
/// 用户已选过/配置过的模型 ID 列表（按配置出现顺序）。
fn extract_preferred_model_ids(entries: &[Value]) -> Vec<String> {
    let mut preferred = Vec::new();
    let mut push_unique = |raw: &str| {
        let stripped = crate::catalog::strip_client_model_prefix(raw);
        if !stripped.is_empty()
            && !preferred
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(stripped))
        {
            preferred.push(stripped.to_string());
        }
    };
    for entry in entries {
        if entry.get("vendor").and_then(Value::as_str) == Some("agent-host-copilotcli") {
            if let Some(settings) = entry.get("settings").and_then(Value::as_object) {
                for key in settings.keys() {
                    push_unique(key);
                }
            }
        }
    }
    for entry in entries {
        if is_managed(entry) {
            if let Some(settings) = entry.get("settings").and_then(Value::as_object) {
                for key in settings.keys() {
                    push_unique(key);
                }
            }
        }
    }
    preferred
}

/// 读取用户在 VS Code Copilot 中最近选中的且在当前目录中的模型 ID。
pub fn preferred_selected_model_id(
    catalog_entries: &[crate::catalog::CatalogEntry],
) -> Option<String> {
    let file_entries = read_entries(&chat_language_models_path()).ok()?;
    for id in extract_preferred_model_ids(&file_entries) {
        if let Some(found) = catalog_entries.iter().find(|entry| {
            entry.public_id.eq_ignore_ascii_case(&id)
                || entry.upstream_slug.eq_ignore_ascii_case(&id)
        }) {
            return Some(found.public_id.clone());
        }
    }
    None
}

/// 把托管条目并入条目列表：已有则原位替换，否则追加；其余条目不动。
/// `settings` 由 VS Code 按模型维护（如 `reasoningEffort`），重同步时必须保留。
/// 同时将用户在 `agent-host-copilotcli` 中选中的模型排在 `models[]` 首位，防止
/// VS Code 重新加载配置或新开对话时把默认选中模型切回 `gemini-3.8-flash`。
fn merge_entry(entries: &mut Vec<Value>, managed: Value) {
    let preferred_ids = extract_preferred_model_ids(entries);
    let mut placed = false;
    entries.retain_mut(|entry| {
        if !is_managed(entry) {
            return true;
        }
        if placed {
            return false; // 重复的托管项只留一份
        }
        let mut replacement = managed.clone();
        if let Some(replacement_obj) = replacement.as_object_mut() {
            if let Some(settings) = entry.get("settings").cloned() {
                replacement_obj.insert("settings".into(), settings);
            }
            if !preferred_ids.is_empty() {
                if let Some(models) = replacement_obj
                    .get_mut("models")
                    .and_then(Value::as_array_mut)
                {
                    models.sort_by_key(|model| {
                        let id = model.get("id").and_then(Value::as_str).unwrap_or("");
                        preferred_ids
                            .iter()
                            .position(|pref| pref.eq_ignore_ascii_case(id))
                            .unwrap_or(usize::MAX)
                    });
                }
            }
        }
        *entry = replacement;
        placed = true;
        true
    });
    if !placed {
        let mut managed = managed;
        if !preferred_ids.is_empty() {
            if let Some(models) = managed.get_mut("models").and_then(Value::as_array_mut) {
                models.sort_by_key(|model| {
                    let id = model.get("id").and_then(Value::as_str).unwrap_or("");
                    preferred_ids
                        .iter()
                        .position(|pref| pref.eq_ignore_ascii_case(id))
                        .unwrap_or(usize::MAX)
                });
            }
        }
        entries.push(managed);
    }
}

fn write_entries(path: &Path, entries: &[Value]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        crate::config::ensure_dir_with_context(parent)?;
    }
    write_json_file(path, &Value::Array(entries.to_vec()))
}

fn backup(path: &Path) -> AppResult<()> {
    if path.exists() {
        crate::backup::backup_file_named(path, FILE_NAME, BACKUP_KEEP)?;
    }
    Ok(())
}

fn apply_single_managed_at(path: &Path, managed: Value) -> AppResult<()> {
    let before = read_entries(path)?;
    let mut entries = before.clone();
    merge_entry(&mut entries, managed);
    if path.exists() && entries == before {
        // 即使内容一致也重新落盘（不额外备份），触发 VS Code 的文件变更监听，
        // 避免扩展宿主重启后 customendpoint 语言模型未被重新加载。
        return write_entries(path, &entries);
    }
    backup(path)?;
    write_entries(path, &entries)
}

fn remove_single_managed_at(path: &Path) -> AppResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut entries = read_entries(path)?;
    let before = entries.len();
    entries.retain(|entry| !is_managed(entry));
    if entries.len() == before {
        return Ok(false);
    }
    backup(path)?;
    write_entries(path, &entries)?;
    Ok(true)
}

fn patch_vscode_agent_host_claude_vision() {
    if std::env::var_os("AISW_VSCODE_USER_DIR").is_some_and(|v| !v.is_empty())
        || crate::config::paths::test_isolation_enabled()
    {
        return;
    }
    if let Some(vscode_path) = detect_vscode_installation().vscode_path {
        let path = PathBuf::from(vscode_path);
        let root = if path.is_file() {
            path.parent().and_then(Path::parent).map(Path::to_path_buf)
        } else {
            Some(path)
        };
        if let Some(root) = root {
            if let Err(error) = patch_vscode_agent_host_claude_vision_in_root(&root) {
                log::warn!("Claude Agent SDK 图片兼容补丁未应用：{error}");
            }
        }
    }
}

pub fn apply_managed_at(path: &Path, managed: Value) -> AppResult<()> {
    let _guard = lock()?;
    apply_single_managed_at(path, managed.clone())?;
    for secondary in secondary_chat_language_models_paths(path) {
        let _ = apply_single_managed_at(&secondary, managed.clone());
    }
    if path == chat_language_models_path() {
        patch_vscode_agent_host_claude_vision();
    }
    Ok(())
}

/// 当主配置已处于绑定且同步状态时，自动将缺失或漂移的自定义 Profile（`User/profiles/<id>/chatLanguageModels.json`）
/// 补齐同步，避免用户在非默认 VS Code Profile 窗口切到「本地（Local）」模式时看不到网关模型。
pub fn heal_secondary_managed_at(primary: &Path, expected: &Value) {
    let Ok(_guard) = lock() else {
        return;
    };
    for secondary in secondary_chat_language_models_paths(primary) {
        let current = read_managed_at(&secondary).ok().flatten();
        if !diff_managed(expected, current.as_ref()).is_empty() {
            let _ = apply_single_managed_at(&secondary, expected.clone());
        }
    }
    if primary == chat_language_models_path() {
        patch_vscode_agent_host_claude_vision();
    }
}

/// 只移除托管条目，用户其他 provider 保留。返回是否有改动。
pub fn remove_managed_at(path: &Path) -> AppResult<bool> {
    let _guard = lock()?;
    let mut changed = remove_single_managed_at(path)?;
    for secondary in secondary_chat_language_models_paths(path) {
        if let Ok(true) = remove_single_managed_at(&secondary) {
            changed = true;
        }
    }
    Ok(changed)
}

pub fn read_managed_at(path: &Path) -> AppResult<Option<Value>> {
    Ok(read_entries(path)?.into_iter().find(is_managed))
}

/// 漂移比较结果。只含字段名，不含 token 原值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftField {
    pub field: String,
    pub applied_present: bool,
    pub current_present: bool,
}

/// 对比期望条目与当前托管条目，返回不一致的字段名。
pub fn diff_managed(expected: &Value, current: Option<&Value>) -> Vec<DriftField> {
    let Some(current) = current else {
        return vec![DriftField {
            field: MANAGED_NAME.to_string(),
            applied_present: true,
            current_present: false,
        }];
    };
    let mut fields = Vec::new();
    for key in ["apiKey", "apiType", "vendor"] {
        if expected.get(key) != current.get(key) {
            fields.push(DriftField {
                field: key.to_string(),
                applied_present: expected.get(key).is_some(),
                current_present: current.get(key).is_some(),
            });
        }
    }
    let ids = |value: &Value| -> Vec<String> {
        value
            .get("models")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let sorted_models = |value: &Value| -> Vec<Value> {
        let mut items = value
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        items.sort_by(|a, b| {
            let id_a = a.get("id").and_then(Value::as_str).unwrap_or("");
            let id_b = b.get("id").and_then(Value::as_str).unwrap_or("");
            id_a.cmp(id_b)
        });
        items
    };
    if sorted_models(expected) != sorted_models(current) {
        let (mut want, mut have) = (ids(expected), ids(current));
        want.sort();
        have.sort();
        fields.push(DriftField {
            field: if want == have { "models" } else { "models.ids" }.to_string(),
            applied_present: !want.is_empty(),
            current_present: !have.is_empty(),
        });
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> CopilotModel {
        CopilotModel {
            id: id.into(),
            name: id.into(),
            tool_calling: true,
            vision: false,
            thinking: false,
            reasoning_efforts: Vec::new(),
            context_window: 200_000,
            max_input_tokens: 168_000,
            max_output_tokens: 32_000,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aisw-vscode-copilot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(FILE_NAME)
    }

    #[test]
    fn merge_keeps_user_entries_and_replaces_managed_in_place() {
        let mut entries = vec![
            json!({"name": "Mine", "vendor": "customendpoint", "apiKey": "k"}),
            managed_entry("http://127.0.0.1:15828/v1", "gwt_old", &[model("a")]),
            json!({"name": "Other", "vendor": "azure"}),
        ];
        merge_entry(
            &mut entries,
            managed_entry("http://127.0.0.1:15828/v1", "gwt_new", &[model("b")]),
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0]["name"], "Mine");
        assert_eq!(entries[1]["apiKey"], PROVIDER_API_KEY_PLACEHOLDER);
        assert_eq!(
            entries[1]["models"][0]["requestHeaders"]["x-api-key"],
            "gwt_new"
        );
        assert_eq!(entries[1]["models"][0]["id"], "b");
        assert_eq!(entries[2]["name"], "Other");
    }

    #[test]
    fn merge_preserves_vscode_managed_model_settings() {
        let mut current = managed_entry("http://127.0.0.1:15828/v1", "gwt_old", &[model("a")]);
        current["settings"] = json!({
            "a": { "reasoningEffort": "low" },
            "removed-model": { "reasoningEffort": "high" }
        });
        let mut entries = vec![current];

        merge_entry(
            &mut entries,
            managed_entry("http://127.0.0.1:15828/v1", "gwt_new", &[model("b")]),
        );

        assert_eq!(entries[0]["apiKey"], PROVIDER_API_KEY_PLACEHOLDER);
        assert_eq!(
            entries[0]["models"][0]["requestHeaders"]["x-api-key"],
            "gwt_new"
        );
        assert_eq!(entries[0]["models"][0]["id"], "b");
        assert_eq!(entries[0]["settings"]["a"]["reasoningEffort"], "low");
        assert_eq!(
            entries[0]["settings"]["removed-model"]["reasoningEffort"],
            "high"
        );
    }

    #[test]
    fn managed_entry_has_no_provider_level_url() {
        let entry = managed_entry("http://127.0.0.1:15828/v1", "gwt_x", &[model("a")]);
        assert!(entry.get("url").is_none());
        assert_eq!(entry["apiKey"], PROVIDER_API_KEY_PLACEHOLDER);
        assert_eq!(entry["models"][0]["url"], "http://127.0.0.1:15828/v1");
        assert_eq!(entry["models"][0]["apiType"], "responses");
        assert_eq!(entry["models"][0]["requestHeaders"]["x-api-key"], "gwt_x");
        assert_eq!(entry["models"][0]["toolCalling"], true);
    }

    #[test]
    fn reasoning_effort_is_an_array_never_a_bool() {
        let mut m = model("a");
        m.thinking = true;
        m.reasoning_efforts = vec!["low".into(), "high".into()];
        let entry = managed_entry("u", "k", &[m]);
        assert_eq!(
            entry["models"][0]["supportsReasoningEffort"],
            json!(["low", "high"])
        );
        let plain = managed_entry("u", "k", &[model("b")]);
        assert!(plain["models"][0].get("supportsReasoningEffort").is_none());
    }

    #[test]
    fn apply_and_remove_preserve_foreign_entries() {
        let path = temp_path("roundtrip");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"[{"name":"Mine","vendor":"customendpoint","apiKey":"${input:x}"}]"#,
        )
        .unwrap();
        apply_managed_at(
            &path,
            managed_entry("http://127.0.0.1:15828/v1", "gwt_1", &[model("a")]),
        )
        .unwrap();
        let entries = read_entries(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(remove_managed_at(&path).unwrap());
        let entries = read_entries(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["name"], "Mine");
        assert!(!remove_managed_at(&path).unwrap());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn non_array_root_is_rejected_not_rebuilt() {
        let path = temp_path("badroot");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"name":"oops"}"#).unwrap();
        assert!(apply_managed_at(&path, managed_entry("u", "k", &[])).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"name":"oops"}"#
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn drift_reports_field_names_without_secret_values() {
        let expected = managed_entry("u", "gwt_secret", &[model("a")]);
        let mut current = expected.clone();
        current["apiKey"] = json!("changed");
        current["models"] = json!([]);
        let fields = diff_managed(&expected, Some(&current));
        let names: Vec<_> = fields.iter().map(|f| f.field.as_str()).collect();
        assert_eq!(names, ["apiKey", "models.ids"]);
        assert!(diff_managed(&expected, Some(&expected)).is_empty());
        assert_eq!(diff_managed(&expected, None).len(), 1);
    }

    #[test]
    fn detect_vscode_installation_reads_package_json_and_copilot_extension() {
        let dir =
            std::env::temp_dir().join(format!("aisw-vscode-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app_dir = dir.join("resources").join("app");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(
            app_dir.join("package.json"),
            r#"{"name":"code","version":"1.99.2"}"#,
        )
        .unwrap();
        assert_eq!(version_from_install_root(&dir).as_deref(), Some("1.99.2"));

        let ext_dir = dir.join("extensions");
        std::fs::create_dir_all(ext_dir.join("github.copilot-chat-0.26.0")).unwrap();
        assert!(has_copilot_extension_in(&ext_dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_and_remove_sync_custom_user_profiles() {
        let path = temp_path("profiles");
        let user_dir = path.parent().unwrap();
        let prof_cmake = user_dir.join("profiles").join("33e3145b");
        let prof_tuyaos = user_dir.join("profiles").join("a497bfb");
        let prof_builtin = user_dir.join("profiles").join("builtin");
        std::fs::create_dir_all(&prof_cmake).unwrap();
        std::fs::create_dir_all(&prof_tuyaos).unwrap();
        std::fs::create_dir_all(&prof_builtin).unwrap();
        std::fs::write(prof_cmake.join("settings.json"), "{}").unwrap();
        std::fs::write(prof_tuyaos.join(FILE_NAME), "[]").unwrap();
        std::fs::write(prof_builtin.join("settings.json"), "{}").unwrap();

        let expected = managed_entry("http://127.0.0.1:15828/v1", "gwt_prof", &[model("a")]);
        apply_managed_at(&path, expected).unwrap();

        assert!(read_managed_at(&path).unwrap().is_some());
        assert!(read_managed_at(&prof_cmake.join(FILE_NAME))
            .unwrap()
            .is_some());
        assert!(read_managed_at(&prof_tuyaos.join(FILE_NAME))
            .unwrap()
            .is_some());
        assert!(!prof_builtin.join(FILE_NAME).exists());

        assert!(remove_managed_at(&path).unwrap());
        assert!(read_managed_at(&path).unwrap().is_none());
        assert!(read_managed_at(&prof_cmake.join(FILE_NAME))
            .unwrap()
            .is_none());
        assert!(read_managed_at(&prof_tuyaos.join(FILE_NAME))
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(user_dir);
    }

    const CLAUDE_AGENT_HOST_FIXTURE: &str = "const provider=\"vscode_claude_code\";function iMe(r,i){let e=(r.supportedEffortLevels??[]).filter(IW),t=RW(e);return{provider:i,id:r.value,name:r.displayName,supportsVision:!1,...t?{configSchema:t}:{}}}";

    fn agent_host_fixture(tag: &str, content: &str, product: &Value) -> (PathBuf, PathBuf) {
        let root = temp_path(tag).parent().unwrap().to_path_buf();
        let app_dir = root.join("6d62b1d5da").join("resources").join("app");
        let ah_js = app_dir.join("out/vs/platform/agentHost/node/agentHostMain.js");
        std::fs::create_dir_all(ah_js.parent().unwrap()).unwrap();
        std::fs::write(
            app_dir.join("package.json"),
            r#"{"name":"code","version":"1.142.0"}"#,
        )
        .unwrap();
        std::fs::write(app_dir.join("product.json"), product.to_string()).unwrap();
        std::fs::write(&ah_js, content).unwrap();
        (root, ah_js)
    }

    #[test]
    fn patch_agent_host_claude_vision_flips_supports_vision_flag() {
        let (root, ah_js) = agent_host_fixture(
            "vision-flip",
            CLAUDE_AGENT_HOST_FIXTURE,
            &json!({"checksums": {}}),
        );
        assert!(patch_vscode_agent_host_claude_vision_in_root(&root).unwrap());
        let after = std::fs::read_to_string(&ah_js).unwrap();
        assert_eq!(
            after,
            CLAUDE_AGENT_HOST_FIXTURE.replace("supportsVision:!1", "supportsVision:!0")
        );
        assert_eq!(after.len(), CLAUDE_AGENT_HOST_FIXTURE.len());
        let backup = ah_js.with_extension("js.aisw-claude-vision.bak");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            CLAUDE_AGENT_HOST_FIXTURE
        );
        assert!(!patch_vscode_agent_host_claude_vision_in_root(&root).unwrap());
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            CLAUDE_AGENT_HOST_FIXTURE
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_agent_host_claude_vision_skips_unknown_or_ambiguous_builds() {
        for (tag, content) in [
            (
                "vision-other",
                "name:r.displayName,supportsVision:!1".to_string(),
            ),
            (
                "vision-no-provider",
                CLAUDE_AGENT_HOST_FIXTURE.replace("vscode_claude_code", "other"),
            ),
            ("vision-ambiguous", CLAUDE_AGENT_HOST_FIXTURE.repeat(2)),
            (
                "vision-already-fixed",
                CLAUDE_AGENT_HOST_FIXTURE.replace("supportsVision:!1", "supportsVision:!0"),
            ),
        ] {
            let (root, ah_js) = agent_host_fixture(tag, &content, &json!({"checksums": {}}));
            assert!(!patch_vscode_agent_host_claude_vision_in_root(&root).unwrap());
            assert_eq!(std::fs::read_to_string(&ah_js).unwrap(), content);
            assert!(!ah_js.with_extension("js.aisw-claude-vision.bak").exists());
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn patch_agent_host_claude_vision_preserves_other_models() {
        let other_model = "const model={name:r.displayName,supportsVision:!1};";
        let content = format!("{other_model}{CLAUDE_AGENT_HOST_FIXTURE}");
        let (root, ah_js) =
            agent_host_fixture("vision-other-model", &content, &json!({"checksums": {}}));
        assert!(patch_vscode_agent_host_claude_vision_in_root(&root).unwrap());
        let after = std::fs::read_to_string(&ah_js).unwrap();
        assert!(after.starts_with(other_model));
        assert_eq!(after.matches("supportsVision:!0").count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_agent_host_claude_vision_skips_checked_or_unverifiable_files() {
        for (tag, product) in [
            (
                "vision-checked",
                json!({"checksums": {"vs/platform/agentHost/node/agentHostMain.js": "checksum"}}),
            ),
            ("vision-no-checksums", json!({})),
            ("vision-invalid-checksums", json!({"checksums": null})),
        ] {
            let (root, ah_js) = agent_host_fixture(tag, CLAUDE_AGENT_HOST_FIXTURE, &product);
            assert!(!patch_vscode_agent_host_claude_vision_in_root(&root).unwrap());
            assert_eq!(
                std::fs::read_to_string(&ah_js).unwrap(),
                CLAUDE_AGENT_HOST_FIXTURE
            );
            assert!(!ah_js.with_extension("js.aisw-claude-vision.bak").exists());
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn patch_agent_host_claude_vision_refuses_stale_backup() {
        let (root, ah_js) = agent_host_fixture(
            "vision-stale-backup",
            CLAUDE_AGENT_HOST_FIXTURE,
            &json!({"checksums": {}}),
        );
        let backup = ah_js.with_extension("js.aisw-claude-vision.bak");
        std::fs::write(&backup, "another build").unwrap();
        assert!(patch_vscode_agent_host_claude_vision_in_root(&root).is_err());
        assert_eq!(
            std::fs::read_to_string(&ah_js).unwrap(),
            CLAUDE_AGENT_HOST_FIXTURE
        );
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "another build");
        let _ = std::fs::remove_dir_all(&root);
    }
}
