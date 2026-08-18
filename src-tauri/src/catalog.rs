//! Gateway catalog: public model IDs for Claude Code / Codex local-proxy routing.
//!
//! OpenCode / Pi / Dsh write every provider into the agent's own config. Claude
//! Code and Codex only speak one upstream, so the local proxy exposes a merged
//! model list and routes each request by `model`.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::database::dao::settings::get_setting;
use crate::database::Database;
use crate::provider::{
    catalog_models_from_provider, effective_model_context_window, resolve_upstream_model,
    ProtocolType, Provider, ProviderTarget, CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID,
    CLAUDE_OPUS_ROLE_ID, CLAUDE_SONNET_ROLE_ID,
};

pub const GATEWAY_CATALOG_CODE_KEY: &str = "gateway_catalog_claude_code";
pub const GATEWAY_CATALOG_CODEX_KEY: &str = "gateway_catalog_codex";
pub const GATEWAY_CATALOG_CODE_SUBAGENT_KEY: &str = "gateway_catalog_claude_code_subagent";
pub const GATEWAY_CATALOG_CODEX_SUBAGENT_KEY: &str = "gateway_catalog_codex_subagent";
pub const GATEWAY_CATALOG_HIDE_OFFICIAL_CODE_KEY: &str = "gateway_catalog_hide_official_claude_code";
pub const GATEWAY_CATALOG_HIDE_OFFICIAL_CODEX_KEY: &str = "gateway_catalog_hide_official_codex";

pub fn setting_key(target: ProviderTarget) -> Option<&'static str> {
    match target {
        ProviderTarget::ClaudeCode => Some(GATEWAY_CATALOG_CODE_KEY),
        ProviderTarget::Codex => Some(GATEWAY_CATALOG_CODEX_KEY),
        _ => None,
    }
}

pub fn subagent_setting_key(target: ProviderTarget) -> Option<&'static str> {
    match target {
        ProviderTarget::ClaudeCode => Some(GATEWAY_CATALOG_CODE_SUBAGENT_KEY),
        ProviderTarget::Codex => Some(GATEWAY_CATALOG_CODEX_SUBAGENT_KEY),
        _ => None,
    }
}

pub fn hide_official_setting_key(target: ProviderTarget) -> Option<&'static str> {
    match target {
        ProviderTarget::ClaudeCode => Some(GATEWAY_CATALOG_HIDE_OFFICIAL_CODE_KEY),
        ProviderTarget::Codex => Some(GATEWAY_CATALOG_HIDE_OFFICIAL_CODEX_KEY),
        _ => None,
    }
}

pub fn enabled_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> bool {
    let Some(key) = setting_key(target) else {
        return false;
    };
    get_setting(conn, key).ok().flatten().as_deref() == Some("true")
}

pub fn enabled(db: &Database, target: ProviderTarget) -> bool {
    db.with_conn(|conn| Ok(enabled_for_conn(conn, target)))
        .unwrap_or(false)
}

pub fn hide_official_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> bool {
    let Some(key) = hide_official_setting_key(target) else {
        return false;
    };
    get_setting(conn, key).ok().flatten().as_deref() == Some("true")
}

pub fn hide_official(db: &Database, target: ProviderTarget) -> bool {
    db.with_conn(|conn| Ok(hide_official_for_conn(conn, target)))
        .unwrap_or(false)
}

