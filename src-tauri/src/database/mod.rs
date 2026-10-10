//! SQLite storage layer.
//!
//! Writes go through a mutex-guarded `rusqlite::Connection`. Heavy analytics
//! reads use [`Database::with_read_conn`] (a pooled WAL reader, cap 4) so usage
//! dashboard scans do not stall gateway log inserts.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::config::paths::get_app_db_path;
use crate::error::{AppError, AppResult};

pub mod dao;
pub mod schema;
pub mod seed;

/// Wraps a mutex-guarded SQLite connection.
///
/// Analytics reads use [`Self::with_read_conn`] so they do not hold this mutex
/// for seconds (usage period switches were stalling gateway log inserts).
const READ_POOL_CAP: usize = 4;

pub struct Database {
    conn: Mutex<Connection>,
    /// On-disk path. `None` for in-memory databases (tests).
    path: Option<PathBuf>,
    read_pool: Mutex<Vec<Connection>>,
    pub(crate) gateway_upstream_limiter: crate::gateway::upstream_limits::UpstreamLimiter,
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
            path: Some(path),
            read_pool: Mutex::new(Vec::with_capacity(READ_POOL_CAP)),
            gateway_upstream_limiter: crate::gateway::upstream_limits::UpstreamLimiter::new(),
        };
        db.ensure_schema()?;
        db.with_read_conn(|conn| {
            crate::gateway::upstream_limits::load_into_limiter(conn, &db.gateway_upstream_limiter)
        })?;
        Ok(db)
    }

    /// In-memory database for tests.
    #[cfg(test)]
    pub fn memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
            path: None,
            read_pool: Mutex::new(Vec::new()),
            gateway_upstream_limiter: crate::gateway::upstream_limits::UpstreamLimiter::new(),
        };
        db.ensure_schema()?;
        db.with_read_conn(|conn| {
            crate::gateway::upstream_limits::load_into_limiter(conn, &db.gateway_upstream_limiter)
        })?;
        Ok(db)
    }

    /// Create tables and apply migrations, setting `PRAGMA user_version`.
    fn ensure_schema(&self) -> AppResult<()> {
        let conn = lock_conn!(self.conn);
        let current = conn.query_row("PRAGMA user_version;", [], |r| r.get::<_, u32>(0))?;
        if current > schema::SCHEMA_VERSION {
            return Err(AppError::Database(format!(
                "数据库版本 {current} 高于当前支持的 {}，已停止打开",
                schema::SCHEMA_VERSION
            )));
        }
        // 先备份，再改表；SQLite backup API 会包含 WAL 中已提交的数据。
        if current > 0 && current < schema::SCHEMA_VERSION {
            if let Some(path) = self.path.as_ref() {
                let suffix = if current == 33 {
                    ".v33.bak".to_string()
                } else {
                    format!(".v{current}.bak")
                };
                let mut backup_path = path.as_os_str().to_os_string();
                backup_path.push(suffix);
                let backup_path = PathBuf::from(backup_path);
                // 已有备份属于此前的迁移尝试，不能静默覆盖。
                if !backup_path.exists() {
                    let staging = tempfile::NamedTempFile::new_in(
                        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
                    )?;
                    {
                        let mut destination = Connection::open(staging.path())?;
                        let backup = rusqlite::backup::Backup::new(&conn, &mut destination)?;
                        backup.run_to_completion(
                            100,
                            std::time::Duration::from_millis(5),
                            None,
                        )?;
                    }
                    staging.persist_noclobber(&backup_path).map_err(|error| {
                        AppError::Io(format!("迁移前数据库备份失败，已停止升级: {error}"))
                    })?;
                }
                let probe = Connection::open(&backup_path)?;
                let version: u32 = probe.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
                let integrity: String = probe.query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
                if version != current || integrity != "ok" {
                    return Err(AppError::Database(format!(
                        "迁移备份无效（version={version}, integrity={integrity}），已停止升级: {}",
                        backup_path.display()
                    )));
                }
            }
        }
        schema::create_tables(&conn)?;
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

    /// Read-only connection that does not take the write mutex.
    ///
    /// WAL lets this overlap with gateway/proxy inserts. In-memory databases
    /// have no file to open, so they fall back to [`Self::with_conn`].
    pub fn with_read_conn<F, T>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(&Connection) -> AppResult<T>,
    {
        let Some(path) = self.path.as_ref() else {
            return self.with_conn(f);
        };
        let conn = self.take_read_conn(path)?;
        let result = f(&conn);
        self.return_read_conn(conn);
        result
    }

    fn take_read_conn(&self, path: &std::path::Path) -> AppResult<Connection> {
        if let Ok(mut pool) = self.read_pool.lock() {
            if let Some(conn) = pool.pop() {
                return Ok(conn);
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA query_only = ON; PRAGMA busy_timeout = 5000;")?;
        Ok(conn)
    }

    fn return_read_conn(&self, conn: Connection) {
        if let Ok(mut pool) = self.read_pool.lock() {
            if pool.len() < READ_POOL_CAP {
                pool.push(conn);
            }
        }
    }

    fn drain_read_pool(&self) {
        if let Ok(mut pool) = self.read_pool.lock() {
            pool.clear();
        }
    }

    #[cfg(test)]
    fn read_pool_len(&self) -> usize {
        self.read_pool.lock().map(|pool| pool.len()).unwrap_or(0)
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
        self.drain_read_pool();
        let conn = lock_conn!(self.conn);
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// 导出可由旧版打开的 Schema 33 数据库，不改变运行中的资料库。
    pub fn export_rollback_v34(&self, destination: &std::path::Path) -> AppResult<()> {
        if destination.exists() {
            return Err(AppError::Config("回滚导出路径已存在，禁止覆盖".into()));
        }
        let parent = destination.parent().filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let staging = tempfile::NamedTempFile::new_in(parent)?;
        {
            let source = lock_conn!(self.conn);
            let version: u32 = source.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
            if (version != 34 && version != 35 && version != 36) || !dao::gateway::is_v34_migration_done(&source) {
                return Err(AppError::Config("资料库没有有效的 Schema 34 迁移快照，无法回滚".into()));
            }
            let mut copy = Connection::open(staging.path())?;
            {
                let backup = rusqlite::backup::Backup::new(&source, &mut copy)?;
                backup.run_to_completion(100, std::time::Duration::from_millis(5), None)?;
            }
            copy.execute_batch("PRAGMA foreign_keys = ON;")?;
            dao::gateway::rollback_v34(&copy)?;
            let integrity: String = copy.query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
            let version: u32 = copy.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
            if integrity != "ok" || version != 33 {
                return Err(AppError::Database("回滚导出校验失败，未保存产物".into()));
            }
            copy.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")?;
        }
        staging.persist_noclobber(destination).map_err(|error| {
            AppError::Io(format!("保存回滚数据库失败: {error}"))
        })?;
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

        let path = self.path.as_ref().ok_or_else(|| {
            AppError::Config("内存资料库不能替换磁盘文件".into())
        })?.clone();
        self.drain_read_pool();
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
        crate::gateway::upstream_limits::load_into_limiter(&guard, &self.gateway_upstream_limiter)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn read_conn_does_not_wait_for_write_mutex() {
        let path = std::env::temp_dir().join(format!(
            "aisw-read-conn-{}-{}.db",
            std::process::id(),
            UtcStamp::now()
        ));
        let _ = std::fs::remove_file(&path);
        let db = Arc::new(Database::init_at(path.clone()).expect("init db"));
        let writer = Arc::clone(&db);
        let (locked_tx, locked_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            writer
                .with_conn(|_conn| {
                    locked_tx.send(()).unwrap();
                    thread::sleep(Duration::from_millis(250));
                    Ok(())
                })
                .unwrap();
        });
        locked_rx.recv().expect("write lock held");
        let started = Instant::now();
        db.with_read_conn(|conn| {
            let one: i32 = conn.query_row("SELECT 1;", [], |row| row.get(0))?;
            assert_eq!(one, 1);
            Ok(())
        })
        .expect("read conn");
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "read conn blocked on write mutex: {:?}",
            started.elapsed()
        );
        handle.join().unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn concurrent_read_conn_does_not_grow_past_pool_cap() {
        let path = std::env::temp_dir().join(format!(
            "aisw-read-pool-{}-{}.db",
            std::process::id(),
            UtcStamp::now()
        ));
        let _ = std::fs::remove_file(&path);
        let db = Arc::new(Database::init_at(path.clone()).expect("init db"));
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let db = Arc::clone(&db);
                thread::spawn(move || {
                    db.with_read_conn(|conn| {
                        let one: i32 = conn.query_row("SELECT 1;", [], |row| row.get(0))?;
                        assert_eq!(one, 1);
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert!(
            db.read_pool_len() <= READ_POOL_CAP,
            "read pool grew to {}",
            db.read_pool_len()
        );
        db.checkpoint_wal().expect("checkpoint");
        assert_eq!(db.read_pool_len(), 0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn replace_database_reads_the_new_file() {
        let stamp = UtcStamp::now();
        let path = std::env::temp_dir().join(format!(
            "aisw-replace-live-{}-{}.db",
            std::process::id(),
            stamp
        ));
        let incoming = std::env::temp_dir().join(format!(
            "aisw-replace-new-{}-{}.db",
            std::process::id(),
            stamp
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&incoming);
        let db = Database::init_at(path.clone()).expect("init live");
        db.with_conn(|conn| {
            crate::database::dao::settings::set_setting(conn, "pool_probe", "old")
        })
        .unwrap();
        db.with_read_conn(|conn| {
            let value = crate::database::dao::settings::get_setting(conn, "pool_probe")?;
            assert_eq!(value.as_deref(), Some("old"));
            Ok(())
        })
        .unwrap();

        let incoming_db = Database::init_at(incoming.clone()).expect("init incoming");
        incoming_db
            .with_conn(|conn| {
                crate::database::dao::settings::set_setting(conn, "pool_probe", "new")
            })
            .unwrap();
        drop(incoming_db);

        db.replace_on_disk_and_reopen(&incoming).expect("replace actual instance path");
        db.with_read_conn(|conn| {
            let value = crate::database::dao::settings::get_setting(conn, "pool_probe")?;
            assert_eq!(value.as_deref(), Some("new"));
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&incoming);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(format!("{}-wal", incoming.display()));
        let _ = std::fs::remove_file(format!("{}-shm", incoming.display()));
    }

    #[test]
    fn migration_backup_includes_wal_at_actual_database_path() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("isolated.db");
        let db = Database::init_at(path.clone()).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("PRAGMA wal_autocheckpoint = 0; PRAGMA user_version = 33;")?;
            dao::settings::set_setting(conn, "wal_only_probe", "committed")
        })
        .unwrap();
        db.ensure_schema().unwrap();
        let backup = Connection::open(home.path().join("isolated.db.v33.bak")).unwrap();
        let version: u32 = backup.query_row("PRAGMA user_version;", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 33);
        assert_eq!(
            dao::settings::get_setting(&backup, "wal_only_probe").unwrap().as_deref(),
            Some("committed")
        );
    }

    #[test]
    fn invalid_existing_migration_backup_blocks_upgrade() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("isolated.db");
        let db = Database::init_at(path.clone()).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("PRAGMA user_version = 33;")?;
            Ok(())
        })
        .unwrap();
        let backup_path = home.path().join("isolated.db.v33.bak");
        std::fs::write(&backup_path, b"not a SQLite database").unwrap();
        assert!(db.ensure_schema().is_err());
        db.with_conn(|conn| {
            let version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
            assert_eq!(version, 33);
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(backup_path).unwrap(), b"not a SQLite database");
    }

    #[test]
    fn rollback_export_does_not_downgrade_live_database_or_overwrite_files() {
        let home = tempfile::tempdir().unwrap();
        let db = Database::init_at(home.path().join("live.db")).unwrap();
        let output = home.path().join("rollback.db");
        db.export_rollback_v34(&output).unwrap();
        db.with_conn(|conn| {
            let version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
            assert_eq!(version, schema::SCHEMA_VERSION);
            Ok(())
        }).unwrap();
        let restored = Connection::open(&output).unwrap();
        assert_eq!(restored.query_row("PRAGMA user_version;", [], |row| row.get::<_, u32>(0)).unwrap(), 33);
        drop(restored);
        let original = std::fs::read(&output).unwrap();
        assert!(db.export_rollback_v34(&output).is_err());
        assert_eq!(std::fs::read(output).unwrap(), original);
    }

    struct UtcStamp;
    impl UtcStamp {
        fn now() -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0)
        }
    }
}

