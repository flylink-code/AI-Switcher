//! Provider-row queries and mutations, scoped to one Claude application.

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::catalog;
use crate::error::{AppError, AppResult};
use crate::provider::{
    normalize_provider_base_url, normalized_auto_review_model_override, normalized_model_mapping,
    validate_provider_kind, validate_target_protocol, ClaudeModelMapping, ProtocolType, Provider,
    ProviderInput, ProviderKind, ProviderTarget,
};
use crate::secrets;

#[derive(Debug, Clone)]
pub struct ProviderModelCache {
    pub models: Vec<String>,
    pub checked_at: i64,
}

/// Number of providers belonging to a target application.
pub fn count_providers(conn: &Connection, target: ProviderTarget) -> AppResult<i64> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM providers WHERE target_app = ?;",
        params![target.as_str()],
        |row| row.get(0),
    )?;
    Ok(n)
}

/// List providers for one target application, ordered by sort index then creation time.
pub fn list_providers(conn: &Connection, target: ProviderTarget) -> AppResult<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index,
                is_current, created_at,
                (SELECT status FROM provider_health WHERE provider_id = providers.id),
                (SELECT checked_at FROM provider_health WHERE provider_id = providers.id),
                model_mapping_json, model_context_window, auto_review_model_override,
                provider_kind, auth_binding, web_search_enabled, failover_group, failover_models,
                (SELECT detail FROM provider_health WHERE provider_id = providers.id),
                thinking_config_json, hidden_models_json, custom_headers_json
         FROM providers WHERE target_app = ? ORDER BY sort_index ASC, created_at ASC;",
    )?;
    let rows = stmt.query_map(params![target.as_str()], row_to_provider)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Fetch a provider by id.
pub fn get_provider(conn: &Connection, id: &str) -> AppResult<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index,
                is_current, created_at,
                (SELECT status FROM provider_health WHERE provider_id = providers.id),
                (SELECT checked_at FROM provider_health WHERE provider_id = providers.id),
                model_mapping_json, model_context_window, auto_review_model_override,
                provider_kind, auth_binding, web_search_enabled, failover_group, failover_models,
                (SELECT detail FROM provider_health WHERE provider_id = providers.id),
                thinking_config_json, hidden_models_json, custom_headers_json
         FROM providers WHERE id = ?;",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_provider(row)?)),
        None => Ok(None),
    }
}

/// The current provider for one target application, if any.
pub fn get_current_provider(conn: &Connection, target: ProviderTarget) -> AppResult<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index,
                is_current, created_at,
                (SELECT status FROM provider_health WHERE provider_id = providers.id),
                (SELECT checked_at FROM provider_health WHERE provider_id = providers.id),
                model_mapping_json, model_context_window, auto_review_model_override,
                provider_kind, auth_binding, web_search_enabled, failover_group, failover_models,
                (SELECT detail FROM provider_health WHERE provider_id = providers.id),
                thinking_config_json, hidden_models_json, custom_headers_json
         FROM providers WHERE target_app = ? AND is_current = 1 LIMIT 1;",
    )?;
    let mut rows = stmt.query(params![target.as_str()])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_provider(row)?)),
        None => Ok(None),
    }
}

