//! Provider-row queries and mutations, scoped to one Claude application.

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::provider::{ProtocolType, Provider, ProviderInput, ProviderTarget};

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
                is_current, created_at
         FROM providers WHERE target_app = ? ORDER BY sort_index ASC, created_at ASC;",
    )?;
    let rows = stmt.query_map(params![target.as_str()], row_to_provider)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Fetch a provider by id.
pub fn get_provider(conn: &Connection, id: &str) -> AppResult<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index,
                is_current, created_at
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
                is_current, created_at
         FROM providers WHERE target_app = ? AND is_current = 1 LIMIT 1;",
    )?;
    let mut rows = stmt.query(params![target.as_str()])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_provider(row)?)),
        None => Ok(None),
    }
}

/// Insert or update a provider. Providers are always scoped by their input target.
pub fn upsert_provider(conn: &Connection, input: &ProviderInput) -> AppResult<Provider> {
    if input.name.trim().is_empty() {
        return Err(AppError::Config("供应商名称不能为空".to_string()));
    }
    if input.base_url.trim().is_empty() {
        return Err(AppError::Config("API 地址不能为空".to_string()));
    }

    let now = Utc::now().timestamp_millis();
    if let Some(id) = input.id.as_ref() {
        let existing = get_provider(conn, id)?
            .ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
        if existing.target_app != input.target_app {
            return Err(AppError::Config("不能跨应用更新供应商".to_string()));
        }
        conn.execute(
            "UPDATE providers SET name = ?, base_url = ?, api_key = ?, model = ?,
                protocol_type = ?, notes = ? WHERE id = ?;",
            params![
                input.name, input.base_url, input.api_key, input.model,
                input.protocol_type.as_str(), input.notes, id,
            ],
        )?;
        return get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")));
    }

    let id = format!("p_{}", uuid_v8());
    let next_sort = next_sort_index(conn, input.target_app)?;
    conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index, is_current, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?);",
        params![
            id, input.name, input.base_url, input.api_key, input.model,
            input.protocol_type.as_str(), input.target_app.as_str(), input.notes, next_sort, now,
        ],
    )?;
    get_provider(conn, &id)?.ok_or_else(|| AppError::Config("插入后未能读回供应商".to_string()))
}

/// Delete a provider by id. The active provider for its target cannot be deleted.
pub fn delete_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let provider = get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    if provider.is_current {
        return Err(AppError::Config("不能删除当前激活的供应商".to_string()));
    }
    conn.execute("DELETE FROM providers WHERE id = ?;", params![id])?;
    Ok(())
}

/// Mark `id` as the sole active provider for its target application.
pub fn set_current_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let provider = get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))?;
    conn.execute(
        "UPDATE providers SET is_current = 0 WHERE target_app = ?;",
        params![provider.target_app.as_str()],
    )?;
    conn.execute("UPDATE providers SET is_current = 1 WHERE id = ?;", params![id])?;
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

/// Insert an already-formed provider (used by migration/import).
pub fn insert_provider_direct(conn: &Connection, provider: &Provider) -> AppResult<()> {
    conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index, is_current, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name, base_url = excluded.base_url, api_key = excluded.api_key,
            model = excluded.model, protocol_type = excluded.protocol_type,
            target_app = excluded.target_app, notes = excluded.notes,
            sort_index = excluded.sort_index, is_current = excluded.is_current;",
        params![
            provider.id, provider.name, provider.base_url, provider.api_key, provider.model,
            provider.protocol_type.as_str(), provider.target_app.as_str(), provider.notes,
            provider.sort_index, provider.is_current as i64, provider.created_at,
        ],
    )?;
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
    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        api_key: row.get(3)?,
        model: row.get(4)?,
        protocol_type: ProtocolType::from_str_lossy(&protocol),
        target_app: ProviderTarget::from_str_lossy(&target),
        notes: row.get(7)?,
        sort_index: row.get(8)?,
        is_current: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
    })
}

fn uuid_v8() -> String {
    let mut buf = uuid::Uuid::encode_buffer();
    let value = uuid::Uuid::new_v4().simple().encode_upper(&mut buf);
    value[..8].to_string()
}