pub fn subagent_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> Option<String> {
    let key = subagent_setting_key(target)?;
    get_setting(conn, key)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn subagent_model(db: &Database, target: ProviderTarget) -> Option<String> {
    db.with_conn(|conn| Ok(subagent_for_conn(conn, target)))
        .ok()
        .flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStyle {
    /// Claude Code `/v1/models` discovery. IDs must contain `claude` or `anthropic`.
    Claude,
    /// Codex catalog / OpenAI `/v1/models`. Any slug is fine; collisions get a prefix.
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub public_id: String,
    pub display_name: String,
    pub upstream_slug: String,
    pub provider_id: String,
    pub context_window: u64,
    pub anthropic_upstream: bool,
    pub web_search_enabled: bool,
}

pub fn provider_slug(name: &str, id: &str) -> String {
    let slug: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        let short = id.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>();
        if short.is_empty() {
            "provider".to_string()
        } else {
            short.to_ascii_lowercase()
        }
    } else {
        slug
    }
}

pub fn passes_claude_discovery(id: &str) -> bool {
    let normalized = id.to_ascii_lowercase();
    normalized.contains("claude") || normalized.contains("anthropic")
}

pub fn collect_provider_slugs(provider: &Provider, cached: &[String]) -> Vec<String> {
    collect_provider_slugs_with(provider, cached, false)
}

pub fn collect_provider_slugs_with(
    provider: &Provider,
    cached: &[String],
    hide_official: bool,
) -> Vec<String> {
    let mut ids = catalog_models_from_provider(provider);
    extend_unique(&mut ids, cached.iter().cloned());
    if uses_antigravity_catalog(provider) {
        extend_unique(&mut ids, crate::antigravity::list_model_ids());
        extend_unique(
            &mut ids,
            crate::antigravity::model_catalog::provider_suggestion_ids(24),
        );
        crate::antigravity::model_catalog::extend_flash_level_variants(&mut ids);
        ids.retain(|id| {
            let trimmed = id.trim();
            crate::antigravity::model_catalog::is_agent_facing_model(trimmed)
                && !crate::antigravity::model_catalog::is_retired_model(trimmed)
                && !crate::antigravity::model_catalog::should_remap_legacy_gemini(trimmed)
        });
    }
    if hide_official {
        ids.retain(|id| {
            !is_injected_official_model_slug(id) || is_explicit_saved_model(provider, id)
        });
    }
    provider.filter_hidden_models(ids)
}

pub fn build_catalog(style: CatalogStyle, providers: &[(Provider, Vec<String>)]) -> Vec<CatalogEntry> {
    build_catalog_with(style, providers, false)
}

pub fn build_catalog_with(
    style: CatalogStyle,
    providers: &[(Provider, Vec<String>)],
    hide_official: bool,
) -> Vec<CatalogEntry> {
    let mut taken = BTreeSet::new();
    let mut entries = Vec::new();
    for (provider, cached) in providers {
        let slug = provider_slug(&provider.name, &provider.id);
        let anthropic_upstream = provider.protocol_type == ProtocolType::Anthropic;
        let web_search_enabled = !anthropic_upstream && provider.web_search_enabled.unwrap_or(true);
        let context_window = effective_model_context_window(provider);
        for upstream in collect_provider_slugs_with(provider, cached, hide_official) {
            let public_id = unique_public_id(style, &upstream, &slug, &mut taken);
            entries.push(CatalogEntry {
                public_id,
                display_name: format!("{} · {}", provider.name.trim(), upstream),
                upstream_slug: upstream,
                provider_id: provider.id.clone(),
                context_window,
                anthropic_upstream,
                web_search_enabled,
            });
        }
    }
    entries
}

/// Map a client-facing model id to `(provider_id, upstream_slug)`.
pub fn resolve_request(
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested: &str,
) -> Option<(String, String)> {
    let requested = requested.trim();
    if requested.is_empty() {
        let first = providers.first()?;
        return Some((first.id.clone(), first.model.trim().to_string()));
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.public_id.eq_ignore_ascii_case(requested))
    {
        return Some((entry.provider_id.clone(), entry.upstream_slug.clone()));
    }
    let upstream_hits: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| entry.upstream_slug.eq_ignore_ascii_case(requested))
        .collect();
    if let Some(entry) = upstream_hits.first() {
        return Some((entry.provider_id.clone(), entry.upstream_slug.clone()));
    }
    if is_claude_role_request(requested) {
        let first = providers.first()?;
        let default = first.model.trim();
        let upstream = if default.is_empty() {
            resolve_upstream_model(first, requested)
        } else {
            default.to_string()
        };
        return Some((first.id.clone(), upstream));
    }
    if let Some(stripped) = strip_claude_alias(requested) {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.upstream_slug.eq_ignore_ascii_case(stripped))
        {
            return Some((entry.provider_id.clone(), entry.upstream_slug.clone()));
        }
    }
    let first = providers.first()?;
    Some((first.id.clone(), resolve_upstream_model(first, requested)))
}

