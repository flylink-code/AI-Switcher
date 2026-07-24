//! Key/value settings table accessors.
//!
//! Foundational — consumed from P1+ (settings persistence, directory overrides).

use rusqlite::params;

use crate::error::AppResult;

/// Read a setting value, or `None` when absent.
#[allow(dead_code)]
pub fn get_setting(conn: &rusqlite::Connection, key: &str) -> AppResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?;")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

/// Insert or update a setting.
#[allow(dead_code)]
pub fn set_setting(conn: &rusqlite::Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        params![key, value],
    )?;
    Ok(())
}
