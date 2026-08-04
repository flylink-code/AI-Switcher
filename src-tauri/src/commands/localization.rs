//! Unified localization status and safe installers for Claude Code editors.
//!
//! Claude Desktop resource localization remains in `desktop_localization`. This
//! module only installs the public Claude Code plugin and the VS Code/Cursor
//! patch helper; it never runs third-party patch scripts or edits extensions.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::backup::{backup_file_named, DEFAULT_BACKUP_KEEP};
use crate::commands::tools::get_claude_code_version;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::error::{AppError, AppResult};

const CLAUDE_PLUGIN_ID: &str = "claude-code-zh-cn@claude-code-zh-cn";
const CLAUDE_MARKETPLACE_REPOSITORY: &str = "https://github.com/taekchef/claude-code-zh-cn";
const PATCH_HELPER_EXTENSION: &str = "shanjiancaofu.claude-code-zh-cn-patch-helper";
const CLAUDE_EXTENSION_PREFIX: &str = "anthropic.claude-code-";
const PATCH_HELPER_PREFIX: &str = "shanjiancaofu.claude-code-zh-cn-patch-helper-";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationHubStatus {
    pub claude_code: ClaudeCodeLocalizationStatus,
    pub editors: Vec<EditorLocalizationStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeLocalizationStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub executable_path: Option<String>,
    pub plugin_enabled: bool,
    pub settings_configured: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLocalizationStatus {
    pub id: String,
    pub label: String,
    pub editor_detected: bool,
    pub editor_cli_path: Option<String>,
    pub claude_extension_path: Option<String>,
    pub helper_installed: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
struct EditorDefinition {
    id: &'static str,
    label: &'static str,
    extension_dir: PathBuf,
    command_name: &'static str,
    cli_candidates: Vec<PathBuf>,
}

#[tauri::command]
pub async fn get_localization_hub_status() -> AppResult<LocalizationHubStatus> {
    let code_info = get_claude_code_version(Some(false)).await?;
    let settings = read_json_file::<Value>(&get_claude_settings_path())?.unwrap_or(Value::Object(Map::new()));
    let plugin_enabled = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get(CLAUDE_PLUGIN_ID))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let settings_configured = settings.get("language").and_then(Value::as_str) == Some("Chinese")
        && settings.get("spinnerTipsEnabled").and_then(Value::as_bool) == Some(true);
    let spinner_verbs_invalid = settings.get("spinnerVerbs").is_some_and(Value::is_array);
    let claude_code = ClaudeCodeLocalizationStatus {
        installed: code_info.installed,
        version: code_info.current_version,
        executable_path: code_info.executable_path,
        plugin_enabled,
        settings_configured,
        message: if !code_info.installed {
            "未检测到 Claude Code".to_string()
        } else if spinner_verbs_invalid {
            "settings.json 中 spinnerVerbs 格式无效（写成了数组），会导致 Claude Code 整份设置失效；请重新执行「安装中文」以自动修复".to_string()
        } else if plugin_enabled && settings_configured {
            "中文插件与基础设置已启用".to_string()
        } else {
            "可安装中文插件并启用基础中文设置".to_string()
        },
    };
    Ok(LocalizationHubStatus {
        claude_code,
        editors: editor_definitions().into_iter().map(editor_status).collect(),
    })
}

#[tauri::command]
pub async fn install_claude_code_localization() -> AppResult<String> {
    let info = get_claude_code_version(Some(false)).await?;
    let executable = info.executable_path
        .ok_or_else(|| AppError::Config("未检测到 Claude Code，无法安装中文插件".to_string()))?;
    if info.environment == "wsl" {
        return Err(AppError::Config("请在 WSL 终端中安装 Claude Code 中文插件".to_string()));
    }
    run_claude_plugin_command(&executable, ["plugin", "marketplace", "add", "--scope", "user", CLAUDE_MARKETPLACE_REPOSITORY])?;
    run_claude_plugin_command(&executable, ["plugin", "install", CLAUDE_PLUGIN_ID, "--scope", "user"])?;
    merge_claude_code_chinese_settings()?;
    Ok("Claude Code 中文插件已安装，并已启用中文基础设置".to_string())
}

#[tauri::command]
pub fn install_editor_localization_helper(editor: String) -> AppResult<String> {
    let definition = editor_definitions().into_iter()
        .find(|definition| definition.id == editor)
        .ok_or_else(|| AppError::Config("不支持的编辑器".to_string()))?;
    let status = editor_status(definition.clone());
    if status.claude_extension_path.is_none() {
        return Err(AppError::Config(format!("{} 未检测到 Claude Code for VS Code 扩展", status.label)));
    }
    let cli = status.editor_cli_path
        .ok_or_else(|| AppError::Config(format!("未找到 {} 命令行工具，无法安装补丁助手", status.label)))?;
    run_editor_extension_install(Path::new(&cli))?;
    Ok(format!("{} 中文补丁助手已安装；请在编辑器命令面板运行 Apply Patch 并重载窗口", status.label))
}

fn merge_claude_code_chinese_settings() -> AppResult<()> {
    let path = get_claude_settings_path();
    if path.is_file() {
        backup_file_named(&path, "claude-code-zh-cn-settings", DEFAULT_BACKUP_KEEP)?;
    }
    let mut settings = read_json_file::<Value>(&path)?.unwrap_or(Value::Object(Map::new()));
    let object = settings.as_object_mut()
        .ok_or_else(|| AppError::Config("Claude Code settings.json 必须是 JSON 对象".to_string()))?;
    object.insert("language".to_string(), Value::String("Chinese".to_string()));
    object.insert("spinnerTipsEnabled".to_string(), Value::Bool(true));
    normalize_spinner_verbs(object);
    write_json_file(&path, &settings)
}

/// Claude Code expects `spinnerVerbs` as `{ mode, verbs }`. Some Chinese tip packs
/// write a bare string array, which makes the whole settings file fail validation
/// ("Expected object, but received array") and disables permission rules.
fn normalize_spinner_verbs(settings: &mut Map<String, Value>) {
    let Some(Value::Array(verbs)) = settings.get("spinnerVerbs").cloned() else {
        return;
    };
    let verbs: Vec<Value> = verbs
        .into_iter()
        .filter(|value| value.is_string())
        .collect();
    let mut object = Map::new();
    object.insert("mode".to_string(), Value::String("replace".to_string()));
    object.insert("verbs".to_string(), Value::Array(verbs));
    settings.insert("spinnerVerbs".to_string(), Value::Object(object));
}

fn run_claude_plugin_command<const N: usize>(executable: &str, args: [&str; N]) -> AppResult<()> {
    let output = run_command(Path::new(executable), &args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::Other(format!("Claude Code 插件命令失败: {}", command_detail(&output))))
}

fn run_editor_extension_install(cli: &Path) -> AppResult<()> {
    let output = run_command(cli, &["--install-extension", PATCH_HELPER_EXTENSION])?;
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::Other(format!("安装中文补丁助手失败: {}", command_detail(&output))))
}

