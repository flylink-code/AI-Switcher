//! Per-account simulated device shared by the Antigravity and Kiro gateways.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::dao::settings::{get_setting, set_setting};
use crate::database::Database;
use crate::error::{AppError, AppResult};

/// Real client platforms only; an impossible os/arch pair would stand out upstream.
const PLATFORMS: &[(&str, &str)] = &[
    ("darwin", "arm64"),
    ("darwin", "amd64"),
    ("windows", "amd64"),
    ("linux", "amd64"),
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    /// IANA name; empty follows the global gateway timezone.
    #[serde(default)]
    pub timezone: String,
}

pub fn generate() -> DeviceProfile {
    let id = Uuid::new_v4();
    let (os, arch) = PLATFORMS[id.as_bytes()[0] as usize % PLATFORMS.len()];
    DeviceProfile {
        device_id: id.to_string(),
        os: os.into(),
        arch: arch.into(),
        timezone: String::new(),
    }
}

/// Stable device for accounts that never stored one, so existing accounts also
/// get distinct devices without rewriting the account file.
pub fn derive(seed: &str) -> DeviceProfile {
    let digest = Sha256::digest(format!("ai-switcher-device:{seed}").as_bytes());
    let (os, arch) = PLATFORMS[digest[0] as usize % PLATFORMS.len()];
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[1..17]);
    DeviceProfile {
        device_id: uuid::Builder::from_random_bytes(bytes).into_uuid().to_string(),
        os: os.into(),
        arch: arch.into(),
        timezone: String::new(),
    }
}

/// Empty means "follow the global timezone"; anything else must be an IANA name.
pub fn normalize_timezone(value: &str) -> AppResult<String> {
    let value = value.trim();
    if !value.is_empty() && value.parse::<chrono_tz::Tz>().is_err() {
        return Err(AppError::Config(format!("无效的时区: {value}")));
    }
    Ok(value.to_string())
}

const GLOBAL_TIMEZONE_SETTING: &str = "gateway_timezone";
const DEFAULT_GLOBAL_TIMEZONE: &str = "Asia/Singapore";

/// Unset means the default; an explicitly saved empty string means "follow system".
pub fn read_global_timezone(db: &Database) -> String {
    db.with_read_conn(|conn| get_setting(conn, GLOBAL_TIMEZONE_SETTING))
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_GLOBAL_TIMEZONE.to_string())
}

pub fn write_global_timezone(db: &Database, value: &str) -> AppResult<String> {
    let value = normalize_timezone(value)?;
    db.with_conn(|conn| set_setting(conn, GLOBAL_TIMEZONE_SETTING, &value))?;
    Ok(value)
}

/// `Platform:` / `Shell:` / `OS Version:` values Claude Code would report on this device.
fn environment_lines(device: &DeviceProfile) -> (&'static str, &'static str, &'static str) {
    match device.os.as_str() {
        "windows" => ("win32", "bash", "Windows 11 Enterprise 10.0.26200"),
        "linux" => ("linux", "bash", "Linux 6.8.0-45-generic"),
        _ if device.arch == "arm64" => ("darwin", "zsh", "Darwin 24.6.0"),
        _ => ("darwin", "zsh", "Darwin 23.6.0"),
    }
}

/// One rewritten environment field: label (`Platform`, ...), old value, new value.
pub type EnvChange = (&'static str, String, String);

/// Real environment values are short single tokens or phrases; anything longer or
/// containing code-like characters is prose that merely quotes the label.
fn is_env_value(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && !value.contains(['"', '\\', '(', ')', ';', '{', '}', '`'])
}

/// The value of an exact `Label: value` environment line (optionally `- ` prefixed).
fn env_line_value<'a>(line: &'a str, label: &str, max_len: usize) -> Option<&'a str> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("- ").unwrap_or(rest).trim_start();
    let value = rest.strip_prefix(label)?.trim_end();
    is_env_value(value, max_len).then_some(value)
}

/// Rewrite one whole line if it is exactly `label` + a plausible value.
fn rewrite_env_line(
    line: &str,
    label: &str,
    name: &'static str,
    value: &str,
    max_len: usize,
    changes: &mut Vec<EnvChange>,
) -> Option<String> {
    let old = env_line_value(line, label, max_len)?;
    let value_pos = line.find(label)? + label.len();
    changes.push((name, old.to_string(), value.to_string()));
    Some(format!("{}{}", &line[..value_pos], value))
}

