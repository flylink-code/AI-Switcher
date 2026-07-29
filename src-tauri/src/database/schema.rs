//! Schema definition and a `PRAGMA user_version` migration framework.
//!
//! Tables created here are the skeletons the later phases (P1+) will populate.
//! All timestamps are stored as unix-epoch milliseconds (INTEGER) for parity
//! with cc-switch.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Bump whenever the schema changes. Each migration step moves user_version
/// from N-1 to N.
pub const SCHEMA_VERSION: u32 = 11;

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
            model_mapping_json TEXT NOT NULL DEFAULT '{}',
            protocol_type TEXT NOT NULL DEFAULT 'anthropic',
            target_app    TEXT NOT NULL DEFAULT 'claude_code',
            notes         TEXT NOT NULL DEFAULT '',
            sort_index    INTEGER NOT NULL DEFAULT 0,
            is_current    BOOLEAN NOT NULL DEFAULT 0,
            created_at    INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // P7: sanitized provider health and model-discovery cache. No response body
    // or credential is stored here.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_health (
            provider_id TEXT PRIMARY KEY,
            status      TEXT NOT NULL,
            detail      TEXT NOT NULL DEFAULT '',
            checked_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_models (
            provider_id TEXT PRIMARY KEY,
            models_json TEXT NOT NULL DEFAULT '[]',
            checked_at  INTEGER NOT NULL
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
            enabled_codex          BOOLEAN NOT NULL DEFAULT 0,
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
            cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            usage_available BOOLEAN NOT NULL DEFAULT 0,
            duration_ms  INTEGER NOT NULL DEFAULT 0,
            target_app   TEXT,
            protocol     TEXT,
            route        TEXT,
            is_stream    BOOLEAN NOT NULL DEFAULT 0,
            error_category TEXT,
            diagnostic   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_logs_created_at ON proxy_request_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_logs_provider  ON proxy_request_logs(provider_id);
        CREATE INDEX IF NOT EXISTS idx_logs_model     ON proxy_request_logs(model);",
    )?;

    // Custom per-model pricing for cost estimation.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_pricing (
            model              TEXT PRIMARY KEY,
            provider           TEXT NOT NULL DEFAULT '',
            input_price_per_million  REAL NOT NULL DEFAULT 0,
            cache_read_price_per_million REAL NOT NULL DEFAULT 0,
            cache_write_price_per_million REAL NOT NULL DEFAULT 0,
            output_price_per_million REAL NOT NULL DEFAULT 0,
            batch_input_price_per_million REAL NOT NULL DEFAULT 0,
            batch_output_price_per_million REAL NOT NULL DEFAULT 0,
            currency           TEXT NOT NULL DEFAULT 'USD',
            source_url         TEXT NOT NULL DEFAULT '',
            effective_date     TEXT NOT NULL DEFAULT '',
            is_default         BOOLEAN NOT NULL DEFAULT 0
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
    if current < 2 {
        migrate_v1_to_v2(conn)?;
    }
    if current < 3 {
        migrate_v2_to_v3(conn)?;
    }
    if current < 4 {
        migrate_v3_to_v4(conn)?;
    }
    if current < 5 {
        migrate_v4_to_v5(conn)?;
    }
    if current < 6 {
        migrate_v5_to_v6(conn)?;
    }
    if current < 7 {
        migrate_v6_to_v7(conn)?;
    }
    if current < 8 {
        migrate_v7_to_v8(conn)?;
    }
    if current < 9 {
        migrate_v8_to_v9(conn)?;
    }
    if current < 10 {
        migrate_v9_to_v10(conn)?;
    }
    if current < 11 {
        migrate_v10_to_v11(conn)?;
    }
    Ok(())
}

fn migrate_v0_to_v1(conn: &Connection) -> AppResult<()> {
    // Tables already created idempotently by create_tables(); just stamp the version.
    set_user_version(conn, 1)
}

/// Split legacy shared providers into independent Claude Code and Claude Desktop
/// records. Retain old ids for Code and duplicate them with a desktop prefix.
fn migrate_v1_to_v2(conn: &Connection) -> AppResult<()> {
    let has_target: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'target_app';",
        [],
        |row| row.get(0),
    )?;
    if has_target == 0 {
        conn.execute_batch("ALTER TABLE providers ADD COLUMN target_app TEXT NOT NULL DEFAULT 'claude_code';")?;
    }
    conn.execute_batch(
        "UPDATE providers SET target_app = 'claude_code' WHERE target_app IS NULL OR target_app = '';
         -- Remove only the known shipped P1 presets. User-created and imported
         -- providers use other ids and are retained for the split migration.
         DELETE FROM providers WHERE id IN ('preset_0', 'preset_1', 'preset_2', 'preset_3', 'preset_4', 'preset_5');
         INSERT OR IGNORE INTO providers
            (id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index, is_current, created_at)
         SELECT 'desktop_' || id, name, base_url, api_key, model, protocol_type, 'claude_desktop',
                notes, sort_index, is_current, created_at
         FROM providers WHERE target_app = 'claude_code';
         CREATE INDEX IF NOT EXISTS idx_providers_target ON providers(target_app);",
    )?;
    set_user_version(conn, 2)
}

