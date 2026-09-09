//! Structured route-mode matching (plan / think / edit / long_context / ...).

use serde_json::Value;

use crate::database::dao::gateway::{list_route_modes, RouteMode, SHARED_PROFILE_ID};
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

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "background" => Some(Self::Background),
            "plan" => Some(Self::Plan),
            "think" => Some(Self::Think),
            "edit" => Some(Self::Edit),
            "long_context" => Some(Self::LongContext),
            "web_search" => Some(Self::WebSearch),
            "vision" => Some(Self::Vision),
            "image_gen" => Some(Self::ImageGen),
            _ => None,
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
    contains_image(body)
}

fn contains_image(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("image") {
                return true;
            }
            if map.get("type").and_then(Value::as_str) == Some("image_url") {
                return true;
            }
            map.values().any(contains_image)
        }
        Value::Array(items) => items.iter().any(contains_image),
        _ => false,
    }
}

pub fn has_thinking_signal(body: &Value) -> bool {
    if body.get("thinking").is_some() || body.get("reasoning").is_some() {
        return true;
    }
    if body.get("reasoning_effort").is_some() {
        return true;
    }
    if body
        .get("reasoning")
        .and_then(Value::as_object)
        .and_then(|object| object.get("effort"))
        .is_some()
    {
        return true;
    }
    false
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

pub fn looks_like_edit(tools: &[String]) -> bool {
    tools.iter().any(|name| {
        let lower = name.to_ascii_lowercase();
        matches!(
            lower.as_str(),
            "edit"
                | "write"
                | "notebookedit"
                | "fileedit"
                | "filewrite"
                | "apply_patch"
                | "apply-patch"
                | "shell"
                | "bash"
        )
    })
}

pub fn select_mode<'a>(modes: &'a [RouteMode], signals: &ModeSignals) -> Option<&'a RouteMode> {
    let enabled = |id: ModeId| {
        modes
            .iter()
            .find(|mode| mode.id == id.as_str() && mode.enabled && !mode.model.trim().is_empty())
    };
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
    if let Some(mode) = enabled(ModeId::LongContext) {
        if signals.token_count as i64 >= mode.threshold.max(1) {
            return Some(mode);
        }
    }
    if signals.is_subagent {
        if let Some(mode) = enabled(ModeId::Background) {
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
    if looks_like_edit(&signals.tool_names) && !looks_like_plan(&signals.tool_names, signals.target) {
        if let Some(mode) = enabled(ModeId::Edit) {
            return Some(mode);
        }
    }
    enabled(ModeId::Default).or_else(|| modes.iter().find(|mode| mode.id == "default"))
}

pub fn load_modes(conn: &rusqlite::Connection) -> crate::error::AppResult<Vec<RouteMode>> {
    list_route_modes(conn, SHARED_PROFILE_ID)
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
                ..ModeSignals::default()
            },
        )
        .unwrap();
        assert_eq!(hit.id, "edit");
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
}