fn run_command(program: &Path, args: &[&str]) -> AppResult<std::process::Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = if requires_command_shell(program) {
            // Paths like `...\Microsoft VS Code\bin\code.cmd` contain spaces.
            // Pass a single /C string so cmd.exe does not split on spaces.
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C"]).arg(format_cmd_script_line(program, args));
            command
        } else {
            let mut command = Command::new(program);
            command.args(args);
            command
        };
        command.creation_flags(CREATE_NO_WINDOW);
        command.output().map_err(|error| AppError::Other(format!("启动命令失败: {error}")))
    }
    #[cfg(not(windows))]
    {
        Command::new(program).args(args).output()
            .map_err(|error| AppError::Other(format!("启动命令失败: {error}")))
    }
}

/// Build `call "program" arg1 arg2` for cmd.exe /C.
fn format_cmd_script_line(program: &Path, args: &[&str]) -> String {
    let mut line = format!("call {}", quote_cmd_arg(&program.to_string_lossy()));
    for arg in args {
        line.push(' ');
        line.push_str(&quote_cmd_arg(arg));
    }
    line
}

fn quote_cmd_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if !value.contains([' ', '\t', '"', '&', '|', '<', '>', '^', '%']) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn requires_command_shell(program: &Path) -> bool {
    program.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
}

