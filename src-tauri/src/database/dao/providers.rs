//! Provider-row queries and mutations.

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::provider::{ProtocolType, Provider, ProviderInput};

/// Number of rows in `providers`.
pub fn count_providers(conn: &Connection) -> AppResult<i64> {
    let n: i64 = conn.query_row("SELECT count(*) FROM providers;", [], |r| r.get(0))?;
    Ok(n)
}

/// List all providers ordered by `sort_index` then `created_at`.
pub fn list_providers(conn: &Connection) -> AppResult<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, notes, sort_index,
                is_current, created_at
         FROM providers ORDER BY sort_index ASC, created_at ASC;",
    )?;
    let rows = stmt.query_map([], row_to_provider)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Fetch a single provider by id.
pub fn get_provider(conn: &Connection, id: &str) -> AppResult<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, notes, sort_index,
                is_current, created_at
         FROM providers WHERE id = ?;",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_provider(row)?)),
        None => Ok(None),
    }
}

/// The currently-active provider, if any.
pub fn get_current_provider(conn: &Connection) -> AppResult<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, protocol_type, notes, sort_index,
                is_current, created_at
         FROM providers WHERE is_current = 1 LIMIT 1;",
    )?;
    let mut rows = stmt.query([])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_provider(row)?)),
        None => Ok(None),
    }
}

/// Insert or update a provider. Returns the persisted provider.
///
/// On create (no `id`), assigns a new id and appends it after the highest sort_index.
pub fn upsert_provider(conn: &Connection, input: &ProviderInput) -> AppResult<Provider> {
    // Validate required fields.
    if input.name.trim().is_empty() {
        return Err(AppError::Config("供应商名称不能为空".to_string()));
    }
    if input.base_url.trim().is_empty() {
        return Err(AppError::Config("API 地址不能为空".to_string()));
    }

    let now = Utc::now().timestamp_millis();

    if let Some(id) = input.id.as_ref() {
        // Update existing.
        let changed = conn.execute(
            "UPDATE providers SET
                name = ?, base_url = ?, api_key = ?, model = ?,
                protocol_type = ?, notes = ?
             WHERE id = ?;",
            params![
                input.name,
                input.base_url,
                input.api_key,
                input.model,
                input.protocol_type.as_str(),
                input.notes,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::Config(format!("供应商不存在: {id}")));
        }
        return get_provider(conn, id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")));
    }

    // Create new.
    let id = format!("p_{}", uuid_v8());
    let next_sort = next_sort_index(conn)?;
    conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, notes, sort_index, is_current, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?);",
        params![
            id,
            input.name,
            input.base_url,
            input.api_key,
            input.model,
            input.protocol_type.as_str(),
            input.notes,
            next_sort,
            now,
        ],
    )?;
    get_provider(conn, &id)?.ok_or_else(|| AppError::Config("插入后未能读回供应商".to_string()))
}

/// Delete a provider by id. The active provider cannot be deleted.
pub fn delete_provider(conn: &Connection, id: &str) -> AppResult<()> {
    let current = get_current_provider(conn)?;
    if current.as_ref().is_some_and(|p| p.id == id) {
        return Err(AppError::Config("不能删除当前激活的供应商".to_string()));
    }
    let changed = conn.execute("DELETE FROM providers WHERE id = ?;", params![id])?;
    if changed == 0 {
        return Err(AppError::Config(format!("供应商不存在: {id}")));
    }
    Ok(())
}

/// Mark `id` as the sole active provider (transactional).
pub fn set_current_provider(conn: &Connection, id: &str) -> AppResult<()> {
    ensure_exists(conn, id)?;
    conn.execute("UPDATE providers SET is_current = 0;", [])?;
    let changed =
        conn.execute("UPDATE providers SET is_current = 1 WHERE id = ?;", params![id])?;
    if changed == 0 {
        return Err(AppError::Config(format!("供应商不存在: {id}")));
    }
    Ok(())
}

