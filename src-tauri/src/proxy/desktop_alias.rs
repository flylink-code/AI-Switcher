//! Claude Desktop only keeps model ids that read as Anthropic. Catalog ids
//! that name another vendor are listed as a stable `anthropic/aisw-` alias
//! and mapped back to the public id before routing.

use serde_json::{json, Value};

use crate::catalog::{is_auto_public_id, CatalogEntry};
use crate::provider::{
    ClaudeModelRole, Provider, CLAUDE_FABLE_ROLE_ID, CLAUDE_HAIKU_ROLE_ID, CLAUDE_OPUS_ROLE_ID,
    CLAUDE_SONNET_ROLE_ID,
};

const ALIAS_PREFIX: &str = "anthropic/aisw-";

const DENIED_NEEDLES: &[&str] = &[
    "ark-code",
    "astron",
    "command-r",
    "deepseek",
    "doubao",
    "gemini",
    "gemma",
    "glm",
    "gpt",
    "grok",
    "hermes",
    "hy3",
    "kimi",
    "lfm",
    "llama",
    "longcat",
    "mimo",
    "minimax",
    "mistral",
    "mixtral",
    "moonshot",
    "nemotron",
    "openai",
    "phi-",
    "qianfan",
    "qwen",
    "tc-code",
    "yi-",
    "stepfun",
    "step-3",
    "seed-",
    "bytedance",
    "hunyuan",
    "granite",
    "amazon.nova",
    "nova-",
    "devstral",
    "ministral",
    "ernie",
    "codex",
    "arcee",
    "trinity",
    "abab",
    "jamba",
    "arctic",
    "solar",
    "mercury",
    "zamba",
    "kat-coder",
    "dpsk",
];

const ACCEPTED_WORDS: &[&str] = &[
    "claude", "sonnet", "opus", "haiku", "fable", "mythos", "anthropic",
];

pub fn desktop_accepts(id: &str) -> bool {
    let id = id.trim().to_ascii_lowercase();
    if is_bare_tier(&id) {
        return true;
    }
    if denied(&id) {
        return false;
    }
    ACCEPTED_WORDS.iter().any(|word| id.contains(word))
}

pub fn alias_for(public_id: &str) -> String {
    format!("{ALIAS_PREFIX}{:010}", fnv1a64(public_id) % 10_000_000_000)
}

pub fn listed_id(public_id: &str) -> String {
    if desktop_accepts(public_id) {
        public_id.to_string()
    } else {
        alias_for(public_id)
    }
}

pub fn discovery_payload(entries: &[CatalogEntry]) -> Value {
    let visible: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| !is_auto_public_id(&entry.public_id))
        .collect();
    let titles = display_titles(&visible);
    let data: Vec<Value> = visible
        .iter()
        .zip(titles)
        .map(|(entry, title)| discovery_row(entry, &title))
        .collect();
    let first_id = data.first().and_then(|row| row.get("id")).cloned().unwrap_or(Value::String(String::new()));
    let last_id = data.last().and_then(|row| row.get("id")).cloned().unwrap_or(Value::String(String::new()));
    json!({
        "object": "list",
        "data": data,
        "has_more": false,
        "first_id": first_id,
        "last_id": last_id,
    })
}

pub fn discovery_model(entry: &CatalogEntry, entries: &[CatalogEntry]) -> Value {
    let visible: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|item| !is_auto_public_id(&item.public_id))
        .collect();
    let titles = display_titles(&visible);
    let title = visible
        .iter()
        .zip(titles)
        .find(|(item, _)| item.public_id == entry.public_id)
        .map(|(_, title)| title)
        .unwrap_or_else(|| model_title(entry));
    discovery_row(entry, &title)
}

pub fn public_id_for_listed(listed: &str, entries: &[CatalogEntry]) -> Option<String> {
    let listed = listed.trim();
    if listed.is_empty() {
        return None;
    }
    entries.iter().find_map(|entry| {
        if is_auto_public_id(&entry.public_id) {
            return None;
        }
        if entry.public_id.eq_ignore_ascii_case(listed) || listed_id(&entry.public_id).eq_ignore_ascii_case(listed) {
            Some(entry.public_id.clone())
        } else {
            None
        }
    })
}

/// True when a Desktop request is neither Auto, a bare role, a catalog id, nor
/// a unique upstream slug. The gateway should reject it instead of sending the
/// first upstream's default model.
pub fn is_unmapped_desktop_request(requested: &str, entries: &[CatalogEntry]) -> bool {
    let requested = requested.trim();
    if requested.is_empty()
        || is_auto_public_id(requested)
        || crate::gateway::is_auto_model_id(requested)
    {
        return false;
    }
    let (stem, _) = crate::provider::split_model_window_label(requested);
    if desktop_role(&stem).is_some() {
        return false;
    }
    if public_id_for_listed(&stem, entries).is_some() {
        return false;
    }
    let slug_hits = entries
        .iter()
        .filter(|entry| {
            !is_auto_public_id(&entry.public_id) && entry.upstream_slug.eq_ignore_ascii_case(&stem)
        })
        .count();
    slug_hits != 1
}

