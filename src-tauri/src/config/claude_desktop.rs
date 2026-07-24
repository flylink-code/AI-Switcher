//! Claude Desktop configuration directory discovery.
//!
//! Claude Desktop stores its per-provider gateway configs under a `configLibrary`
//! folder inside its install/support directory. The candidate order mirrors the
//! reference implementation in `examples/cc-proxy-master/claude_config.py`:
//!
//! - Windows: `%LOCALAPPDATA%\Claude`, then `%LOCALAPPDATA%\ClaudeZhCN`
//!   (the Chinese-locale folder name), then `%APPDATA%\Claude`.
//! - macOS: `~/Library/Application Support/Claude`.
//!
//! Linux is unsupported (Claude Desktop does not ship there); detection returns
//! `None`.

use serde::Serialize;
use std::path::PathBuf;

/// Subdirectory inside the Claude install dir that holds provider configs.
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
/// The registry file listing available provider entries + the applied one.
const META_FILE: &str = "_meta.json";

/// All Claude Desktop paths relevant to config writing. Fields are `None` when
/// the platform is unsupported or Claude Desktop is not installed.
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeDesktopPaths {
    /// Install/support base dir (e.g. `%LOCALAPPDATA%\Claude`).
    pub base: Option<PathBuf>,
    /// `configLibrary` dir inside base.
    pub config_library: Option<PathBuf>,
    /// `configLibrary/_meta.json`.
    pub meta_path: Option<PathBuf>,
}

impl ClaudeDesktopPaths {
    fn not_detected() -> Self {
        ClaudeDesktopPaths {
            base: None,
            config_library: None,
            meta_path: None,
        }
    }
}

/// Whether Claude Desktop config management is supported on this OS.
pub fn is_supported_platform() -> bool {
    cfg!(target_os = "windows") || cfg!(target_os = "macos")
}

/// Probe candidate directories and return the first that exists, along with its
/// `configLibrary` and `_meta.json` paths.
pub fn detect_claude_desktop() -> ClaudeDesktopPaths {
    if !is_supported_platform() {
        return ClaudeDesktopPaths::not_detected();
    }

    for candidate in candidate_base_dirs() {
        if candidate.is_dir() {
            let config_library = candidate.join(CONFIG_LIBRARY_DIR);
            let meta_path = config_library.join(META_FILE);
            return ClaudeDesktopPaths {
                base: Some(candidate),
                config_library: Some(config_library),
                meta_path: Some(meta_path),
            };
        }
    }
    ClaudeDesktopPaths::not_detected()
}

/// Ordered list of base directories to probe for the current platform.
fn candidate_base_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let roaming_app_data = std::env::var_os("APPDATA").map(PathBuf::from);
        if let Some(lad) = local_app_data {
            out.push(lad.join("Claude"));
            out.push(lad.join("ClaudeZhCN"));
        }
        if let Some(rad) = roaming_app_data {
            out.push(rad.join("Claude"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            out.push(home.join("Library/Application Support/Claude"));
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Unsupported platform; no candidates. Nothing to add.
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_returns_none() {
        // This test is only meaningful on Linux; on win/mac detection may succeed.
        if !is_supported_platform() {
            let p = detect_claude_desktop();
            assert!(p.base.is_none());
        }
    }

    #[test]
    fn candidate_dirs_nonempty_on_supported() {
        if is_supported_platform() {
            assert!(!candidate_base_dirs().is_empty());
        }
    }
}
