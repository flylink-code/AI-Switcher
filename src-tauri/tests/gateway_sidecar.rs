//! Runs the real executable as the gateway sidecar in an isolated home.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_claude-switcher");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn sidecar_command(home: &Path, version: &str) -> Command {
    let mut command = Command::new(EXE);
    command
        .args(["--gateway-sidecar", "--sidecar-version", version])
        .env("AISW_ALLOW_TEST_HOME", "1")
        .env("AISW_TEST_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn healthz(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    if stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 32];
    let read = stream.read(&mut buf).unwrap_or(0);
    buf[..read].starts_with(b"HTTP/1.") && buf[..read].windows(4).any(|w| w == b" 200")
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    condition()
}

/// Closing stdin is the documented stop signal; returns whether the child exited 0 in time.
fn stop_gracefully(mut child: Child) -> bool {
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return status.success();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

#[test]
fn version_mismatch_exits_with_code_3() {
    let home = tempfile::tempdir().unwrap();
    let status = sidecar_command(home.path(), "0.0.0").status().unwrap();
    assert_eq!(status.code(), Some(3));
}

#[test]
fn sidecar_serves_enabled_gateway_and_stops_on_stdin_close() {
    let home = tempfile::tempdir().unwrap();
    let db_path = home.path().join(".claude-switcher").join("app.db");

    // First run creates and migrates the database; nothing is enabled yet.
    let first = sidecar_command(home.path(), VERSION).spawn().unwrap();
    assert!(
        wait_for(Duration::from_secs(30), || db_path.exists()),
        "sidecar never created the database"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(stop_gracefully(first), "idle sidecar did not exit cleanly");

    let port = free_port();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES ('kiro_gateway_enabled', '1'), ('kiro_gateway_port', ?1)",
            [port.to_string()],
        )
        .unwrap();
    }

    let child = sidecar_command(home.path(), VERSION).spawn().unwrap();
    assert!(
        wait_for(Duration::from_secs(30), || healthz(port)),
        "kiro gateway never became healthy on {port}"
    );
    assert!(stop_gracefully(child), "sidecar did not exit cleanly after stdin close");
    assert!(
        wait_for(Duration::from_secs(5), || !healthz(port)),
        "port still answering after shutdown"
    );
}

#[test]
fn sidecar_exits_when_parent_pipe_breaks() {
    // Dropping the child's stdin without any other signal is what a crashed parent looks like.
    let home = tempfile::tempdir().unwrap();
    let mut child = sidecar_command(home.path(), VERSION).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    drop(child.stdin.take());
    assert!(
        wait_for(Duration::from_secs(10), || matches!(child.try_wait(), Ok(Some(_)))),
        "sidecar outlived its parent pipe"
    );
}
