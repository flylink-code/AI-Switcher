//! Moonshot / Kimi Chat Completions schema compatibility for the Codex bridge.
//!
//! Moonshot validates `tools[].function.parameters` with a pre-2019-09 reading
//! of JSON Schema: `$ref` plus sibling keywords is HTTP 400. Move `$ref` into
//! `allOf` and leave siblings in place. Only rewrite when the upstream host is
//! Moonshot/Kimi so other providers keep byte-identical tool schemas.

use serde_json::{json, Map, Value};
use url::Url;

const MOONSHOT_HOST_SUFFIXES: &[&str] = &["moonshot.cn", "moonshot.ai", "kimi.com"];

const SINGLE_SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "additionalItems",
    "unevaluatedItems",
    "contains",
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "contentSchema",
];

const SCHEMA_ARRAY_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "dependencies",
];

pub fn upstream_requires_ref_sibling_all_of(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    MOONSHOT_HOST_SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

pub fn rewrite_chat_tools_if_needed(base_url: &str, chat_body: &mut Value) {
    if upstream_requires_ref_sibling_all_of(base_url) {
        wrap_ref_siblings_in_chat_tools(chat_body);
    }
}

pub fn wrap_ref_siblings_in_chat_tools(chat_body: &mut Value) -> usize {
    let Some(tools) = chat_body.get_mut("tools").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut changed = 0;
    for tool in tools.iter_mut() {
        let Some(parameters) = tool
            .get_mut("function")
            .and_then(|function| function.get_mut("parameters"))
        else {
            continue;
        };
        if wrap_ref_siblings(parameters) > 0 {
            changed += 1;
        }
    }
    changed
}

pub fn wrap_ref_siblings(schema: &mut Value) -> usize {
    let Value::Object(map) = schema else {
        return 0;
    };
    let mut rewritten = 0;
    if map.len() > 1 && map.get("$ref").is_some_and(Value::is_string) {
        move_ref_into_all_of(map);
        rewritten += 1;
    }
    for (key, child) in map.iter_mut() {
        let key = key.as_str();
        if SCHEMA_MAP_KEYWORDS.contains(&key) {
            if let Value::Object(entries) = child {
                rewritten += entries.values_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SCHEMA_ARRAY_KEYWORDS.contains(&key) {
            if let Value::Array(entries) = child {
                rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SINGLE_SCHEMA_KEYWORDS.contains(&key) {
            match child {
                Value::Array(entries) => {
                    rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
                }
                other => rewritten += wrap_ref_siblings(other),
            }
        }
    }
    rewritten
}

fn move_ref_into_all_of(map: &mut Map<String, Value>) {
    let Some(reference) = map.remove("$ref") else {
        return;
    };
    let branch = json!({ "$ref": reference });
    match map.get_mut("allOf") {
        Some(Value::Array(branches)) => branches.push(branch),
        _ => {
            map.insert("allOf".to_string(), Value::Array(vec![branch]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop_like_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": { "$ref": "#/$defs/__schema20", "description": "Prompt to run" },
                "mode": { "type": "string", "enum": ["fast", "slow"] }
            },
            "required": ["prompt"],
            "$defs": {
                "__schema20": { "$ref": "#/$defs/__schema2", "type": "string", "minLength": 1 },
                "__schema2": { "type": "string" }
            }
        })
    }

    fn has_ref_with_siblings(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                (map.len() > 1 && map.contains_key("$ref"))
                    || map.values().any(has_ref_with_siblings)
            }
            Value::Array(items) => items.iter().any(has_ref_with_siblings),
            _ => false,
        }
    }

    #[test]
    fn gate_matches_moonshot_and_kimi_hosts_only() {
        for url in [
            "https://api.moonshot.cn/v1",
            "https://api.moonshot.ai/v1/",
            "https://api.kimi.com/coding/v1",
        ] {
            assert!(upstream_requires_ref_sibling_all_of(url), "{url}");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://kimi-relay.example.com/v1",
            "https://api.kimi.com.evil.net/v1",
            "api.moonshot.cn/v1",
            "",
        ] {
            assert!(!upstream_requires_ref_sibling_all_of(url), "{url}");
        }
    }

    #[test]
    fn wraps_ref_siblings_in_properties_and_defs() {
        let mut schema = desktop_like_schema();
        assert_eq!(wrap_ref_siblings(&mut schema), 2);
        assert!(!has_ref_with_siblings(&schema));
        assert_eq!(
            schema["properties"]["prompt"]["allOf"][0]["$ref"],
            "#/$defs/__schema20"
        );
        assert_eq!(
            schema["properties"]["prompt"]["description"],
            "Prompt to run"
        );
    }

    #[test]
    fn bare_refs_are_untouched() {
        let original = json!({
            "type": "object",
            "properties": { "a": { "$ref": "#/$defs/A" } },
            "$defs": { "A": { "type": "string" } }
        });
        let mut schema = original.clone();
        assert_eq!(wrap_ref_siblings(&mut schema), 0);
        assert_eq!(schema, original);
    }

    #[test]
    fn rewrite_chat_tools_only_for_moonshot_hosts() {
        let mut chat = json!({
            "tools": [{
                "type": "function",
                "function": { "name": "desktop", "parameters": desktop_like_schema() }
            }]
        });
        rewrite_chat_tools_if_needed("https://api.openai.com/v1", &mut chat);
        assert!(has_ref_with_siblings(
            &chat["tools"][0]["function"]["parameters"]
        ));
        rewrite_chat_tools_if_needed("https://api.kimi.com/coding/v1", &mut chat);
        assert!(!has_ref_with_siblings(
            &chat["tools"][0]["function"]["parameters"]
        ));
    }
}