/// Make the environment block of a Claude Code request match the account's simulated
/// device: platform lines, and today's date in the account's timezone.
///
/// Works line by line and only touches a line that is exactly `Label: value`, so code or
/// prose elsewhere in the conversation that merely mentions a label is left alone.
pub fn rewrite_environment(
    text: &mut String,
    device: &DeviceProfile,
    timezone: &str,
    changes: &mut Vec<EnvChange>,
) {
    let (platform, shell, os_version) = environment_lines(device);
    let today = timezone
        .parse::<chrono_tz::Tz>()
        .ok()
        .map(|tz| chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string());
    let mut out = String::with_capacity(text.len());
    let mut touched = false;
    for line in text.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        let rewritten = rewrite_env_line(body, "Platform: ", "Platform", platform, 16, changes)
            .or_else(|| rewrite_env_line(body, "Shell: ", "Shell", shell, 16, changes))
            .or_else(|| rewrite_env_line(body, "OS Version: ", "OS Version", os_version, 80, changes))
            .or_else(|| {
                let today = today.as_deref()?;
                let old = body.trim().strip_prefix("Today's date is ")?.trim_end_matches('.');
                (old.len() == 10 && old.chars().all(|c| c.is_ascii_digit() || c == '-')).then(|| {
                    changes.push(("Date", old.to_string(), today.to_string()));
                    format!("Today's date is {today}.")
                })
            });
        match rewritten {
            Some(next) => {
                touched = true;
                out.push_str(&next);
            }
            None => out.push_str(body),
        }
        out.push_str(newline);
    }
    if touched {
        *text = out;
    }
}

const DEBUG_ENV_REWRITE_SETTING: &str = "gateway_debug_env_rewrite";

pub fn read_debug_env_rewrite(db: &Database) -> bool {
    db.with_read_conn(|conn| get_setting(conn, DEBUG_ENV_REWRITE_SETTING))
        .ok()
        .flatten()
        .is_some_and(|value| value == "1")
}

pub fn write_debug_env_rewrite(db: &Database, enabled: bool) -> AppResult<bool> {
    db.with_conn(|conn| {
        set_setting(conn, DEBUG_ENV_REWRITE_SETTING, if enabled { "1" } else { "0" })
    })?;
    Ok(enabled)
}

/// One log line per rewritten field; never logs prompt text, paths or tokens.
pub fn log_env_changes(gateway: &str, account_id: &str, user_agent: &str, changes: &[EnvChange]) {
    let id: String = account_id.chars().take(8).collect();
    let fields = changes
        .iter()
        .map(|(name, old, new)| format!("{name}: {old:?} -> {new:?}"))
        .collect::<Vec<_>>()
        .join("; ");
    log::info!("{gateway} env rewrite account={id} ua={user_agent:?} [{fields}]");
}

/// Apply [`rewrite_environment`] to every text block of an Anthropic `messages` request.
pub fn rewrite_request_environment(
    request: &mut serde_json::Value,
    device: &DeviceProfile,
    timezone: &str,
) -> Vec<EnvChange> {
    let mut changes = Vec::new();
    let Some(messages) = request
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return changes;
    };
    for message in messages {
        let Some(content) = message.get_mut("content") else {
            continue;
        };
        if let Some(text) = content.as_str() {
            let mut next = text.to_string();
            rewrite_environment(&mut next, device, timezone, &mut changes);
            *content = serde_json::Value::String(next);
        } else if let Some(blocks) = content.as_array_mut() {
            for block in blocks {
                let Some(text) = block.get("text").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let mut next = text.to_string();
                rewrite_environment(&mut next, device, timezone, &mut changes);
                block["text"] = serde_json::Value::String(next);
            }
        }
    }
    changes
}

/// Same rewrite for a Gemini-shaped request (`contents[].parts[].text`).
pub fn rewrite_gemini_environment(
    request: &mut serde_json::Value,
    device: &DeviceProfile,
    timezone: &str,
) -> Vec<EnvChange> {
    let mut changes = Vec::new();
    let Some(contents) = request
        .get_mut("contents")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return changes;
    };
    for content in contents {
        let Some(parts) = content
            .get_mut("parts")
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for part in parts {
            let Some(text) = part.get("text").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let mut next = text.to_string();
            rewrite_environment(&mut next, device, timezone, &mut changes);
            part["text"] = serde_json::Value::String(next);
        }
    }
    changes
}

