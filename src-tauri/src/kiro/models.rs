//! Public Kiro model ids and the normalization used on the way out.
//!
//! The id the client sent is echoed back on the response. Only the upstream
//! payload uses the normalized id. `-thinking` is a request flag, not a catalog row.

use std::collections::HashSet;
use std::fs;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::config::get_app_config_dir;
use crate::error::{AppError, AppResult};

pub const DEFAULT_MODEL: &str = "claude-sonnet-4.6";
pub const THINKING_BUDGET_CAP: u32 = 24_576;

const FAMILIES: [&str; 5] = ["sonnet", "opus", "haiku", "fable", "mythos"];
const MODELS_FILE: &str = "kiro_models.json";

/// Ids Kiro listed for a real account (`ListAvailableModels`), used until the first
/// live refresh. `auto` is Kiro's router and is not a catalog row.
const BUILTIN_IDS: &[&str] = &[
    "claude-opus-5.5",
    "claude-sonnet-5.5",
    "claude-opus-5",
    "claude-sonnet-5",
    "claude-opus-4.8",
    "claude-opus-4.7",
    "claude-opus-4.6",
    "claude-sonnet-4.6",
    "claude-opus-4.5",
    "claude-sonnet-4.5",
    "claude-sonnet-4",
    "claude-haiku-4.5",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "deepseek-3.2",
    "minimax-m2.5",
    "minimax-m2.1",
    "glm-5",
    "qwen3-coder-next",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub images: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredModels {
    #[serde(default)]
    fetched_at: i64,
    #[serde(default)]
    models: Vec<KiroModelInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroModelList {
    pub models: Vec<KiroModelInfo>,
    /// `live` after a successful refresh, otherwise `builtin`.
    pub source: String,
    pub fetched_at: i64,
}

fn live_cell() -> &'static RwLock<StoredModels> {
    static LIVE: OnceLock<RwLock<StoredModels>> = OnceLock::new();
    LIVE.get_or_init(|| {
        let stored = fs::read_to_string(get_app_config_dir().join(MODELS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str::<StoredModels>(&text).ok())
            .unwrap_or_default();
        RwLock::new(stored)
    })
}

fn builtin_models() -> Vec<KiroModelInfo> {
    BUILTIN_IDS
        .iter()
        .map(|id| KiroModelInfo {
            id: (*id).to_string(),
            name: (*id).to_string(),
            context: 0,
            output: 0,
            images: false,
        })
        .collect()
}

pub fn model_list() -> KiroModelList {
    let live = live_cell()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if live.models.is_empty() {
        return KiroModelList {
            models: builtin_models(),
            source: "builtin".into(),
            fetched_at: 0,
        };
    }
    KiroModelList {
        models: live.models,
        source: "live".into(),
        fetched_at: live.fetched_at,
    }
}

/// Models the local `/v1/models` list exposes. No synthetic `-thinking` ids.
pub fn catalog_ids() -> Vec<String> {
    model_list()
        .models
        .into_iter()
        .map(|model| model.id)
        .collect()
}

/// Stores a live list and persists it next to the account file.
pub fn save_live_models(models: Vec<KiroModelInfo>, fetched_at: i64) -> AppResult<KiroModelList> {
    let stored = StoredModels { fetched_at, models };
    let text = serde_json::to_string_pretty(&stored)
        .map_err(|error| AppError::Json(format!("序列化 Kiro 模型列表失败: {error}")))?;
    if crate::sidecar::delegated() {
        crate::sidecar::commit_store(crate::sidecar::STORE_KIRO_MODELS, &text, None)?;
    } else {
        let path = get_app_config_dir().join(MODELS_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    *live_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = stored;
    Ok(model_list())
}

/// Replace the cached model list from a main-process snapshot and write the file.
pub(crate) fn apply_committed_snapshot(raw: &str) -> AppResult<(String, u64)> {
    let stored: StoredModels = serde_json::from_str(raw)
        .map_err(|error| AppError::Config(format!("Kiro 模型快照无效: {error}")))?;
    let path = get_app_config_dir().join(MODELS_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, raw)?;
    *live_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = stored;
    let meta = fs::metadata(&path)?;
    let mtime_ns = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    Ok((mtime_ns, meta.len()))
}

/// Parses `ListAvailableModels`. `auto` and rows without an id are dropped; the
/// service's own order (newest first) is kept.
pub fn parse_available_models(body: &str) -> Result<Vec<KiroModelInfo>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("Kiro 模型列表不是有效 JSON: {error}"))?;
    let rows = value
        .get("models")
        .and_then(|rows| rows.as_array())
        .ok_or_else(|| "Kiro 模型列表缺少 models 字段".to_string())?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        let id = row
            .get("modelId")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .unwrap_or("");
        if id.is_empty() || id.eq_ignore_ascii_case("auto") || !seen.insert(id.to_string()) {
            continue;
        }
        let name = row
            .get("modelName")
            .and_then(|name| name.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(id);
        let limit = |key: &str| {
            row.pointer(&format!("/tokenLimits/{key}"))
                .and_then(|limit| limit.as_u64())
                .unwrap_or(0)
        };
        let images = row
            .get("supportedInputTypes")
            .and_then(|types| types.as_array())
            .is_some_and(|types| {
                types.iter().any(|kind| {
                    kind.as_str()
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("image"))
                })
            });
        models.push(KiroModelInfo {
            id: id.to_string(),
            name: name.to_string(),
            context: limit("maxInputTokens"),
            output: limit("maxOutputTokens"),
            images,
        });
    }
    if models.is_empty() {
        return Err("Kiro 没有返回可用模型".into());
    }
    Ok(models)
}

pub fn preferred_default_model() -> String {
    DEFAULT_MODEL.to_string()
}

pub fn preferred_opus() -> String {
    "claude-opus-4.6".to_string()
}

pub fn preferred_haiku() -> String {
    "claude-haiku-4.5".to_string()
}

/// Strip thinking/latest/date suffixes and rewrite dated Claude ids.
/// Non-Claude ids pass through unchanged when they are non-empty.
pub fn map_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || trimmed.chars().any(|ch| ch.is_control()) {
        return None;
    }
    normalize_claude_model(trimmed).or_else(|| Some(trimmed.to_string()))
}