pub fn claude_discovery_payload(entries: &[CatalogEntry]) -> Value {
    json!({
        "data": entries.iter().map(|entry| json!({
            "id": entry.public_id,
            "display_name": entry.display_name,
        })).collect::<Vec<_>>(),
        "has_more": false,
    })
}

pub fn openai_models_payload(entries: &[CatalogEntry]) -> Value {
    json!({
        "object": "list",
        "data": entries.iter().map(|entry| json!({
            "id": entry.public_id,
            "object": "model",
            "owned_by": "ai-switcher",
        })).collect::<Vec<_>>(),
    })
}

pub fn rewrite_json_model(body: &[u8], upstream: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    if let Some(object) = value.as_object_mut() {
        object.insert("model".to_string(), Value::String(upstream.to_string()));
    }
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

/// Built-in ChatGPT/Codex picker slugs that are not user-saved catalog models.
pub fn is_injected_official_model_slug(id: &str) -> bool {
    is_official_openai_builtin(catalog_model_stem(id))
}

pub fn catalog_in_entries(entries: &[CatalogEntry], requested: &str) -> bool {
    let requested = requested.trim();
    if requested.is_empty() {
        return false;
    }
    entries.iter().any(|entry| {
        entry.public_id.eq_ignore_ascii_case(requested)
            || entry.upstream_slug.eq_ignore_ascii_case(requested)
    })
}

pub fn catalog_fallback_id(
    entries: &[CatalogEntry],
    providers: &[Provider],
    subagent: Option<&str>,
) -> String {
    if let Some(sub) = subagent.map(str::trim).filter(|value| !value.is_empty()) {
        return sub.to_string();
    }
    let current = providers
        .iter()
        .find(|provider| provider.is_current)
        .or_else(|| providers.first());
    let Some(current) = current else {
        return String::new();
    };
    let default = current.model.trim();
    if let Some(entry) = entries.iter().find(|entry| {
        entry.provider_id == current.id && entry.upstream_slug.eq_ignore_ascii_case(default)
    }) {
        return entry.public_id.clone();
    }
    default.to_string()
}

/// Codex/Claude Code subagents must not inherit the current default (`*-high`).
/// Empty catalog-subagent setting → lightest flash-low already in the catalog.
fn catalog_subagent_target(
    entries: &[CatalogEntry],
    providers: &[Provider],
    subagent: Option<&str>,
) -> String {
    if let Some(sub) = subagent.map(str::trim).filter(|value| !value.is_empty()) {
        return sub.to_string();
    }
    if let Some(id) = light_flash_catalog_id(entries) {
        return id;
    }
    catalog_fallback_id(entries, providers, None)
}

fn light_flash_catalog_id(entries: &[CatalogEntry]) -> Option<String> {
    let rank = |slug: &str| -> u8 {
        let lower = slug.to_ascii_lowercase();
        if !(lower.contains("flash") && lower.ends_with("-low")) {
            return 99;
        }
        if lower.contains("gemini-3.6") {
            0
        } else if lower.contains("gemini-3.7") {
            1
        } else {
            2
        }
    };
    entries
        .iter()
        .filter(|entry| rank(&entry.upstream_slug) < 99)
        .min_by_key(|entry| rank(&entry.upstream_slug))
        .map(|entry| entry.public_id.clone())
}

/// Rewrite client-facing ids that should not hit official ChatGPT/Claude built-ins.
pub fn normalize_client_request(
    style: CatalogStyle,
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested: &str,
    hide_official: bool,
    subagent: Option<&str>,
    force_subagent: bool,
) -> String {
    let fallback = catalog_fallback_id(entries, providers, subagent);
    if force_subagent {
        return catalog_subagent_target(entries, providers, subagent);
    }
    let requested = requested.trim();
    if requested.is_empty() {
        return fallback;
    }
    let in_catalog = catalog_in_entries(entries, requested);
    let lower = requested.to_ascii_lowercase();
    let haiku_or_subagent = lower.contains("haiku") || lower.contains("subagent");
    // Claude Code Explore / compact / Task agents send the stable Haiku role
    // id. That must hit the catalog subagent slot even when "hide official"
    // is off — otherwise resolve_request treats it as a role and inherits the
    // current /model default (same SKU as the main session).
    if style == CatalogStyle::Claude && haiku_or_subagent && !in_catalog {
        return catalog_subagent_target(entries, providers, subagent);
    }
    if hide_official && is_injected_official_model_slug(requested) && !in_catalog {
        return fallback;
    }
    if hide_official
        && style == CatalogStyle::Claude
        && is_claude_role_request(requested)
        && !in_catalog
    {
        return fallback;
    }
    requested.to_string()
}

/// On catalog failover, send the takeover provider's own default (or a subagent
/// that actually belongs to that provider), never the original Gemini id.
pub fn failover_upstream_for_provider(
    fallback: &Provider,
    entries: &[CatalogEntry],
    subagent: Option<&str>,
) -> String {
    if let Some(sub) = subagent.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(entry) = entries.iter().find(|entry| {
            entry.provider_id == fallback.id
                && (entry.public_id.eq_ignore_ascii_case(sub)
                    || entry.upstream_slug.eq_ignore_ascii_case(sub))
        }) {
            return entry.upstream_slug.clone();
        }
    }
    let default = fallback.model.trim();
    if !default.is_empty() {
        return default.to_string();
    }
    entries
        .iter()
        .find(|entry| entry.provider_id == fallback.id)
        .map(|entry| entry.upstream_slug.clone())
        .unwrap_or_default()
}

