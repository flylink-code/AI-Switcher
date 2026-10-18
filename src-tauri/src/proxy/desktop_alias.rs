//! Claude Desktop only keeps model ids that read as Anthropic, and it picks its
//! own background model (title generation, side queries) from any id in the
//! list that looks like a Sonnet / Haiku / Opus. Every catalog id is therefore
//! listed as a stable `claude-mythos-5-aisw-` alias and mapped back to the
//! public id before routing:
//! - Desktop's `f3t` matches `claude-mythos-*` and unlocks 5-level reasoning
//!   effort (`low`, `medium`, `high`, `xhigh`, `max`) on every model.
//! - `claude.exe` (`dU` / `Bb` / `Jie` / `Qpt`) matches `claude-mythos-5` and
//!   preserves `output_config.effort` even in 3p gateway mode.
//! - Desktop's `zT` only probes `haiku` / `sonnet` / `opus`, so it never
//!   hijacks any catalog row as a background probe model.

use std::sync::Mutex;

use serde_json::{json, Value};

use crate::catalog::{is_auto_public_id, CatalogEntry};
use crate::database::dao::{get_setting, set_setting};
use crate::database::Database;

const ALIAS_PREFIX: &str = "claude-mythos-5-aisw-";
const LEGACY_ALIAS_PREFIX: &str = "anthropic/aisw-";

pub fn is_desktop_alias(id: &str) -> bool {
    let trimmed = id.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with(ALIAS_PREFIX) || lower.starts_with(LEGACY_ALIAS_PREFIX)
}

pub fn alias_for(public_id: &str) -> String {
    format!("{ALIAS_PREFIX}{:010}", fnv1a64(public_id) % 10_000_000_000)
}

fn legacy_alias_for(public_id: &str) -> String {
    format!(
        "{LEGACY_ALIAS_PREFIX}{:010}",
        fnv1a64(public_id) % 10_000_000_000
    )
}

pub fn listed_id(public_id: &str) -> String {
    alias_for(public_id)
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
    let first_id = data
        .first()
        .and_then(|row| row.get("id"))
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let last_id = data
        .last()
        .and_then(|row| row.get("id"))
        .cloned()
        .unwrap_or(Value::String(String::new()));
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
        if entry.public_id.eq_ignore_ascii_case(listed)
            || listed_id(&entry.public_id).eq_ignore_ascii_case(listed)
            || legacy_alias_for(&entry.public_id).eq_ignore_ascii_case(listed)
        {
            Some(entry.public_id.clone())
        } else {
            None
        }
    })
}

/// Settings key that remembers the model Desktop last used successfully.
const SELECTED_SETTING: &str = "claude_desktop_selected_model";
/// Settings key that remembers the reasoning effort Desktop's main thread last used.
const SELECTED_EFFORT_SETTING: &str = "claude_desktop_selected_effort";

/// Marks which catalog id scheme the stored selection belongs to.
const ID_SCHEME_SETTING: &str = "catalog_id_scheme";
/// Scheme 2: provider segment dropped from public ids, same-name models merged.
const ID_SCHEME: &str = "2";

/// Aliases are hashes of the public id, so the id scheme change (provider segment
/// dropped, same-name models merged) orphans the remembered alias. Forget it once;
/// the user picks a model again in Desktop. Later startups leave the selection alone.
pub fn reset_selection_for_catalog_id_scheme(db: &Database) {
    let current = db
        .with_read_conn(|conn| get_setting(conn, ID_SCHEME_SETTING))
        .ok()
        .flatten();
    if current.as_deref() == Some(ID_SCHEME) {
        return;
    }
    let _ = db.with_conn(|conn| {
        set_setting(conn, SELECTED_SETTING, "")?;
        set_setting(conn, ID_SCHEME_SETTING, ID_SCHEME)
    });
    if let Ok(mut slot) = LAST_SELECTED.lock() {
        *slot = None;
    }
}

static LAST_SELECTED: Mutex<Option<String>> = Mutex::new(None);
static LAST_SELECTED_EFFORT: Mutex<Option<String>> = Mutex::new(None);

pub fn last_selected(db: &Database) -> Option<String> {
    if let Some(stored) = db
        .with_read_conn(|conn| get_setting(conn, SELECTED_SETTING))
        .ok()
        .flatten()
        .filter(|value| is_desktop_alias(value))
    {
        if let Ok(mut slot) = LAST_SELECTED.lock() {
            *slot = Some(stored.clone());
        }
        return Some(stored);
    }
    LAST_SELECTED.lock().ok().and_then(|slot| slot.clone())
}