/// Clear the active marker (no provider active — e.g. official login mode).
pub fn clear_current_provider(conn: &Connection) -> AppResult<()> {
    conn.execute("UPDATE providers SET is_current = 0;", [])?;
    Ok(())
}

/// Reorder providers to match the given id sequence (ascending sort_index).
/// Any ids not present keep their relative order after the listed ones.
pub fn reorder_providers(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    for (idx, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE providers SET sort_index = ? WHERE id = ?;",
            params![idx as i64, id],
        )?;
    }
    Ok(())
}

/// Insert a provider directly from a fully-formed struct (used by seeding and
/// live import). Idempotent on `id`.
pub fn insert_provider_direct(conn: &Connection, p: &Provider) -> AppResult<()> {
    conn.execute(
        "INSERT INTO providers
            (id, name, base_url, api_key, model, protocol_type, notes, sort_index, is_current, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name, base_url=excluded.base_url, api_key=excluded.api_key,
            model=excluded.model, protocol_type=excluded.protocol_type, notes=excluded.notes,
            sort_index=excluded.sort_index;",
        params![
            p.id,
            p.name,
            p.base_url,
            p.api_key,
            p.model,
            p.protocol_type.as_str(),
            p.notes,
            p.sort_index,
            p.is_current as i64,
            p.created_at,
        ],
    )?;
    Ok(())
}

fn ensure_exists(conn: &Connection, id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM providers WHERE id = ?;",
        params![id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(AppError::Config(format!("供应商不存在: {id}")));
    }
    Ok(())
}

fn next_sort_index(conn: &Connection) -> AppResult<i64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(sort_index) FROM providers;",
        [],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(-1) + 1)
}

fn row_to_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    let protocol_str: String = row.get(5)?;
    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        api_key: row.get(3)?,
        model: row.get(4)?,
        protocol_type: ProtocolType::from_str_lossy(&protocol_str),
        notes: row.get(6)?,
        sort_index: row.get(7)?,
        is_current: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

/// 8-char hex id derived from a UUIDv4 (matches cc-proxy's convention).
fn uuid_v8() -> String {
    let mut buf = uuid::Uuid::encode_buffer();
    let s = uuid::Uuid::new_v4().simple().encode_upper(&mut buf);
    s[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn input(name: &str, url: &str) -> ProviderInput {
        ProviderInput {
            id: None,
            name: name.to_string(),
            base_url: url.to_string(),
            api_key: "sk-test".to_string(),
            model: "m1".to_string(),
            protocol_type: ProtocolType::Anthropic,
            notes: String::new(),
        }
    }

    #[test]
    fn crud_and_current_works() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            assert_eq!(count_providers(conn)?, 0);

            let a = upsert_provider(conn, &input("A", "https://a"))?;
            let b = upsert_provider(conn, &input("B", "https://b"))?;
            assert_eq!(count_providers(conn)?, 2);
            assert_ne!(a.id, b.id);

            // current is the active one.
            set_current_provider(conn, &a.id)?;
            assert_eq!(get_current_provider(conn)?.unwrap().id, a.id);
            set_current_provider(conn, &b.id)?;
            assert_eq!(get_current_provider(conn)?.unwrap().id, b.id);

            // cannot delete current.
            assert!(delete_provider(conn, &b.id).is_err());
            // can delete non-current.
            delete_provider(conn, &a.id)?;
            assert_eq!(count_providers(conn)?, 1);

            // update path.
            let mut upd = input("A", "https://a");
            upd.id = Some(a.id.clone());
            assert!(upsert_provider(conn, &upd).is_err()); // a was deleted
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn validation_rejects_empty_name() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut bad = input("A", "https://a");
            bad.name = "  ".to_string();
            assert!(upsert_provider(conn, &bad).is_err());
            Ok(())
        })
        .unwrap();
    }
}
