//! Gateway catalog: public model IDs for Claude Code / Codex local-proxy routing.
//!
//! OpenCode / Pi / Dsh write every provider into the agent's own config. Claude
//! Code and Codex only speak one upstream, so the local proxy exposes a merged
//! model list and routes each request by `model`.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::database::dao::gateway;
use crate::database::dao::settings::get_setting;
use crate::database::Database;
use crate::provider::{
    catalog_models_from_provider, resolve_upstream_model, ProtocolType, Provider, ProviderTarget,
    CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID, CLAUDE_OPUS_ROLE_ID, CLAUDE_SONNET_ROLE_ID,
};

pub const GATEWAY_CATALOG_CODE_KEY: &str = "gateway_catalog_claude_code";
pub const GATEWAY_CATALOG_CODEX_KEY: &str = "gateway_catalog_codex";
pub const GATEWAY_CATALOG_CODE_SUBAGENT_KEY: &str = "gateway_catalog_claude_code_subagent";
pub const GATEWAY_CATALOG_CODEX_SUBAGENT_KEY: &str = "gateway_catalog_codex_subagent";
pub const GATEWAY_CATALOG_HIDE_OFFICIAL_CODE_KEY: &str =
    "gateway_catalog_hide_official_claude_code";
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

/// Snapshot of catalog flags for one Agent. Loaded in a single connection so
/// proxy requests do not take the write mutex once per flag.
#[derive(Debug, Clone, Default)]
pub struct CatalogView {
    pub enabled: bool,
    pub hide_official: bool,
    pub subagent_model: Option<String>,
}

const VIEW_TTL: Duration = Duration::from_millis(1500);

struct CachedView {
    loaded_at: Instant,
    view: CatalogView,
}

static VIEW_CACHE: OnceLock<Mutex<HashMap<ProviderTarget, CachedView>>> = OnceLock::new();

fn lock_view_cache() -> std::sync::MutexGuard<'static, HashMap<ProviderTarget, CachedView>> {
    VIEW_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Drop cached catalog flags after a bind/unbind or catalog-setting write.
pub fn invalidate_view_cache() {
    lock_view_cache().clear();
}

pub fn view_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> CatalogView {
    CatalogView {
        enabled: enabled_for_conn(conn, target),
        hide_official: hide_official_for_conn(conn, target),
        subagent_model: subagent_for_conn(conn, target),
    }
}

pub fn view(db: &Database, target: ProviderTarget) -> CatalogView {
    {
        let cache = lock_view_cache();
        if let Some(entry) = cache.get(&target) {
            if entry.loaded_at.elapsed() < VIEW_TTL {
                return entry.view.clone();
            }
        }
    }
    let loaded = db
        .with_conn(|conn| Ok(view_for_conn(conn, target)))
        .unwrap_or_default();
    let mut cache = lock_view_cache();
    cache.insert(
        target,
        CachedView {
            loaded_at: Instant::now(),
            view: loaded.clone(),
        },
    );
    loaded
}

pub fn enabled_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> bool {
    gateway::is_gateway_connection(conn, target)
}

pub fn enabled(db: &Database, target: ProviderTarget) -> bool {
    view(db, target).enabled
}

pub fn hide_official_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> bool {
    if let Ok(Some(profile)) = gateway::current_profile(conn, target) {
        return profile.hide_official;
    }
    let Some(key) = hide_official_setting_key(target) else {
        return false;
    };
    get_setting(conn, key).ok().flatten().as_deref() == Some("true")
}

pub fn hide_official(db: &Database, target: ProviderTarget) -> bool {
    view(db, target).hide_official
}

