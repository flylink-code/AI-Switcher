//! Per-upstream model defaults; advertised capability is independent of opt-in.
use std::collections::BTreeSet;

use rusqlite::Connection;
use serde_json::Value;

use crate::database::dao::{gateway, providers, settings};
use crate::error::{AppError, AppResult};
use crate::provider::ProtocolType;

fn key(upstream_id: &str) -> String {
    format!("gateway.fast_models.{upstream_id}")
}

fn model_key(model: &str) -> String {
    crate::provider::split_model_window_label(model).0.to_ascii_lowercase()
}

pub fn enabled_models(conn: &Connection, upstream_id: &str) -> AppResult<BTreeSet<String>> {
    settings::get_setting(conn, &key(upstream_id))?
        .map(|raw| serde_json::from_str(&raw).map_err(Into::into))
        .unwrap_or_else(|| Ok(BTreeSet::new()))
}

pub fn supports_protocol(protocol: ProtocolType) -> bool {
    matches!(protocol, ProtocolType::OpenAiChat | ProtocolType::OpenAiResponses | ProtocolType::Proxy)
}

pub fn set_models(
    conn: &Connection,
    upstream_id: &str,
    model_ids: &[String],
    enabled: bool,
    confirm_unknown: bool,
) -> AppResult<Vec<gateway::GatewayUpstreamModelRow>> {
    let upstream = gateway::get_upstream_provider(conn, upstream_id)?
        .ok_or_else(|| AppError::Config(format!("上游不存在: {upstream_id}")))?;
    let rows = gateway::list_upstream_models(conn, upstream_id)?;
    let mut selected = Vec::new();
    for id in model_ids {
        let row = rows.iter().find(|row| row.model_id.eq_ignore_ascii_case(id.trim()))
            .ok_or_else(|| AppError::Config(format!("模型不存在: {id}")))?;
        if enabled && (!supports_protocol(upstream.protocol_type) || row.fast_supported == Some(false)) {
            return Err(AppError::Config(format!("模型未提供 OpenAI Priority 模式: {id}")));
        }
        if enabled && row.fast_supported.is_none() && !confirm_unknown {
            return Err(AppError::Config(format!("请先确认供应商支持此模型的 Priority 模式及费用: {id}")));
        }
        selected.push(model_key(&row.model_id));
    }
    let mut models = enabled_models(conn, upstream_id)?;
    for id in selected {
        if enabled { models.insert(id); } else { models.remove(&id); }
    }
    settings::set_setting(conn, &key(upstream_id), &serde_json::to_string(&models)?)?;
    gateway::list_upstream_models(conn, upstream_id)
}

pub fn is_enabled(conn: &Connection, upstream_id: &str, model: &str) -> AppResult<bool> {
    if !enabled_models(conn, upstream_id)?.contains(&model_key(model)) {
        return Ok(false);
    }
    let cache = providers::get_provider_model_cache(conn, upstream_id)?;
    let support = cache.as_ref().and_then(|cache| cache.metadata.iter()
        .find(|(id, _)| model_key(id) == model_key(model))
        .and_then(|(_, meta)| meta.priority_supported));
    Ok(support != Some(false))
}

pub fn remove(conn: &Connection, upstream_id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?;", [key(upstream_id)])?;
    Ok(())
}

/// Apply after protocol conversion, on each attempt, without mutating the client request.
pub fn apply_request(outgoing: &mut Value, incoming: &Value, enabled: bool) {
    if let Some(tier) = incoming.get("service_tier").filter(|tier| !tier.is_null()) {
        // Anthropic's standard_only is not an OpenAI service tier, but still opts out.
        if tier != "standard_only" {
            outgoing["service_tier"] = if tier == "fast" { Value::String("priority".into()) } else { tier.clone() };
        }
    } else if incoming.get("speed").and_then(Value::as_str) == Some("fast")
        || (enabled && incoming.get("speed").is_none())
    {
        outgoing["service_tier"] = Value::String("priority".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fast_mode_preserves_agent_tiers_after_conversion() {
        for tier in ["auto", "default", "flex", "priority", "fast", "ultrafast"] {
            for enabled in [false, true] {
                let mut outgoing = json!({"model": "test"});
                apply_request(&mut outgoing, &json!({"service_tier": tier}), enabled);
                assert_eq!(outgoing["service_tier"], if tier == "fast" { "priority" } else { tier });
            }
        }
    }

    #[test]
    fn fast_mode_defaults_and_anthropic_speed() {
        for (incoming, enabled, expected) in [
            (json!({}), false, None),
            (json!({}), true, Some("priority")),
            (json!({"service_tier": null}), true, Some("priority")),
            (json!({"speed": "fast"}), false, Some("priority")),
            (json!({"speed": "standard"}), true, None),
            (json!({"service_tier": "standard_only"}), true, None),
        ] {
            let mut outgoing = json!({"model": "test"});
            apply_request(&mut outgoing, &incoming, enabled);
            assert_eq!(outgoing.get("service_tier").and_then(Value::as_str), expected);
        }
    }

    #[test]
    fn fast_mode_defaults_do_not_leak_to_next_attempt() {
        let incoming = json!({"model": "test"});
        let mut first = incoming.clone();
        apply_request(&mut first, &incoming, true);
        let mut fallback = incoming.clone();
        apply_request(&mut fallback, &incoming, false);
        assert_eq!(first["service_tier"], "priority");
        assert!(fallback.get("service_tier").is_none());
        assert!(incoming.get("service_tier").is_none());
    }
}
