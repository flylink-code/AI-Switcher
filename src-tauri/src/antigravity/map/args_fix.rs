//! Tool-call argument key correction.
//!
//! The upstream (Gemini via Cloud Code, `toolConfig.mode = AUTO`) performs no
//! constrained decoding, so the model sometimes invents parameter names — the
//! classic case is dropping the leading `-` from CLI-style flags (`-n` → `n`).
//! Client-side strict schema validation then rejects the call
//! (`InputValidationError`). Since the gateway knows the declared parameter
//! names from this request's `functionDeclarations`, it can rename args keys
//! back when the correction is unambiguous.

use serde_json::Value;

/// Tool name → declared top-level parameter keys (from sanitized
/// `functionDeclarations[].parameters.properties`).
pub type ToolParamKeys = std::collections::HashMap<String, Vec<String>>;

/// Extract declared parameter keys per tool from Gemini function declarations.
pub fn param_keys_from_declarations(declarations: &[Value]) -> ToolParamKeys {
    let mut map = ToolParamKeys::new();
    for decl in declarations {
        let Some(name) = decl.get("name").and_then(Value::as_str) else {
            continue;
        };
        let keys: Vec<String> = decl
            .get("parameters")
            .and_then(|p| p.get("properties"))
            .and_then(Value::as_object)
            .map(|props| props.keys().cloned().collect())
            .unwrap_or_default();
        map.insert(name.to_string(), keys);
    }
    map
}

/// Normalized comparison form: strip leading dashes, lowercase, `_` → `-`.
/// `-n` / `n` / `N` all normalize to `n`; `output_mode` / `output-mode` match.
fn normalize_key(key: &str) -> String {
    key.trim_start_matches('-').to_lowercase().replace('_', "-")
}

/// Rename args keys that are not declared but uniquely match a declared key
/// after normalization, and normalize common Gemini `toolConfig.mode = AUTO`
/// value type drift (Windows drive paths, `view_range`, integer/boolean fields).
pub fn correct_tool_args(tool_name: &str, args: Value, tool_params: &ToolParamKeys) -> Value {
    let Value::Object(mut map) = args else {
        return args;
    };
    if let Some(declared) = tool_params.get(tool_name) {
        let original_keys: Vec<String> = map.keys().cloned().collect();
        for key in original_keys {
            if declared.iter().any(|d| d == &key) {
                continue;
            }
            let normalized = normalize_key(&key);
            if normalized.is_empty() {
                continue;
            }
            let matches: Vec<&String> = declared
                .iter()
                .filter(|d| normalize_key(d) == normalized)
                .collect();
            // Only rename on a unique hit — never guess between candidates.
            if matches.len() == 1 {
                if let Some(value) = map.remove(&key) {
                    map.insert(matches[0].clone(), value);
                }
            }
        }
    }
    normalize_tool_arg_values(tool_name, &mut map);
    Value::Object(map)
}

fn normalize_tool_arg_values(tool_name: &str, map: &mut serde_json::Map<String, Value>) {
    let lower_tool = tool_name.trim().to_ascii_lowercase();
    let is_uri_field = |key: &str| matches!(key, "uri" | "fileUri" | "resourceUri" | "url");

    for (key, value) in map.iter_mut() {
        if matches!(
            key.as_str(),
            "path" | "filePath" | "file_path" | "workingDirectory" | "cwd" | "workspaceFolder"
        ) {
            if let Value::String(raw) = value {
                if let Some(fixed) = normalize_windows_fs_path(raw) {
                    *raw = fixed;
                }
            }
        } else if matches!(key.as_str(), "paths" | "files" | "coverageFiles" | "selectFiles")
            && !is_uri_field(key)
        {
            if let Value::Array(items) = value {
                for item in items.iter_mut() {
                    if let Value::String(raw) = item {
                        if let Some(fixed) = normalize_windows_fs_path(raw) {
                            *raw = fixed;
                        }
                    }
                }
            }
        } else if key == "view_range" {
            normalize_view_range_value(value);
        } else if key == "requestedSchema" {
            normalize_ask_user_schema(value);
        } else if matches!(
            key.as_str(),
            "offset"
                | "limit"
                | "start_line"
                | "end_line"
                | "line"
                | "-A"
                | "-B"
                | "-C"
                | "head_limit"
                | "initial_wait"
                | "delay"
                | "timeout"
        ) {
            if let Some(int_val) = coerce_to_i64(value) {
                *value = serde_json::json!(int_val);
            }
        } else if matches!(
            key.as_str(),
            "forceReadLargeFiles" | "-i" | "-n" | "multiline" | "raw" | "detach"
        ) {
            if let Value::String(s) = value {
                match s.trim().to_ascii_lowercase().as_str() {
                    "true" => *value = Value::Bool(true),
                    "false" => *value = Value::Bool(false),
                    _ => {}
                }
            }
        }
    }

    // For `view` / `read_file` / `Read`, drop invalid empty `view_range` arrays so
    // the client falls back to reading the whole file instead of rejecting the call.
    if matches!(lower_tool.as_str(), "view" | "read_file" | "read") {
        if let Some(Value::Array(arr)) = map.get("view_range") {
            if arr.len() != 2 || !arr.iter().all(|v| v.as_i64().is_some()) {
                map.remove("view_range");
            }
        }
    }
}

