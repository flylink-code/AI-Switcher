//! SQLite storage layer.
//!
//! A single `rusqlite::Connection` is guarded by a `Mutex` (rusqlite's
//! `Connection` is `!Sync`). The connection lives inside [`Database`], which is
//! shared across commands via Tauri's managed state.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::config::paths::get_app_db_path;
use crate::error::{AppError, AppResult};

pub mod dao;
pub mod schema;
pub mod seed;

/// Wraps a mutex-guarded SQLite connection.
pub struct Database {
    conn: Mutex<Connection>,
}

/// Convenience: lock the connection, returning a `Result` of the guard.
macro_rules! lock_conn {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::Database(format!("数据库互斥锁获取失败: {e}")))?
    };
}

impl Database {
    /// Open (or create) the app database at `~/.claude-switcher/app.db` and run
    /// schema setup + migrations.
    pub fn init() -> AppResult<Self> {
        Self::init_at(get_app_db_path())
    }

    /// Open the database at an explicit path (testable).
    pub fn init_at(path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db_exists = path.exists();
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        if !db_exists {
            // `auto_vacuum` must be set before any table is created to take effect.
            conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])?;
        }
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.ensure_schema()?;
        Ok(db)
    }

    /// In-memory database for tests.
    #[cfg(test)]
    pub fn memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.ensure_schema()?;
        Ok(db)
    }

    /// Create tables and apply migrations, setting `PRAGMA user_version`.
    fn ensure_schema(&self) -> AppResult<()> {
        let conn = lock_conn!(self.conn);
        schema::create_tables(&conn)?;
        // Snapshot the DB before any data-touching migration, so we can recover if
        // a migration fails midway. Only meaningful for on-disk databases.
        let current = conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, u32>(0))?;
        if current > 0 && current < schema::SCHEMA_VERSION {
            if let Err(e) = crate::backup::backup_file(&crate::config::get_app_db_path(), 10) {
                log::warn!("迁移前数据库备份失败（继续迁移）: {e}");
            }
        }
        schema::migrate(&conn)?;
        seed::run_seed(&conn)?;
        Ok(())
    }

    /// Run a closure with a locked connection handle.
    pub fn with_conn<F, T>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(&Connection) -> AppResult<T>,
    {
        let conn = lock_conn!(self.conn);
        f(&conn)
    }

    /// Run a mutable closure while holding the database lock.
    pub fn with_conn_mut<F, T>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(&mut Connection) -> AppResult<T>,
    {
        let mut conn = lock_conn!(self.conn);
        f(&mut conn)
    }
}
