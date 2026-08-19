//! Cross-cutting process helpers for Windows GUI launches.

use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::error::{AppError, AppResult};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Upper bound for plugin CLI (`claude plugin` / `codex plugin`) including marketplace git fetch.
pub const CLI_COMMAND_TIMEOUT: Duration = Duration::from_secs(90);

/// Hide the console window when spawning helper processes from a GUI app.
pub fn apply_no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

/// Run `f` off the UI/async worker so Tauri commands cannot freeze the window.
pub async fn spawn_blocking_result<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Other(format!("后台任务失败: {e}")))?
}

/// `Command::output()` with a hard deadline. On timeout the process tree is killed (Windows `/T`).
pub fn output_with_timeout(command: &mut Command, timeout: Duration) -> AppResult<Output> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let child = command
        .spawn()
        .map_err(|e| AppError::Other(format!("启动进程失败: {e}")))?;
    let pid = child.id();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(AppError::Other(format!("等待进程退出失败: {e}"))),
        Err(_) => {
            kill_process_tree(pid);
            let _ = rx.recv_timeout(Duration::from_secs(3));
            Err(AppError::Other(format!(
                "命令超时（{}s），已终止进程",
                timeout.as_secs()
            )))
        }
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_no_window(&mut command);
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        let pid_text = pid.to_string();
        let _ = Command::new("pkill")
            .args(["-TERM", "-P", &pid_text])
            .status();
        let _ = Command::new("kill").args(["-TERM", &pid_text]).status();
        std::thread::sleep(Duration::from_millis(200));
        let _ = Command::new("pkill")
            .args(["-KILL", "-P", &pid_text])
            .status();
        let _ = Command::new("kill").args(["-KILL", &pid_text]).status();
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn output_with_timeout_kills_a_hanging_process() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "30", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        apply_no_window(&mut command);
        let started = Instant::now();
        let error = output_with_timeout(&mut command, Duration::from_secs(1)).unwrap_err();
        assert!(
            error.to_string().contains("超时"),
            "unexpected error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "timeout took too long: {:?}",
            started.elapsed()
        );
    }
}