/// Account timezone, else the global one; empty means "leave the date alone".
pub fn effective_timezone(profile_timezone: &str, db: &Database) -> String {
    if profile_timezone.is_empty() {
        read_global_timezone(db)
    } else {
        profile_timezone.to_string()
    }
}

/// `std::env::consts::OS` spelling (`macos`), as Kiro's `envState.operatingSystem` expects.
pub fn rust_os(os: &str) -> &str {
    if os == "darwin" {
        "macos"
    } else {
        os
    }
}

/// Node.js `process.platform` spelling used inside AWS SDK user agents.
pub fn node_platform(os: &str) -> &str {
    if os == "windows" {
        "win32"
    } else {
        os
    }
}

/// New random device that keeps the account's timezone choice.
pub fn regenerate(current: Option<&DeviceProfile>) -> DeviceProfile {
    let mut next = generate();
    if let Some(current) = current {
        next.timezone = current.timezone.clone();
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_device_uses_a_real_platform() {
        for _ in 0..32 {
            let profile = generate();
            assert!(PLATFORMS.contains(&(profile.os.as_str(), profile.arch.as_str())));
            assert!(!profile.device_id.is_empty());
        }
    }

    #[test]
    fn derived_device_is_stable_per_seed_and_differs_across_seeds() {
        assert_eq!(derive("a1"), derive("a1"));
        assert_ne!(derive("a1").device_id, derive("a2").device_id);
        let profile = derive("a1");
        assert!(PLATFORMS.contains(&(profile.os.as_str(), profile.arch.as_str())));
    }

    #[test]
    fn timezone_must_be_iana_or_empty() {
        assert_eq!(normalize_timezone(" Asia/Shanghai ").unwrap(), "Asia/Shanghai");
        assert_eq!(normalize_timezone("").unwrap(), "");
        assert!(normalize_timezone("Mars/Base").is_err());
    }

    fn mac() -> DeviceProfile {
        DeviceProfile {
            device_id: "d".into(),
            os: "darwin".into(),
            arch: "arm64".into(),
            timezone: String::new(),
        }
    }

    #[test]
    fn rewrite_changes_only_the_environment_values() {
        let mut text = "- Primary working directory: E:\\repo\n - Platform: win32\n - Shell: bash\n - OS Version: Windows 11 Enterprise 10.0.26200\n - Other: keep\nToday's date is 2000-01-01.\nmore".to_string();
        rewrite_environment(&mut text, &mac(), "Asia/Singapore", &mut Vec::new());
        assert!(text.contains(" - Platform: darwin\n"));
        assert!(text.contains(" - Shell: zsh\n"));
        assert!(text.contains(" - OS Version: Darwin 24.6.0\n"));
        assert!(text.contains("Primary working directory: E:\\repo\n"));
        assert!(text.contains(" - Other: keep\n"));
        assert!(!text.contains("2000-01-01"));
        assert!(text.ends_with(".\nmore"));
    }

    #[test]
    fn quoted_code_and_prose_mentioning_labels_are_left_alone() {
        let code = "replace_line_value(text, \"Platform: \", platform);\nlet s = \"Shell: bash\";\n    println!(\"OS Version: {v}\");\nPlatform: the platform we run on, e.g. a long sentence about it\nI said Platform: win32 in passing\nToday's date is a string literal\n";
        let mut text = code.to_string();
        let mut changes = Vec::new();
        rewrite_environment(&mut text, &mac(), "Asia/Singapore", &mut changes);
        assert_eq!(text, code);
        assert!(changes.is_empty());
    }

    #[test]
    fn changes_never_hold_long_values() {
        let mut text = format!(" - Platform: {}\n - OS Version: Windows 11\n", "x".repeat(500));
        let mut changes = Vec::new();
        rewrite_environment(&mut text, &mac(), "Asia/Singapore", &mut changes);
        assert!(changes.iter().all(|c| c.1.len() <= 80));
        assert!(text.starts_with(&format!(" - Platform: {}", "x".repeat(500))));
    }

    #[test]
    fn rewrite_is_a_noop_without_environment_text() {
        let mut text = "hello world".to_string();
        rewrite_environment(&mut text, &mac(), "Asia/Singapore", &mut Vec::new());
        assert_eq!(text, "hello world");
    }

    #[test]
    fn rewrite_request_walks_string_and_block_content() {
        let mut request = serde_json::json!({
            "messages": [
                { "role": "user", "content": "Platform: win32\n" },
                { "role": "user", "content": [
                    { "type": "text", "text": "Platform: win32\n" },
                    { "type": "image", "source": {} }
                ]}
            ]
        });
        rewrite_request_environment(&mut request, &mac(), "Asia/Singapore");
        assert_eq!(request["messages"][0]["content"], "Platform: darwin\n");
        assert_eq!(request["messages"][1]["content"][0]["text"], "Platform: darwin\n");
    }

    #[test]
    fn rewrite_gemini_request_only_touches_text_parts() {
        let mut request = serde_json::json!({
            "contents": [{ "role": "user", "parts": [
                { "text": "Platform: win32\n" },
                { "inlineData": { "mimeType": "image/png", "data": "Platform: win32" } }
            ]}]
        });
        rewrite_gemini_environment(&mut request, &mac(), "Asia/Singapore");
        assert_eq!(request["contents"][0]["parts"][0]["text"], "Platform: darwin\n");
        assert_eq!(
            request["contents"][0]["parts"][1]["inlineData"]["data"],
            "Platform: win32"
        );
    }

    #[test]
    fn changes_report_old_and_new_values_without_touching_other_text() {
        let mut request = serde_json::json!({
            "messages": [{ "role": "user", "content":
                "SECRET prompt\n - Primary working directory: E:\\private\n - Platform: win32\n - Shell: bash\n - OS Version: Windows 11\nToday's date is 2000-01-01.\n"
            }]
        });
        let changes = rewrite_request_environment(&mut request, &mac(), "Asia/Singapore");
        let names: Vec<_> = changes.iter().map(|c| c.0).collect();
        assert_eq!(names, ["Platform", "Shell", "OS Version", "Date"]);
        assert_eq!(changes[0].1, "win32");
        assert_eq!(changes[0].2, "darwin");
        let joined = format!("{changes:?}");
        assert!(!joined.contains("SECRET"));
        assert!(!joined.contains("private"));
    }

    #[test]
    fn no_environment_text_means_no_changes_to_log() {
        let mut request = serde_json::json!({ "messages": [{ "role": "user", "content": "hi" }] });
        assert!(rewrite_request_environment(&mut request, &mac(), "Asia/Singapore").is_empty());
    }

    #[test]
    fn debug_env_rewrite_is_off_by_default_and_toggles() {
        let db = Database::memory().unwrap();
        assert!(!read_debug_env_rewrite(&db));
        write_debug_env_rewrite(&db, true).unwrap();
        assert!(read_debug_env_rewrite(&db));
        write_debug_env_rewrite(&db, false).unwrap();
        assert!(!read_debug_env_rewrite(&db));
    }

    #[test]
    fn effective_timezone_prefers_the_account_over_global() {
        let db = Database::memory().unwrap();
        assert_eq!(effective_timezone("Asia/Tokyo", &db), "Asia/Tokyo");
        assert_eq!(effective_timezone("", &db), "Asia/Singapore");
    }

    #[test]
    fn date_follows_the_account_timezone() {
        let mut tokyo = "Today's date is 2000-01-01.".to_string();
        let mut honolulu = tokyo.clone();
        rewrite_environment(&mut tokyo, &mac(), "Asia/Tokyo", &mut Vec::new());
        rewrite_environment(&mut honolulu, &mac(), "Pacific/Honolulu", &mut Vec::new());
        let parse = |s: &str| {
            chrono::NaiveDate::parse_from_str(
                s.trim_start_matches("Today's date is ").trim_end_matches('.'),
                "%Y-%m-%d",
            )
            .unwrap()
        };
        let gap = (parse(&tokyo) - parse(&honolulu)).num_days();
        assert!((0..=1).contains(&gap));
    }

    #[test]
    fn global_timezone_defaults_to_singapore_and_keeps_explicit_choice() {
        let db = Database::memory().unwrap();
        assert_eq!(read_global_timezone(&db), "Asia/Singapore");
        write_global_timezone(&db, "Asia/Tokyo").unwrap();
        assert_eq!(read_global_timezone(&db), "Asia/Tokyo");
        write_global_timezone(&db, "").unwrap();
        assert_eq!(read_global_timezone(&db), "");
    }

    #[test]
    fn regenerate_changes_device_but_keeps_timezone() {
        let current = DeviceProfile {
            device_id: "old".into(),
            os: "linux".into(),
            arch: "amd64".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let next = regenerate(Some(&current));
        assert_ne!(next.device_id, "old");
        assert_eq!(next.timezone, "Asia/Shanghai");
    }
}
