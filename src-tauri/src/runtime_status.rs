//! Detect whether Claude Code / Claude Desktop / Codex are currently running.
//!
//! Used by the shell header. Classification prefers the process image path so
//! `claude.exe` from npm (Code) is not confused with Anthropic's Desktop app.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAppRuntimeStatus {
    pub claude_code: bool,
    pub claude_desktop: bool,
    pub codex: bool,
    pub opencode: bool,
}

pub fn get_managed_apps_runtime_status() -> ManagedAppRuntimeStatus {
    #[cfg(windows)]
    {
        windows_status()
    }
    #[cfg(not(windows))]
    {
        unix_status()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppKind {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    OpenCode,
}

fn classify_process(image_path: &str) -> Option<AppKind> {
    let normalized = image_path.replace('\\', "/").to_ascii_lowercase();
    // Drop argv after " --" so Linux `claude-desktop --type=gpu` still matches,
    // without splitting Windows paths that contain spaces (`Program Files`).
    let without_args = normalized.split_once(" --").map(|(path, _)| path).unwrap_or(&normalized);
    let file_name = without_args.rsplit('/').next().unwrap_or(without_args);

    if file_name == "codex.exe" || file_name == "codex" {
        return Some(AppKind::Codex);
    }

    if file_name == "opencode.exe" || file_name == "opencode" {
        return Some(AppKind::OpenCode);
    }

    if file_name == "claude-desktop" || file_name == "claude-desktop.exe" {
        return Some(AppKind::ClaudeDesktop);
    }

    if file_name != "claude.exe" && file_name != "claude" {
        return None;
    }

    // Claude Desktop install layouts
    if normalized.contains("anthropicclaude")
        || normalized.contains("/claude/claude.exe")
        || normalized.contains("/claude app/")
        || normalized.contains("claude desktop")
    {
        return Some(AppKind::ClaudeDesktop);
    }

    // Claude Code CLI / npm / fnm / local bin
    if normalized.contains("claude-code")
        || normalized.contains("/npm/")
        || normalized.contains("\\npm\\")
        || normalized.contains("/.local/bin/")
        || normalized.contains("/fnm/")
        || normalized.contains("/nvs/")
        || normalized.contains("/volta/")
        || normalized.contains("/asdf/")
    {
        return Some(AppKind::ClaudeCode);
    }

    // Bare `Claude.exe` with no path hints is almost always Desktop on Windows.
    Some(AppKind::ClaudeDesktop)
}

#[cfg(windows)]
fn windows_status() -> ManagedAppRuntimeStatus {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut status = ManagedAppRuntimeStatus {
        claude_code: false,
        claude_desktop: false,
        codex: false,
        opencode: false,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return status;
    }

    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let name_len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe_name = std::ffi::OsString::from_wide(&entry.szExeFile[..name_len])
            .to_string_lossy()
            .to_ascii_lowercase();

        let interesting = matches!(
            exe_name.as_str(),
            "claude.exe" | "claude" | "codex.exe" | "codex" | "opencode.exe" | "opencode"
        );
        if interesting {
            let image = process_image_path(entry.th32ProcessID).unwrap_or(exe_name.clone());
            match classify_process(&image) {
                Some(AppKind::ClaudeCode) => status.claude_code = true,
                Some(AppKind::ClaudeDesktop) => status.claude_desktop = true,
                Some(AppKind::Codex) => status.codex = true,
                Some(AppKind::OpenCode) => status.opencode = true,
                None => {}
            }
        }

        if status.claude_code && status.claude_desktop && status.codex && status.opencode {
            break;
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    unsafe {
        CloseHandle(snapshot);
    }
    status
}

#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let mut buffer = vec![0u16; 1024];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 || size == 0 {
        return None;
    }
    Some(
        std::ffi::OsString::from_wide(&buffer[..size as usize])
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(not(windows))]
fn unix_status() -> ManagedAppRuntimeStatus {
    use std::process::Command;

    let mut status = ManagedAppRuntimeStatus {
        claude_code: false,
        claude_desktop: false,
        codex: false,
        opencode: false,
    };

    let Ok(output) = Command::new("ps").args(["-ax", "-o", "command="]).output() else {
        return status;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        match classify_process(line.trim()) {
            Some(AppKind::ClaudeCode) => status.claude_code = true,
            Some(AppKind::ClaudeDesktop) => status.claude_desktop = true,
            Some(AppKind::Codex) => status.codex = true,
            Some(AppKind::OpenCode) => status.opencode = true,
            None => {}
        }
        if status.claude_code && status.claude_desktop && status.codex && status.opencode {
            break;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_desktop_and_code_claude_paths() {
        assert_eq!(
            classify_process(r"C:\Users\a\AppData\Local\AnthropicClaude\claude.exe"),
            Some(AppKind::ClaudeDesktop)
        );
        assert_eq!(
            classify_process(r"C:\Users\a\AppData\Roaming\npm\claude.exe"),
            Some(AppKind::ClaudeCode)
        );
        assert_eq!(
            classify_process(r"C:\Users\a\AppData\Roaming\npm\codex.exe"),
            Some(AppKind::Codex)
        );
        assert_eq!(
            classify_process(r"C:\Users\a\.opencode\bin\opencode.exe"),
            Some(AppKind::OpenCode)
        );
        assert_eq!(
            classify_process(r"C:\Program Files\Something\Claude.exe"),
            Some(AppKind::ClaudeDesktop)
        );
        assert_eq!(
            classify_process("/usr/bin/claude-desktop --type=gpu-process"),
            Some(AppKind::ClaudeDesktop)
        );
    }
}
