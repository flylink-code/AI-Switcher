//! Cheap "did another process rewrite this file?" check.
//!
//! The Antigravity / Kiro account stores keep an in-memory copy. When the proxy
//! gateways run in a sidecar process, both processes share the same account
//! files, so each store re-reads its file when the on-disk stamp changes.

use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

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
        let mut seen = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if *seen == current {
            return false;
        }
        *seen = current;
        true
    }

    /// Record our own write so it is not mistaken for an external change.
    pub fn note_written(&self, path: &Path) {
        let current = stamp_of(path);
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = current;
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
}
