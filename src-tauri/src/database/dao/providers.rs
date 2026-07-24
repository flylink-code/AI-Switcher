//! Provider-row queries. Real CRUD + presets arrive in P1.

use rusqlite::Connection;

use crate::error::AppResult;

/// Number of rows in `providers`.
pub fn count_providers(conn: &Connection) -> AppResult<i64> {
    let n: i64 = conn.query_row("SELECT count(*) FROM providers;", [], |r| r.get(0))?;
    Ok(n)
}
