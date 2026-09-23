//! Gemini 3 thought_signature 独立持久存储与缓存（L1 内存 + L2 SQLite）。
//!
//! - 架构设计：
//!   - 运行读仅访问 L1 内存（工具 1024 / 会话 256 / 轮次 2048），接受 L1 容量边界，不做运行时同步 L2 查询，消除热路径磁盘 I/O 阻塞；
//!   - 节流 Touch：读取时 L1 命中且距上次落盘超过节流间隔（默认 60s）时，非阻塞向后台提交 Touch 操作更新 SQLite `updated_at`，保证重启后滑动 TTL 不丢失；
//!   - 启动阶段：在 init 中有界预热（至多填满 L1 容量限额的有效近期记录），过滤空签名、sentinel 哨兵及负索引，可由应用 setup 异步提前触发；
//!   - 异步批写：后台有界队列（默认 2048），热路径通过非阻塞 `try_send` 提交，后台按批次落盘至独立 SQLite（`thought-signatures.db`）；
//!   - 退出与排空：shutdown 时释放 sender，worker 读到通道断开后自然排空剩余待写队列，提交事务后安全退出；
//!   - 容错与恢复：准确识别 `DatabaseCorrupt` 及 `NotADatabase` 损坏；对 busy/permission 不误隔离直接平滑降级纯内存；
//!   - 隔离文件保护：若 rename 失败直接安全降级，禁止删除用户原文件；历史隔离归档上限 5 份；
//!   - 安全脱敏：日志不打印签名、键名及 SQLite 内部错误明细，避免泄漏敏感路径。
//!
//! 注：只持久化三类 key 与签名本身及更新时间，不保存正文、提示词或 token 计数。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_L1_TOOL_CAP: usize = 1024;
pub const DEFAULT_L1_SESSION_CAP: usize = 256;
pub const DEFAULT_L1_SESSION_INDEX_CAP: usize = 2048;
pub const DEFAULT_L2_CAPACITY: usize = 10_000;
pub const DEFAULT_TTL_SECS: i64 = 15 * 86400; // 15 天
pub const DEFAULT_QUEUE_BOUND: usize = 2048;
pub const DEFAULT_BUSY_TIMEOUT_MS: u32 = 250;
pub const TOUCH_THROTTLE_SECS: i64 = 60; // 60 秒节流 Touch
const MAX_QUARANTINE_FILES: usize = 5;

#[derive(Debug, Clone)]
pub(crate) struct StoreConfig {
    pub(crate) path: PathBuf,
    pub(crate) ttl_secs: i64,
    pub(crate) max_l2_capacity: usize,
    pub(crate) max_l1_tool: usize,
    pub(crate) max_l1_session: usize,
    pub(crate) max_l1_session_index: usize,
    pub(crate) queue_bound: usize,
    pub(crate) busy_timeout_ms: u32,
    pub(crate) touch_throttle_secs: i64,
}