fn is_explicit_saved_model(provider: &Provider, id: &str) -> bool {
    let needle = id.trim();
    if needle.is_empty() {
        return false;
    }
    if provider.model.trim().eq_ignore_ascii_case(needle) {
        return true;
    }
    provider
        .failover_models
        .iter()
        .any(|model| model.trim().eq_ignore_ascii_case(needle))
}

fn catalog_model_stem(id: &str) -> &str {
    let trimmed = id.trim();
    if let Some(rest) = trimmed.strip_prefix("claude.") {
        if let Some((_, model)) = rest.split_once('.') {
            return model;
        }
    }
    trimmed
}

fn is_official_openai_builtin(id: &str) -> bool {
    let m = id.trim().to_ascii_lowercase();
    m == "gpt-5.6-luna"
        || m.starts_with("gpt-5.6-luna-")
        || m == "gpt-5.6-sol"
        || m.starts_with("gpt-5.6-sol-")
        || m == "gpt-5.6-terra"
        || m.starts_with("gpt-5.6-terra-")
        || m == "gpt-5.5"
        || m.starts_with("gpt-5.5-")
        || m == "gpt-5.4"
        || m.starts_with("gpt-5.4-")
        || m == "gpt-5.3-codex"
        || m.starts_with("gpt-5.3-codex-")
}