/// Map a Desktop request id back to a catalog public id.
///
/// Aliases reverse to the hashed public id. Bare role names use an explicit
/// upstream mapping when one is set, otherwise a catalog slug that contains
/// the tier, otherwise the first catalog model.
pub fn canonical_request(requested: &str, entries: &[CatalogEntry], providers: &[Provider]) -> String {
    let requested = requested.trim();
    if requested.is_empty() || is_auto_public_id(requested) {
        return requested.to_string();
    }
    let (stem, _) = crate::provider::split_model_window_label(requested);
    if let Some(public_id) = public_id_for_listed(&stem, entries) {
        return public_id;
    }
    if let Some(role) = desktop_role(&stem) {
        return resolve_role(role, &stem, entries, providers);
    }
    requested.to_string()
}

fn discovery_row(entry: &CatalogEntry, title: &str) -> Value {
    let mut row = crate::catalog::claude_discovery_model(entry);
    row["id"] = json!(listed_id(&entry.public_id));
    row["display_name"] = json!(title);
    row
}

fn display_titles(entries: &[&CatalogEntry]) -> Vec<String> {
    let raw: Vec<String> = entries.iter().copied().map(model_title).collect();
    raw.iter()
        .enumerate()
        .map(|(index, name)| {
            let duplicated = raw.iter().filter(|other| *other == name).count() > 1;
            if duplicated {
                entries[index].display_name.clone()
            } else {
                name.clone()
            }
        })
        .collect()
}

fn model_title(entry: &CatalogEntry) -> String {
    let slug = entry.upstream_slug.trim();
    if slug.is_empty() {
        entry.public_id.clone()
    } else {
        slug.to_string()
    }
}

fn resolve_role(
    role: ClaudeModelRole,
    requested: &str,
    entries: &[CatalogEntry],
    providers: &[Provider],
) -> String {
    if let Some(provider) = providers.first() {
        let configured = match role {
            ClaudeModelRole::Sonnet => provider.model_mapping.sonnet.trim(),
            ClaudeModelRole::Opus => provider.model_mapping.opus.trim(),
            ClaudeModelRole::Haiku => provider.model_mapping.haiku.trim(),
            ClaudeModelRole::Fable => provider.model_mapping.fable.trim(),
            ClaudeModelRole::Subagent => provider.model_mapping.subagent.trim(),
        };
        if !configured.is_empty() {
            if let Some(entry) = entries.iter().find(|entry| {
                !is_auto_public_id(&entry.public_id)
                    && (entry.upstream_slug.eq_ignore_ascii_case(configured)
                        || entry.public_id.eq_ignore_ascii_case(configured))
            }) {
                return entry.public_id.clone();
            }
            return requested.to_string();
        }
    }
    let needle = match role {
        ClaudeModelRole::Sonnet => "sonnet",
        ClaudeModelRole::Opus => "opus",
        ClaudeModelRole::Haiku => "haiku",
        ClaudeModelRole::Fable => "fable",
        ClaudeModelRole::Subagent => "subagent",
    };
    if let Some(entry) = entries.iter().find(|entry| {
        !is_auto_public_id(&entry.public_id)
            && (entry.upstream_slug.to_ascii_lowercase().contains(needle)
                || entry.public_id.to_ascii_lowercase().contains(needle))
    }) {
        return entry.public_id.clone();
    }
    entries
        .iter()
        .find(|entry| !is_auto_public_id(&entry.public_id))
        .map(|entry| entry.public_id.clone())
        .unwrap_or_else(|| requested.to_string())
}

fn desktop_role(requested: &str) -> Option<ClaudeModelRole> {
    let normalized = requested.trim().to_ascii_lowercase();
    if normalized == "sonnet" || normalized == CLAUDE_SONNET_ROLE_ID {
        Some(ClaudeModelRole::Sonnet)
    } else if normalized == "opus" || normalized == CLAUDE_OPUS_ROLE_ID {
        Some(ClaudeModelRole::Opus)
    } else if normalized == "haiku" || normalized == CLAUDE_HAIKU_ROLE_ID {
        Some(ClaudeModelRole::Haiku)
    } else if normalized == "fable" || normalized == CLAUDE_FABLE_ROLE_ID {
        Some(ClaudeModelRole::Fable)
    } else {
        None
    }
}

fn is_bare_tier(id: &str) -> bool {
    let (tier, rest) = match id.split_once('-') {
        Some((tier, rest)) => (tier, Some(rest)),
        None => (id, None),
    };
    if !matches!(tier, "sonnet" | "opus" | "haiku" | "fable" | "mythos") {
        return false;
    }
    rest.is_none_or(|rest| {
        !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    })
}