impl Default for StoreConfig {
    fn default() -> Self {
        #[cfg(test)]
        let (path, max_l2_capacity) = (PathBuf::new(), 0);
        #[cfg(not(test))]
        let (path, max_l2_capacity) = (
            crate::config::get_app_config_dir().join("thought-signatures.db"),
            DEFAULT_L2_CAPACITY,
        );

        Self {
            path,
            ttl_secs: DEFAULT_TTL_SECS,
            max_l2_capacity,
            max_l1_tool: DEFAULT_L1_TOOL_CAP,
            max_l1_session: DEFAULT_L1_SESSION_CAP,
            max_l1_session_index: DEFAULT_L1_SESSION_INDEX_CAP,
            queue_bound: DEFAULT_QUEUE_BOUND,
            busy_timeout_ms: DEFAULT_BUSY_TIMEOUT_MS,
            touch_throttle_secs: TOUCH_THROTTLE_SECS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum SignatureKind {
    Tool = 0,
    Session = 1,
    SessionIndex = 2,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SignatureKey {
    Tool(String),
    Session(String),
    SessionIndex(String, usize),
}

impl SignatureKey {
    pub(crate) fn kind(&self) -> SignatureKind {
        match self {
            Self::Tool(_) => SignatureKind::Tool,
            Self::Session(_) => SignatureKind::Session,
            Self::SessionIndex(_, _) => SignatureKind::SessionIndex,
        }
    }

    pub(crate) fn primary_key(&self) -> &str {
        match self {
            Self::Tool(id) => id.as_str(),
            Self::Session(sess) => sess.as_str(),
            Self::SessionIndex(sess, _) => sess.as_str(),
        }
    }

    pub(crate) fn secondary_key(&self) -> i64 {
        match self {
            Self::Tool(_) => 0,
            Self::Session(_) => 0,
            Self::SessionIndex(_, idx) => *idx as i64,
        }
    }
}

#[derive(Clone)]
struct L1Entry {
    signature: String,
    updated_at: i64,
    last_persisted_at: i64,
    access_seq: u64,
}

#[derive(Default)]
struct L1Cache {
    tool: HashMap<String, L1Entry>,
    session: HashMap<String, L1Entry>,
    session_index: HashMap<(String, usize), L1Entry>,
    access_seq: u64,
}

impl L1Cache {
    fn next_seq(&mut self) -> u64 {
        self.access_seq = self.access_seq.wrapping_add(1);
        self.access_seq
    }

    fn get_tool(
        &mut self,
        id: &str,
        now: i64,
        ttl_secs: i64,
        throttle_secs: i64,
    ) -> (Option<String>, bool) {
        let seq = self.next_seq();
        if let Some(entry) = self.tool.get_mut(id) {
            if now.saturating_sub(entry.updated_at) > ttl_secs {
                self.tool.remove(id);
                (None, false)
            } else {
                entry.updated_at = now;
                entry.access_seq = seq;
                let should_touch = now.saturating_sub(entry.last_persisted_at) >= throttle_secs;
                if should_touch {
                    entry.last_persisted_at = now;
                }
                (Some(entry.signature.clone()), should_touch)
            }
        } else {
            (None, false)
        }
    }

    fn insert_tool(&mut self, id: &str, sig: &str, now: i64, cap: usize, ttl_secs: i64) {
        if cap == 0 {
            return;
        }
        if self.tool.len() >= cap && !self.tool.contains_key(id) {
            self.tool
                .retain(|_, entry| now.saturating_sub(entry.updated_at) <= ttl_secs);
            if self.tool.len() >= cap {
                if let Some(oldest_key) = self
                    .tool
                    .iter()
                    .min_by_key(|(_, entry)| entry.access_seq)
                    .map(|(k, _)| k.clone())
                {
                    self.tool.remove(&oldest_key);
                }
            }
        }
        let seq = self.next_seq();
        self.tool.insert(
            id.to_string(),
            L1Entry {
                signature: sig.to_string(),
                updated_at: now,
                last_persisted_at: now,
                access_seq: seq,
            },
        );
    }

    fn get_session(
        &mut self,
        key: &str,
        now: i64,
        ttl_secs: i64,
        throttle_secs: i64,
    ) -> (Option<String>, bool) {
        let seq = self.next_seq();
        if let Some(entry) = self.session.get_mut(key) {
            if now.saturating_sub(entry.updated_at) > ttl_secs {
                self.session.remove(key);
                (None, false)
            } else {
                entry.updated_at = now;
                entry.access_seq = seq;
                let should_touch = now.saturating_sub(entry.last_persisted_at) >= throttle_secs;
                if should_touch {
                    entry.last_persisted_at = now;
                }
                (Some(entry.signature.clone()), should_touch)
            }
        } else {
            (None, false)
        }
    }

    fn insert_session(
        &mut self,
        key: &str,
        sig: &str,
        now: i64,
        cap: usize,
        ttl_secs: i64,
    ) -> bool {
        if cap == 0 {
            return false;
        }
        if let Some(existing) = self.session.get(key) {
            if sig.len() < existing.signature.len()
                && now.saturating_sub(existing.updated_at) <= ttl_secs
            {
                return false;
            }
        }
        if self.session.len() >= cap && !self.session.contains_key(key) {
            self.session
                .retain(|_, entry| now.saturating_sub(entry.updated_at) <= ttl_secs);
            if self.session.len() >= cap {
                if let Some(oldest_key) = self
                    .session
                    .iter()
                    .min_by_key(|(_, entry)| entry.access_seq)
                    .map(|(k, _)| k.clone())
                {
                    self.session.remove(&oldest_key);
                }
            }
        }
        let seq = self.next_seq();
        self.session.insert(
            key.to_string(),
            L1Entry {
                signature: sig.to_string(),
                updated_at: now,
                last_persisted_at: now,
                access_seq: seq,
            },
        );
        true
    }

    fn get_session_index(
        &mut self,
        key: &str,
        idx: usize,
        now: i64,
        ttl_secs: i64,
        throttle_secs: i64,
    ) -> (Option<String>, bool) {
        let seq = self.next_seq();
        let map_key = (key.to_string(), idx);
        if let Some(entry) = self.session_index.get_mut(&map_key) {
            if now.saturating_sub(entry.updated_at) > ttl_secs {
                self.session_index.remove(&map_key);
                (None, false)
            } else {
                entry.updated_at = now;
                entry.access_seq = seq;
                let should_touch = now.saturating_sub(entry.last_persisted_at) >= throttle_secs;
                if should_touch {
                    entry.last_persisted_at = now;
                }
                (Some(entry.signature.clone()), should_touch)
            }
        } else {
            (None, false)
        }
    }

    fn insert_session_index(
        &mut self,
        key: &str,
        idx: usize,
        sig: &str,
        now: i64,
        cap: usize,
        ttl_secs: i64,
    ) {
        if cap == 0 {
            return;
        }
        let map_key = (key.to_string(), idx);
        if self.session_index.len() >= cap && !self.session_index.contains_key(&map_key) {
            self.session_index
                .retain(|_, entry| now.saturating_sub(entry.updated_at) <= ttl_secs);
            if self.session_index.len() >= cap {
                if let Some(oldest_key) = self
                    .session_index
                    .iter()
                    .min_by_key(|(_, entry)| entry.access_seq)
                    .map(|(k, _)| k.clone())
                {
                    self.session_index.remove(&oldest_key);
                }
            }
        }
        let seq = self.next_seq();
        self.session_index.insert(
            map_key,
            L1Entry {
                signature: sig.to_string(),
                updated_at: now,
                last_persisted_at: now,
                access_seq: seq,
            },
        );
    }
}

pub(crate) fn usable(signature: &str) -> bool {
    let trimmed = signature.trim();
    !trimmed.is_empty() && trimmed != super::thought_sig::SKIP_VALIDATOR_SENTINEL
}

enum WriteOp {
    Upsert {
        key: SignatureKey,
        signature: String,
        updated_at: i64,
    },
    Touch {
        key: SignatureKey,
        signature: String,
        updated_at: i64,
    },
    Flush(std::sync::mpsc::SyncSender<()>),
}

fn is_corruption_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi, msg) => {
            if ffi.code == rusqlite::ffi::ErrorCode::DatabaseCorrupt
                || ffi.code == rusqlite::ffi::ErrorCode::NotADatabase
            {
                return true;
            }
            if let Some(msg) = msg {
                let lower = msg.to_ascii_lowercase();
                lower.contains("corrupt")
                    || lower.contains("malformed")
                    || lower.contains("not a database")
            } else {
                false
            }
        }
        _ => false,
    }
}

fn cleanup_old_quarantine_files(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("thought-signatures.db") && name.contains(".corrupt.")
        })
        .collect();

    if files.len() <= keep {
        return;
    }

    files.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH)
    });

    let to_remove = files.len().saturating_sub(keep);
    for p in files.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(p);
    }
}