/// Insert or update a provider. Providers are always scoped by their input target.
///
/// Credential handling (P7): a non-empty `input.api_key` is stored in the OS
/// credential store and the DB column holds only a `kr://<id>` reference. An
/// empty `api_key` on **update** preserves the existing key; on **create** it
/// leaves the provider with no key.
pub fn upsert_provider(conn: &Connection, input: &ProviderInput) -> AppResult<Provider> {
    if input.name.trim().is_empty() {
        return Err(AppError::Config("供应商名称不能为空".to_string()));
    }
    validate_provider_kind(input.target_app, input.provider_kind)?;
    let is_codex_oauth = input.provider_kind == ProviderKind::CodexOauth;
    if !is_codex_oauth && input.base_url.trim().is_empty() {
        return Err(AppError::Config("API 地址不能为空".to_string()));
    }
    if is_codex_oauth && input.auth_binding.trim().is_empty() {
        return Err(AppError::Config("ChatGPT 账户绑定不能为空".to_string()));
    }
    if input.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空".to_string()));
    }
    let protocol_type = if is_codex_oauth {
        ProtocolType::OpenAiResponses
    } else {
        input.protocol_type
    };
    validate_target_protocol(input.target_app, protocol_type)?;
    let base_url = if is_codex_oauth {
        crate::codex_oauth::CODEX_OAUTH_BASE_URL.to_string()
    } else {
        normalize_provider_base_url(input.target_app, protocol_type, &input.base_url)?
    };
    let model_mapping_json = serde_json::to_string(&normalized_model_mapping(
        input.target_app,
        input.model_mapping.clone(),
    ))?;
    let auto_review_model_override =
        normalized_auto_review_model_override(input.target_app, input.auto_review_model_override.clone());
    let thinking_config_json = serde_json::to_string(
        input
            .thinking_config
            .as_ref()
            .filter(|cfg| !cfg.is_empty())
            .unwrap_or(&crate::provider::ThinkingConfig::default()),
    )?;
    let custom_headers_json = serde_json::to_string(
        input
            .custom_headers
            .as_ref()
            .filter(|headers| !headers.is_empty())
            .unwrap_or(&std::collections::HashMap::new()),
    )?;

    let now = Utc::now().timestamp_millis();
    if let Some(id) = input.id.as_ref() {
        let existing = get_provider(conn, id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
        if existing.target_app != input.target_app {
            return Err(AppError::Config("不能跨应用更新供应商".to_string()));
        }
        if input.clear_api_key && !input.api_key.trim().is_empty() {
            return Err(AppError::Config("不能同时替换和删除 API Key".to_string()));
        }
        // Determine the DB column value without allowing an implicit empty-key
        // update to erase an existing credential.
        let api_key_col = if is_codex_oauth {
            String::new()
        } else if input.clear_api_key {
            String::new()
        } else if !input.api_key.trim().is_empty() {
            secrets::store_key(id, input.api_key.trim())?;
            secrets::keyring_ref(id)
        } else {
            existing.api_key.clone()
        };
        conn.execute(
            "UPDATE providers SET name = ?, base_url = ?, api_key = ?, model = ?,
                protocol_type = ?, notes = ?, model_mapping_json = ?, model_context_window = ?,
                auto_review_model_override = ?, provider_kind = ?, auth_binding = ?,
                web_search_enabled = ?, failover_group = ?, failover_models = ?,
                thinking_config_json = ?, hidden_models_json = ?, custom_headers_json = ? WHERE id = ?;",
            params![
                input.name, base_url, api_key_col, input.model,
                protocol_type.as_str(), input.notes, model_mapping_json,
                input.model_context_window, auto_review_model_override,
                input.provider_kind.as_str(), input.auth_binding.trim(),
                web_search_sql(input.web_search_enabled),
                input.failover_group,
                serde_json::to_string(&normalize_failover_models(&input.failover_models))?,
                thinking_config_json,
                serde_json::to_string(&normalize_failover_models(&input.hidden_models))?,
                custom_headers_json,
                id,
            ],
        )?;
        if input.clear_api_key || is_codex_oauth {
            secrets::delete_key(id)?;
        }
        return get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")));
    }

    let id = format!("p_{}", uuid_v8());
    let next_sort = next_sort_index(conn, input.target_app)?;
    // Create: store key (if provided) before inserting the row.
    if input.clear_api_key {
        return Err(AppError::Config("新建供应商时不能删除不存在的 API Key".to_string()));
    }
    let api_key_col = if is_codex_oauth {
        String::new()
    } else if !input.api_key.trim().is_empty() {
        secrets::store_key(&id, input.api_key.trim())?;
        secrets::keyring_ref(&id)
    } else {
        String::new()
    };
    if let Err(error) = conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, provider_kind, auth_binding, target_app, notes, sort_index, is_current, created_at, model_mapping_json, model_context_window, auto_review_model_override, web_search_enabled, failover_group, failover_models, thinking_config_json, hidden_models_json, custom_headers_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        params![
            id, input.name, base_url, api_key_col, input.model,
            protocol_type.as_str(), input.provider_kind.as_str(), input.auth_binding.trim(),
            input.target_app.as_str(), input.notes, next_sort, now,
            model_mapping_json, input.model_context_window, auto_review_model_override,
            web_search_sql(input.web_search_enabled),
            input.failover_group,
            serde_json::to_string(&normalize_failover_models(&input.failover_models))?,
            thinking_config_json,
            serde_json::to_string(&normalize_failover_models(&input.hidden_models))?,
            custom_headers_json,
        ],
    ) {
        if !api_key_col.is_empty() {
            let _ = secrets::delete_key(&id);
        }
        return Err(error.into());
    }
    get_provider(conn, &id)?.ok_or_else(|| AppError::Config("插入后未能读回供应商".to_string()))
}