pub fn subagent_for_conn(conn: &rusqlite::Connection, target: ProviderTarget) -> Option<String> {
    if let Ok(Some(profile)) = gateway::current_profile(conn, target) {
        let value = profile.subagent_model.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let key = subagent_setting_key(target)?;
    get_setting(conn, key)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn subagent_model(db: &Database, target: ProviderTarget) -> Option<String> {
    view(db, target).subagent_model
}

/// Opus Plan is removed; keep the helper so leftover IPC callers stay off.
pub fn opusplan_should_write_alias(_enabled: bool, _plan: Option<&str>, _execute: Option<&str>) -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStyle {
    /// Claude Code `/v1/models` discovery. IDs must contain `claude` or `anthropic`.
    Claude,
    /// Codex catalog / OpenAI `/v1/models`. Any slug is fine; collisions get a prefix.
    Codex,
}

pub fn catalog_style_for(target: ProviderTarget) -> CatalogStyle {
    match target {
        ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop => CatalogStyle::Claude,
        ProviderTarget::Codex
        | ProviderTarget::OpenCode
        | ProviderTarget::Pi
        | ProviderTarget::Dsh
        | ProviderTarget::Cline => CatalogStyle::Codex,
    }
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
    /// Lower-priority providers that publish the same model name, in pool order.
    /// `provider_id` is the highest-priority source; these are only used when it
    /// is cooling down. Empty for models with a single source.
    pub alt_provider_ids: Vec<String>,
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
        let short = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect::<String>();
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
    if uses_kiro_catalog(provider) {
        extend_unique(
            &mut ids,
            crate::kiro::models::catalog_ids()
                .iter()
                .map(|id| (*id).to_string()),
        );
    }
    if hide_official {
        ids.retain(|id| {
            !is_injected_official_model_slug(id)
                || is_explicit_saved_model(provider, id)
                || is_explicit_visible_cached_model(provider, cached, id)
        });
    }
    provider.filter_hidden_models(ids)
}

pub fn build_catalog(
    style: CatalogStyle,
    providers: &[(Provider, Vec<String>)],
) -> Vec<CatalogEntry> {
    build_catalog_with(style, providers, false)
}

pub fn build_catalog_with(
    style: CatalogStyle,
    providers: &[(Provider, Vec<String>)],
    hide_official: bool,
) -> Vec<CatalogEntry> {
    let mut taken = BTreeSet::new();
    let mut entries: Vec<CatalogEntry> = Vec::new();
    // Same model name from several providers is one catalog row. `providers` is in
    // pool order, so the first provider to publish a name owns the row.
    let mut row_by_model: HashMap<String, usize> = HashMap::new();
    for (provider, cached) in providers {
        let slug = provider_slug(&provider.name, &provider.id);
        let anthropic_upstream = provider.protocol_type == ProtocolType::Anthropic;
        let web_search_enabled = !anthropic_upstream && provider.web_search_enabled.unwrap_or(true);
        for upstream in collect_provider_slugs_with(provider, cached, hide_official) {
            let (slug_id, _) = crate::provider::split_model_window_label(&upstream);
            if let Some(&row) = row_by_model.get(&slug_id.to_ascii_lowercase()) {
                let owner = &mut entries[row];
                if owner.provider_id != provider.id && !owner.alt_provider_ids.contains(&provider.id) {
                    owner.alt_provider_ids.push(provider.id.clone());
                }
                continue;
            }
            let context_window = crate::provider::context_window_for_model(provider, &upstream);
            let public_id = unique_public_id(style, &slug_id, &slug, &mut taken);
            row_by_model.insert(slug_id.to_ascii_lowercase(), entries.len());
            entries.push(CatalogEntry {
                public_id,
                display_name: format!("{} · {}", provider.name.trim(), slug_id),
                upstream_slug: slug_id,
                provider_id: provider.id.clone(),
                context_window,
                anthropic_upstream,
                web_search_enabled,
                alt_provider_ids: Vec::new(),
            });
        }
    }
    entries
}

/// Codex / OpenAI-style Auto id. Claude discovery cannot use this bare slug.
pub const AUTO_PUBLIC_ID: &str = "auto";
/// Claude Code `/v1/models` Auto id; must contain `claude` to pass discovery.
pub const CLAUDE_AUTO_PUBLIC_ID: &str = "claude.auto";

pub fn is_auto_public_id(id: &str) -> bool {
    let trimmed = id.trim();
    trimmed.eq_ignore_ascii_case(AUTO_PUBLIC_ID)
        || trimmed.eq_ignore_ascii_case(CLAUDE_AUTO_PUBLIC_ID)
}

fn claude_role_stem(id: &str) -> String {
    let (stem, _) = crate::provider::split_model_window_label(id.trim());
    stem.to_ascii_lowercase()
}

/// Official Claude Code role ids injected into `/v1/models` (with optional `[1m]`).
pub fn is_injected_claude_role_id(id: &str) -> bool {
    matches!(
        claude_role_stem(id).as_str(),
        CLAUDE_SONNET_ROLE_ID
            | CLAUDE_OPUS_ROLE_ID
            | CLAUDE_HAIKU_ROLE_ID
            | CLAUDE_FABLE_ROLE_ID
            | "sonnet"
            | "opus"
            | "haiku"
            | "fable"
    )
}

/// Sonnet / Opus / Fable roles that may inherit the last explicit catalog model.
pub fn is_sticky_remap_role_id(id: &str) -> bool {
    matches!(
        claude_role_stem(id).as_str(),
        CLAUDE_SONNET_ROLE_ID
            | CLAUDE_OPUS_ROLE_ID
            | CLAUDE_FABLE_ROLE_ID
            | "sonnet"
            | "opus"
            | "fable"
    )
}

fn is_haiku_or_subagent_slug(id: &str) -> bool {
    let lower = id.trim().to_ascii_lowercase();
    lower.contains("haiku") || lower.contains("subagent")
}

/// User-picked upstream catalog id — not Auto, not an injected official role.
pub fn is_explicit_catalog_passthrough(entries: &[CatalogEntry], requested: &str) -> bool {
    let requested = requested.trim();
    if requested.is_empty() || is_auto_public_id(requested) {
        return false;
    }
    if is_injected_claude_role_id(requested) || is_haiku_or_subagent_slug(requested) {
        return false;
    }
    entries.iter().any(|entry| {
        !entry.provider_id.trim().is_empty()
            && (entry.public_id.eq_ignore_ascii_case(requested)
                || entry.upstream_slug.eq_ignore_ascii_case(requested))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogRequestError {
    /// The client retained a gateway-issued model id which is no longer published.
    StalePublicId(String),
    /// The client sent a gateway-looking id which is not known to this catalog.
    UnknownPublicId(String),
    /// A raw upstream slug exists on more than one provider and cannot be selected safely.
    AmbiguousUpstream(String),
}

impl CatalogRequestError {
    #[allow(dead_code)]
    pub fn requested(&self) -> &str {
        match self {
            Self::StalePublicId(value)
            | Self::UnknownPublicId(value)
            | Self::AmbiguousUpstream(value) => value,
        }
    }

    #[allow(dead_code)]
    pub fn is_model_not_found(&self) -> bool {
        !matches!(self, Self::AmbiguousUpstream(_))
    }
}

impl fmt::Display for CatalogRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StalePublicId(model) => {
                write!(f, "模型目录中的模型已不可用: {model}，请刷新模型列表后重新选择")
            }
            Self::UnknownPublicId(model) => {
                write!(f, "模型不在智能网关目录中: {model}，请刷新模型列表后重新选择")
            }
            Self::AmbiguousUpstream(model) => {
                write!(f, "上游模型 {model} 在多个供应商中重复，请使用目录中的供应商限定模型 ID")
            }
        }
    }
}

/// Classify a model request before applying route modes. Explicit gateway ids
/// must never silently fall through to the first provider.
pub fn resolve_request_strict(
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested: &str,
) -> Result<Option<(String, String)>, CatalogRequestError> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Ok(providers
            .first()
            .map(|provider| (provider.id.clone(), provider.model.trim().to_string())));
    }
    if let Some(entry) = entries.iter().find(|entry| {
        entry.public_id.eq_ignore_ascii_case(requested) && !entry.provider_id.trim().is_empty()
    }) {
        return Ok(Some((entry.provider_id.clone(), entry.upstream_slug.clone())));
    }

    let upstream_hits: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| {
            !entry.provider_id.trim().is_empty()
                && entry.upstream_slug.eq_ignore_ascii_case(requested)
        })
        .collect();
    if upstream_hits.len() == 1 {
        let entry = upstream_hits[0];
        return Ok(Some((entry.provider_id.clone(), entry.upstream_slug.clone())));
    }
    if upstream_hits.len() > 1 {
        return Err(CatalogRequestError::AmbiguousUpstream(requested.to_string()));
    }

    if is_claude_role_request(requested) {
        let first = providers.first().ok_or_else(|| {
            CatalogRequestError::UnknownPublicId(requested.to_string())
        })?;
        return Ok(Some((
            first.id.clone(),
            resolve_upstream_model(first, requested),
        )));
    }
    // Ids issued before same-name models were merged (`claude.<provider>.<model>`
    // or `<provider>.<model>`) keep resolving to the merged row.
    // A provider-qualified id still pins that provider when it is a source of the model.
    let qualified = requested.strip_prefix("claude.").unwrap_or(requested);
    if let Some((first, model)) = qualified.split_once('.') {
        if let Some(provider) = providers
            .iter()
            .find(|p| provider_slug(&p.name, &p.id).eq_ignore_ascii_case(first))
        {
            if let Some(entry) = entries.iter().find(|entry| {
                entry.upstream_slug.eq_ignore_ascii_case(model)
                    && (entry.provider_id == provider.id
                        || entry.alt_provider_ids.contains(&provider.id))
            }) {
                return Ok(Some((provider.id.clone(), entry.upstream_slug.clone())));
            }
        }
    }
    let legacy_candidates = claude_alias_candidates(requested)
        .into_iter()
        .chain(provider_qualified_model(requested, providers));
    for candidate in legacy_candidates {
        if let Some(entry) = entries.iter().find(|entry| {
            !entry.provider_id.trim().is_empty()
                && entry.upstream_slug.eq_ignore_ascii_case(candidate)
        }) {
            return Ok(Some((entry.provider_id.clone(), entry.upstream_slug.clone())));
        }
    }

    // Keep arbitrary raw model ids working for non-gateway callers and legacy
    // provider configurations, but never treat gateway-qualified ids as raw.
    let looks_gateway_qualified = match requested.strip_prefix("claude.") {
        Some(rest) => rest.contains('.'),
        None => requested.contains('.') && requested.split('.').next().is_some_and(|part| {
            providers.iter().any(|provider| {
                provider_slug(&provider.name, &provider.id).eq_ignore_ascii_case(part)
            })
        }),
    };
    if looks_gateway_qualified {
        return Err(CatalogRequestError::StalePublicId(requested.to_string()));
    }
    if requested.starts_with("claude.") || requested.starts_with("ccs-") {
        return Err(CatalogRequestError::UnknownPublicId(requested.to_string()));
    }
    let first = providers.first().ok_or_else(|| {
        CatalogRequestError::UnknownPublicId(requested.to_string())
    })?;
    Ok(Some((
        first.id.clone(),
        resolve_upstream_model(first, requested),
    )))
}
/// Same-name models are one catalog row owned by the highest-priority provider.
/// While that owner is cooling down, use the next source that is available. When
/// none is, keep the owner so the caller sees its real error. Never changes the
/// model, and is only a pre-flight choice: a failed request is not retried here.
pub fn pick_available_source(
    entries: &[CatalogEntry],
    provider_id: &str,
    upstream_slug: &str,
    is_available: impl Fn(&str) -> bool,
) -> String {
    if is_available(provider_id) {
        return provider_id.to_string();
    }
    entries
        .iter()
        .find(|entry| entry.provider_id == provider_id && entry.upstream_slug == upstream_slug)
        .and_then(|entry| entry.alt_provider_ids.iter().find(|id| is_available(id)))
        .cloned()
        .unwrap_or_else(|| provider_id.to_string())
}

