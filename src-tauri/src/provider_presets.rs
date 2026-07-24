//! Built-in provider presets.
//!
//! Each preset ships with `api_key` empty — the user fills in their token before
//! switching. Base URLs and model names follow the vendors' Anthropic-compatible
//! endpoints (sourced from cc-switch's preset definitions).

use crate::provider::{ProtocolType, Provider};

/// A template a user can add to their list. Distinct from [`Provider`] in that it
/// has no `id`/`is_current`/`created_at` yet.
pub struct Preset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
    pub notes: &'static str,
}

/// The bundled preset catalog. Order is the display order.
pub fn presets() -> &'static [Preset] {
    &[
        Preset {
            name: "Claude 官方登录",
            base_url: "",
            model: "",
            notes: "使用 Claude Code 原生 OAuth 登录（清空第三方配置）",
        },
        Preset {
            name: "Kimi",
            base_url: "https://api.moonshot.cn/anthropic",
            model: "kimi-k2.7-code",
            notes: "Moonshot AI · 国内官方",
        },
        Preset {
            name: "Kimi For Coding",
            base_url: "https://api.kimi.com/coding/",
            model: "kimi-for-coding",
            notes: "Kimi 编程版 · 大上下文",
        },
        Preset {
            name: "DeepSeek",
            base_url: "https://api.deepseek.com/anthropic",
            model: "deepseek-v4-pro",
            notes: "深度求索 · 国内官方",
        },
        Preset {
            name: "智谱 GLM",
            base_url: "https://open.bigmodel.cn/api/anthropic",
            model: "glm-4.6",
            notes: "智谱 AI · GLM 系列",
        },
        Preset {
            name: "小米 MiMo",
            base_url: "https://api.mimo.xiaomi.com/anthropic",
            model: "mimo-v2.5-pro",
            notes: "小米 AI · MiMo Coding",
        },
    ]
}

/// Convert a preset into a full [`Provider`] ready to insert, assigned the given
/// id/sort_index/created_at.
pub fn preset_to_provider(p: &Preset, id: String, sort_index: i64, created_at: i64) -> Provider {
    Provider {
        id,
        name: p.name.to_string(),
        base_url: p.base_url.to_string(),
        api_key: String::new(),
        model: p.model.to_string(),
        protocol_type: ProtocolType::Anthropic,
        notes: p.notes.to_string(),
        sort_index,
        is_current: false,
        created_at,
    }
}
