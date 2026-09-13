//! Structured route-mode matching (plan / think / edit / long_context / ...).

use serde_json::Value;

use crate::database::dao::gateway::{
    list_route_modes, repair_long_context_one_token_threshold, RouteMode,
};
use crate::provider::ProviderTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeId {
    Default,
    Background,
    Plan,
    Think,
    Edit,
    LongContext,
    WebSearch,
    Vision,
    ImageGen,
}

impl ModeId {
    pub fn as_str(self) -> &'static str {
        match self {
            ModeId::Default => "default",
            ModeId::Background => "background",
            ModeId::Plan => "plan",
            ModeId::Think => "think",
            ModeId::Edit => "edit",
            ModeId::LongContext => "long_context",
            ModeId::WebSearch => "web_search",
            ModeId::ImageGen => "image_gen",
            ModeId::Vision => "vision",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModeSignals {
    pub token_count: u32,
    pub has_web_search: bool,
    pub has_vision: bool,
    pub has_thinking: bool,
    pub is_subagent: bool,
    pub is_image_gen: bool,
    pub tool_names: Vec<String>,
    pub recent_write_tool: Option<String>,
    pub target: Option<ProviderTarget>,
    pub path: String,
}

pub fn extract_tool_names(body: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                names.push(name.to_string());
            } else if let Some(name) = tool
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

pub fn has_vision_content(body: &Value) -> bool {
    latest_user_turn(body).is_some_and(contains_visible_image)
}

fn latest_user_turn(body: &Value) -> Option<&Value> {
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        return messages
            .iter()
            .rev()
            .find(|item| item.get("role").and_then(Value::as_str) == Some("user"));
    }
    let input = body.get("input").and_then(Value::as_array)?;
    input
        .iter()
        .rev()
        .find(|item| item.get("role").and_then(Value::as_str) == Some("user"))
        .or_else(|| input.last())
}

fn contains_visible_image(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("tool_result") {
                return false;
            }
            if matches!(
                map.get("type").and_then(Value::as_str),
                Some("image" | "image_url")
            ) {
                return true;
            }
            map.values().any(contains_visible_image)
        }
        Value::Array(items) => items.iter().any(contains_visible_image),
        _ => false,
    }
}

pub fn has_thinking_signal(body: &Value) -> bool {
    thinking_field_enabled(body.get("thinking"))
        || thinking_field_enabled(body.get("reasoning"))
        || thinking_field_enabled(body.get("reasoning_effort"))
}

fn thinking_field_enabled(value: Option<&Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value {
        Value::Null => false,
        Value::Bool(enabled) => *enabled,
        Value::Number(_) => true,
        Value::String(text) => !is_disabled_thinking_token(text),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => {
            if map.is_empty() {
                return false;
            }
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(is_disabled_thinking_token)
            {
                return false;
            }
            if map.get("enabled") == Some(&Value::Bool(false)) {
                return false;
            }
            if map
                .get("effort")
                .and_then(Value::as_str)
                .is_some_and(is_disabled_thinking_token)
            {
                return false;
            }
            true
        }
    }
}

fn is_disabled_thinking_token(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "disabled" | "none" | "off" | "false"
    )
}

pub fn looks_like_plan(tools: &[String], _target: Option<ProviderTarget>) -> bool {
    tools.iter().any(|name| {
        let lower = name.to_ascii_lowercase().replace('-', "_");
        matches!(
            lower.as_str(),
            "exitplanmode" | "enterplanmode" | "update_plan"
        )
    })
}

fn is_write_tool_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase().replace('-', "_");
    matches!(
        lower.as_str(),
        "edit"
            | "write"
            | "notebookedit"
            | "fileedit"
            | "filewrite"
            | "apply_patch"
            | "shell"
            | "bash"
    )
}

pub fn looks_like_edit(recent_write_tool: Option<&str>) -> bool {
    recent_write_tool.is_some_and(is_write_tool_name)
}

/// Last assistant round's write-tool name, not the advertised `tools` catalog.
pub fn extract_recent_write_tool(body: &Value) -> Option<String> {
    last_assistant_tool_use(body)
        .into_iter()
        .find(|name| is_write_tool_name(name))
        .or_else(|| {
            last_function_call_batch(body)
                .into_iter()
                .find(|name| is_write_tool_name(name))
        })
}

fn last_assistant_tool_use(body: &Value) -> Vec<String> {
    let Some(message) = body.get("messages").and_then(Value::as_array).and_then(|messages| {
        messages
            .iter()
            .rev()
            .find(|item| item.get("role").and_then(Value::as_str) == Some("assistant"))
    }) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for block in content {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            if let Some(name) = block.get("name").and_then(Value::as_str) {
                names.push(name.to_string());
            }
        }
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            if let Some(name) = call
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

fn last_function_call_batch(body: &Value) -> Vec<String> {
    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for item in items.iter().rev() {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "function_call_output" {
            continue;
        }
        if kind != "function_call" {
            break;
        }
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            names.push(name.to_string());
        }
    }
    names.reverse();
    names
}