/// Today's public id for a saved model id, when it names a published model under
/// an older id scheme (`claude.<provider>.<model>`). `None` when the id is already
/// current or does not name a catalog model.
pub fn canonical_public_id(
    entries: &[CatalogEntry],
    providers: &[Provider],
    saved: &str,
) -> Option<String> {
    let saved = saved.trim();
    if saved.is_empty()
        || is_auto_public_id(saved)
        || entries.iter().any(|entry| entry.public_id.eq_ignore_ascii_case(saved))
    {
        return None;
    }
    let (provider_id, slug) = resolve_request_strict(entries, providers, saved).ok().flatten()?;
    entries
        .iter()
        .find(|entry| {
            entry.upstream_slug == slug
                && (entry.provider_id == provider_id || entry.alt_provider_ids.contains(&provider_id))
        })
        .map(|entry| entry.public_id.clone())
}

/// Map a client-facing model id to `(provider_id, upstream_slug)`.
pub fn resolve_request(
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested: &str,
) -> Option<(String, String)> {
    resolve_request_strict(entries, providers, requested).ok().flatten()
}

/// Return a stable, non-secret fingerprint for a published catalog snapshot.
pub fn catalog_revision(style: CatalogStyle, entries: &[CatalogEntry]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(match style {
        CatalogStyle::Claude => b"claude\0" as &[u8],
        CatalogStyle::Codex => b"codex\0" as &[u8],
    });
    for entry in entries {
        for value in [
            entry.public_id.as_str(),
            entry.display_name.as_str(),
            entry.upstream_slug.as_str(),
            entry.provider_id.as_str(),
        ] {
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }
        hasher.update(entry.context_window.to_le_bytes());
        hasher.update([entry.anthropic_upstream as u8, entry.web_search_enabled as u8]);
    }
    let digest = hasher.finalize();
    digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One Anthropic-style model row. Claude Code statusline / auto-compact read
/// `context_window` (and `max_input_tokens`) from the live `/v1/models` fetch;
/// `~/.claude/cache/gateway-models.json` only keeps `id` + `display_name`.
pub fn claude_discovery_model(entry: &CatalogEntry) -> Value {
    let slug = if entry.upstream_slug.trim().is_empty() {
        entry.public_id.as_str()
    } else {
        entry.upstream_slug.as_str()
    };
    let max_output = crate::gateway::metadata::max_output_for(slug, entry.context_window);
    json!({
        "type": "model",
        "id": entry.public_id,
        "display_name": entry.display_name,
        "created_at": "2025-01-01T00:00:00Z",
        "context_window": entry.context_window,
        "max_input_tokens": entry.context_window,
        "max_output_tokens": max_output,
        "max_tokens": max_output,
    })
}

pub fn claude_discovery_payload(entries: &[CatalogEntry]) -> Value {
    let first_id = entries.first().map(|entry| entry.public_id.as_str()).unwrap_or("");
    let last_id = entries.last().map(|entry| entry.public_id.as_str()).unwrap_or("");
    json!({
        "object": "list",
        "data": entries.iter().map(claude_discovery_model).collect::<Vec<_>>(),
        "has_more": false,
        "first_id": first_id,
        "last_id": last_id,
    })
}

pub fn find_catalog_entry<'a>(
    entries: &'a [CatalogEntry],
    requested: &str,
) -> Option<&'a CatalogEntry> {
    let requested = requested.trim();
    if requested.is_empty() {
        return None;
    }
    let (stem, _) = crate::provider::split_model_window_label(requested);
    entries.iter().find(|entry| {
        entry.public_id.eq_ignore_ascii_case(requested)
            || entry.upstream_slug.eq_ignore_ascii_case(requested)
            || entry.public_id.eq_ignore_ascii_case(&stem)
            || entry.upstream_slug.eq_ignore_ascii_case(&stem)
    })
}