/// Remember an alias that just completed. Other ids (title generation, bare
/// roles) are never a selection.
pub fn remember_selected(db: &Database, model: &str) {
    let model = model.trim();
    if !is_desktop_alias(model) {
        return;
    }
    if let Ok(mut slot) = LAST_SELECTED.lock() {
        *slot = Some(model.to_string());
    }
    let stored = db
        .with_read_conn(|conn| get_setting(conn, SELECTED_SETTING))
        .ok()
        .flatten();
    if stored.as_deref() != Some(model) {
        let _ = db.with_conn(|conn| set_setting(conn, SELECTED_SETTING, model));
    }
}

fn normalize_effort_level(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" => Some(normalized),
        _ => None,
    }
}

pub fn last_selected_effort(db: &Database) -> Option<String> {
    match db.with_read_conn(|conn| get_setting(conn, SELECTED_EFFORT_SETTING)) {
        Ok(Some(stored)) => {
            let normalized = normalize_effort_level(&stored);
            if let Some(ref level) = normalized {
                if let Ok(mut slot) = LAST_SELECTED_EFFORT.lock() {
                    *slot = Some(level.clone());
                }
            }
            normalized
        }
        Ok(None) => None,
        Err(_) => LAST_SELECTED_EFFORT
            .lock()
            .ok()
            .and_then(|slot| slot.clone()),
    }
}

pub fn remember_selected_effort(db: &Database, effort: &str) {
    let Some(normalized) = normalize_effort_level(effort) else {
        return;
    };
    if let Ok(mut slot) = LAST_SELECTED_EFFORT.lock() {
        *slot = Some(normalized.clone());
    }
    let stored = db
        .with_read_conn(|conn| get_setting(conn, SELECTED_EFFORT_SETTING))
        .ok()
        .flatten();
    if stored.as_deref() != Some(normalized.as_str()) {
        let _ = db.with_conn(|conn| set_setting(conn, SELECTED_EFFORT_SETTING, &normalized));
    }
}

fn extract_explicit_effort(body: &Value) -> Option<String> {
    [
        body.pointer("/output_config/effort")
            .and_then(Value::as_str),
        body.get("effort").and_then(Value::as_str),
        body.pointer("/thinking/effort").and_then(Value::as_str),
        body.pointer("/reasoning/effort").and_then(Value::as_str),
        body.get("reasoning_effort").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .find_map(normalize_effort_level)
}

/// True when `body` belongs to Claude Desktop's main conversation loop rather
/// than an internal sub-query (`WebFetch` `web_fetch_apply`, `sideQuery`,
/// `hook_prompt`, `compact`) or a `Task`/`Agent` subagent.
///
/// In `claude.exe`, `AskUserQuestion` and `EnterPlanMode` are non-deferred
/// main-thread tools that are unconditionally stripped from every subagent tool
/// pool (`Jrt`) and absent from tool-less helper queries (`yJ` / `oH`).
pub fn is_main_thread_request(requested_model: &str, body: &Value) -> bool {
    if !is_desktop_alias(requested_model) {
        return false;
    }
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return false;
    };
    tools.iter().any(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| matches!(name, "AskUserQuestion" | "EnterPlanMode" | "ExitPlanMode"))
    })
}

fn apply_effort_to_body(body: &mut Value, effort: &str) {
    let Some(normalized) = normalize_effort_level(effort) else {
        return;
    };
    let Some(root) = body.as_object_mut() else {
        return;
    };
    let output_config = root
        .entry("output_config".to_string())
        .or_insert_with(|| json!({}));
    if !output_config.is_object() {
        *output_config = json!({});
    }
    if let Some(obj) = output_config.as_object_mut() {
        obj.insert("effort".to_string(), Value::String(normalized.clone()));
    }
    if root.get("effort").is_some_and(Value::is_string) {
        root.insert("effort".to_string(), Value::String(normalized.clone()));
    }
    if root.get("reasoning_effort").is_some_and(Value::is_string) {
        root.insert(
            "reasoning_effort".to_string(),
            Value::String(normalized.clone()),
        );
    }
    if let Some(reasoning) = root.get_mut("reasoning").and_then(Value::as_object_mut) {
        if reasoning.contains_key("effort") {
            reasoning.insert("effort".to_string(), Value::String(normalized.clone()));
        }
    }
    if let Some(thinking) = root.get_mut("thinking").and_then(Value::as_object_mut) {
        if thinking.contains_key("effort") {
            thinking.insert("effort".to_string(), Value::String(normalized));
        }
    }
}