fn unique_public_id(
    style: CatalogStyle,
    upstream: &str,
    provider_slug: &str,
    taken: &mut BTreeSet<String>,
) -> String {
    let preferred = match style {
        CatalogStyle::Claude if passes_claude_discovery(upstream) => upstream.to_string(),
        CatalogStyle::Claude => format!("claude.{provider_slug}.{upstream}"),
        CatalogStyle::Codex => upstream.to_string(),
    };
    if taken.insert(preferred.clone()) {
        return preferred;
    }
    let prefixed = match style {
        CatalogStyle::Claude => format!("claude.{provider_slug}.{upstream}"),
        CatalogStyle::Codex => format!("{provider_slug}.{upstream}"),
    };
    if taken.insert(prefixed.clone()) {
        return prefixed;
    }
    let mut index = 2u32;
    loop {
        let candidate = format!("{prefixed}-{index}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn is_claude_role_request(requested: &str) -> bool {
    let normalized = requested.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        CLAUDE_SONNET_ROLE_ID
            | CLAUDE_OPUS_ROLE_ID
            | CLAUDE_HAIKU_ROLE_ID
            | CLAUDE_FABLE_ROLE_ID
            | "sonnet"
            | "opus"
            | "haiku"
            | "fable"
    ) || normalized.contains("sonnet")
        || normalized.contains("opus")
        || normalized.contains("haiku")
        || normalized.contains("fable")
        || normalized.contains("subagent")
}

fn strip_claude_alias(requested: &str) -> Option<&str> {
    let rest = requested.strip_prefix("claude.")?;
    rest.split_once('.').map(|(_, model)| model)
}

fn uses_antigravity_catalog(provider: &Provider) -> bool {
    if provider.is_antigravity() {
        return true;
    }
    let lower = provider.base_url.trim().to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower.as_str());
    let host_port = without_scheme.split('/').next().unwrap_or("");
    matches!(
        host_port,
        "127.0.0.1:15830"
            | "localhost:15830"
            | "[::1]:15830"
            | "127.0.0.1:8045"
            | "localhost:8045"
            | "[::1]:8045"
    )
}

