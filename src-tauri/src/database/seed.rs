//! First-run seeding: insert presets and import the live Claude Code config.
//!
//! Called after [`Database::init`] succeeds. Idempotent: presets are only inserted
//! when the providers table is empty.

use chrono::Utc;

use crate::config::claude_code::read_current_live_provider;
use crate::database::dao;
use crate::error::AppResult;
use crate::provider_presets::{preset_to_provider, presets};
use crate::provider::Provider;

/// Seed presets (if empty) and mark the live-config-matching provider as current.
pub fn run_seed(conn: &rusqlite::Connection) -> AppResult<()> {
    let count = dao::count_providers(conn)?;
    let now = Utc::now().timestamp_millis();

    if count == 0 {
        for (idx, preset) in presets().iter().enumerate() {
            // Stable preset ids so re-seeding is a no-op.
            let id = format!("preset_{idx}");
            let provider = preset_to_provider(preset, id, idx as i64, now);
            dao::insert_provider_direct(conn, &provider)?;
        }
        log::info!("已植入 {} 个供应商预设", presets().len());
    }

    // Reflect the live settings.json: if it matches a known provider, mark it
    // current; otherwise add an "Imported (current)" row.
    match read_current_live_provider()? {
        Some(live) => {
            let existing = dao::list_providers(conn)?;
            let match_id = existing
                .iter()
                .find(|p| !live.base_url.is_empty() && p.base_url == live.base_url)
                .map(|p| p.id.clone());

            if let Some(id) = match_id {
                // Also persist the live token/model onto the matched preset.
                dao::insert_provider_direct(conn, &enrich_with_live(&live, existing.iter().find(|p| p.id == id).unwrap()))?;
                dao::set_current_provider(conn, &id)?;
                log::info!("已将现有配置匹配到供应商 {id} 并标记为当前");
            } else {
                let id = format!("preset_live_{}", now);
                let p = Provider {
                    id: id.clone(),
                    name: "当前配置（已导入）".to_string(),
                    base_url: live.base_url,
                    api_key: live.auth_token,
                    model: live.model,
                    protocol_type: crate::provider::ProtocolType::Anthropic,
                    notes: "首次启动从 ~/.claude/settings.json 导入".to_string(),
                    sort_index: presets().len() as i64,
                    is_current: true,
                    created_at: now,
                };
                dao::insert_provider_direct(conn, &p)?;
                log::info!("已导入现有配置为供应商 {id}");
            }
        }
        None => {
            // No live third-party config; leave current unset (official login).
            log::info!("未检测到第三方配置，保持官方登录状态");
        }
    }

    Ok(())
}

fn enrich_with_live(live: &crate::provider::LiveProviderInfo, base: &Provider) -> Provider {
    let mut p = base.clone();
    if !live.auth_token.is_empty() {
        p.api_key = live.auth_token.clone();
    }
    if !live.model.is_empty() {
        p.model = live.model.clone();
    }
    p
}