/// Convert `file:///c:/...`, `file:///c%3A/...`, or forward-slash Windows drive
/// paths `C:/Users/...` into canonical Windows backslash paths `C:\Users\...`.
/// Leaves non-Windows paths (`/etc/hosts`, `src/foo.rs`) untouched.
fn normalize_windows_fs_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = if let Some(rest) = trimmed
        .strip_prefix("file:///")
        .or_else(|| trimmed.strip_prefix("FILE:///"))
    {
        percent_decode_path(rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("file://")
        .or_else(|| trimmed.strip_prefix("FILE://"))
    {
        percent_decode_path(rest)
    } else {
        trimmed.to_string()
    };

    let bytes = stripped.as_bytes();
    let is_windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\');
    if !is_windows_drive {
        return None;
    }

    let normalized = stripped.replace('/', "\\");
    if normalized != raw {
        Some(normalized)
    } else {
        None
    }
}

fn percent_decode_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn coerce_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64().and_then(|f| {
                if f.is_finite() && (f - f.round()).abs() < 1e-6 {
                    Some(f.round() as i64)
                } else {
                    None
                }
            })
        }),
        Value::String(s) => {
            let t = s.trim();
            t.parse::<i64>().ok().or_else(|| {
                t.parse::<f64>().ok().and_then(|f| {
                    if f.is_finite() && (f - f.round()).abs() < 1e-6 {
                        Some(f.round() as i64)
                    } else {
                        None
                    }
                })
            })
        }
        _ => None,
    }
}

fn normalize_view_range_value(value: &mut Value) {
    if let Value::String(s) = value {
        let trimmed = s.trim();
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            *value = parsed;
        } else if let Some((a, b)) = trimmed
            .trim_matches(|c| c == '[' || c == ']')
            .split_once(|c: char| c == ',' || c == '-' || c == ':')
        {
            if let (Ok(start), Ok(end)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                *value = serde_json::json!([start, end]);
            }
        }
    }

    if let Value::Array(items) = value {
        for item in items.iter_mut() {
            if let Some(int_val) = coerce_to_i64(item) {
                *item = serde_json::json!(int_val);
            }
        }
        if items.len() == 2 {
            if let (Some(mut start), Some(mut end)) = (items[0].as_i64(), items[1].as_i64()) {
                if start < 1 {
                    start = 1;
                }
                if end != -1 && end < start {
                    end = start;
                }
                items[0] = serde_json::json!(start);
                items[1] = serde_json::json!(end);
            }
        }
    }
}

fn infer_json_schema_value_type(val: &Value) -> &'static str {
    match val {
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        _ => "string",
    }
}

fn infer_field_schema_type(field_map: &serde_json::Map<String, Value>) -> &'static str {
    if field_map.contains_key("items") {
        return "array";
    }
    if field_map.contains_key("properties") {
        return "object";
    }
    for union_key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(branches)) = field_map.get(union_key) {
            for branch in branches {
                if let Value::Object(bmap) = branch {
                    if let Some(t) = bmap.get("type").and_then(Value::as_str) {
                        if t != "null" && !t.is_empty() {
                            return match t {
                                "boolean" => "boolean",
                                "integer" => "integer",
                                "number" => "number",
                                "array" => "array",
                                "object" => "object",
                                _ => "string",
                            };
                        }
                    }
                    if let Some(c) = bmap.get("const") {
                        return infer_json_schema_value_type(c);
                    }
                }
            }
        }
    }
    if let Some(Value::Array(enums)) = field_map.get("enum") {
        if let Some(first) = enums.first() {
            return infer_json_schema_value_type(first);
        }
    }
    if let Some(def) = field_map.get("default") {
        return infer_json_schema_value_type(def);
    }
    "string"
}