/// Resolve a provider's API key to plaintext at runtime.
///
/// - `kr://<id>` reference → looked up in the OS credential store.
/// - Empty string → `Ok(None)` (no key configured).
/// - Any leftover plaintext value is rejected as an incomplete migration.
///
/// Callers that write the key into config files (settings.json, configLibrary)
/// or forward it upstream must go through this so the DB never holds plaintext.
pub fn resolve_api_key(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    let provider = get_provider(conn, id)?
        .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    Ok(match provider.api_key.as_str() {
        "" => None,
        v if secrets::is_keyring_ref(v) => {
            let account = &v[secrets::KEYRING_REF_PREFIX.len()..];
            secrets::load_key(account)?
        }
        _ => return Err(AppError::Config("检测到未迁移的明文 API Key，请重新启动以完成凭据迁移".to_string())),
    })
}

pub fn get_provider_model_cache(
    conn: &Connection,
    provider_id: &str,
) -> AppResult<Option<ProviderModelCache>> {
    let mut stmt = conn.prepare(
        "SELECT models_json, checked_at FROM provider_models WHERE provider_id = ?;",
    )?;
    let mut rows = stmt.query(params![provider_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let models_json: String = row.get(0)?;
    let models = serde_json::from_str::<Vec<String>>(&models_json).unwrap_or_default();
    Ok(Some(ProviderModelCache {
        models,
        checked_at: row.get(1)?,
    }))
}

pub fn save_provider_model_cache(
    conn: &Connection,
    provider_id: &str,
    models: &[String],
    checked_at: i64,
) -> AppResult<()> {
    let models_json = serde_json::to_string(models)?;
    conn.execute(
        "INSERT INTO provider_models (provider_id, models_json, checked_at) VALUES (?, ?, ?)
         ON CONFLICT(provider_id) DO UPDATE SET
            models_json = excluded.models_json,
            checked_at = excluded.checked_at;",
        params![provider_id, models_json, checked_at],
    )?;
    Ok(())
}

/// Migrate all legacy plaintext values. Schema version advancement is left to
/// the caller so a credential-store failure keeps this operation retryable.
pub fn migrate_plaintext_api_keys(conn: &Connection) -> AppResult<()> {
    let mut stmt = conn.prepare("SELECT id, api_key FROM providers WHERE api_key <> ''")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, value) = row?;
        if secrets::is_keyring_ref(&value) {
            continue;
        }
        secrets::store_key(&id, &value)?;
        conn.execute(
            "UPDATE providers SET api_key = ? WHERE id = ? AND api_key = ?",
            params![secrets::keyring_ref(&id), id, value],
        )?;
    }
    Ok(())
}

/// Delete a provider by id. The active provider for switchable targets cannot
/// be deleted. Catalog / gateway-catalog targets have no exclusive activation,
/// so a leftover `is_current` marker must not block the last remaining row.
/// Also removes the stored credential from the OS keyring (best-effort).
pub fn delete_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let provider = get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    if provider.is_current
        && !provider.target_app.is_catalog_target()
        && !catalog::enabled_for_conn(conn, provider.target_app)
    {
        return Err(AppError::Config("不能删除当前激活的供应商".to_string()));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM provider_models WHERE provider_id = ?;", params![id])?;
    tx.execute("DELETE FROM provider_health WHERE provider_id = ?;", params![id])?;
    tx.execute("DELETE FROM providers WHERE id = ?;", params![id])?;
    tx.commit()?;
    // Clean up the keyring entry; a missing entry is not an error.
    if let Err(e) = secrets::delete_key(id) {
        log::warn!("删除供应商 {id} 的凭据失败（已忽略）: {e}");
    }
    Ok(())
}

/// Mark `id` as the sole active provider for its target application.
pub fn set_current_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let provider = get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE providers SET is_current = 0 WHERE target_app = ?;",
        params![provider.target_app.as_str()],
    )?;
    tx.execute("UPDATE providers SET is_current = 1 WHERE id = ?;", params![id])?;
    tx.commit()?;
    Ok(())
}

/// Clear the active marker for one target application (official-login mode).
pub fn clear_current_provider(conn: &Connection, target: ProviderTarget) -> AppResult<()> {
    conn.execute(
        "UPDATE providers SET is_current = 0 WHERE target_app = ?;",
        params![target.as_str()],
    )?;
    Ok(())
}

/// Reorder provider ids within their target application.
pub fn reorder_providers(conn: &Connection, ordered_ids: &[String], target: ProviderTarget) -> AppResult<()> {
    for (idx, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE providers SET sort_index = ? WHERE id = ? AND target_app = ?;",
            params![idx as i64, id, target.as_str()],
        )?;
    }
    Ok(())
}