/// Move existing plaintext provider credentials into the OS credential store.
/// This migration is fail-closed; `user_version` is not advanced on failure.
fn migrate_v2_to_v3(conn: &Connection) -> AppResult<()> {
    crate::database::dao::migrate_plaintext_api_keys(conn)?;
    set_user_version(conn, 3)
}

fn migrate_v3_to_v4(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_health (
            provider_id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '',
            checked_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_models (
            provider_id TEXT PRIMARY KEY,
            models_json TEXT NOT NULL DEFAULT '[]',
            checked_at INTEGER NOT NULL
        );",
    )?;
    set_user_version(conn, 4)
}

/// Add non-sensitive proxy diagnostics.  Each column is independent so an
/// interrupted migration can safely be retried on the next launch.
fn migrate_v4_to_v5(conn: &Connection) -> AppResult<()> {
    for (name, definition) in [
        ("target_app", "TEXT"),
        ("protocol", "TEXT"),
        ("route", "TEXT"),
        ("is_stream", "BOOLEAN NOT NULL DEFAULT 0"),
        ("error_category", "TEXT"),
        ("diagnostic", "TEXT"),
    ] {
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('proxy_request_logs') WHERE name = ?;",
            [name],
            |row| row.get(0),
        )?;
        if exists == 0 {
            conn.execute_batch(&format!("ALTER TABLE proxy_request_logs ADD COLUMN {name} {definition};"))?;
        }
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_logs_target_created
             ON proxy_request_logs(target_app, created_at);",
    )?;
    set_user_version(conn, 5)
}

/// Canonicalize legacy provider URLs that stored a complete request endpoint.
///
/// Invalid or ambiguous legacy values remain untouched so migration never
/// destroys user data; normal provider operations will return a configuration
/// error until those records are corrected.
fn migrate_v5_to_v6(conn: &Connection) -> AppResult<()> {
    let rows = {
        let mut stmt = conn.prepare("SELECT id, base_url FROM providers;")?;
        let mapped = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };

    for (id, base_url) in rows {
        if let Ok(normalized) = crate::provider::normalize_base_url(&base_url) {
            if normalized != base_url {
                conn.execute(
                    "UPDATE providers SET base_url = ? WHERE id = ? AND base_url = ?;",
                    rusqlite::params![normalized, id, base_url],
                )?;
            }
        }
    }
    set_user_version(conn, 6)
}

/// Add optional Claude role mappings while retaining `model` as the default.
fn migrate_v6_to_v7(conn: &Connection) -> AppResult<()> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('providers') WHERE name = 'model_mapping_json';",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        conn.execute_batch(
            "ALTER TABLE providers
             ADD COLUMN model_mapping_json TEXT NOT NULL DEFAULT '{}';",
        )?;
    }
    set_user_version(conn, 7)
}

fn migrate_v7_to_v8(conn: &Connection) -> AppResult<()> {
    for name in ["cache_read_input_tokens", "cache_creation_input_tokens"] {
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('proxy_request_logs') WHERE name = ?;",
            [name],
            |row| row.get(0),
        )?;
        if exists == 0 {
            conn.execute_batch(&format!(
                "ALTER TABLE proxy_request_logs ADD COLUMN {name} INTEGER NOT NULL DEFAULT 0;"
            ))?;
        }
    }
    set_user_version(conn, 8)
}

/// Add versioned pricing dimensions without modifying any existing user rates.
fn migrate_v8_to_v9(conn: &Connection) -> AppResult<()> {
    // Some very early/incomplete databases reached v8 without the optional
    // pricing table. Create it first so this forward migration remains
    // recoverable instead of failing on `pragma_table_info`/ALTER TABLE.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_pricing (
            model TEXT PRIMARY KEY,
            provider TEXT NOT NULL DEFAULT '',
            input_price_per_million REAL NOT NULL DEFAULT 0,
            cache_read_price_per_million REAL NOT NULL DEFAULT 0,
            cache_write_price_per_million REAL NOT NULL DEFAULT 0,
            output_price_per_million REAL NOT NULL DEFAULT 0,
            batch_input_price_per_million REAL NOT NULL DEFAULT 0,
            batch_output_price_per_million REAL NOT NULL DEFAULT 0,
            currency TEXT NOT NULL DEFAULT 'USD',
            source_url TEXT NOT NULL DEFAULT '',
            effective_date TEXT NOT NULL DEFAULT '',
            is_default BOOLEAN NOT NULL DEFAULT 0
        );",
    )?;
    for (name, definition) in [
        ("provider", "TEXT NOT NULL DEFAULT ''"),
        ("cache_read_price_per_million", "REAL NOT NULL DEFAULT 0"),
        ("cache_write_price_per_million", "REAL NOT NULL DEFAULT 0"),
        ("batch_input_price_per_million", "REAL NOT NULL DEFAULT 0"),
        ("batch_output_price_per_million", "REAL NOT NULL DEFAULT 0"),
        ("source_url", "TEXT NOT NULL DEFAULT ''"),
        ("effective_date", "TEXT NOT NULL DEFAULT ''"),
        ("is_default", "BOOLEAN NOT NULL DEFAULT 0"),
    ] {
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('model_pricing') WHERE name = ?;",
            [name],
            |row| row.get(0),
        )?;
        if exists == 0 {
            conn.execute_batch(&format!("ALTER TABLE model_pricing ADD COLUMN {name} {definition};"))?;
        }
    }
    set_user_version(conn, 9)
}

