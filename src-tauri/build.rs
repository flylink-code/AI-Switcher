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
    tauri_build::build()
}