fn isolate_corrupt_files(db_path: &Path) -> std::io::Result<()> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let db_str = db_path.to_string_lossy();
    let wal_path = PathBuf::from(format!("{}-wal", db_str));
    let shm_path = PathBuf::from(format!("{}-shm", db_str));

    let q_db = PathBuf::from(format!("{}.corrupt.{}", db_str, ts));
    let q_wal = PathBuf::from(format!("{}-wal.corrupt.{}", db_str, ts));
    let q_shm = PathBuf::from(format!("{}-shm.corrupt.{}", db_str, ts));

    if db_path.exists() {
        if let Err(e) = std::fs::rename(db_path, &q_db) {
            log::warn!("Failed to rename corrupt database file");
            return Err(e);
        }
    }
    if wal_path.exists() {
        if std::fs::rename(&wal_path, &q_wal).is_err() {
            log::warn!("Failed to rename corrupt database WAL file");
        }
    }
    if shm_path.exists() {
        if std::fs::rename(&shm_path, &q_shm).is_err() {
            log::warn!("Failed to rename corrupt database SHM file");
        }
    }

    if let Some(parent) = db_path.parent() {
        cleanup_old_quarantine_files(parent, MAX_QUARANTINE_FILES);
    }

    log::info!("Thought signature store quarantined corrupt database files");
    Ok(())
}

fn check_and_recover_db(path: &Path, busy_timeout_ms: u32) -> Result<(), ()> {
    if !path.exists() {
        return Ok(());
    }

    let check_result = {
        match rusqlite::Connection::open(path) {
            Ok(conn) => {
                let _ = conn.busy_timeout(Duration::from_millis(busy_timeout_ms as u64));
                match conn.query_row("PRAGMA quick_check(1);", [], |row| row.get::<_, String>(0)) {
                    Ok(res) => {
                        if res == "ok" {
                            Ok(())
                        } else {
                            Err(true)
                        }
                    }
                    Err(err) => {
                        if is_corruption_error(&err) {
                            Err(true)
                        } else {
                            Err(false)
                        }
                    }
                }
            }
            Err(err) => {
                if is_corruption_error(&err) {
                    Err(true)
                } else {
                    Err(false)
                }
            }
        }
    };

    match check_result {
        Ok(()) => Ok(()),
        Err(true) => {
            log::warn!("Thought signature database file corruption detected at startup; quarantining files");
            isolate_corrupt_files(path).map_err(|_| ())?;
            Ok(())
        }
        Err(false) => {
            log::warn!(
                "Thought signature database is busy or inaccessible; degrading to in-memory mode"
            );
            Err(())
        }
    }
}

fn open_connection(
    path: &Path,
    busy_timeout_ms: u32,
) -> Result<rusqlite::Connection, rusqlite::Error> {
    let conn = rusqlite::Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(busy_timeout_ms as u64))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(conn)
}

fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thought_signatures (
            kind INTEGER NOT NULL,
            key_primary TEXT NOT NULL,
            key_secondary INTEGER NOT NULL DEFAULT 0,
            signature TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (kind, key_primary, key_secondary)
        );
        CREATE INDEX IF NOT EXISTS idx_thought_sig_updated_at ON thought_signatures (updated_at);",
    )?;
    Ok(())
}