fn extend_unique(ids: &mut Vec<String>, extra: impl IntoIterator<Item = String>) {
    for model in extra {
        let model = model.trim();
        if !model.is_empty() && !ids.iter().any(|id| id == model) {
            ids.push(model.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProviderKind, ProviderTarget};

    fn provider(id: &str, name: &str, model: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: "https://api.example.test/v1".into(),
            api_key: String::new(),
            api_key_set: false,
            model: model.into(),
            model_context_window: Some(200_000),
            auto_review_model_override: None,
            web_search_enabled: Some(true),
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    #[test]
    fn claude_non_claude_slugs_get_discovery_prefix() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![])]);
        assert_eq!(catalog[0].public_id, "claude.kimi.kimi-k2");
        assert!(passes_claude_discovery(&catalog[0].public_id));
        assert_eq!(catalog[0].upstream_slug, "kimi-k2");
        assert_eq!(catalog[0].display_name, "Kimi · kimi-k2");
    }

    #[test]
    fn claude_native_slugs_keep_original_id_until_collision() {
        let first = provider("a", "Alpha", "claude-sonnet-4");
        let second = provider("b", "Beta", "claude-sonnet-4");
        let catalog = build_catalog(CatalogStyle::Claude, &[(first, vec![]), (second, vec![])]);
        assert_eq!(catalog[0].public_id, "claude-sonnet-4");
        assert_eq!(catalog[1].public_id, "claude.beta.claude-sonnet-4");
    }

    #[test]
    fn codex_colliding_slugs_get_provider_prefix() {
        let first = provider("a", "Alpha", "deepseek-v3");
        let second = provider("b", "Beta", "deepseek-v3");
        let catalog = build_catalog(CatalogStyle::Codex, &[(first, vec![]), (second, vec![])]);
        assert_eq!(catalog[0].public_id, "deepseek-v3");
        assert_eq!(catalog[1].public_id, "beta.deepseek-v3");
    }

    #[test]
    fn resolve_prefers_public_id_then_upstream() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let ds = provider("p2", "DeepSeek", "deepseek-v3");
        let providers = vec![kimi.clone(), ds.clone()];
        let catalog = build_catalog(
            CatalogStyle::Claude,
            &[(kimi, vec![]), (ds, vec![])],
        );
        assert_eq!(
            resolve_request(&catalog, &providers, "claude.kimi.kimi-k2"),
            Some(("p1".into(), "kimi-k2".into()))
        );
        assert_eq!(
            resolve_request(&catalog, &providers, "deepseek-v3"),
            Some(("p2".into(), "deepseek-v3".into()))
        );
        assert_eq!(
            resolve_request(&catalog, &providers, CLAUDE_SONNET_ROLE_ID),
            Some(("p1".into(), "kimi-k2".into()))
        );
    }

    #[test]
    fn claude_catalog_antigravity_gemini_public_id_rewrites_to_upstream() {
        let mut ag = provider("ag", "Antigravity (Built-in)", "gemini-3.6-flash-low");
        ag.provider_kind = ProviderKind::Antigravity;
        ag.protocol_type = ProtocolType::Anthropic;
        let providers = vec![ag.clone()];
        let catalog = build_catalog(
            CatalogStyle::Claude,
            &[(ag, vec!["gemini-3.6-flash-low".into()])],
        );
        let public = catalog
            .iter()
            .find(|entry| entry.upstream_slug == "gemini-3.6-flash-low")
            .expect("catalog keeps an explicitly saved 3.6-flash-low default");
        assert_eq!(
            public.public_id,
            "claude.antigravity--built-in.gemini-3.6-flash-low"
        );
        assert_eq!(
            resolve_request(&catalog, &providers, &public.public_id),
            Some(("ag".into(), "gemini-3.6-flash-low".into()))
        );
    }

    #[test]
    fn collect_provider_slugs_omits_hidden_but_keeps_default() {
        let mut kimi = provider("p1", "Kimi", "kimi-k2");
        kimi.failover_models = vec!["kimi-hidden".into(), "kimi-ok".into()];
        kimi.hidden_models = vec!["kimi-hidden".into(), "kimi-k2".into()];
        let ids = collect_provider_slugs(&kimi, &["kimi-cached".into(), "kimi-hidden".into()]);
        assert!(ids.iter().any(|id| id == "kimi-k2"));
        assert!(ids.iter().any(|id| id == "kimi-ok"));
        assert!(ids.iter().any(|id| id == "kimi-cached"));
        assert!(!ids.iter().any(|id| id == "kimi-hidden"));
    }

    #[test]
    fn rewrite_json_model_replaces_slug() {
        let body = br#"{"model":"claude.kimi.kimi-k2","stream":true}"#;
        let rewritten = rewrite_json_model(body, "kimi-k2");
        let value: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(value["model"], "kimi-k2");
        assert_eq!(value["stream"], true);
    }

    #[test]
    fn hide_official_keeps_explicit_default_and_drops_suggested_slugs() {
        let mut relay = provider("p1", "sub2api", "gpt-5.4-mini");
        relay.failover_models = vec!["kimi-k2".into()];
        let cached = vec!["gpt-5.6-luna".into(), "gpt-5.4-mini".into(), "kimi-k2".into()];
        let visible = collect_provider_slugs_with(&relay, &cached, true);
        assert!(visible.iter().any(|id| id == "gpt-5.4-mini"));
        assert!(visible.iter().any(|id| id == "kimi-k2"));
        assert!(!visible.iter().any(|id| id == "gpt-5.6-luna"));
        let injected = collect_provider_slugs_with(&relay, &cached, false);
        assert!(injected.iter().any(|id| id == "gpt-5.6-luna"));
    }

    #[test]
    fn normalize_rewrites_official_slug_to_catalog_subagent() {
        let mut ag = provider("ag", "Antigravity", "gemini-3.7-flash-high");
        ag.is_current = true;
        let relay = provider("p2", "sub2api", "gpt-5.4-mini");
        let providers = vec![ag.clone(), relay.clone()];
        let catalog = build_catalog_with(
            CatalogStyle::Codex,
            &[(ag, vec![]), (relay, vec![])],
            true,
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Codex,
                &catalog,
                &providers,
                "gpt-5.6-luna",
                true,
                Some("gpt-5.4-mini"),
                false,
            ),
            "gpt-5.4-mini"
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Codex,
                &catalog,
                &providers,
                "gemini-3.7-flash-high",
                true,
                Some("gpt-5.4-mini"),
                false,
            ),
            "gemini-3.7-flash-high"
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Codex,
                &catalog,
                &providers,
                "gpt-5.6-luna",
                true,
                Some("gpt-5.4-mini"),
                true,
            ),
            "gpt-5.4-mini"
        );
    }

    #[test]
    fn catalog_subagent_without_setting_uses_flash_low() {
        let mut ag = provider("ag", "Antigravity", "gemini-3.7-flash-high");
        ag.provider_kind = ProviderKind::Antigravity;
        ag.base_url = "http://127.0.0.1:15830".into();
        ag.is_current = true;
        let catalog = build_catalog_with(CatalogStyle::Codex, &[(ag.clone(), vec![])], false);
        assert!(
            catalog
                .iter()
                .any(|entry| entry.upstream_slug.ends_with("flash-low")),
            "AG catalog must list flash-low for subagent routing: {:?}",
            catalog.iter().map(|e| &e.upstream_slug).collect::<Vec<_>>()
        );
        let routed = normalize_client_request(
            CatalogStyle::Codex,
            &catalog,
            &[ag],
            "gpt-5.6-codex",
            false,
            None,
            true,
        );
        assert!(
            routed.to_ascii_lowercase().contains("flash-low"),
            "empty catalog subagent must not inherit flash-high, got {routed}"
        );
    }

    #[test]
    fn hide_official_maps_claude_role_ids_to_subagent() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let providers = vec![kimi.clone()];
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![])]);
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                CLAUDE_HAIKU_ROLE_ID,
                true,
                Some("claude.kimi.kimi-k2"),
                false,
            ),
            "claude.kimi.kimi-k2"
        );
    }

    #[test]
    fn catalog_haiku_uses_subagent_when_official_models_are_visible() {
        let mut ag = provider("ag", "Antigravity (Built-in)", "gemini-3.6-flash-high");
        ag.provider_kind = ProviderKind::Antigravity;
        let providers = vec![ag.clone()];
        let catalog = build_catalog(
            CatalogStyle::Claude,
            &[(
                ag,
                vec![
                    "gemini-3.6-flash-high".into(),
                    "gemini-3.6-flash-low".into(),
                ],
            )],
        );
        let routed = normalize_client_request(
            CatalogStyle::Claude,
            &catalog,
            &providers,
            CLAUDE_HAIKU_ROLE_ID,
            false,
            Some("claude.antigravity--built-in.gemini-3.6-flash-low"),
            false,
        );
        assert_eq!(
            routed,
            "claude.antigravity--built-in.gemini-3.6-flash-low",
            "Haiku/Explore must use the catalog subagent, not the current default high"
        );
    }

    #[test]
    fn catalog_failover_uses_takeover_default_not_gemini() {
        let ag = provider("ag", "Antigravity", "gemini-3.7-flash-high");
        let relay = provider("p2", "sub2api", "gpt-5.4-mini");
        let catalog = build_catalog(
            CatalogStyle::Codex,
            &[(ag, vec![]), (relay.clone(), vec![])],
        );
        assert_eq!(
            failover_upstream_for_provider(&relay, &catalog, Some("gemini-3.7-flash-high")),
            "gpt-5.4-mini"
        );
        assert_eq!(
            failover_upstream_for_provider(&relay, &catalog, Some("gpt-5.4-mini")),
            "gpt-5.4-mini"
        );
    }
}
