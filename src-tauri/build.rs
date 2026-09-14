use std::env;
use std::fs;
use std::path::Path;

/// 把仓库根目录 `.env` 的键值注入编译期环境（供源码里的 `env!` 使用）。
/// `.env` 已 gitignore；CI 可直接通过环境变量提供同名键，无需该文件。
fn load_dotenv() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dotenv = Path::new(&manifest_dir).join("..").join(".env");
    println!("cargo:rerun-if-changed={}", dotenv.display());
    let Ok(content) = fs::read_to_string(&dotenv) else {
        return;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !key.is_empty() {
            println!("cargo:rustc-env={key}={value}");
        }
    }
}

fn main() {
    load_dotenv();
    embed_windows_common_controls_manifest();
    tauri_build::build()
}

/// Cargo's `--lib` test harness is a console EXE without Tauri's `resource.lib`.
/// tao/wry import `TaskDialogIndirect` (comctl32 v6). Adding a second
/// `MANIFESTINPUT` to bins duplicates Tauri's RT_MANIFEST (CVT1100 / LNK1123).
/// `MANIFESTDEPENDENCY` is a linker flag (not a second resource) so it applies
/// to the `--lib` harness without colliding with the app binary.
fn embed_windows_common_controls_manifest() {
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest = Path::new(&manifest_dir).join("windows-common-controls.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!(
        "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
}