/// Distinguish a real zero-token response from an upstream response that did
/// not return usage details. Existing records remain unavailable because their
/// original response body is no longer available for reliable backfill.
fn migrate_v9_to_v10(conn: &Connection) -> AppResult<()> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('proxy_request_logs') WHERE name = 'usage_available';",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        conn.execute_batch(
            "ALTER TABLE proxy_request_logs
             ADD COLUMN usage_available BOOLEAN NOT NULL DEFAULT 0;",
        )?;
    }
    set_user_version(conn, 10)
}

/// Add Codex participation without changing the two existing enable flags.
fn migrate_v10_to_v11(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            server_config TEXT NOT NULL DEFAULT '{}',
            enabled_claude_code BOOLEAN NOT NULL DEFAULT 0,
            enabled_claude_desktop BOOLEAN NOT NULL DEFAULT 0,
            enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info('mcp_servers') WHERE name = 'enabled_codex';",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        conn.execute_batch(
            "ALTER TABLE mcp_servers ADD COLUMN enabled_codex BOOLEAN NOT NULL DEFAULT 0;",
        )?;
    }
    set_user_version(conn, 11)
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
            for table in ["providers", "settings", "mcp_servers", "proxy_request_logs", "model_pricing", "provider_health", "provider_models"] {
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

    #[test]
    fn legacy_proxy_logs_migrate_before_creating_target_index() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE proxy_request_logs (
                id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                provider_id TEXT,
                provider_name TEXT,
                model TEXT,
                status_code INTEGER,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0
            );
            PRAGMA user_version = 4;",
        )
        .unwrap();

        create_tables(&conn).unwrap();
        migrate(&conn).unwrap();

        let target_column_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('proxy_request_logs') WHERE name = 'target_app';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target_column_count, 1);

        let target_index_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_logs_target_created';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target_index_count, 1);
    }

    #[test]
    fn legacy_full_provider_endpoints_are_canonicalized() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO providers
                (id, name, base_url, api_key, model, protocol_type, target_app, notes, sort_index, is_current, created_at)
             VALUES ('legacy', 'Legacy', 'https://gateway.example.test/openai/v1/chat/completions',
                     '', 'model', 'openai_chat', 'claude_code', '', 0, 0, 0);",
            [],
        )
        .unwrap();
        set_user_version(&conn, 5).unwrap();

        migrate(&conn).unwrap();

        let base_url: String = conn
            .query_row(
                "SELECT base_url FROM providers WHERE id = 'legacy';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(base_url, "https://gateway.example.test/openai/v1");
        let version: u32 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn v6_provider_rows_gain_empty_model_mapping_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL DEFAULT '',
                model TEXT,
                protocol_type TEXT NOT NULL DEFAULT 'anthropic',
                target_app TEXT NOT NULL DEFAULT 'claude_code',
                notes TEXT NOT NULL DEFAULT '',
                sort_index INTEGER NOT NULL DEFAULT 0,
                is_current BOOLEAN NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE proxy_request_logs (
                id TEXT PRIMARY KEY
            );
            INSERT INTO providers
                (id, name, base_url, model)
            VALUES ('legacy', 'Legacy', 'https://api.example.test', 'old-default');
            PRAGMA user_version = 6;",
        )
        .unwrap();

        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        let mapping: String = conn
            .query_row(
                "SELECT model_mapping_json FROM providers WHERE id = 'legacy';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mapping, "{}");
        let version: u32 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn v7_proxy_logs_gain_cache_usage_columns_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE proxy_request_logs (
                id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0
            );
            PRAGMA user_version = 7;",
        )
        .unwrap();

        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        for name in ["cache_read_input_tokens", "cache_creation_input_tokens"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('proxy_request_logs') WHERE name = ?;",
                    [name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing {name}");
        }
    }
}
