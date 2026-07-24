//! Schema definition and a `PRAGMA user_version` migration framework.
//!
//! Tables created here are the skeletons the later phases (P1+) will populate.
//! All timestamps are stored as unix-epoch milliseconds (INTEGER) for parity
//! with cc-switch.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Bump whenever the schema changes. Each migration step moves user_version
/// from N-1 to N.
pub const SCHEMA_VERSION: u32 = 1;

/// Create all tables (idempotent — uses `IF NOT EXISTS`).
pub fn create_tables(conn: &Connection) -> AppResult<()> {
    // Third-party API providers.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS providers (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            base_url      TEXT NOT NULL,
            api_key       TEXT NOT NULL DEFAULT '',
            model         TEXT,
            protocol_type TEXT NOT NULL DEFAULT 'anthropic',
            notes         TEXT NOT NULL DEFAULT '',
            sort_index    INTEGER NOT NULL DEFAULT 0,
            is_current    BOOLEAN NOT NULL DEFAULT 0,
            created_at    INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // Simple key/value store for app-level settings (UI prefs, overrides, ...).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    // Per-MCP-server row, written back to ~/.claude.json and Claude Desktop config.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_servers (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            server_config TEXT NOT NULL DEFAULT '{}',
            enabled_claude_code    BOOLEAN NOT NULL DEFAULT 0,
            enabled_claude_desktop BOOLEAN NOT NULL DEFAULT 0,
            created_at    INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // Proxy request log (populated from P2 onward by the local proxy).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS proxy_request_logs (
            id           TEXT PRIMARY KEY,
            created_at   INTEGER NOT NULL,
            provider_id  TEXT,
            provider_name TEXT,
            model        TEXT,
            status_code  INTEGER,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            duration_ms  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_logs_created_at ON proxy_request_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_logs_provider  ON proxy_request_logs(provider_id);
        CREATE INDEX IF NOT EXISTS idx_logs_model     ON proxy_request_logs(model);",
    )?;

    // Custom per-model pricing for cost estimation.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_pricing (
            model              TEXT PRIMARY KEY,
            input_price_per_million  REAL NOT NULL DEFAULT 0,
            output_price_per_million REAL NOT NULL DEFAULT 0,
            currency           TEXT NOT NULL DEFAULT 'USD'
        );",
    )?;

    Ok(())
}

/// Apply forward-only migrations up to [`SCHEMA_VERSION`].
///
/// On a fresh database `user_version` is 0, so all `migrate_v0_to_v1` steps run.
/// Databases newer than this build are rejected to avoid silent data loss.
pub fn migrate(conn: &Connection) -> AppResult<()> {
    let current = conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, u32>(0))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::Database(format!(
            "数据库版本 (v{current}) 比当前程序支持版本 (v{SCHEMA_VERSION}) 更新，请升级程序。"
        )));
    }
    if current < 1 {
        migrate_v0_to_v1(conn)?;
    }
    // Future: if current < 2 { migrate_v1_to_v2(conn)?; }
    Ok(())
}

fn migrate_v0_to_v1(conn: &Connection) -> AppResult<()> {
    // Tables already created idempotently by create_tables(); just stamp the version.
    set_user_version(conn, 1)
}

pub fn set_user_version(conn: &Connection, version: u32) -> AppResult<()> {
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn schema_is_created_and_versioned() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let v: u32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0))?;
            assert_eq!(v, SCHEMA_VERSION);
            // Tables exist.
            for table in ["providers", "settings", "mcp_servers", "proxy_request_logs", "model_pricing"] {
                let n: i64 = conn.query_row(
                    &format!("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='{table}';"),
                    [],
                    |r| r.get(0),
                )?;
                assert_eq!(n, 1, "missing table {table}");
            }
            Ok(())
        })
        .unwrap();
    }
}