/// Synchronize reasoning effort across Claude Desktop's main conversation loop
/// and its child requests (`WebFetch` summarizer, `Task`/`Agent` subagents,
/// `sideQuery`).
///
/// `claude.exe` stores the user's UI-selected effort in `appState.sessionEffort`
/// and passes it on main-thread turns, but `web_fetch_apply` (`yJ`) and
/// subagent/side queries omit `effortValue` and fall back to the model default
/// (`high` on `claude-mythos-5`) or `~/.claude/settings.json` `effortLevel`.
pub fn sync_request_effort(db: &Database, requested_model: &str, body: &mut Value) {
    if is_main_thread_request(requested_model, body) {
        if let Some(effort) = extract_explicit_effort(body) {
            remember_selected_effort(db, &effort);
        }
        return;
    }
    if let Some(saved_effort) = last_selected_effort(db) {
        apply_effort_to_body(body, &saved_effort);
    }
}

/// Resolve a Desktop request to a catalog public id.
///
/// Only ids the gateway listed (alias or public id) count as a selection.
/// Anything else is a Desktop built-in default (title generation, bare
/// `sonnet` / `haiku`, the CLI side model). Those must not call a model the
/// user did not pick, so they follow the last model that completed. `Err`
/// carries the unresolved id when no selection is known yet.
pub fn resolve_selected(
    requested: &str,
    entries: &[CatalogEntry],
    last_selected: Option<&str>,
) -> Result<String, String> {
    let requested = requested.trim();
    if requested.is_empty()
        || is_auto_public_id(requested)
        || crate::gateway::is_auto_model_id(requested)
    {
        return Ok(requested.to_string());
    }
    let (stem, _) = crate::provider::split_model_window_label(requested);
    if let Some(public_id) = public_id_for_listed(&stem, entries) {
        return Ok(public_id);
    }
    if !is_desktop_alias(requested) {
        if let Some(public_id) = last_selected.and_then(|last| public_id_for_listed(last, entries))
        {
            return Ok(public_id);
        }
    }
    Err(requested.to_string())
}