fn denied(id: &str) -> bool {
    if DENIED_NEEDLES.iter().any(|needle| id.contains(needle)) {
        return true;
    }
    if has_word(id, "ling") || has_word(id, "unic") {
        return true;
    }
    if has_bounded_prefix(id, "k2.") || has_bounded_prefix(id, "m2.") || has_bounded_prefix(id, "ds-") {
        return true;
    }
    let bytes = id.as_bytes();
    bytes.windows(4).any(|window| window.starts_with(b"phi") && window[3].is_ascii_digit())
}

fn has_word(id: &str, word: &str) -> bool {
    id.match_indices(word).any(|(index, _)| {
        let before = index == 0 || !id.as_bytes()[index - 1].is_ascii_alphanumeric();
        let end = index + word.len();
        let after = end == id.len() || !id.as_bytes()[end].is_ascii_alphanumeric();
        before && after
    })
}

fn has_bounded_prefix(id: &str, needle: &str) -> bool {
    id.match_indices(needle)
        .any(|(index, _)| index == 0 || !id.as_bytes()[index - 1].is_ascii_alphanumeric())
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProtocolType, ProviderKind, ProviderTarget};

    fn entry(public_id: &str, slug: &str) -> CatalogEntry {
        CatalogEntry {
            public_id: public_id.to_string(),
            display_name: format!("Upstream · {slug}"),
            upstream_slug: slug.to_string(),
            provider_id: "up".into(),
            context_window: 200_000,
            anthropic_upstream: false,
            web_search_enabled: false,
        }
    }

    fn provider_with(mapping: ClaudeModelMapping) -> Provider {
        Provider {
            id: "up".into(),
            name: "Upstream".into(),
            base_url: "https://example.test".into(),
            api_key: "k".into(),
            api_key_set: true,
            model: "default-model".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: mapping,
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeDesktop,
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
    fn vendor_catalog_ids_alias_and_map_back() {
        let public_id = "claude.sub2api.gpt-6-astra";
        assert!(!desktop_accepts(public_id));
        assert!(desktop_accepts("claude-sonnet-5"));
        let alias = alias_for(public_id);
        assert!(alias.starts_with(ALIAS_PREFIX));
        assert_eq!(alias.len(), ALIAS_PREFIX.len() + 10);
        assert_eq!(alias_for(public_id), alias);

        let entries = vec![
            entry("claude.auto", "auto"),
            entry(public_id, "gpt-6-astra"),
            entry("claude-sonnet-5", "claude-sonnet-5"),
        ];
        let payload = discovery_payload(&entries);
        let ids: Vec<&str> = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert!(!ids.contains(&"claude.auto"));
        assert!(ids.contains(&"claude-sonnet-5"));
        assert!(ids.contains(&alias.as_str()));
        let gpt = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_str() == Some(alias.as_str()))
            .unwrap();
        assert_eq!(gpt["display_name"], "gpt-6-astra");
        assert_eq!(public_id_for_listed(&alias, &entries).as_deref(), Some(public_id));
        assert_eq!(
            canonical_request(&alias, &entries, &[]),
            public_id
        );
    }

    #[test]
    fn bare_roles_use_mapping_then_slug_then_first() {
        let entries = vec![
            entry("claude.ag.gemini-3.8-flash-high", "gemini-3.8-flash-high"),
            entry("claude.deepseek.deepseek-v4", "deepseek-v4"),
        ];
        let mapped = provider_with(ClaudeModelMapping {
            sonnet: "deepseek-v4".into(),
            ..ClaudeModelMapping::default()
        });
        assert_eq!(
            canonical_request("sonnet", &entries, &[mapped]),
            "claude.deepseek.deepseek-v4"
        );

        let unmapped = provider_with(ClaudeModelMapping::default());
        let opus_entries = vec![
            entry("claude.vendor.gpt-5", "gpt-5"),
            entry("claude.vendor.claude-opus-4-8", "claude-opus-4-8"),
        ];
        assert_eq!(
            canonical_request("opus", &opus_entries, &[unmapped.clone()]),
            "claude.vendor.claude-opus-4-8"
        );
        assert_eq!(
            canonical_request("haiku", &entries, &[unmapped]),
            "claude.ag.gemini-3.8-flash-high"
        );
    }

    #[test]
    fn unknown_alias_is_unmapped_and_known_alias_resolves() {
        let public_id = "claude.ag.gemini-3.8-flash";
        let entries = vec![entry(public_id, "gemini-3.8-flash")];
        let alias = alias_for(public_id);
        assert_eq!(canonical_request(&alias, &entries, &[]), public_id);
        assert!(!is_unmapped_desktop_request(&alias, &entries));
        assert!(!is_unmapped_desktop_request(
            &canonical_request(&alias, &entries, &[]),
            &entries
        ));
        assert!(is_unmapped_desktop_request("anthropic/aisw-0000000000", &entries));
        assert!(!is_unmapped_desktop_request("sonnet", &entries));
        assert!(!is_unmapped_desktop_request("claude.auto", &entries));
    }
}