/// Window advertised to Agents. `[1m]` labels win; smart-gateway extra models
/// use the id's inferred window so Auto's max slot does not inflate every row.
pub fn advertised_context_window(provider: &Provider, model_id: &str) -> u64 {
    let (stem, labeled) = crate::provider::split_model_window_label(model_id);
    if let Some(window) = labeled {
        return window.max(1);
    }
    let inferred = crate::gateway::metadata::context_window_for(&stem);
    if provider.is_smart_gateway() && !is_auto_public_id(model_id) {
        return inferred.max(1);
    }
    provider
        .model_context_window
        .filter(|window| *window > 0)
        .unwrap_or(inferred)
        .max(1)
}

pub fn openai_discovery_model(entry: &CatalogEntry) -> Value {
    let slug = if entry.upstream_slug.trim().is_empty() {
        entry.public_id.as_str()
    } else {
        entry.upstream_slug.as_str()
    };
    let max_output = crate::gateway::metadata::max_output_for(slug, entry.context_window);
    json!({
        "id": entry.public_id,
        "object": "model",
        "owned_by": "ai-switcher",
        "context_window": entry.context_window,
        "max_input_tokens": entry.context_window,
        "max_output_tokens": max_output,
        "max_tokens": max_output,
    })
}

pub fn openai_models_payload(entries: &[CatalogEntry]) -> Value {
    json!({
        "object": "list",
        "data": entries.iter().map(openai_discovery_model).collect::<Vec<_>>(),
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

/// Codex/Claude Code subagents follow the current default unless a catalog
/// subagent id is explicitly configured.
pub(crate) fn catalog_subagent_target(
    entries: &[CatalogEntry],
    providers: &[Provider],
    subagent: Option<&str>,
) -> String {
    catalog_fallback_id(entries, providers, subagent)
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
    plan: Option<&str>,
    execute: Option<&str>,
) -> String {
    normalize_client_request_with(
        style,
        entries,
        providers,
        requested,
        hide_official,
        subagent,
        force_subagent,
        plan,
        execute,
        true,
    )
}

pub fn normalize_client_request_with(
    style: CatalogStyle,
    entries: &[CatalogEntry],
    providers: &[Provider],
    requested: &str,
    hide_official: bool,
    subagent: Option<&str>,
    force_subagent: bool,
    plan: Option<&str>,
    execute: Option<&str>,
    role_routing_enabled: bool,
) -> String {
    let fallback = catalog_fallback_id(entries, providers, subagent);
    if force_subagent {
        let requested = requested.trim();
        // Mode matching already picked a concrete upstream (usually background).
        // Do not throw that away and follow is_current / empty profile.subagent_model.
        if !requested.is_empty()
            && !is_auto_public_id(requested)
            && !is_haiku_or_subagent_slug(requested)
            && is_explicit_catalog_passthrough(entries, requested)
        {
            return requested.to_string();
        }
        return catalog_subagent_target(entries, providers, subagent);
    }
    let requested = requested.trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case("auto") {
        return fallback;
    }
    let in_catalog = catalog_in_entries(entries, requested);
    // Claude Code Explore / compact / Task agents send the stable Haiku role
    // id. That must hit the catalog subagent slot even when "hide official"
    // is off — otherwise resolve_request treats it as a role and inherits the
    // current /model default (same SKU as the main session).
    if style == CatalogStyle::Claude && is_haiku_or_subagent_slug(requested) && !in_catalog {
        return catalog_subagent_target(entries, providers, subagent);
    }
    let _ = (plan, execute, role_routing_enabled);
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

/// `hide official` removes synthetic GPT picker suggestions, but the provider
/// visibility editor stores a user's selection as a hidden-model blacklist.
/// Once that blacklist is non-empty, an unhidden cached GPT model is therefore
/// explicitly selected and must remain available in the unified catalog.
fn is_explicit_visible_cached_model(provider: &Provider, cached: &[String], id: &str) -> bool {
    if provider.hidden_models.is_empty() {
        return false;
    }
    let needle = id.trim();
    if needle.is_empty()
        || provider
            .hidden_models
            .iter()
            .any(|hidden| hidden.trim().eq_ignore_ascii_case(needle))
    {
        return false;
    }
    cached
        .iter()
        .any(|model| model.trim().eq_ignore_ascii_case(needle))
}

fn catalog_model_stem(id: &str) -> &str {
    let trimmed = id.trim();
    if let Some(rest) = trimmed.strip_prefix("claude.") {
        // `claude.<model>` where the model name itself has dots (`gpt-5.4`).
        if is_official_openai_builtin(rest) {
            return rest;
        }
        if let Some((_, model)) = rest.split_once('.') {
            return model;
        }
    }
    trimmed
}

fn is_official_openai_builtin(id: &str) -> bool {
    let m = id.trim().to_ascii_lowercase();
    m == "gpt-6-astra"
        || m.starts_with("gpt-6-astra-")
        || m == "gpt-5.6-luna"
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
    // Same-name models are merged before this runs, so the model name is already
    // unique; the provider slug is only a collision fallback. Claude discovery
    // still needs `claude` / `anthropic` in the id, hence the bare `claude.` prefix.
    let preferred = match style {
        CatalogStyle::Claude if passes_claude_discovery(upstream) => upstream.to_string(),
        CatalogStyle::Claude => format!("claude.{upstream}"),
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

/// Model-name candidates behind a `claude.` id: `claude.<model>` (current) and the
/// older provider-qualified `claude.<provider>.<model>`. Model names may contain
/// dots (`gpt-5.4`), so both readings are tried, whole name first.
fn claude_alias_candidates(requested: &str) -> Vec<&str> {
    let Some(rest) = requested.strip_prefix("claude.") else {
        return Vec::new();
    };
    let mut candidates = vec![rest];
    if let Some((_, model)) = rest.split_once('.') {
        candidates.push(model);
    }
    candidates
}

/// Older Codex-style collision ids were `<provider>.<model>`.
fn provider_qualified_model<'a>(requested: &'a str, providers: &[Provider]) -> Option<&'a str> {
    let (first, model) = requested.split_once('.')?;
    providers
        .iter()
        .any(|provider| provider_slug(&provider.name, &provider.id).eq_ignore_ascii_case(first))
        .then_some(model)
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

fn uses_kiro_catalog(provider: &Provider) -> bool {
    if provider.is_kiro() {
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
        "127.0.0.1:15831" | "localhost:15831" | "[::1]:15831"
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
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
            enabled: true,
        }
    }

    #[test]
    fn claude_non_claude_slugs_get_discovery_prefix() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![])]);
        assert_eq!(catalog[0].public_id, "claude.kimi-k2");
        assert!(passes_claude_discovery(&catalog[0].public_id));
        assert_eq!(catalog[0].upstream_slug, "kimi-k2");
        assert_eq!(catalog[0].display_name, "Kimi · kimi-k2");
    }

    #[test]
    fn kiro_gateway_uses_local_catalog_not_gemini() {
        let mut kiro = provider("k", "Kiro", "claude-sonnet-4.6");
        kiro.provider_kind = ProviderKind::Kiro;
        kiro.base_url = "http://127.0.0.1:15831".into();
        let ids = collect_provider_slugs(&kiro, &[]);
        assert!(ids.iter().any(|id| id == "claude-sonnet-4.6"));
        assert!(ids.iter().any(|id| id == "claude-haiku-4.5"));
        assert!(!ids.iter().any(|id| id.contains("gemini")));

        let mut by_url = provider("u", "Loop", "claude-sonnet-4.6");
        by_url.base_url = "http://127.0.0.1:15831/v1".into();
        let url_ids = collect_provider_slugs(&by_url, &[]);
        assert!(url_ids.iter().any(|id| id == "claude-opus-4.6"));
        assert!(!url_ids.iter().any(|id| id.contains("gemini")));
    }

    #[test]
    fn same_name_claude_models_merge_into_the_first_provider() {
        let first = provider("a", "Alpha", "claude-sonnet-4");
        let second = provider("b", "Beta", "claude-sonnet-4");
        let catalog = build_catalog(CatalogStyle::Claude, &[(first, vec![]), (second, vec![])]);
        assert_eq!(catalog.len(), 1, "same model name is one row");
        assert_eq!(catalog[0].public_id, "claude-sonnet-4");
        assert_eq!(catalog[0].provider_id, "a", "pool order decides the owner");
        assert_eq!(catalog[0].alt_provider_ids, vec!["b".to_string()]);
    }

    #[test]
    fn same_name_codex_models_merge_and_keep_a_bare_id() {
        let first = provider("a", "Alpha", "deepseek-v3");
        let second = provider("b", "Beta", "DeepSeek-V3");
        let third = provider("c", "Gamma", "deepseek-v3");
        let catalog = build_catalog(
            CatalogStyle::Codex,
            &[(first, vec![]), (second, vec![]), (third, vec![])],
        );
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].public_id, "deepseek-v3");
        assert_eq!(catalog[0].provider_id, "a");
        assert_eq!(
            catalog[0].alt_provider_ids,
            vec!["b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn model_names_with_dots_resolve_and_legacy_ids_still_work() {
        let relay = provider("p1", "Relay", "gpt-5.4");
        let kimi = provider("p2", "Kimi", "kimi-k2");
        let providers = vec![relay.clone(), kimi.clone()];
        let catalog = build_catalog(CatalogStyle::Claude, &[(relay, vec![]), (kimi, vec![])]);
        assert_eq!(catalog[0].public_id, "claude.gpt-5.4");
        assert_eq!(
            resolve_request_strict(&catalog, &providers, "claude.gpt-5.4"),
            Ok(Some(("p1".into(), "gpt-5.4".into())))
        );
        // Ids issued before same-name models were merged.
        assert_eq!(
            resolve_request_strict(&catalog, &providers, "claude.kimi.kimi-k2"),
            Ok(Some(("p2".into(), "kimi-k2".into())))
        );
        assert_eq!(
            resolve_request_strict(&catalog, &providers, "claude.relay.gpt-5.4"),
            Ok(Some(("p1".into(), "gpt-5.4".into())))
        );
        let codex = build_catalog(
            CatalogStyle::Codex,
            &[(providers[0].clone(), vec![]), (providers[1].clone(), vec![])],
        );
        assert_eq!(
            resolve_request_strict(&codex, &providers, "kimi.kimi-k2"),
            Ok(Some(("p2".into(), "kimi-k2".into())))
        );
        assert_eq!(
            canonical_public_id(&catalog, &providers, "claude.kimi.kimi-k2").as_deref(),
            Some("claude.kimi-k2")
        );
        assert_eq!(canonical_public_id(&catalog, &providers, "claude.kimi-k2"), None);
        assert_eq!(canonical_public_id(&catalog, &providers, "claude.auto"), None);
        // A retired model is still rejected rather than sent to the first provider.
        assert!(matches!(
            resolve_request_strict(&catalog, &providers, "claude.gpt-5.9"),
            Err(CatalogRequestError::StalePublicId(_))
        ));
    }

    #[test]
    fn cooling_owner_hands_a_same_name_model_to_the_next_available_source() {
        let first = provider("a", "Alpha", "shared-model");
        let second = provider("b", "Beta", "shared-model");
        let third = provider("c", "Gamma", "shared-model");
        let catalog = build_catalog(
            CatalogStyle::Codex,
            &[(first, vec![]), (second, vec![]), (third, vec![])],
        );
        let pick = |down: &[&str]| {
            pick_available_source(&catalog, "a", "shared-model", |id| !down.contains(&id))
        };
        assert_eq!(pick(&[]), "a");
        assert_eq!(pick(&["a"]), "b");
        assert_eq!(pick(&["a", "b"]), "c");
        assert_eq!(pick(&["a", "b", "c"]), "a", "all cooling: keep the owner");
    }

    #[test]
    fn merged_same_name_model_resolves_to_the_highest_priority_provider() {
        let first = provider("a", "Alpha", "shared-model");
        let second = provider("b", "Beta", "shared-model");
        let providers = vec![first.clone(), second.clone()];
        let catalog = build_catalog(CatalogStyle::Codex, &[(first, vec![]), (second, vec![])]);
        assert_eq!(
            resolve_request_strict(&catalog, &providers, "shared-model"),
            Ok(Some(("a".into(), "shared-model".into())))
        );
    }

    #[test]
    fn resolve_prefers_public_id_then_upstream() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let ds = provider("p2", "DeepSeek", "deepseek-v3");
        let providers = vec![kimi.clone(), ds.clone()];
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![]), (ds, vec![])]);
        assert_eq!(
            resolve_request(&catalog, &providers, "claude.kimi-k2"),
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
        let mut mapped = providers[0].clone();
        mapped.model_mapping.sonnet = "mapped-sonnet".into();
        let mapped_providers = vec![mapped, providers[1].clone()];
        assert_eq!(
            resolve_request(&catalog, &mapped_providers, CLAUDE_SONNET_ROLE_ID),
            Some(("p1".into(), "mapped-sonnet".into()))
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
            "claude.gemini-3.6-flash-low"
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
    fn strict_resolver_rejects_ambiguous_raw_models() {
        // `build_catalog` merges same-name models, so duplicates only reach the
        // resolver from hand-built entries; it must still refuse to guess.
        let first = provider("a", "Alpha", "shared-model");
        let second = provider("b", "Beta", "shared-model");
        let providers = vec![first.clone(), second.clone()];
        let mut entries = build_catalog(CatalogStyle::Claude, &[(first, vec![])]);
        let mut duplicate = entries[0].clone();
        duplicate.provider_id = "b".into();
        duplicate.public_id = "claude.beta.shared-model".into();
        entries.push(duplicate);

        assert_eq!(
            resolve_request_strict(&entries, &providers, "shared-model"),
            Err(CatalogRequestError::AmbiguousUpstream("shared-model".into()))
        );
    }

    #[test]
    fn strict_resolver_rejects_unknown_gateway_ids() {
        let first = provider("a", "Alpha", "alpha-model");
        let providers = vec![first.clone()];
        let entries = build_catalog(CatalogStyle::Claude, &[(first, vec![])]);

        assert!(matches!(
            resolve_request_strict(&entries, &providers, "claude.alpha.retired-model"),
            Err(CatalogRequestError::StalePublicId(value)) if value == "claude.alpha.retired-model"
        ));
        assert!(matches!(
            resolve_request_strict(&entries, &providers, "ccs-retired-model"),
            Err(CatalogRequestError::UnknownPublicId(value)) if value == "ccs-retired-model"
        ));
    }

    #[test]
    fn catalog_revision_is_stable_and_changes_with_published_metadata() {
        let first = provider("a", "Alpha", "alpha-model");
        let entries = build_catalog(CatalogStyle::Claude, &[(first, vec![])]);
        let revision = catalog_revision(CatalogStyle::Claude, &entries);
        assert_eq!(revision, catalog_revision(CatalogStyle::Claude, &entries));
        assert_eq!(revision.len(), 24);

        let mut changed = entries.clone();
        changed[0].display_name.push_str(" changed");
        assert_ne!(revision, catalog_revision(CatalogStyle::Claude, &changed));
    }

    #[test]
    fn rewrite_json_model_replaces_slug() {
        let body = br#"{"model":"claude.kimi-k2","stream":true}"#;
        let rewritten = rewrite_json_model(body, "kimi-k2");
        let value: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(value["model"], "kimi-k2");
        assert_eq!(value["stream"], true);
    }

    #[test]
    fn hide_official_keeps_explicit_default_and_drops_suggested_slugs() {
        let mut relay = provider("p1", "sub2api", "gpt-5.4-mini");
        relay.failover_models = vec!["kimi-k2".into()];
        let cached = vec![
            "gpt-5.6-luna".into(),
            "gpt-5.4-mini".into(),
            "kimi-k2".into(),
        ];
        let visible = collect_provider_slugs_with(&relay, &cached, true);
        assert!(visible.iter().any(|id| id == "gpt-5.4-mini"));
        assert!(visible.iter().any(|id| id == "kimi-k2"));
        assert!(!visible.iter().any(|id| id == "gpt-5.6-luna"));
        let injected = collect_provider_slugs_with(&relay, &cached, false);
        assert!(injected.iter().any(|id| id == "gpt-5.6-luna"));
    }

    #[test]
    fn hide_official_drops_gpt6_astra_suggestions() {
        let relay = provider("p1", "sub2api", "gpt-5.4-mini");
        let cached = vec!["gpt-6-astra".into(), "gpt-5.4-mini".into()];
        let visible = collect_provider_slugs_with(&relay, &cached, true);
        assert!(visible.iter().any(|id| id == "gpt-5.4-mini"));
        assert!(!visible.iter().any(|id| id == "gpt-6-astra"));
        let injected = collect_provider_slugs_with(&relay, &cached, false);
        assert!(injected.iter().any(|id| id == "gpt-6-astra"));
    }

    #[test]
    fn hide_official_keeps_user_selected_cached_models() {
        let mut relay = provider("p1", "sub2api", "gpt-5.6-terra");
        relay.hidden_models = vec!["gpt-5.4".into(), "gpt-5.5".into()];
        let cached = vec![
            "gpt-5.4".into(),
            "gpt-5.5".into(),
            "gpt-5.6-luna".into(),
            "gpt-5.6-sol".into(),
            "gpt-5.6-terra".into(),
        ];
        let visible = collect_provider_slugs_with(&relay, &cached, true);
        assert_eq!(
            visible,
            vec![
                "gpt-5.6-terra".to_string(),
                "gpt-5.6-luna".to_string(),
                "gpt-5.6-sol".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_rewrites_official_slug_to_catalog_subagent() {
        let mut ag = provider("ag", "Antigravity", "gemini-3.7-flash-high");
        ag.is_current = true;
        let relay = provider("p2", "sub2api", "gpt-5.4-mini");
        let providers = vec![ag.clone(), relay.clone()];
        let catalog =
            build_catalog_with(CatalogStyle::Codex, &[(ag, vec![]), (relay, vec![])], true);
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Codex,
                &catalog,
                &providers,
                "gpt-5.6-luna",
                true,
                Some("gpt-5.4-mini"),
                false,
                None,
                None,
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
                None,
                None,
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
                None,
                None,
            ),
            "gpt-5.4-mini"
        );
    }

    #[test]
    fn catalog_subagent_without_setting_follows_current_default() {
        let mut ag = provider("ag", "Antigravity", "gemini-3.7-flash-high");
        ag.provider_kind = ProviderKind::Antigravity;
        ag.base_url = "http://127.0.0.1:15830".into();
        ag.is_current = true;
        let catalog = build_catalog_with(CatalogStyle::Codex, &[(ag.clone(), vec![])], false);
        let routed = normalize_client_request(
            CatalogStyle::Codex,
            &catalog,
            &[ag],
            "gpt-5.6-codex",
            false,
            None,
            true,
            None,
            None,
        );
        assert!(
            routed.to_ascii_lowercase().contains("flash-high"),
            "empty catalog subagent must follow the current default, got {routed}"
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
                Some("claude.kimi-k2"),
                false,
                None,
                None,
            ),
            "claude.kimi-k2"
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
            Some("claude.gemini-3.6-flash-low"),
            false,
            None,
            None,
        );
        assert_eq!(
            routed, "claude.gemini-3.6-flash-low",
            "Haiku/Explore must use the catalog subagent, not the current default high"
        );
    }

    #[test]
    fn force_subagent_keeps_mode_chosen_upstream() {
        let mut ag = provider("ag", "Antigravity", "gemini-3.8-flash-high");
        ag.provider_kind = ProviderKind::Antigravity;
        ag.is_current = true;
        let providers = vec![ag.clone()];
        let catalog = build_catalog(
            CatalogStyle::Claude,
            &[(
                ag,
                vec![
                    "gemini-3.8-flash-high".into(),
                    "gemini-3.8-flash-low".into(),
                ],
            )],
        );
        let routed = normalize_client_request(
            CatalogStyle::Claude,
            &catalog,
            &providers,
            "gemini-3.8-flash-low",
            false,
            None,
            true,
            None,
            None,
        );
        assert_eq!(
            routed, "gemini-3.8-flash-low",
            "background lookup must not fall back to is_current default high"
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

    #[test]
    fn leftover_opusplan_slots_do_not_remap_role_ids() {
        let mut ag = provider("ag", "Antigravity (Built-in)", "gemini-3.8-flash-high");
        ag.provider_kind = ProviderKind::Antigravity;
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let providers = vec![ag.clone(), kimi.clone()];
        let catalog = build_catalog(
            CatalogStyle::Claude,
            &[
                (ag, vec!["gemini-3.8-flash-high".into()]),
                (kimi, vec!["kimi-k2".into()]),
            ],
        );
        let plan = "claude.gemini-3.8-flash-high";
        let execute = "claude.kimi-k2";
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                CLAUDE_OPUS_ROLE_ID,
                false,
                None,
                false,
                Some(plan),
                Some(execute),
            ),
            CLAUDE_OPUS_ROLE_ID
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                "claude-opus-5[1m]",
                false,
                None,
                false,
                Some(plan),
                Some(execute),
            ),
            "claude-opus-5[1m]"
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                CLAUDE_SONNET_ROLE_ID,
                false,
                None,
                false,
                Some(plan),
                Some(execute),
            ),
            CLAUDE_SONNET_ROLE_ID
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                "opusplan",
                false,
                None,
                false,
                Some(plan),
                Some(execute),
            ),
            "opusplan"
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                CLAUDE_HAIKU_ROLE_ID,
                true,
                Some("claude.kimi-k2"),
                false,
                Some(plan),
                Some("claude.gemini-3.8-flash-high"),
            ),
            "claude.kimi-k2"
        );
        assert_eq!(
            normalize_client_request(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                plan,
                true,
                None,
                false,
                Some("unused-plan"),
                Some(execute),
            ),
            plan,
            "catalog public ids must not be stolen by role rewriting"
        );
        assert_eq!(
            normalize_client_request_with(
                CatalogStyle::Claude,
                &catalog,
                &providers,
                CLAUDE_OPUS_ROLE_ID,
                false,
                None,
                false,
                Some(plan),
                Some(execute),
                false,
            ),
            CLAUDE_OPUS_ROLE_ID,
            "leftover plan/execute slots must not remap role ids"
        );
        assert!(!opusplan_should_write_alias(true, Some(plan), None));
        assert!(!opusplan_should_write_alias(true, None, None));
        assert!(!opusplan_should_write_alias(false, Some(plan), Some(execute)));
    }

    #[test]
    fn auto_ids_are_recognised_but_never_listed() {
        assert!(is_auto_public_id("auto"));
        assert!(is_auto_public_id(" Claude.Auto "));
        assert!(!is_auto_public_id("claude.kimi-k2"));

        let kimi = provider("p1", "Kimi", "kimi-k2");
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![])]);
        assert!(catalog.iter().all(|entry| !is_auto_public_id(&entry.public_id)));
        let payload = claude_discovery_payload(&catalog);
        let ids: Vec<&str> = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["claude.kimi-k2"]);
        assert!(passes_claude_discovery(ids[0]));
        let row = &payload["data"][0];
        assert_eq!(row["type"], "model");
        assert!(row["max_output_tokens"].as_u64().unwrap_or(0) > 0);
        assert_eq!(row["max_tokens"], row["max_output_tokens"]);
    }

    #[test]
    fn injected_claude_roles_are_not_explicit_passthrough() {
        let kimi = provider("p1", "Kimi", "kimi-k2");
        let catalog = build_catalog(CatalogStyle::Claude, &[(kimi, vec![])]);
        assert!(!is_explicit_catalog_passthrough(
            &catalog,
            CLAUDE_SONNET_ROLE_ID
        ));
        assert!(!is_explicit_catalog_passthrough(
            &catalog,
            "claude-sonnet-5[1m]"
        ));
        assert!(!is_explicit_catalog_passthrough(&catalog, "claude.auto"));
        assert!(is_explicit_catalog_passthrough(
            &catalog,
            "claude.kimi-k2"
        ));
        assert!(is_sticky_remap_role_id(CLAUDE_SONNET_ROLE_ID));
        assert!(!is_sticky_remap_role_id(CLAUDE_HAIKU_ROLE_ID));
    }

    #[test]
    fn openai_discovery_lists_windows() {
        let first = provider("a", "Alpha", "gemini-3.8-flash-high");
        let catalog = build_catalog(CatalogStyle::Codex, &[(first, vec![])]);
        let payload = openai_models_payload(&catalog);
        assert_eq!(payload["object"], "list");
        assert_eq!(payload["data"].as_array().map(Vec::len), Some(1));
        let retrieved = find_catalog_entry(&catalog, "gemini-3.8-flash-high").expect("gemini");
        let row = openai_discovery_model(retrieved);
        assert_eq!(row["id"], retrieved.public_id);
        assert_eq!(row["object"], "model");
        assert_eq!(row["context_window"], retrieved.context_window);
        assert_eq!(row["max_input_tokens"], retrieved.context_window);
        assert!(row["max_output_tokens"].as_u64().unwrap_or(0) > 0);
        assert_eq!(row["max_tokens"], row["max_output_tokens"]);
    }
    #[test]
    fn advertised_window_does_not_inflate_gateway_extra_models() {
        let mut auto = provider("sg", "Auto", "auto");
        auto.provider_kind = ProviderKind::SmartGateway;
        auto.model_context_window = Some(1_000_000);
        assert_eq!(advertised_context_window(&auto, "auto"), 1_000_000);
        assert_eq!(advertised_context_window(&auto, "kimi-k2"), 200_000);
        assert_eq!(
            advertised_context_window(&auto, "gemini-3.8-flash-high"),
            1_000_000
        );
    }
}