pub fn select_mode<'a>(modes: &'a [RouteMode], signals: &ModeSignals) -> Option<&'a RouteMode> {
    let enabled = |id: ModeId| {
        modes
            .iter()
            .find(|mode| mode.id == id.as_str() && mode.enabled && !mode.model.trim().is_empty())
    };
    // Haiku / Explore / x-cs-subagent: only background (or default). Long-context
    // and thinking fields on those requests must not steal them.
    // After that: image → web → vision → plan → think → edit → long_context → default.
    if signals.is_subagent {
        if let Some(mode) = enabled(ModeId::Background) {
            return Some(mode);
        }
        return enabled(ModeId::Default).or_else(|| modes.iter().find(|mode| mode.id == "default"));
    }
    if signals.is_image_gen {
        if let Some(mode) = enabled(ModeId::ImageGen) {
            return Some(mode);
        }
    }
    if signals.has_web_search {
        if let Some(mode) = enabled(ModeId::WebSearch) {
            return Some(mode);
        }
    }
    if signals.has_vision {
        if let Some(mode) = enabled(ModeId::Vision) {
            return Some(mode);
        }
    }
    if looks_like_plan(&signals.tool_names, signals.target) {
        if let Some(mode) = enabled(ModeId::Plan) {
            return Some(mode);
        }
    }
    if signals.has_thinking {
        if let Some(mode) = enabled(ModeId::Think) {
            return Some(mode);
        }
    }
    if looks_like_edit(signals.recent_write_tool.as_deref())
        && !looks_like_plan(&signals.tool_names, signals.target)
    {
        if let Some(mode) = enabled(ModeId::Edit) {
            return Some(mode);
        }
    }
    if let Some(mode) = enabled(ModeId::LongContext) {
        if mode.threshold > 0 && signals.token_count as i64 >= mode.threshold {
            return Some(mode);
        }
    }
    enabled(ModeId::Default).or_else(|| modes.iter().find(|mode| mode.id == "default"))
}

pub fn load_modes(conn: &rusqlite::Connection, profile_id: &str) -> crate::error::AppResult<Vec<RouteMode>> {
    repair_long_context_one_token_threshold(conn)?;
    list_route_modes(conn, profile_id)
}