fn normalize_ask_user_schema(schema: &mut Value) {
    if let Value::String(raw) = schema {
        if let Ok(parsed) = serde_json::from_str::<Value>(raw.trim()) {
            *schema = parsed;
        }
    }
    let Value::Object(schema_map) = schema else {
        return;
    };
    let Some(Value::Object(props)) = schema_map.get_mut("properties") else {
        return;
    };
    for (_field_name, field_schema) in props.iter_mut() {
        let Value::Object(field_map) = field_schema else {
            continue;
        };
        if let Some(Value::Object(items_map)) = field_map.get_mut("items") {
            if !items_map.contains_key("type") {
                let item_ty = infer_field_schema_type(items_map);
                items_map.insert("type".to_string(), Value::String(item_ty.to_string()));
            }
        }
        if !field_map.contains_key("type") {
            let field_ty = infer_field_schema_type(field_map);
            field_map.insert("type".to_string(), Value::String(field_ty.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grep_params() -> ToolParamKeys {
        ToolParamKeys::from([(
            "Grep".to_string(),
            vec![
                "pattern".to_string(),
                "path".to_string(),
                "-n".to_string(),
                "-A".to_string(),
                "-B".to_string(),
                "-C".to_string(),
                "output_mode".to_string(),
            ],
        )])
    }

    #[test]
    fn missing_dash_is_corrected_on_unique_hit() {
        let args = json!({ "pattern": "antigravity", "n": true, "-C": 3 });
        let fixed = correct_tool_args("Grep", args, &grep_params());
        assert_eq!(fixed["-n"], json!(true));
        assert!(fixed.get("n").is_none());
        // 已声明的 key 原样保留。
        assert_eq!(fixed["-C"], json!(3));
        assert_eq!(fixed["pattern"], json!("antigravity"));
    }

    #[test]
    fn unknown_key_without_match_stays() {
        let args = json!({ "pattern": "x", "foo": 1 });
        let fixed = correct_tool_args("Grep", args, &grep_params());
        assert_eq!(fixed["foo"], json!(1));
    }

    #[test]
    fn ambiguous_match_is_not_renamed() {
        let params =
            ToolParamKeys::from([("Tool".to_string(), vec!["-a".to_string(), "a".to_string()])]);
        // "A" 归一后同时命中 "-a" 和 "a" → 不改。
        let args = json!({ "A": 1 });
        let fixed = correct_tool_args("Tool", args, &params);
        assert_eq!(fixed["A"], json!(1));
        assert!(fixed.get("-a").is_none());
    }

    #[test]
    fn case_and_underscore_normalized() {
        let args = json!({ "output-mode": "content" });
        let fixed = correct_tool_args("Grep", args, &grep_params());
        assert_eq!(fixed["output_mode"], json!("content"));
    }

    #[test]
    fn unknown_tool_and_non_object_args_pass_through() {
        let params = grep_params();
        let args = json!({ "n": true });
        assert_eq!(
            correct_tool_args("UnknownTool", args.clone(), &params),
            args
        );
        let scalar = json!("not-an-object");
        assert_eq!(correct_tool_args("Grep", scalar.clone(), &params), scalar);
    }

    #[test]
    fn param_keys_extracted_from_declarations() {
        let declarations = vec![json!({
            "name": "Grep",
            "description": "search",
            "parameters": { "type": "object", "properties": { "pattern": {}, "-n": {} } }
        })];
        let keys = param_keys_from_declarations(&declarations);
        assert_eq!(keys["Grep"], vec!["pattern".to_string(), "-n".to_string()]);
    }

    #[test]
    fn view_tool_normalizes_windows_paths_and_view_range() {
        let params = ToolParamKeys::from([(
            "view".to_string(),
            vec![
                "path".to_string(),
                "view_range".to_string(),
                "forceReadLargeFiles".to_string(),
            ],
        )]);
        let args = json!({
            "path": "file:///c%3A/Users/admin/AppData/Local/Temp/out.txt",
            "view-range": [0.0, "100"],
            "forceReadLargeFiles": "false"
        });
        let fixed = correct_tool_args("view", args, &params);
        assert_eq!(
            fixed["path"],
            json!(r"c:\Users\admin\AppData\Local\Temp\out.txt")
        );
        assert_eq!(fixed["view_range"], json!([1, 100]));
        assert_eq!(fixed["forceReadLargeFiles"], json!(false));

        let forward_slash = json!({
            "path": "C:/Users/admin/AppData/Local/Temp/out.txt",
            "view_range": "[10, 5]"
        });
        let fixed2 = correct_tool_args("view", forward_slash, &params);
        assert_eq!(
            fixed2["path"],
            json!(r"C:\Users\admin\AppData\Local\Temp\out.txt")
        );
        assert_eq!(fixed2["view_range"], json!([10, 10]));
    }

    #[test]
    fn ask_user_schema_infers_missing_property_types_for_one_of() {
        let params = ToolParamKeys::from([(
            "ask_user".to_string(),
            vec!["message".to_string(), "requestedSchema".to_string()],
        )]);
        let args = json!({
            "message": "请选择发布配置",
            "requestedSchema": {
                "required": ["codemapPolicy", "releaseVersion"],
                "properties": {
                    "releaseVersion": {
                        "default": "1.6.2",
                        "oneOf": [
                            { "title": "v1.6.2（正式版，推荐）", "const": "1.6.2" },
                            { "title": "v1.6.2-beta.3（预发布版）", "const": "1.6.2-beta.3" }
                        ]
                    },
                    "codemapPolicy": {
                        "default": "ignore_local",
                        "oneOf": [
                            { "title": "不提交图谱", "const": "ignore_local" },
                            { "title": "提交图谱", "const": "commit_all" }
                        ]
                    }
                }
            }
        });
        let fixed = correct_tool_args("ask_user", args, &params);
        assert_eq!(
            fixed["requestedSchema"]["properties"]["releaseVersion"]["type"],
            json!("string")
        );
        assert_eq!(
            fixed["requestedSchema"]["properties"]["codemapPolicy"]["type"],
            json!("string")
        );
    }
}