pub fn normalize_claude_model(model: &str) -> Option<String> {
    let mut normalized = model.to_ascii_lowercase();
    loop {
        let mut stripped = false;
        for suffix in ["-thinking", "-latest"] {
            if let Some(next) = normalized.strip_suffix(suffix) {
                normalized = next.to_string();
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    if let Some((base, suffix)) = normalized.rsplit_once('-') {
        if suffix.len() == 8 && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            normalized = base.to_string();
        }
    }
    let body = normalized.strip_prefix("claude-")?;
    for family in FAMILIES {
        if let Some(rest) = body.strip_prefix(family) {
            let rest = rest
                .strip_prefix('-')
                .or_else(|| rest.strip_prefix('.'))
                .unwrap_or(rest);
            let parts: Vec<&str> = rest.split('-').filter(|part| !part.is_empty()).collect();
            let version = canonical_version(&parts)?;
            return Some(format!("claude-{family}-{version}"));
        }
    }
    let parts: Vec<&str> = body.split('-').collect();
    let family_index = parts.iter().position(|part| FAMILIES.contains(part))?;
    if family_index == 0 || family_index + 1 != parts.len() {
        return None;
    }
    let version = canonical_version(&parts[..family_index])?;
    Some(format!("claude-{}-{version}", parts[family_index]))
}

fn canonical_version(parts: &[&str]) -> Option<String> {
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    if !parts
        .iter()
        .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return None;
    }
    Some(parts.join("."))
}

/// `enabled` / `adaptive` count. `disabled` and an empty object do not.
pub fn thinking_requested(value: &serde_json::Value) -> bool {
    let thinking = value.get("thinking").filter(|item| !item.is_null());
    match thinking {
        Some(serde_json::Value::Object(map)) if map.is_empty() => false,
        Some(item) => {
            let kind = item
                .get("type")
                .and_then(|kind| kind.as_str())
                .unwrap_or("");
            !kind.eq_ignore_ascii_case("disabled")
        }
        None => value
            .get("model")
            .and_then(|model| model.as_str())
            .is_some_and(|model| model.to_ascii_lowercase().ends_with("-thinking")),
    }
}

pub fn clamped_thinking_budget(value: &serde_json::Value) -> Option<u32> {
    if !thinking_requested(value) {
        return None;
    }
    let raw = value
        .pointer("/thinking/budget_tokens")
        .and_then(|item| item.as_u64())
        .unwrap_or(20_000);
    Some(
        u32::try_from(raw)
            .unwrap_or(THINKING_BUDGET_CAP)
            .min(THINKING_BUDGET_CAP),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dated_sonnet_and_thinking_suffix_normalize() {
        assert_eq!(
            map_model("claude-3-5-sonnet-20241022").as_deref(),
            Some("claude-sonnet-3.5")
        );
        assert_eq!(
            map_model("claude-sonnet-4-6-thinking").as_deref(),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(map_model("glm-5").as_deref(), Some("glm-5"));
    }

    #[test]
    fn parses_live_model_list_without_auto_or_duplicates() {
        let body = json!({
            "defaultModel": {"modelId": "auto"},
            "models": [
                {"modelId": "auto", "modelName": "Auto"},
                {
                    "modelId": "claude-opus-5.5",
                    "modelName": "Claude Opus 5.5",
                    "tokenLimits": {"maxInputTokens": 1000000, "maxOutputTokens": 128000},
                    "supportedInputTypes": ["TEXT", "IMAGE"]
                },
                {"modelId": "glm-5", "supportedInputTypes": ["TEXT"]},
                {"modelId": "glm-5"},
                {"modelId": "  "}
            ]
        })
        .to_string();
        let models = parse_available_models(&body).expect("parse");
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-opus-5.5", "glm-5"]
        );
        assert_eq!(models[0].name, "Claude Opus 5.5");
        assert_eq!((models[0].context, models[0].output), (1_000_000, 128_000));
        assert!(models[0].images);
        assert_eq!(models[1].name, "glm-5");
        assert!(!models[1].images);
        assert!(parse_available_models("{\"models\": []}").is_err());
    }

    #[test]
    fn builtin_catalog_has_no_auto_and_current_families() {
        assert!(BUILTIN_IDS.iter().all(|id| *id != "auto"));
        for id in [
            "claude-opus-5.5",
            "claude-sonnet-5.5",
            "claude-haiku-4.5",
            "glm-5",
        ] {
            assert!(BUILTIN_IDS.contains(&id), "{id}");
        }
        assert_eq!(
            map_model("claude-opus-5.5").as_deref(),
            Some("claude-opus-5.5")
        );
        assert_eq!(
            map_model("claude-sonnet-5-5").as_deref(),
            Some("claude-sonnet-5.5")
        );
    }

    #[test]
    fn thinking_budget_caps_and_ignores_disabled() {
        assert_eq!(
            clamped_thinking_budget(
                &json!({"thinking": {"type": "enabled", "budget_tokens": 99999}})
            ),
            Some(THINKING_BUDGET_CAP)
        );
        assert_eq!(
            clamped_thinking_budget(&json!({"thinking": {"type": "disabled"}})),
            None
        );
        assert_eq!(clamped_thinking_budget(&json!({"thinking": {}})), None);
        assert_eq!(
            clamped_thinking_budget(&json!({"model": "claude-sonnet-4.6-thinking"})),
            Some(20_000)
        );
    }
}