pub fn enabled_background_model(modes: &[RouteMode]) -> Option<String> {
    modes
        .iter()
        .find(|mode| mode.id == "background" && mode.enabled && !mode.model.trim().is_empty())
        .map(|mode| mode.model.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(id: &str, enabled: bool, model: &str, threshold: i64) -> RouteMode {
        RouteMode {
            id: id.into(),
            profile_id: "gprof_shared".into(),
            enabled,
            model: model.into(),
            thinking_config_json: "{}".into(),
            fallback_models: vec![],
            threshold,
            sort_index: 0,
        }
    }

    fn all_modes() -> Vec<RouteMode> {
        vec![
            mode("default", true, "deepseek.chat", 0),
            mode("background", true, "deepseek.bg", 0),
            mode("plan", true, "gpt.plan", 0),
            mode("think", true, "gpt.think", 0),
            mode("edit", true, "deepseek.edit", 0),
            mode("long_context", true, "gemini.long", 200_000),
            mode("web_search", true, "gemini.search", 0),
            mode("vision", true, "gpt.vision", 0),
            mode("image_gen", true, "gpt.image", 0),
        ]
    }

    #[test]
    fn image_gen_beats_web_search() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                is_image_gen: true,
                has_web_search: true,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "image_gen");
    }

    #[test]
    fn plan_tool_beats_edit() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                tool_names: vec!["ExitPlanMode".into(), "Edit".into()],
                recent_write_tool: Some("Edit".into()),
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "plan");
    }

    #[test]
    fn disabled_plan_falls_to_edit() {
        let mut modes = all_modes();
        modes.iter_mut().find(|mode| mode.id == "plan").unwrap().enabled = false;
        let hit = select_mode(
            &modes,
            &ModeSignals {
                tool_names: vec!["Write".into()],
                recent_write_tool: Some("Write".into()),
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "edit");
    }

    #[test]
    fn advertised_edit_tools_without_call_stay_on_default() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                tool_names: vec!["Edit".into(), "Write".into(), "Bash".into()],
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "default");
    }

    #[test]
    fn recent_write_tool_use_hits_edit() {
        let body = serde_json::json!({
            "tools": [{"name": "Edit"}, {"name": "Bash"}],
            "messages": [
                {"role": "user", "content": "change it"},
                {"role": "assistant", "content": [{"type": "tool_use", "name": "Write"}]}
            ]
        });
        assert_eq!(extract_recent_write_tool(&body).as_deref(), Some("Write"));
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                tool_names: extract_tool_names(&body),
                recent_write_tool: extract_recent_write_tool(&body),
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "edit");
    }

    #[test]
    fn recent_codex_apply_patch_hits_edit() {
        let body = serde_json::json!({
            "input": [
                {"type": "message", "role": "user", "content": "patch it"},
                {"type": "function_call", "name": "apply_patch"},
                {"type": "function_call_output", "call_id": "1"}
            ]
        });
        assert_eq!(extract_recent_write_tool(&body).as_deref(), Some("apply_patch"));
    }

    #[test]
    fn long_context_uses_threshold() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "long_context");
    }

    #[test]
    fn long_context_zero_threshold_does_not_match() {
        let mut modes = all_modes();
        modes
            .iter_mut()
            .find(|mode| mode.id == "long_context")
            .unwrap()
            .threshold = 0;
        let hit = select_mode(
            &modes,
            &ModeSignals {
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "default");
    }

    #[test]
    fn long_context_below_threshold_falls_to_default() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                token_count: 1,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "default");
    }

    #[test]
    fn subagent_selects_background() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                is_subagent: true,
                token_count: 1,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "background");
    }

    #[test]
    fn subagent_beats_long_context_think_and_web_search() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                is_subagent: true,
                token_count: 250_000,
                has_thinking: true,
                has_web_search: true,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "background");
        assert_eq!(hit.model, "deepseek.bg");
    }

    #[test]
    fn subagent_without_background_follows_default_not_think() {
        let mut modes = all_modes();
        modes
            .iter_mut()
            .find(|mode| mode.id == "background")
            .unwrap()
            .enabled = false;
        let hit = select_mode(
            &modes,
            &ModeSignals {
                is_subagent: true,
                has_thinking: true,
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "default");
        assert_eq!(hit.model, "deepseek.chat");
    }

    #[test]
    fn main_session_think_still_beats_default_below_long_context() {
        let mut modes = all_modes();
        modes
            .iter_mut()
            .find(|mode| mode.id == "long_context")
            .unwrap()
            .threshold = 20_000;
        let hit = select_mode(
            &modes,
            &ModeSignals {
                has_thinking: true,
                token_count: 5_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "think");
        assert_eq!(hit.model, "gpt.think");
    }

    #[test]
    fn think_beats_long_context_when_both_match() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                has_thinking: true,
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "think");
        assert_eq!(hit.model, "gpt.think");
    }

    #[test]
    fn plan_beats_long_context_when_both_match() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                tool_names: vec!["ExitPlanMode".into()],
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "plan");
        assert_eq!(hit.model, "gpt.plan");
    }

    #[test]
    fn edit_beats_long_context_when_both_match() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                recent_write_tool: Some("Write".into()),
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "edit");
        assert_eq!(hit.model, "deepseek.edit");
    }

    #[test]
    fn long_context_still_wins_without_plan_think_or_edit() {
        let modes = all_modes();
        let hit = select_mode(
            &modes,
            &ModeSignals {
                token_count: 250_000,
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "long_context");
        assert_eq!(hit.model, "gemini.long");
    }

    #[test]
    fn has_thinking_signal_ignores_disabled_and_empty() {
        assert!(!has_thinking_signal(&serde_json::json!({})));
        assert!(!has_thinking_signal(&serde_json::json!({ "thinking": {} })));
        assert!(!has_thinking_signal(
            &serde_json::json!({ "thinking": { "type": "disabled" } })
        ));
        assert!(!has_thinking_signal(&serde_json::json!({ "thinking": "disabled" })));
        assert!(!has_thinking_signal(
            &serde_json::json!({ "reasoning": { "effort": "none" } })
        ));
        assert!(!has_thinking_signal(&serde_json::json!({ "reasoning_effort": "off" })));
        assert!(has_thinking_signal(
            &serde_json::json!({ "thinking": { "type": "enabled", "budget_tokens": 8000 } })
        ));
        assert!(has_thinking_signal(
            &serde_json::json!({ "reasoning": { "effort": "low" } })
        ));
        assert!(has_thinking_signal(&serde_json::json!({ "reasoning_effort": "high" })));
    }

    #[test]
    fn vision_only_looks_at_latest_user_turn() {
        let historical = serde_json::json!({
            "messages": [
                {"role": "user", "content": [{"type": "image"}]},
                {"role": "assistant", "content": "saw it"},
                {"role": "user", "content": [{"type": "text", "text": "thanks"}]}
            ]
        });
        assert!(!has_vision_content(&historical));

        let current = serde_json::json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "see"}, {"type": "image"}]}
            ]
        });
        assert!(has_vision_content(&current));

        let tool_result = serde_json::json!({
            "messages": [
                {"role": "user", "content": [{"type": "tool_result", "content": [{"type": "image"}]}]}
            ]
        });
        assert!(!has_vision_content(&tool_result));

        let codex_history = serde_json::json!({
            "input": [
                {"role": "user", "content": [{"type": "image_url"}]},
                {"role": "user", "content": "next"}
            ]
        });
        assert!(!has_vision_content(&codex_history));
    }
}