/// True for ids the gateway itself listed, so they are a real user selection.
pub fn is_listed_selection(requested: &str, entries: &[CatalogEntry]) -> bool {
    let (stem, _) = crate::provider::split_model_window_label(requested.trim());
    public_id_for_listed(&stem, entries).is_some()
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

    #[test]
    fn id_scheme_migration_clears_the_old_selection_once() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| set_setting(conn, SELECTED_SETTING, "anthropic/aisw-0000000001"))
            .unwrap();
        reset_selection_for_catalog_id_scheme(&db);
        let stored = db
            .with_read_conn(|conn| get_setting(conn, SELECTED_SETTING))
            .unwrap();
        assert_eq!(stored.as_deref(), Some(""));

        // A selection made after the migration survives later startups.
        db.with_conn(|conn| set_setting(conn, SELECTED_SETTING, "anthropic/aisw-0000000002"))
            .unwrap();
        reset_selection_for_catalog_id_scheme(&db);
        let stored = db
            .with_read_conn(|conn| get_setting(conn, SELECTED_SETTING))
            .unwrap();
        assert_eq!(stored.as_deref(), Some("anthropic/aisw-0000000002"));
    }

    #[test]
    fn every_catalog_id_is_listed_as_an_alias_and_maps_back() {
        let vendor = "claude.sub2api.gpt-6-astra";
        let sonnet = "claude-sonnet-5-5-high";
        let entries = vec![
            entry("claude.auto", "auto"),
            entry(vendor, "gpt-6-astra"),
            entry(sonnet, sonnet),
        ];
        let payload = discovery_payload(&entries);
        let ids: Vec<&str> = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|id| id.starts_with(ALIAS_PREFIX)));
        assert!(!ids
            .iter()
            .any(|id| id.to_ascii_lowercase().contains("sonnet")));
        let alias = alias_for(vendor);
        assert_eq!(alias.len(), ALIAS_PREFIX.len() + 10);
        assert_eq!(alias_for(vendor), alias);
        let row = payload["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"].as_str() == Some(alias.as_str()))
            .unwrap();
        assert_eq!(row["display_name"], "gpt-6-astra");
        assert_eq!(
            public_id_for_listed(&alias, &entries).as_deref(),
            Some(vendor)
        );
        assert_eq!(
            public_id_for_listed(&alias_for(sonnet), &entries).as_deref(),
            Some(sonnet)
        );
    }

    #[test]
    fn unlisted_desktop_defaults_follow_the_last_selected_model() {
        let gemini = "claude.ag.gemini-3.8-flash";
        let luna = "claude.sub2api.gpt-6-luna";
        let entries = vec![entry(gemini, "gemini-3.8-flash"), entry(luna, "gpt-6-luna")];
        let luna_alias = alias_for(luna);

        // 已列出的别名就是用户的选择，不看记忆。
        assert_eq!(
            resolve_selected(&alias_for(gemini), &entries, Some(&luna_alias)).as_deref(),
            Ok(gemini)
        );
        // 标题生成、裸角色、旧 slug 都不是用户选的模型，跟随上次成功的选择。
        for unlisted in [
            "claude-sonnet-5-5-high",
            "sonnet",
            "haiku",
            "claude-haiku-4-5",
        ] {
            assert_eq!(
                resolve_selected(unlisted, &entries, Some(&luna_alias)).as_deref(),
                Ok(luna),
                "{unlisted}"
            );
        }
        // 还没有选择记录时不猜模型。
        assert!(resolve_selected("sonnet", &entries, None).is_err());
        // 未知别名是过期选择，不能落到记忆里的模型。
        assert!(
            resolve_selected("anthropic/aisw-0000000000", &entries, Some(&luna_alias)).is_err()
        );
        assert!(
            resolve_selected("claude-mythos-5-aisw-0000000000", &entries, Some(&luna_alias))
                .is_err()
        );
        // 旧版 anthropic/aisw- 别名在过渡期仍能解析到同一目录项。
        let legacy_gemini = legacy_alias_for(gemini);
        assert_eq!(
            resolve_selected(&legacy_gemini, &entries, Some(&luna_alias)).as_deref(),
            Ok(gemini)
        );
        // Auto 仍交给卡片默认。
        assert_eq!(
            resolve_selected("claude.auto", &entries, None).as_deref(),
            Ok("claude.auto")
        );
        assert_eq!(resolve_selected("", &entries, None).as_deref(), Ok(""));
        assert!(is_listed_selection(&luna_alias, &entries));
        assert!(!is_listed_selection("sonnet", &entries));
    }

    #[test]
    fn subagent_and_webfetch_inherit_main_thread_selected_effort() {
        let db = Database::memory().unwrap();
        let astra_alias = alias_for("gpt-6-astra");

        // 1. Main-thread turn carries AskUserQuestion/EnterPlanMode and user's "low" effort.
        let mut main_body = json!({
            "model": astra_alias,
            "tools": [
                { "name": "Bash" },
                { "name": "AskUserQuestion" },
                { "name": "EnterPlanMode" }
            ],
            "output_config": { "effort": "low" }
        });
        sync_request_effort(&db, &astra_alias, &mut main_body);
        assert_eq!(last_selected_effort(&db).as_deref(), Some("low"));
        assert_eq!(main_body["output_config"]["effort"], "low");

        // 2. WebFetch internal summarizer (`web_fetch_apply`) uses the same alias,
        // has empty tools, and falls back to `effort: "high"` in claude.exe.
        let mut webfetch_body = json!({
            "model": astra_alias,
            "tools": [],
            "output_config": { "effort": "high" }
        });
        sync_request_effort(&db, &astra_alias, &mut webfetch_body);
        assert_eq!(webfetch_body["output_config"]["effort"], "low");

        // 3. Task/Agent subagent strips AskUserQuestion & EnterPlanMode from tools
        // and falls back to `effort: "high"`.
        let mut subagent_body = json!({
            "model": astra_alias,
            "tools": [
                { "name": "Bash" },
                { "name": "Read" },
                { "name": "WebFetch" }
            ],
            "output_config": { "effort": "high" }
        });
        sync_request_effort(&db, &astra_alias, &mut subagent_body);
        assert_eq!(subagent_body["output_config"]["effort"], "low");

        // 4. Side query (`claude-sonnet-5-20260203`) omits output_config entirely.
        let mut side_query_body = json!({
            "model": "claude-sonnet-5-20260203",
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": "title" }]
        });
        sync_request_effort(&db, "claude-sonnet-5-20260203", &mut side_query_body);
        assert_eq!(side_query_body["output_config"]["effort"], "low");

        // 5. User switches main-thread effort to "high" in Desktop UI; subsequent
        // child requests follow the updated effort.
        let mut main_high_body = json!({
            "model": astra_alias,
            "tools": [{ "name": "AskUserQuestion" }],
            "output_config": { "effort": "high" }
        });
        sync_request_effort(&db, &astra_alias, &mut main_high_body);
        assert_eq!(last_selected_effort(&db).as_deref(), Some("high"));

        let mut follow_up_subagent = json!({
            "model": astra_alias,
            "tools": [{ "name": "Read" }],
            "output_config": { "effort": "low" }
        });
        sync_request_effort(&db, &astra_alias, &mut follow_up_subagent);
        assert_eq!(follow_up_subagent["output_config"]["effort"], "high");
    }
}