fn next_sort_index(conn: &Connection, target: ProviderTarget) -> AppResult<i64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(sort_index) FROM providers WHERE target_app = ?;",
        params![target.as_str()],
        |row| row.get(0),
    )?;
    Ok(max.unwrap_or(-1) + 1)
}

fn row_to_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    let protocol: String = row.get(5)?;
    let target: String = row.get(6)?;
    let api_key: String = row.get(3)?;
    let model_mapping_json: String = row.get(13)?;
    let model_mapping =
        serde_json::from_str::<ClaudeModelMapping>(&model_mapping_json).unwrap_or_default();
    let provider_kind_raw: String = row.get(16)?;
    let provider_kind = ProviderKind::from_str_lossy(&provider_kind_raw);
    let auth_binding: String = row.get(17)?;
    let failover_models_json: String = row.get(20)?;
    let failover_models = serde_json::from_str::<Vec<String>>(&failover_models_json)
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let health_detail: Option<String> = row.get(21)?;
    let thinking_config_json: Option<String> = row.get(22).ok();
    let thinking_config = thinking_config_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<crate::provider::ThinkingConfig>(raw).ok())
        .filter(|cfg| !cfg.is_empty());
    let hidden_models_json: String = row.get(23).unwrap_or_else(|_| "[]".to_string());
    let hidden_models = serde_json::from_str::<Vec<String>>(&hidden_models_json)
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let custom_headers_json: Option<String> = row.get(24).ok();
    let custom_headers = custom_headers_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<std::collections::HashMap<String, String>>(raw).ok())
        .filter(|headers| !headers.is_empty());
    Ok(Provider {
        api_key_set: !api_key.is_empty()
            || (provider_kind == ProviderKind::CodexOauth && !auth_binding.is_empty()),
        api_key,
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        model: row.get(4)?,
        model_mapping,
        protocol_type: ProtocolType::from_str_lossy(&protocol),
        provider_kind,
        auth_binding,
        target_app: ProviderTarget::from_str_lossy(&target),
        notes: row.get(7)?,
        sort_index: row.get(8)?,
        is_current: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
        health_status: row.get(11)?,
        health_checked_at: row.get(12)?,
        health_latency_ms: parse_health_latency_ms(health_detail.as_deref()),
        model_context_window: row.get(14)?,
        auto_review_model_override: row.get(15)?,
        web_search_enabled: row.get::<_, Option<i64>>(18)?.map(|value| value != 0),
        failover_group: row.get(19)?,
        failover_models,
        hidden_models,
        thinking_config,
        custom_headers,
    })
}

fn parse_health_latency_ms(detail: Option<&str>) -> Option<u64> {
    let detail = detail?;
    let marker = "latency_ms=";
    let start = detail.find(marker)? + marker.len();
    let end = detail[start..]
        .find(|ch: char| !ch.is_ascii_digit())
        .map(|offset| start + offset)
        .unwrap_or(detail.len());
    detail[start..end].parse().ok()
}

#[cfg(test)]
mod health_latency_tests {
    use super::parse_health_latency_ms;

    #[test]
    fn parses_latency_suffix_from_health_detail() {
        assert_eq!(
            parse_health_latency_ms(Some("ok|latency_ms=123")),
            Some(123)
        );
        assert_eq!(parse_health_latency_ms(Some("no latency")), None);
    }
}

