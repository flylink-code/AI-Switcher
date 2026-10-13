//! Cheap "did another process rewrite this file?" check.
//!
//! The Antigravity / Kiro account stores keep an in-memory copy. In sidecar mode
//! the main process does not write these files; it sends a snapshot and then
//! records the stamp the sidecar reports. A stamp that does not match the file
//! means the sidecar wrote (token refresh, quota) and the next read reloads.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Last stamp this process believes is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedFile {
    pub exists: bool,
    pub mtime_ns: String,
    pub len: u64,
}

type Stamp = Option<(SystemTime, u64)>;

fn stamp_of(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub struct FileStamp(Mutex<Stamp>);

impl FileStamp {
    /// Record the current on-disk state (call right after loading the file).
    pub fn new(path: &Path) -> Self {
        Self(Mutex::new(stamp_of(path)))
    }

    /// True when the file changed since the last `new` / `note_written` / `changed`.
    pub fn changed(&self, path: &Path) -> bool {
        let current = stamp_of(path);
        let mut seen = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *seen == current {
            return false;
        }
        *seen = current;
        true
    }

    /// Record our own write so it is not mistaken for an external change.
    pub fn note_written(&self, path: &Path) {
        let current = stamp_of(path);
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = current;
    }

    /// Stamp captured when the in-memory copy was last aligned with the file.
    pub fn observed(&self) -> ObservedFile {
        match *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            Some((time, len)) => ObservedFile {
                exists: true,
                mtime_ns: mtime_to_ns(time),
                len,
            },
            None => ObservedFile {
                exists: false,
                mtime_ns: "0".into(),
                len: 0,
            },
        }
    }

    /// Remember a stamp reported by the process that actually wrote the file.
    pub fn note_signature(&self, mtime_ns: &str, len: u64) {
        let Ok(nanos) = mtime_ns.parse::<u64>() else {
            self.force_reload();
            return;
        };
        let time = UNIX_EPOCH + Duration::from_nanos(nanos);
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((time, len));
    }

    /// Next `changed` reloads, even if the file bytes are untouched.
    pub fn force_reload(&self) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((UNIX_EPOCH, u64::MAX));
    }
}

fn mtime_to_ns(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

/// True when `path` still has the stamp the caller observed.
pub fn signature_matches(path: &Path, exists: bool, mtime_ns: &str, len: u64) -> bool {
    match stamp_of(path) {
        Some((time, file_len)) => exists && file_len == len && mtime_to_ns(time) == mtime_ns,
        None => !exists,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_external_rewrite_but_not_own_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        std::fs::write(&path, "{}").unwrap();
        let stamp = FileStamp::new(&path);
        assert!(!stamp.changed(&path));

        std::fs::write(&path, "{\"accounts\":[]}").unwrap();
        stamp.note_written(&path);
        assert!(!stamp.changed(&path));

        std::fs::write(&path, "{\"accounts\":[1,2,3]}").unwrap();
        assert!(stamp.changed(&path));
        assert!(!stamp.changed(&path));
    }

    #[test]
    fn observed_signature_matches_metadata_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        std::fs::write(&path, "abc").unwrap();
        let stamp = FileStamp::new(&path);
        let observed = stamp.observed();
        assert!(signature_matches(
            &path,
            observed.exists,
            &observed.mtime_ns,
            observed.len
        ));
        stamp.force_reload();
        assert!(stamp.changed(&path));
        stamp.note_signature(&observed.mtime_ns, observed.len);
        assert!(!stamp.changed(&path));
    }
}
