//! Atomic file writes and deterministic JSON IO.
//!
//! The pattern is write-to-temp-then-rename, which avoids leaving a config file
//! in a half-written state if the process is interrupted. On Unix the rename is
//! atomic; on Windows we use `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` so the
//! destination does not need to be deleted first (delete+rename races produce
//! opaque OS error 183 under updater relaunch / antivirus load).

use crate::error::{io_context, AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::time::Duration;

/// Serialize in-process config writes so startup repair and user provider
/// switches cannot interleave remove+rename on the same Windows path (OS 183).
fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Write `data` to `path` atomically: create a temp file beside the target,
/// flush, then rename into place.
pub fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    atomic_write_unlocked(path, data)
}

fn atomic_write_unlocked(path: &Path, data: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Path(format!("路径缺少父目录: {}", path.display())))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tmp = parent.to_path_buf();
    // PID + nanos + seq: Windows clock resolution can collide across writers.
    tmp.push(format!(
        "{file_name}.tmp.{}.{ts}.{seq}",
        std::process::id()
    ));

    // Write + flush the temp file first.
    {
        let mut f = fs::File::create(&tmp).map_err(|e| io_context("创建临时文件失败", e))?;
        f.write_all(data).map_err(|e| io_context("写入临时文件失败", e))?;
        f.flush().map_err(|e| io_context("刷新临时文件失败", e))?;
    }

    // Preserve permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let mode = meta.permissions().mode();
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode));
        }
    }

    #[cfg(windows)]
    {
        if let Err(error) = replace_file_on_windows(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            io_context(
                format!("原子替换失败: {} -> {}", tmp.display(), path.display()),
                e,
            )
        })?;
        Ok(())
    }
}

/// Replace `path` with `tmp` without a delete-first race.
///
/// `std::fs::rename` on Windows refuses to overwrite → OS 183 when another
/// writer (or our own TOCTOU after delete) recreates the destination. Prefer
/// `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`.
#[cfg(windows)]
fn replace_file_on_windows(tmp: &Path, path: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, FALSE};
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let to_wide = |p: &Path| -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };
    let from = to_wide(tmp);
    let to = to_wide(path);

    const MAX_ATTEMPTS: u32 = 24;
    let mut last_os = None;
    for attempt in 0..MAX_ATTEMPTS {
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok != FALSE {
            return Ok(());
        }
        let err = unsafe { GetLastError() };
        // 5 ACCESS_DENIED, 32 SHARING_VIOLATION, 33 LOCK_VIOLATION,
        // 183 ALREADY_EXISTS (should be rare with REPLACE_EXISTING; still retry).
        if matches!(err, 5 | 32 | 33 | 183) {
            last_os = Some(err);
            std::thread::sleep(Duration::from_millis(50 * u64::from(attempt + 1)));
            continue;
        }
        return Err(io_context(
            format!("原子替换失败: {} -> {}", tmp.display(), path.display()),
            std::io::Error::from_raw_os_error(err as i32),
        ));
    }
    Err(io_context(
        format!("原子替换重试超时: {} -> {}", tmp.display(), path.display()),
        std::io::Error::from_raw_os_error(last_os.unwrap_or(32) as i32),
    ))
}

/// Recursively sort the keys of a JSON value so equivalent configs serialize
/// to identical bytes (useful for diffs and stability).
pub fn sort_json_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // serde_json with `preserve_order` uses an indexmap; sort its keys.
            map.sort_keys();
            for (_, v) in map.iter_mut() {
                sort_json_keys(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                sort_json_keys(v);
            }
        }
        _ => {}
    }
}

/// Serialize `value` to pretty JSON (sorted keys) and write atomically.
pub fn write_json_file<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let mut json = serde_json::to_value(value)?;
    sort_json_keys(&mut json);
    let bytes = serde_json::to_vec_pretty(&json)?;
    atomic_write(path, &bytes)
}

/// Read and deserialize a JSON file. Returns `Ok(None)` if the file does not exist.
pub fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(path)?;
    let parsed = serde_json::from_slice(&raw)?;
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_roundtrips_and_removes_tmp() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("config.json");
        atomic_write(&target, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\":1}");

        // No leftover temp files.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "temp file leaked: {leftovers:?}");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("config.json");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
    }

    #[test]
    fn concurrent_atomic_writes_do_not_fail_with_exists() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().unwrap();
        let target = Arc::new(dir.path().join("race.toml"));
        atomic_write(&target, b"seed").unwrap();

        let mut handles = Vec::new();
        for i in 0..8 {
            let path = Arc::clone(&target);
            handles.push(thread::spawn(move || {
                for round in 0..20 {
                    let payload = format!("writer-{i}-round-{round}");
                    atomic_write(&path, payload.as_bytes()).unwrap_or_else(|error| {
                        panic!("concurrent atomic_write failed for writer {i}: {error}");
                    });
                }
            }));
        }
        for handle in handles {
            handle.join().expect("writer thread panicked");
        }
        assert!(target.exists());
        assert!(!fs::read_to_string(target.as_path()).unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_retries_then_succeeds_after_release() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::sync::{Arc, Barrier};
        use std::thread;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let target = Arc::new(dir.path().join("locked.toml"));
        atomic_write(&target, b"old").unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let path_for_lock = Arc::clone(&target);
        let barrier_for_lock = Arc::clone(&barrier);
        let locker = thread::spawn(move || {
            let file = fs::OpenOptions::new()
                .read(true)
                .share_mode(0) // deny delete/rename while held
                .open(path_for_lock.as_path())
                .expect("open locked file");
            barrier_for_lock.wait();
            thread::sleep(Duration::from_millis(400));
            drop(file);
        });

        let path_for_write = Arc::clone(&target);
        let barrier_for_write = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            barrier_for_write.wait();
            atomic_write(path_for_write.as_path(), b"new-after-lock").expect(
                "atomic_write should succeed once the exclusive lock is released",
            );
        });

        locker.join().unwrap();
        writer.join().unwrap();
        assert_eq!(fs::read_to_string(target.as_path()).unwrap(), "new-after-lock");
    }

    #[test]
    fn write_json_sorts_keys() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("out.json");
        // Deliberately out-of-order keys; expect sorted output.
        write_json_file(&target, &json!({"z": 1, "a": 2, "m": [3, {"k": 1, "j": 0}]})).unwrap();
        let s = fs::read_to_string(&target).unwrap();
        let pos_a = s.find("\"a\"").unwrap();
        let pos_m = s.find("\"m\"").unwrap();
        let pos_z = s.find("\"z\"").unwrap();
        assert!(pos_a < pos_m && pos_m < pos_z);
    }

    #[test]
    fn read_json_missing_returns_none() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nope.json");
        let v: Option<Value> = read_json_file(&target).unwrap();
        assert!(v.is_none());
    }
}