fn normalize_failover_models(models: &[String]) -> Vec<String> {
    models
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn web_search_sql(value: Option<bool>) -> Option<i64> {
    value.map(|enabled| if enabled { 1 } else { 0 })
}

fn uuid_v8() -> String {
    let mut buf = uuid::Uuid::encode_buffer();
    let value = uuid::Uuid::new_v4().simple().encode_upper(&mut buf);
    value[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn provider_input(base_url: &str) -> ProviderInput {
        ProviderInput {
            id: None,
            name: "Test provider".to_string(),
            base_url: base_url.to_string(),
            api_key: String::new(),
            clear_api_key: false,
            model: "test-model".to_string(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
        }
    }

    #[test]
    fn create_and_update_store_canonical_base_urls() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut create =
                provider_input("https://gateway.example.test/openai/v1/chat/completions");
            create.model_mapping.sonnet = "sonnet-upstream".to_string();
            let created = upsert_provider(
                conn,
                &create,
            )?;
            assert_eq!(created.base_url, "https://gateway.example.test/openai/v1");
            assert_eq!(created.model_mapping.sonnet, "sonnet-upstream");

            let mut update = provider_input("https://gateway.example.test/openai/v1/responses/");
            update.id = Some(created.id.clone());
            update.model_mapping.opus = "opus-upstream".to_string();
            let updated = upsert_provider(conn, &update)?;
            assert_eq!(updated.base_url, "https://gateway.example.test/openai/v1");
            assert_eq!(updated.model_mapping.opus, "opus-upstream");
            assert!(updated.model_mapping.sonnet.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn invalid_base_url_is_rejected_before_insert() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            assert!(upsert_provider(conn, &provider_input("http://api.example.test")).is_err());
            assert_eq!(count_providers(conn, ProviderTarget::ClaudeCode)?, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn model_cache_roundtrips_and_is_removed_with_provider() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let provider = upsert_provider(conn, &provider_input("https://api.example.test"))?;
            let models = vec!["model-a".to_string(), "model-b".to_string()];
            save_provider_model_cache(conn, &provider.id, &models, 123)?;
            let cache = get_provider_model_cache(conn, &provider.id)?.unwrap();
            assert_eq!(cache.models, models);
            assert_eq!(cache.checked_at, 123);

            delete_provider(conn, &provider.id)?;
            assert!(get_provider_model_cache(conn, &provider.id)?.is_none());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn switching_current_provider_is_visible_to_next_lookup() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut first = provider_input("https://first.example.test/v1");
            first.name = "First".into();
            let first = upsert_provider(conn, &first)?;
            let mut second = provider_input("https://second.example.test/v1");
            second.name = "Second".into();
            let second = upsert_provider(conn, &second)?;

            set_current_provider(conn, &first.id)?;
            let current = get_current_provider(conn, ProviderTarget::ClaudeCode)?.expect("current");
            assert_eq!(current.id, first.id);
            assert_eq!(current.base_url, "https://first.example.test/v1");

            set_current_provider(conn, &second.id)?;
            let current = get_current_provider(conn, ProviderTarget::ClaudeCode)?.expect("current");
            assert_eq!(current.id, second.id);
            assert_eq!(current.base_url, "https://second.example.test/v1");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn catalog_target_can_delete_current_provider() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut input = provider_input("https://pi.example.test/v1");
            input.target_app = ProviderTarget::Pi;
            let provider = upsert_provider(conn, &input)?;
            set_current_provider(conn, &provider.id)?;
            assert!(get_provider(conn, &provider.id)?.unwrap().is_current);
            delete_provider(conn, &provider.id)?;
            assert!(get_provider(conn, &provider.id)?.is_none());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn switchable_target_cannot_delete_current_provider() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut input = provider_input("https://api.example.test");
            input.target_app = ProviderTarget::ClaudeDesktop;
            let provider = upsert_provider(conn, &input)?;
            set_current_provider(conn, &provider.id)?;
            let err = delete_provider(conn, &provider.id).expect_err("current blocked");
            assert!(err.to_string().contains("不能删除当前激活的供应商"));
            assert!(get_provider(conn, &provider.id)?.is_some());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn gateway_catalog_target_can_delete_current_provider() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            crate::database::dao::settings::set_setting(
                conn,
                crate::catalog::GATEWAY_CATALOG_CODE_KEY,
                "true",
            )?;
            let provider = upsert_provider(conn, &provider_input("https://api.example.test"))?;
            set_current_provider(conn, &provider.id)?;
            delete_provider(conn, &provider.id)?;
            assert!(get_provider(conn, &provider.id)?.is_none());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn failover_fields_roundtrip_on_upsert() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut input = provider_input("https://failover.example.test/v1");
            input.failover_group = 2;
            input.failover_models = vec!["claude-opus".into(), " gpt-5 ".into()];
            let created = upsert_provider(conn, &input)?;
            assert_eq!(created.failover_group, 2);
            assert_eq!(
                created.failover_models,
                vec!["claude-opus".to_string(), "gpt-5".to_string()]
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn thinking_config_roundtrips_on_upsert() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut input = provider_input("https://thinking.example.test/v1");
            input.thinking_config = Some(crate::provider::ThinkingConfig {
                mode: Some("budget".into()),
                budget_tokens: Some(16000),
                reasoning_effort: Some("high".into()),
                prefix_thought: Some(true),
            });
            let created = upsert_provider(conn, &input)?;
            let loaded = created.thinking_config.expect("thinking_config");
            assert_eq!(loaded.mode.as_deref(), Some("budget"));
            assert_eq!(loaded.budget_tokens, Some(16000));
            assert_eq!(loaded.reasoning_effort.as_deref(), Some("high"));
            assert_eq!(loaded.prefix_thought, Some(true));
            Ok(())
        })
        .unwrap();
    }
}
