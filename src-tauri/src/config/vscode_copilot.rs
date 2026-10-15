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

/// `chatLanguageModels.json` 路径。`AISW_VSCODE_USER_DIR` 可覆盖（测试与便携版）。
pub fn chat_language_models_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("AISW_VSCODE_USER_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir).join(FILE_NAME);
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join("AppData").join("Roaming"));
    #[cfg(target_os = "macos")]
    let base = get_home_dir().join("Library").join("Application Support");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".config"));
    base.join("Code").join("User").join(FILE_NAME)
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

/// 把托管条目并入条目列表：已有则原位替换，否则追加；其余条目不动。
/// `settings` 由 VS Code 按模型维护（如 `reasoningEffort`），重同步时必须保留。
fn merge_entry(entries: &mut Vec<Value>, managed: Value) {
    let mut placed = false;
    entries.retain_mut(|entry| {
        if !is_managed(entry) {
            return true;
        }
        if placed {
            return false; // 重复的托管项只留一份
        }
        let mut replacement = managed.clone();
        if let (Some(settings), Some(replacement)) =
            (entry.get("settings").cloned(), replacement.as_object_mut())
        {
            replacement.insert("settings".into(), settings);
        }
        *entry = replacement;
        placed = true;
        true
    });
    if !placed {
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

pub fn apply_managed_at(path: &Path, managed: Value) -> AppResult<()> {
    let _guard = lock()?;
    let mut entries = read_entries(path)?;
    backup(path)?;
    merge_entry(&mut entries, managed);
    write_entries(path, &entries)
}

/// 只移除托管条目，用户其他 provider 保留。返回是否有改动。
pub fn remove_managed_at(path: &Path) -> AppResult<bool> {
    let _guard = lock()?;
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
    if expected.get("models") != current.get("models") {
        let (want, have) = (ids(expected), ids(current));
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
}
