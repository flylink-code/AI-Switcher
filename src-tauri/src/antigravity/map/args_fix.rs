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
    key.trim_start_matches('-')
        .to_lowercase()
        .replace('_', "-")
}

/// Rename args keys that are not declared but uniquely match a declared key
/// after normalization. Ambiguous or unmatched keys are left untouched.
pub fn correct_tool_args(tool_name: &str, args: Value, tool_params: &ToolParamKeys) -> Value {
    let Value::Object(mut map) = args else {
        return args;
    };
    let Some(declared) = tool_params.get(tool_name) else {
        return Value::Object(map);
    };
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
    Value::Object(map)
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
        let params = ToolParamKeys::from([(
            "Tool".to_string(),
            vec!["-a".to_string(), "a".to_string()],
        )]);
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
        assert_eq!(correct_tool_args("UnknownTool", args.clone(), &params), args);
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
}
