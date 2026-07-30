//! Provider-row queries and mutations, scoped to one Claude application.

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::provider::{
    normalize_provider_base_url, normalized_model_mapping, validate_target_protocol, ClaudeModelMapping,
    ProtocolType, Provider, ProviderInput, ProviderTarget,
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
                model_mapping_json, model_context_window
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
                model_mapping_json, model_context_window
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
                model_mapping_json, model_context_window
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
    if input.base_url.trim().is_empty() {
        return Err(AppError::Config("API 地址不能为空".to_string()));
    }
    if input.model.trim().is_empty() {
        return Err(AppError::Config("默认模型不能为空".to_string()));
    }
    validate_target_protocol(input.target_app, input.protocol_type)?;
    let base_url = normalize_provider_base_url(input.target_app, input.protocol_type, &input.base_url)?;
    let model_mapping_json = serde_json::to_string(&normalized_model_mapping(
        input.target_app,
        input.model_mapping.clone(),
    ))?;

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
        let api_key_col = if input.clear_api_key {
            String::new()
        } else if !input.api_key.trim().is_empty() {
            secrets::store_key(id, input.api_key.trim())?;
            secrets::keyring_ref(id)
        } else {
            existing.api_key.clone()
        };
        conn.execute(
            "UPDATE providers SET name = ?, base_url = ?, api_key = ?, model = ?,
                protocol_type = ?, notes = ?, model_mapping_json = ?, model_context_window = ? WHERE id = ?;",
            params![
                input.name, base_url, api_key_col, input.model,
                input.protocol_type.as_str(), input.notes, model_mapping_json,
                input.model_context_window, id,
            ],
        )?;
        if input.clear_api_key {
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
    let api_key_col = if !input.api_key.trim().is_empty() {
        secrets::store_key(&id, input.api_key.trim())?;
        secrets::keyring_ref(&id)
    } else {
        String::new()
    };
    if let Err(error) = conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index, is_current, created_at, model_mapping_json, model_context_window)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?);",
        params![
            id, input.name, base_url, api_key_col, input.model,
            input.protocol_type.as_str(), input.target_app.as_str(), input.notes, next_sort, now,
            model_mapping_json, input.model_context_window,
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

/// Delete a provider by id. The active provider for its target cannot be deleted.
/// Also removes the stored credential from the OS keyring (best-effort).
pub fn delete_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let provider = get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    if provider.is_current {
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
    Ok(Provider {
        api_key_set: !api_key.is_empty(),
        api_key,
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        model: row.get(4)?,
        model_mapping,
        protocol_type: ProtocolType::from_str_lossy(&protocol),
        target_app: ProviderTarget::from_str_lossy(&target),
        notes: row.get(7)?,
        sort_index: row.get(8)?,
        is_current: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
        health_status: row.get(11)?,
        health_checked_at: row.get(12)?,
        model_context_window: row.get(14)?,
    })
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
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
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
}