fn command_detail(output: &std::process::Output) -> String {
    let text = if output.stderr.is_empty() { &output.stdout } else { &output.stderr };
    let decoded = String::from_utf8_lossy(text).trim().to_string();
    if decoded.is_empty() { format!("退出码 {}", output.status) } else { decoded }
}

fn editor_definitions() -> Vec<EditorDefinition> {
    let home = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    vec![
        EditorDefinition {
            id: "vscode",
            label: "VS Code",
            extension_dir: home.join(".vscode").join("extensions"),
            command_name: "code",
            cli_candidates: vec![
                local.join("Programs").join("Microsoft VS Code").join("bin").join("code.cmd"),
                PathBuf::from(r"C:\Program Files\Microsoft VS Code\bin\code.cmd"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd"),
            ],
        },
        EditorDefinition {
            id: "cursor",
            label: "Cursor",
            extension_dir: home.join(".cursor").join("extensions"),
            command_name: "cursor",
            cli_candidates: vec![
                local.join("Programs").join("Cursor").join("resources").join("app").join("bin").join("cursor.cmd"),
                local.join("Programs").join("cursor").join("resources").join("app").join("bin").join("cursor.cmd"),
                local.join("Programs").join("Cursor").join("resources").join("app").join("codeBin").join("cursor.cmd"),
                local.join("Programs").join("cursor").join("resources").join("app").join("codeBin").join("cursor.cmd"),
                PathBuf::from(r"C:\Program Files\Cursor\resources\app\bin\cursor.cmd"),
                PathBuf::from(r"C:\Program Files\cursor\resources\app\bin\cursor.cmd"),
            ],
        },
    ]
}

fn editor_status(definition: EditorDefinition) -> EditorLocalizationStatus {
    let claude_extension = find_extension(&definition.extension_dir, CLAUDE_EXTENSION_PREFIX);
    let helper_installed = find_extension(&definition.extension_dir, PATCH_HELPER_PREFIX).is_some();
    let cli = resolve_editor_cli(&definition);
    let editor_detected = cli.is_some() || definition.extension_dir.is_dir();
    let message = if claude_extension.is_none() {
        "未检测到 Claude Code for VS Code 扩展".to_string()
    } else if cli.is_none() {
        format!("已检测到 Claude Code for VS Code 扩展，但未找到 {} 命令行工具；请在编辑器命令面板安装 Shell Command，或将命令加入 PATH", definition.label)
    } else if helper_installed {
        "中文补丁助手已安装；请在编辑器命令面板确认应用补丁".to_string()
    } else {
        "已检测到官方扩展，可安装中文补丁助手".to_string()
    };
    EditorLocalizationStatus {
        id: definition.id.to_string(),
        label: definition.label.to_string(),
        editor_detected,
        editor_cli_path: cli.map(|path| path.to_string_lossy().to_string()),
        claude_extension_path: claude_extension.map(|path| path.to_string_lossy().to_string()),
        helper_installed,
        message,
    }
}

/// Resolve an editor command from its normal installation locations first and
/// then from PATH.  Cursor commonly installs only its user-level command, so
/// treating the extension directory as proof of a CLI used to produce a
/// misleading patch-helper error.
fn resolve_editor_cli(definition: &EditorDefinition) -> Option<PathBuf> {
    definition
        .cli_candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .or_else(|| find_cli_from_install_roots(definition))
        .or_else(|| find_command_on_path(definition.command_name))
}

fn find_cli_from_install_roots(definition: &EditorDefinition) -> Option<PathBuf> {
    for root in editor_install_roots(definition.id) {
        for candidate in cli_paths_for_install_root(definition.id, &root) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn cli_paths_for_install_root(editor_id: &str, root: &Path) -> Vec<PathBuf> {
    match editor_id {
        "vscode" => vec![
            root.join("bin").join("code.cmd"),
            root.join("bin").join("code.exe"),
        ],
        "cursor" => vec![
            root.join("resources").join("app").join("bin").join("cursor.cmd"),
            root.join("resources").join("app").join("bin").join("cursor.exe"),
            root.join("resources").join("app").join("codeBin").join("cursor.cmd"),
            root.join("resources").join("app").join("codeBin").join("cursor.exe"),
        ],
        _ => Vec::new(),
    }
}

#[cfg(windows)]
fn editor_install_roots(editor_id: &str) -> Vec<PathBuf> {
    let needles: &[&str] = match editor_id {
        "vscode" => &["Visual Studio Code"],
        "cursor" => &["Cursor"],
        _ => return Vec::new(),
    };
    let mut roots = Vec::new();
    for hive in [winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER), winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)] {
        for path in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(uninstall) = hive.open_subkey(path) else {
                continue;
            };
            for subkey_name in uninstall.enum_keys().flatten() {
                let Ok(subkey) = uninstall.open_subkey(&subkey_name) else {
                    continue;
                };
                let display_name: String = subkey.get_value("DisplayName").unwrap_or_default();
                if !needles.iter().any(|needle| display_name.contains(needle)) {
                    continue;
                }
                // Avoid matching unrelated "Cursor*" products by requiring exact-ish names.
                if editor_id == "cursor" && !display_name.to_ascii_lowercase().starts_with("cursor") {
                    continue;
                }
                let location: String = subkey.get_value("InstallLocation").unwrap_or_default();
                let location = location.trim().trim_end_matches(['\\', '/']);
                if location.is_empty() {
                    continue;
                }
                let root = PathBuf::from(location);
                if root.is_dir() {
                    roots.push(root);
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(not(windows))]
fn editor_install_roots(_editor_id: &str) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn find_command_on_path(command: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    // GUI-launched apps often miss the latest user PATH; also probe registry PATH.
    if let Some(path) = find_command_in_env_path(command) {
        return Some(path);
    }
    for name in [format!("{command}.cmd"), command.to_string()] {
        let mut where_command = Command::new("where.exe");
        where_command.arg(&name).creation_flags(CREATE_NO_WINDOW);
        let Ok(output) = where_command.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        if let Some(path) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .map(PathBuf::from)
            .find(|path| {
                path.is_file()
                    && !path
                        .components()
                        .any(|part| part.as_os_str() == "WindowsApps")
            })
        {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn find_command_in_env_path(command: &str) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(windows_user_path_dirs());
    let names = [
        format!("{command}.cmd"),
        format!("{command}.exe"),
        format!("{command}.bat"),
        command.to_string(),
    ];
    for dir in dirs {
        // WindowsApps execution aliases are stubs and break --install-extension.
        if dir.components().any(|part| part.as_os_str() == "WindowsApps") {
            continue;
        }
        for name in &names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_user_path_dirs() -> Vec<PathBuf> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let Ok(env) = hkcu.open_subkey("Environment") else {
        return Vec::new();
    };
    let Ok(value) = env.get_value::<String, _>("Path") else {
        return Vec::new();
    };
    let expanded = expand_windows_env(&value);
    std::env::split_paths(&expanded).collect()
}

#[cfg(windows)]
fn expand_windows_env(value: &str) -> String {
    let mut result = value.to_string();
    // Expand common %VAR% references used in user PATH.
    for (key, replacement) in [
        ("%USERPROFILE%", dirs::home_dir().map(|p| p.to_string_lossy().into_owned())),
        ("%LOCALAPPDATA%", dirs::data_local_dir().map(|p| p.to_string_lossy().into_owned())),
        ("%APPDATA%", dirs::config_dir().map(|p| p.to_string_lossy().into_owned())),
    ] {
        if let Some(replacement) = replacement {
            result = result.replace(key, &replacement);
            result = result.replace(&key.to_ascii_lowercase(), &replacement);
        }
    }
    result
}

#[cfg(not(windows))]
fn find_command_on_path(_command: &str) -> Option<PathBuf> {
    None
}

fn find_extension(root: &Path, prefix: &str) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(root).ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(prefix)))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_shell_is_only_used_for_command_scripts() {
        assert!(requires_command_shell(Path::new("C:/tools/code.cmd")));
        assert!(requires_command_shell(Path::new("C:/tools/install.BAT")));
        assert!(!requires_command_shell(Path::new("C:/tools/claude.exe")));
    }

    #[test]
    fn formats_cmd_script_line_with_quoted_spaces() {
        let line = format_cmd_script_line(
            Path::new(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd"),
            &["--install-extension", "shanjiancaofu.claude-code-zh-cn-patch-helper"],
        );
        assert_eq!(
            line,
            r#"call "C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd" --install-extension shanjiancaofu.claude-code-zh-cn-patch-helper"#
        );
    }

    #[test]
    fn builds_cli_paths_under_install_root() {
        let root = Path::new(r"F:\cursor");
        let paths = cli_paths_for_install_root("cursor", root);
        assert!(paths.iter().any(|p| p.ends_with(r"resources\app\bin\cursor.cmd") || p.ends_with("resources/app/bin/cursor.cmd")));
        let vscode = cli_paths_for_install_root("vscode", Path::new(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code"));
        assert!(vscode.iter().any(|p| p.ends_with("bin\\code.cmd") || p.ends_with("bin/code.cmd")));
    }

    #[cfg(windows)]
    #[test]
    fn resolves_local_editor_cli_when_installed() {
        let editors = editor_definitions();
        let vscode = editors.iter().find(|e| e.id == "vscode").unwrap();
        let cursor = editors.iter().find(|e| e.id == "cursor").unwrap();
        // This machine has user-level VS Code under Local\Programs.
        if PathBuf::from(r"C:\Users\admin\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd").is_file() {
            let cli = resolve_editor_cli(vscode).expect("VS Code CLI");
            assert!(cli.to_string_lossy().contains("code"));
        }
        // Portable/custom Cursor install registered via Uninstall InstallLocation.
        if PathBuf::from(r"f:\cursor\resources\app\bin\cursor.cmd").is_file() {
            let cli = resolve_editor_cli(cursor).expect("Cursor CLI");
            assert!(cli.to_string_lossy().to_ascii_lowercase().contains("cursor"));
        }
    }

    #[test]
    fn finds_only_expected_extension_prefix() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("anthropic.claude-code-2.1.183")).unwrap();
        fs::create_dir(dir.path().join("other.claude-code-9.0.0")).unwrap();
        let found = find_extension(dir.path(), CLAUDE_EXTENSION_PREFIX).unwrap();
        assert!(found.ends_with("anthropic.claude-code-2.1.183"));
    }

    #[test]
    fn normalizes_bare_spinner_verbs_array_to_object() {
        let mut settings = Map::new();
        settings.insert(
            "spinnerVerbs".to_string(),
            Value::Array(vec![
                Value::String("思考中".to_string()),
                Value::String("编写中".to_string()),
                Value::Bool(true),
            ]),
        );
        normalize_spinner_verbs(&mut settings);
        let verbs = settings
            .get("spinnerVerbs")
            .and_then(Value::as_object)
            .expect("spinnerVerbs object");
        assert_eq!(verbs.get("mode").and_then(Value::as_str), Some("replace"));
        assert_eq!(
            verbs.get("verbs").and_then(Value::as_array).map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn leaves_valid_spinner_verbs_object_untouched() {
        let mut settings = Map::new();
        settings.insert(
            "spinnerVerbs".to_string(),
            serde_json::json!({
                "mode": "append",
                "verbs": ["Pondering"]
            }),
        );
        normalize_spinner_verbs(&mut settings);
        assert_eq!(
            settings.get("spinnerVerbs").and_then(|value| value.get("mode")).and_then(Value::as_str),
            Some("append")
        );
    }
}
