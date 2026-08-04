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
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )?;
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

    /// Flush WAL before hard process exit (Windows updater `std::process::exit`).
    pub fn checkpoint_wal(&self) -> AppResult<()> {
        let conn = lock_conn!(self.conn);
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Close the live DB file, replace it with `new_db`, reopen, and rematerialize
    /// any plaintext API keys into the OS keyring.
    ///
    /// Replacing `app.db` while a connection is open (especially on Linux) can leave
    /// a malformed database. Callers must still restart for proxy/UI consistency.
    pub fn replace_on_disk_and_reopen(&self, new_db: &std::path::Path) -> AppResult<()> {
        if !new_db.is_file() {
            return Err(AppError::Config(format!(
                "恢复数据库不存在: {}",
                new_db.display()
            )));
        }

        // Validate the staged file before touching the live DB.
        {
            let probe = Connection::open(new_db)?;
            let integrity: String = probe
                .query_row("PRAGMA integrity_check;", [], |row| row.get(0))
                .unwrap_or_else(|_| "failed".to_string());
            if integrity != "ok" {
                return Err(AppError::Database(format!(
                    "归档内数据库损坏（integrity_check={integrity}），已取消导入"
                )));
            }
        }

        let path = get_app_db_path();
        let mut guard = lock_conn!(self.conn);
        let _ = guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        // Drop the file handle so Linux can replace the path safely.
        *guard = Connection::open_in_memory()?;

        let reopen_live = |guard: &mut Connection| -> AppResult<()> {
            let conn = Connection::open(&path)?;
            conn.execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
            )?;
            *guard = conn;
            Ok(())
        };

        let swap_result = (|| -> AppResult<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let wal = PathBuf::from(format!("{}-wal", path.display()));
            let shm = PathBuf::from(format!("{}-shm", path.display()));
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);

            let incoming = PathBuf::from(format!("{}.incoming", path.display()));
            let aside = PathBuf::from(format!(
                "{}.pre-restore-{}",
                path.display(),
                chrono::Utc::now().format("%Y%m%d_%H%M%S_%f")
            ));
            std::fs::copy(new_db, &incoming)?;
            if path.exists() {
                std::fs::rename(&path, &aside)?;
            }
            if let Err(error) = std::fs::rename(&incoming, &path) {
                // Best-effort rollback of the live path.
                if aside.exists() {
                    let _ = std::fs::rename(&aside, &path);
                }
                let _ = std::fs::remove_file(&incoming);
                return Err(AppError::Io(format!("替换数据库失败: {error}")));
            }
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);
            Ok(())
        })();

        if let Err(error) = swap_result {
            let _ = reopen_live(&mut guard);
            return Err(error);
        }

        reopen_live(&mut guard)?;
        // Schema may already be current; still migrate plaintext keys from credential-inclusive archives.
        schema::create_tables(&guard)?;
        schema::migrate(&guard)?;
        if let Err(error) = dao::migrate_plaintext_api_keys(&guard) {
            log::warn!("导入后 API Key 迁入系统凭据失败: {error}");
            return Err(AppError::Config(format!(
                "数据库已导入，但 API Key 未能写入系统凭据库: {error}"
            )));
        }
        seed::run_seed(&guard)?;
        Ok(())
    }
}
