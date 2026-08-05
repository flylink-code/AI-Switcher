//! Atomic file writes and deterministic JSON IO.
//!
//! The pattern is write-to-temp-then-rename, which avoids leaving a config file
//! in a half-written state if the process is interrupted. On Unix the rename is
//! atomic; on Windows we remove the destination first (best-effort, since Windows
//! `rename` refuses to overwrite). Adapted from `examples/cc-switch-main`.

use crate::error::{io_context, AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::time::Duration;

/// Write `data` to `path` atomically: create a temp file beside the target,
/// flush, then rename into place.
pub fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Path(format!("路径缺少父目录: {}", path.display())))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut tmp = parent.to_path_buf();
    tmp.push(format!("{file_name}.tmp.{ts}"));

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

    // Rename into place. Windows can't rename over an existing file, so remove first.
    #[cfg(windows)]
    {
        if let Err(error) = remove_file_for_windows_replace(path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
    }
    fs::rename(&tmp, path).map_err(|e| {
        // Clean up the orphaned temp file on failure.
        let _ = fs::remove_file(&tmp);
        io_context(
            format!("原子替换失败: {} -> {}", tmp.display(), path.display()),
            e,
        )
    })?;

    Ok(())
}

/// Windows returns `ERROR_SHARING_VIOLATION` / `ERROR_ACCESS_DENIED` while a
/// recently relaunched process still has a config file open.  Do not ignore
/// that removal failure: doing so turns it into an opaque `ERROR_FILE_EXISTS`
/// from `rename`, and leaves update-time config repair permanently skipped.
#[cfg(windows)]
fn remove_file_for_windows_replace(path: &Path) -> AppResult<()> {
    const MAX_ATTEMPTS: u32 = 20;
    let mut last_error = None;
    for attempt in 0..MAX_ATTEMPTS {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) if is_retryable_windows_replace_error(&error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(75 * u64::from(attempt + 1)));
            }
            Err(error) => {
                return Err(io_context(
                    format!("删除待替换配置文件失败: {}", path.display()),
                    error,
                ));
            }
        }
    }
    Err(io_context(
        format!("等待配置文件锁释放超时: {}", path.display()),
        last_error.expect("retryable write error was captured"),
    ))
}

#[cfg(windows)]
fn is_retryable_windows_replace_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33 | 183))
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