fn bounded_prewarm_l1(conn: &rusqlite::Connection, l1: &mut L1Cache, config: &StoreConfig) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let cutoff = now.saturating_sub(config.ttl_secs);

    if config.max_l1_tool > 0 {
        if let Ok(mut stmt) = conn.prepare_cached(
            "SELECT key_primary, signature, updated_at FROM thought_signatures
             WHERE kind = 0 AND updated_at >= ?1
             ORDER BY updated_at DESC LIMIT ?2;",
        ) {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![cutoff, config.max_l1_tool as i64], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })
            {
                let mut items: Vec<_> = rows.flatten().collect();
                items.reverse();
                for item in items {
                    let id = item.0.trim();
                    let sig = item.1.trim();
                    if id.is_empty() || !usable(sig) {
                        continue;
                    }
                    let seq = l1.next_seq();
                    l1.tool.insert(
                        id.to_string(),
                        L1Entry {
                            signature: sig.to_string(),
                            updated_at: item.2,
                            last_persisted_at: item.2,
                            access_seq: seq,
                        },
                    );
                }
            }
        }
    }

    if config.max_l1_session > 0 {
        if let Ok(mut stmt) = conn.prepare_cached(
            "SELECT key_primary, signature, updated_at FROM thought_signatures
             WHERE kind = 1 AND updated_at >= ?1
             ORDER BY updated_at DESC LIMIT ?2;",
        ) {
            if let Ok(rows) = stmt.query_map(
                rusqlite::params![cutoff, config.max_l1_session as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            ) {
                let mut items: Vec<_> = rows.flatten().collect();
                items.reverse();
                for item in items {
                    let key = item.0.trim();
                    let sig = item.1.trim();
                    if key.is_empty() || !usable(sig) {
                        continue;
                    }
                    let seq = l1.next_seq();
                    l1.session.insert(
                        key.to_string(),
                        L1Entry {
                            signature: sig.to_string(),
                            updated_at: item.2,
                            last_persisted_at: item.2,
                            access_seq: seq,
                        },
                    );
                }
            }
        }
    }

    if config.max_l1_session_index > 0 {
        if let Ok(mut stmt) = conn.prepare_cached(
            "SELECT key_primary, key_secondary, signature, updated_at FROM thought_signatures
             WHERE kind = 2 AND key_secondary >= 0 AND updated_at >= ?1
             ORDER BY updated_at DESC LIMIT ?2;",
        ) {
            if let Ok(rows) = stmt.query_map(
                rusqlite::params![cutoff, config.max_l1_session_index as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            ) {
                let mut items: Vec<_> = rows.flatten().collect();
                items.reverse();
                for item in items {
                    let key = item.0.trim();
                    let raw_idx = item.1;
                    let sig = item.2.trim();
                    if key.is_empty() || raw_idx < 0 || !usable(sig) {
                        continue;
                    }
                    let idx = raw_idx as usize;
                    let seq = l1.next_seq();
                    l1.session_index.insert(
                        (key.to_string(), idx),
                        L1Entry {
                            signature: sig.to_string(),
                            updated_at: item.3,
                            last_persisted_at: item.3,
                            access_seq: seq,
                        },
                    );
                }
            }
        }
    }
}

fn execute_write_batch(
    conn: &mut rusqlite::Connection,
    batch: &[WriteOp],
    max_capacity: usize,
) -> Result<(), rusqlite::Error> {
    if max_capacity == 0 {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    {
        let mut upsert_tool_or_idx_stmt = tx.prepare_cached(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(kind, key_primary, key_secondary) DO UPDATE SET
                 signature = excluded.signature,
                 updated_at = excluded.updated_at;"
        )?;

        let mut upsert_session_stmt = tx.prepare_cached(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(kind, key_primary, key_secondary) DO UPDATE SET
                 signature = CASE WHEN length(excluded.signature) >= length(signature) THEN excluded.signature ELSE signature END,
                 updated_at = excluded.updated_at;"
        )?;

        let mut touch_stmt = tx.prepare_cached(
            "UPDATE thought_signatures SET updated_at = ?5
             WHERE kind = ?1 AND key_primary = ?2 AND key_secondary = ?3
               AND signature = ?4;",
        )?;

        for op in batch {
            match op {
                WriteOp::Upsert {
                    key,
                    signature,
                    updated_at,
                } => {
                    let kind_val = key.kind() as u8;
                    let primary = key.primary_key();
                    let secondary = key.secondary_key();

                    if key.kind() == SignatureKind::Session {
                        upsert_session_stmt.execute(rusqlite::params![
                            kind_val, primary, secondary, signature, updated_at
                        ])?;
                    } else {
                        upsert_tool_or_idx_stmt.execute(rusqlite::params![
                            kind_val, primary, secondary, signature, updated_at
                        ])?;
                    }
                }
                WriteOp::Touch {
                    key,
                    signature,
                    updated_at,
                } => {
                    let kind_val = key.kind() as u8;
                    let primary = key.primary_key();
                    let secondary = key.secondary_key();

                    touch_stmt.execute(rusqlite::params![
                        kind_val, primary, secondary, signature, updated_at
                    ])?;
                }
                WriteOp::Flush(_) => {}
            }
        }
    }

    tx.commit()?;
    Ok(())
}

fn perform_maintenance(
    conn: &mut rusqlite::Connection,
    ttl_secs: i64,
    max_capacity: usize,
) -> Result<(), rusqlite::Error> {
    if max_capacity == 0 {
        conn.execute("DELETE FROM thought_signatures;", [])?;
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let cutoff = now.saturating_sub(ttl_secs);

    conn.execute(
        "DELETE FROM thought_signatures WHERE updated_at < ?1;",
        rusqlite::params![cutoff],
    )?;

    let count: usize = conn.query_row("SELECT COUNT(*) FROM thought_signatures;", [], |row| {
        row.get(0)
    })?;

    if count > max_capacity {
        let excess = count - max_capacity;
        conn.execute(
            "DELETE FROM thought_signatures WHERE rowid IN (
                SELECT rowid FROM thought_signatures ORDER BY updated_at ASC LIMIT ?1
            );",
            rusqlite::params![excess as i64],
        )?;
    }

    Ok(())
}

fn worker_loop(
    mut conn: rusqlite::Connection,
    rx: std::sync::mpsc::Receiver<WriteOp>,
    ttl_secs: i64,
    max_capacity: usize,
    degraded: Arc<AtomicBool>,
) {
    let mut ops_since_maintenance: usize = 0;

    loop {
        let first_op = match rx.recv() {
            Ok(op) => op,
            Err(_) => {
                // Sender 全部释放（shutdown 阶段）：自然退出
                break;
            }
        };

        let mut batch = Vec::with_capacity(64);
        let mut should_flush = None;

        match first_op {
            WriteOp::Flush(ack) => should_flush = Some(ack),
            op => batch.push(op),
        }

        while batch.len() < 64 {
            match rx.try_recv() {
                Ok(WriteOp::Flush(ack)) => {
                    should_flush = Some(ack);
                    break;
                }
                Ok(op) => batch.push(op),
                Err(_) => break,
            }
        }

        if !batch.is_empty() {
            if let Err(err) = execute_write_batch(&mut conn, &batch, max_capacity) {
                if is_corruption_error(&err) {
                    log::warn!("Thought signature background writer detected database corruption; stopping persistence until next restart");
                    degraded.store(true, Ordering::Release);
                    break;
                }
                log::warn!("Thought signature background write batch error occurred");
            }
            ops_since_maintenance += batch.len();
        }

        if ops_since_maintenance >= 128 || should_flush.is_some() {
            if let Err(err) = perform_maintenance(&mut conn, ttl_secs, max_capacity) {
                if is_corruption_error(&err) {
                    log::warn!("Thought signature background maintenance detected database corruption; stopping persistence until next restart");
                    degraded.store(true, Ordering::Release);
                    break;
                }
                log::debug!("Thought signature maintenance warning occurred");
            }
            ops_since_maintenance = 0;
        }

        if let Some(ack) = should_flush {
            let _ = ack.send(());
        }
    }

    // Worker 退出前排空通道内剩余未处理的所有 op
    let mut remaining = Vec::with_capacity(64);
    while let Ok(op) = rx.try_recv() {
        match op {
            WriteOp::Flush(ack) => {
                let _ = ack.send(());
            }
            op => remaining.push(op),
        }
        if remaining.len() >= 64 {
            let _ = execute_write_batch(&mut conn, &remaining, max_capacity);
            remaining.clear();
        }
    }
    if !remaining.is_empty() {
        let _ = execute_write_batch(&mut conn, &remaining, max_capacity);
    }
    let _ = perform_maintenance(&mut conn, ttl_secs, max_capacity);
}

pub(crate) struct ThoughtSigStore {
    config: StoreConfig,
    l1: Mutex<L1Cache>,
    writer_tx: Mutex<Option<std::sync::mpsc::SyncSender<WriteOp>>>,
    writer_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    degraded: Arc<AtomicBool>,
}

impl ThoughtSigStore {
    pub(crate) fn init(config: StoreConfig) -> Self {
        let degraded = Arc::new(AtomicBool::new(false));
        let l1 = Mutex::new(L1Cache::default());

        if config.max_l2_capacity == 0 {
            degraded.store(true, Ordering::Release);
            return Self {
                config,
                l1,
                writer_tx: Mutex::new(None),
                writer_handle: Mutex::new(None),
                degraded,
            };
        }

        if let Some(parent) = config.path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                log::warn!(
                    "Failed to create thought signatures directory, degrading to in-memory mode"
                );
                degraded.store(true, Ordering::Release);
                return Self {
                    config,
                    l1,
                    writer_tx: Mutex::new(None),
                    writer_handle: Mutex::new(None),
                    degraded,
                };
            }
        }

        if check_and_recover_db(&config.path, config.busy_timeout_ms).is_err() {
            log::warn!(
                "Failed during thought signature database recovery, degrading to in-memory mode"
            );
            degraded.store(true, Ordering::Release);
            return Self {
                config,
                l1,
                writer_tx: Mutex::new(None),
                writer_handle: Mutex::new(None),
                degraded,
            };
        }

        let init_conn = match open_connection(&config.path, config.busy_timeout_ms) {
            Ok(conn) => {
                if ensure_schema(&conn).is_err() {
                    log::warn!(
                        "Failed to create thought signatures schema, degrading to in-memory mode"
                    );
                    degraded.store(true, Ordering::Release);
                    return Self {
                        config,
                        l1,
                        writer_tx: Mutex::new(None),
                        writer_handle: Mutex::new(None),
                        degraded,
                    };
                }
                conn
            }
            Err(_) => {
                log::warn!("Failed to open thought signatures initialization connection, degrading to in-memory mode");
                degraded.store(true, Ordering::Release);
                return Self {
                    config,
                    l1,
                    writer_tx: Mutex::new(None),
                    writer_handle: Mutex::new(None),
                    degraded,
                };
            }
        };

        // 启动阶段在 init 中有界预热 L1 缓存（最多加载 L1 容量上限的记录）
        {
            let mut l1_guard = match l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            bounded_prewarm_l1(&init_conn, &mut l1_guard, &config);
        }
        drop(init_conn);

        let writer_conn = match open_connection(&config.path, config.busy_timeout_ms) {
            Ok(conn) => conn,
            Err(_) => {
                log::warn!("Failed to open thought signatures writer connection, degrading to in-memory mode");
                degraded.store(true, Ordering::Release);
                return Self {
                    config,
                    l1,
                    writer_tx: Mutex::new(None),
                    writer_handle: Mutex::new(None),
                    degraded,
                };
            }
        };

        let (tx, rx) = std::sync::mpsc::sync_channel(config.queue_bound);
        let ttl_secs = config.ttl_secs;
        let max_capacity = config.max_l2_capacity;
        let degraded_clone = degraded.clone();

        let handle = std::thread::Builder::new()
            .name("thought-sig-writer".to_string())
            .spawn(move || {
                worker_loop(writer_conn, rx, ttl_secs, max_capacity, degraded_clone);
            });

        let (writer_tx, writer_handle) = match handle {
            Ok(h) => (Some(tx), Some(h)),
            Err(_) => {
                log::warn!(
                    "Failed to spawn thought signature writer thread, degrading to in-memory mode"
                );
                degraded.store(true, Ordering::Release);
                (None, None)
            }
        };

        Self {
            config,
            l1,
            writer_tx: Mutex::new(writer_tx),
            writer_handle: Mutex::new(writer_handle),
            degraded,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Acquire)
    }

    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn send_upsert(&self, key: SignatureKey, signature: &str) {
        if self.degraded.load(Ordering::Acquire) || self.config.max_l2_capacity == 0 {
            return;
        }
        let now = Self::now_secs();
        let op = WriteOp::Upsert {
            key,
            signature: signature.to_string(),
            updated_at: now,
        };

        let tx = {
            let guard = match self.writer_tx.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.clone()
        };

        if let Some(tx) = tx {
            match tx.try_send(op) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::debug!("Thought signature queue full, dropping L2 write op");
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.degraded.store(true, Ordering::Release);
                }
            }
        }
    }

    fn send_touch(&self, key: SignatureKey, signature: String, now: i64) {
        if self.degraded.load(Ordering::Acquire) || self.config.max_l2_capacity == 0 {
            return;
        }
        let op = WriteOp::Touch {
            key,
            signature,
            updated_at: now,
        };
        let tx = {
            let guard = match self.writer_tx.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.clone()
        };
        if let Some(tx) = tx {
            let _ = tx.try_send(op);
        }
    }

    pub(crate) fn cache_tool(&self, id: &str, sig: &str) {
        let id_clean = id.trim();
        if id_clean.is_empty() || !usable(sig) {
            return;
        }
        let now = Self::now_secs();
        {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.insert_tool(
                id_clean,
                sig,
                now,
                self.config.max_l1_tool,
                self.config.ttl_secs,
            );
        }
        self.send_upsert(SignatureKey::Tool(id_clean.to_string()), sig);
    }

    pub(crate) fn get_tool(&self, id: &str) -> Option<String> {
        let id_clean = id.trim();
        if id_clean.is_empty() {
            return None;
        }
        let now = Self::now_secs();
        let (sig, should_touch) = {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.get_tool(
                id_clean,
                now,
                self.config.ttl_secs,
                self.config.touch_throttle_secs,
            )
        };
        if should_touch {
            if let Some(signature) = sig.as_ref() {
                self.send_touch(
                    SignatureKey::Tool(id_clean.to_string()),
                    signature.clone(),
                    now,
                );
            }
        }
        sig
    }

    pub(crate) fn cache_session(&self, key: &str, sig: &str) {
        let key_clean = key.trim();
        if key_clean.is_empty() || !usable(sig) {
            return;
        }
        let now = Self::now_secs();
        let accepted = {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.insert_session(
                key_clean,
                sig,
                now,
                self.config.max_l1_session,
                self.config.ttl_secs,
            )
        };
        if accepted {
            self.send_upsert(SignatureKey::Session(key_clean.to_string()), sig);
        }
    }

    pub(crate) fn get_session(&self, key: &str) -> Option<String> {
        let key_clean = key.trim();
        if key_clean.is_empty() {
            return None;
        }
        let now = Self::now_secs();
        let (sig, should_touch) = {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.get_session(
                key_clean,
                now,
                self.config.ttl_secs,
                self.config.touch_throttle_secs,
            )
        };
        if should_touch {
            if let Some(signature) = sig.as_ref() {
                self.send_touch(
                    SignatureKey::Session(key_clean.to_string()),
                    signature.clone(),
                    now,
                );
            }
        }
        sig
    }

    pub(crate) fn cache_session_index(&self, key: &str, idx: usize, sig: &str) {
        let key_clean = key.trim();
        if key_clean.is_empty() || !usable(sig) {
            return;
        }
        let now = Self::now_secs();
        {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.insert_session_index(
                key_clean,
                idx,
                sig,
                now,
                self.config.max_l1_session_index,
                self.config.ttl_secs,
            );
        }
        self.send_upsert(SignatureKey::SessionIndex(key_clean.to_string(), idx), sig);
    }

    pub(crate) fn get_session_index(&self, key: &str, idx: usize) -> Option<String> {
        let key_clean = key.trim();
        if key_clean.is_empty() {
            return None;
        }
        let now = Self::now_secs();
        let (sig, should_touch) = {
            let mut l1 = match self.l1.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            l1.get_session_index(
                key_clean,
                idx,
                now,
                self.config.ttl_secs,
                self.config.touch_throttle_secs,
            )
        };
        if should_touch {
            if let Some(signature) = sig.as_ref() {
                self.send_touch(
                    SignatureKey::SessionIndex(key_clean.to_string(), idx),
                    signature.clone(),
                    now,
                );
            }
        }
        sig
    }

    pub(crate) fn resolve(
        &self,
        tool_use_id: Option<&str>,
        session_key: Option<&str>,
        message_index: Option<usize>,
    ) -> String {
        if let Some(id) = tool_use_id {
            if let Some(signature) = self.get_tool(id) {
                return signature;
            }
        }
        if let (Some(session), Some(index)) = (session_key, message_index) {
            if let Some(signature) = self.get_session_index(session, index) {
                return signature;
            }
        }
        if let Some(session) = session_key {
            if let Some(signature) = self.get_session(session) {
                return signature;
            }
        }
        super::thought_sig::SKIP_VALIDATOR_SENTINEL.to_string()
    }

    pub(crate) fn flush(&self) {
        if self.degraded.load(Ordering::Acquire) {
            return;
        }
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
        let tx = {
            let guard = match self.writer_tx.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.clone()
        };

        if let Some(tx) = tx {
            if tx.try_send(WriteOp::Flush(ack_tx)).is_ok() {
                let _ = ack_rx.recv_timeout(Duration::from_millis(1000));
            }
        }
    }

    pub(crate) fn shutdown(&self) {
        {
            let mut guard = match self.writer_tx.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            // 释放 sender 让 worker 线程在读到 EOF 时自然排空剩余待写队列
            let _ = guard.take();
        }
        {
            let mut guard = match self.writer_handle.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for ThoughtSigStore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(dir: &Path) -> StoreConfig {
        StoreConfig {
            path: dir.join("thought-signatures.db"),
            ttl_secs: 15 * 86400,
            max_l2_capacity: 10_000,
            max_l1_tool: 1024,
            max_l1_session: 256,
            max_l1_session_index: 2048,
            queue_bound: 2048,
            busy_timeout_ms: 250,
            touch_throttle_secs: 60,
        }
    }

    #[test]
    fn test_restart_persistence_and_bounded_prewarm() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp_config(temp.path());

        {
            let store = ThoughtSigStore::init(config.clone());
            store.cache_tool("tool_persist_1", "sig_persist_tool");
            store.cache_session("sess_persist_1", "sig_persist_session");
            store.cache_session_index("sess_persist_1", 3, "sig_persist_idx");
            store.flush();
        }

        let store2 = ThoughtSigStore::init(config);
        assert_eq!(
            store2.get_tool("tool_persist_1").as_deref(),
            Some("sig_persist_tool")
        );
        assert_eq!(
            store2.get_session("sess_persist_1").as_deref(),
            Some("sig_persist_session")
        );
        assert_eq!(
            store2.get_session_index("sess_persist_1", 3).as_deref(),
            Some("sig_persist_idx")
        );
    }

    #[test]
    fn test_bounded_prewarm_filters_sentinel_empty_and_negative_index() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("thought-signatures.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        ensure_schema(&conn).unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 插入无效/应被过滤的数据
        conn.execute(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (0, 'tool_sentinel', 0, 'skip_thought_signature_validator', ?1);",
            rusqlite::params![now],
        ).unwrap();
        conn.execute(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (0, 'tool_empty', 0, '   ', ?1);",
            rusqlite::params![now],
        ).unwrap();
        conn.execute(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (2, 'sess_neg', -5, 'sig_neg', ?1);",
            rusqlite::params![now],
        ).unwrap();
        conn.execute(
            "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
             VALUES (0, 'tool_valid', 0, 'sig_valid', ?1);",
            rusqlite::params![now],
        ).unwrap();
        drop(conn);

        let config = temp_config(temp.path());
        let store = ThoughtSigStore::init(config);

        assert_eq!(store.get_tool("tool_sentinel"), None);
        assert_eq!(store.get_tool("tool_empty"), None);
        assert_eq!(store.get_tool("tool_valid").as_deref(), Some("sig_valid"));
    }

    #[test]
    fn test_l1_ttl_expiry_without_manual_clearing() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.ttl_secs = 1;

        let store = ThoughtSigStore::init(config);
        store.cache_tool("tool_ttl_auto", "sig_ttl_auto");
        store.flush();

        assert_eq!(
            store.get_tool("tool_ttl_auto").as_deref(),
            Some("sig_ttl_auto")
        );

        std::thread::sleep(Duration::from_millis(2100));

        assert_eq!(store.get_tool("tool_ttl_auto"), None);
    }

    #[test]
    fn test_throttled_touch_persists_to_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.touch_throttle_secs = 1; // 1秒节流阈值以供测试
        let db_path = config.path.clone();

        let store = ThoughtSigStore::init(config);
        store.cache_tool("tool_touch_test", "sig_touch");
        store.flush();

        // 读出写入时的初始时间戳
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let initial_updated_at: i64 = conn
            .query_row(
                "SELECT updated_at FROM thought_signatures WHERE key_primary = 'tool_touch_test';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        // 休眠 1.2 秒（超过 1 秒节流时间）
        std::thread::sleep(Duration::from_millis(1200));

        // 调用 get_tool，触发节流 Touch
        assert_eq!(
            store.get_tool("tool_touch_test").as_deref(),
            Some("sig_touch")
        );
        store.flush(); // 等待 Touch 写入落盘

        // 验证 SQLite 中的 updated_at 已被更新
        let conn2 = rusqlite::Connection::open(&db_path).unwrap();
        let touched_updated_at: i64 = conn2
            .query_row(
                "SELECT updated_at FROM thought_signatures WHERE key_primary = 'tool_touch_test';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            touched_updated_at > initial_updated_at,
            "Touch 必须向 SQLite 刷新 updated_at 时间戳"
        );
    }

    #[test]
    fn test_l1_capacities_and_eviction() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.max_l1_tool = 2;

        let store = ThoughtSigStore::init(config);
        store.cache_tool("t1", "sig1");
        store.cache_tool("t2", "sig2");
        store.cache_tool("t3", "sig3");

        assert_eq!(store.get_tool("t1"), None);
        assert_eq!(store.get_tool("t2").as_deref(), Some("sig2"));
        assert_eq!(store.get_tool("t3").as_deref(), Some("sig3"));
    }

    #[test]
    fn test_cap_zero_must_not_insert() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.max_l1_tool = 0;
        config.max_l2_capacity = 0;

        let store = ThoughtSigStore::init(config);
        store.cache_tool("tool_zero", "sig_zero");
        store.flush();

        assert_eq!(store.get_tool("tool_zero"), None);
    }

    #[test]
    fn test_resolution_priority() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp_config(temp.path());
        let store = ThoughtSigStore::init(config);

        store.cache_tool("tool_pri", "sig_pri_tool");
        store.cache_session("sess_pri", "sig_pri_session");
        store.cache_session_index("sess_pri", 5, "sig_pri_idx");

        assert_eq!(
            store.resolve(Some("tool_pri"), Some("sess_pri"), Some(5)),
            "sig_pri_tool"
        );
        assert_eq!(
            store.resolve(None, Some("sess_pri"), Some(5)),
            "sig_pri_idx"
        );
        assert_eq!(
            store.resolve(None, Some("sess_pri"), Some(99)),
            "sig_pri_session"
        );
        assert_eq!(
            store.resolve(None, Some("unknown_sess"), None),
            super::super::thought_sig::SKIP_VALIDATOR_SENTINEL
        );
    }

    #[test]
    fn test_startup_corruption_recovery_and_quarantine_prune() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("thought-signatures.db");
        let wal_path = temp.path().join("thought-signatures.db-wal");
        let shm_path = temp.path().join("thought-signatures.db-shm");

        for i in 0..7 {
            let old_quarantine = temp
                .path()
                .join(format!("thought-signatures.db.corrupt.{}", 1000 + i));
            std::fs::write(&old_quarantine, b"old").unwrap();
        }

        std::fs::write(&db_path, b"CORRUPTED_RANDOM_GARBAGE_BYTES_SQLITE_HEADER").unwrap();
        std::fs::write(&wal_path, b"CORRUPTED_WAL").unwrap();
        std::fs::write(&shm_path, b"CORRUPTED_SHM").unwrap();

        let config = temp_config(temp.path());
        let store = ThoughtSigStore::init(config);

        let quarantine_count = std::fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .count();
        assert!(quarantine_count <= MAX_QUARANTINE_FILES + 2);

        store.cache_tool("tool_after_recovery", "sig_recovered");
        store.flush();
        assert_eq!(
            store.get_tool("tool_after_recovery").as_deref(),
            Some("sig_recovered")
        );
    }

    #[test]
    fn test_unwritable_path_degrades_to_memory() {
        let temp = tempfile::tempdir().unwrap();
        let regular_file = temp.path().join("blocking_file");
        std::fs::write(&regular_file, b"cannot be directory").unwrap();
        let invalid_path = regular_file.join("sub").join("thought-signatures.db");

        let mut config = temp_config(temp.path());
        config.path = invalid_path;

        let store = ThoughtSigStore::init(config);
        assert!(store.is_degraded());

        store.cache_tool("tool_mem", "sig_mem");
        assert_eq!(store.get_tool("tool_mem").as_deref(), Some("sig_mem"));
    }

    #[test]
    fn test_prewarm_preserves_recent_lru_order() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("thought-signatures.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        ensure_schema(&conn).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for (id, age) in [("tool_old", 2), ("tool_middle", 1), ("tool_new", 0)] {
            conn.execute(
                "INSERT INTO thought_signatures (kind, key_primary, key_secondary, signature, updated_at)
                 VALUES (0, ?1, 0, ?2, ?3);",
                rusqlite::params![id, format!("sig_{id}"), now - age],
            )
            .unwrap();
        }
        drop(conn);

        let mut config = temp_config(temp.path());
        config.max_l1_tool = 3;
        let store = ThoughtSigStore::init(config);
        store.cache_tool("tool_added", "sig_added");

        assert_eq!(store.get_tool("tool_old"), None);
        assert_eq!(store.get_tool("tool_new").as_deref(), Some("sig_tool_new"));
    }

    #[test]
    fn test_session_shorter_signature_does_not_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp_config(temp.path());
        let store = ThoughtSigStore::init(config);

        store.cache_session("sess_len", "long-signature-12345");
        store.cache_session("sess_len", "short");
        store.flush();

        assert_eq!(
            store.get_session("sess_len").as_deref(),
            Some("long-signature-12345")
        );
        drop(store);

        let reopened = ThoughtSigStore::init(temp_config(temp.path()));
        assert_eq!(
            reopened.get_session("sess_len").as_deref(),
            Some("long-signature-12345")
        );
    }

    #[test]
    fn test_bounded_queue_overflow_non_blocking() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.queue_bound = 2;

        let store = ThoughtSigStore::init(config);
        for i in 0..50 {
            store.cache_tool(&format!("tool_flood_{i}"), &format!("sig_{i}"));
        }
        store.flush();

        assert_eq!(store.get_tool("tool_flood_49").as_deref(), Some("sig_49"));
    }

    #[test]
    fn test_is_corruption_error_detection() {
        let err_corrupt = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseCorrupt,
                extended_code: 11,
            },
            Some("database disk image is malformed".to_string()),
        );
        assert!(is_corruption_error(&err_corrupt));

        let err_not_a_db = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::NotADatabase,
                extended_code: 26,
            },
            Some("file is not a database".to_string()),
        );
        assert!(is_corruption_error(&err_not_a_db));

        let err_busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        assert!(!is_corruption_error(&err_busy));
    }

    #[test]
    fn test_write_phase_degradation_fallback_to_memory() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp_config(temp.path());
        let store = ThoughtSigStore::init(config);

        store.degraded.store(true, Ordering::Release);

        store.cache_tool("tool_degraded", "sig_degraded");
        assert_eq!(
            store.get_tool("tool_degraded").as_deref(),
            Some("sig_degraded")
        );
    }

    #[test]
    fn test_busy_database_is_not_quarantined() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp_config(temp.path());
        let conn = rusqlite::Connection::open(&config.path).unwrap();
        ensure_schema(&conn).unwrap();
        conn.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        assert!(check_and_recover_db(&config.path, 10).is_err());
        assert!(config.path.exists());
        assert_eq!(
            std::fs::read_dir(temp.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt."))
                .count(),
            0
        );
        conn.execute_batch("ROLLBACK;").unwrap();
    }

    #[test]
    fn test_l2_capacity_and_expired_rows_are_pruned() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("thought-signatures.db");
        let mut conn = open_connection(&path, 10).unwrap();
        ensure_schema(&conn).unwrap();
        let now = ThoughtSigStore::now_secs();
        for (key, timestamp) in [("expired", now - 100), ("old", now - 2), ("new", now)] {
            conn.execute(
                "INSERT INTO thought_signatures VALUES (0, ?1, 0, 'signature', ?2)",
                rusqlite::params![key, timestamp],
            )
            .unwrap();
        }
        perform_maintenance(&mut conn, 10, 1).unwrap();
        let key: String = conn
            .query_row("SELECT key_primary FROM thought_signatures", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(key, "new");
    }

    #[test]
    fn test_touch_does_not_extend_replaced_signature() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = temp_config(temp.path());
        config.touch_throttle_secs = 1;
        let path = config.path.clone();
        let store = ThoughtSigStore::init(config);
        store.cache_tool("tool_touch_race", "old_signature");
        store.flush();
        std::thread::sleep(Duration::from_millis(1100));
        let (old_signature, should_touch) = {
            let mut l1 = store.l1.lock().unwrap();
            l1.get_tool("tool_touch_race", ThoughtSigStore::now_secs(), 100, 1)
        };
        assert!(should_touch);
        let replacement_updated_at = ThoughtSigStore::now_secs();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE thought_signatures SET signature = 'new_signature', updated_at = ?1 WHERE key_primary = 'tool_touch_race'",
            rusqlite::params![replacement_updated_at],
        ).unwrap();
        drop(conn);
        store.send_touch(
            SignatureKey::Tool("tool_touch_race".to_string()),
            old_signature.unwrap(),
            99,
        );
        store.flush();
        let conn = rusqlite::Connection::open(path).unwrap();
        let (signature, updated_at): (String, i64) = conn.query_row(
            "SELECT signature, updated_at FROM thought_signatures WHERE key_primary = 'tool_touch_race'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(signature, "new_signature");
        assert_eq!(updated_at, replacement_updated_at);
    }

    #[test]
    fn test_independent_store_lifecycle_and_shutdown() {
        let temp1 = tempfile::tempdir().unwrap();
        let temp2 = tempfile::tempdir().unwrap();

        let store1 = ThoughtSigStore::init(temp_config(temp1.path()));
        store1.cache_tool("tool_iso_1", "sig_iso_1");
        store1.flush();
        assert_eq!(store1.get_tool("tool_iso_1").as_deref(), Some("sig_iso_1"));
        store1.shutdown();

        let store2 = ThoughtSigStore::init(temp_config(temp2.path()));
        assert_eq!(store2.get_tool("tool_iso_1"), None);
        store2.cache_tool("tool_iso_2", "sig_iso_2");
        store2.flush();
        assert_eq!(store2.get_tool("tool_iso_2").as_deref(), Some("sig_iso_2"));
        store2.shutdown();
    }
}
