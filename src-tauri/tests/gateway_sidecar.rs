//! Runs the real executable as the gateway sidecar in an isolated home.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, UNIX_EPOCH};

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
    assert!(
        stop_gracefully(child),
        "sidecar did not exit cleanly after stdin close"
    );
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
        wait_for(Duration::from_secs(10), || matches!(
            child.try_wait(),
            Ok(Some(_))
        )),
        "sidecar outlived its parent pipe"
    );
}

fn commit_line(id: u64, store: &str, body: &str, exists: bool, mtime_ns: &str, len: u64) -> String {
    let body = serde_json::to_string(body).unwrap();
    format!(
        r#"{{"id":{id},"op":"commit","store":"{store}","body":{body},"base":{{"exists":{exists},"mtime_ns":"{mtime_ns}","len":{len}}}}}"#
    )
}

fn file_base(path: &Path) -> (bool, String, u64) {
    let meta = std::fs::metadata(path).unwrap();
    let mtime_ns = meta
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    (true, mtime_ns, meta.len())
}

fn spawn_line_reader(stdout: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(line.trim().to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

#[test]
fn sidecar_applies_account_commit_and_rejects_a_stale_baseline() {
    let home = tempfile::tempdir().unwrap();
    let mut child = sidecar_command(home.path(), VERSION)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let lines = spawn_line_reader(child.stdout.take().unwrap());
    let mut stdin = child.stdin.take().unwrap();

    let body = r#"{"accounts":[],"activeId":"from-main"}"#;
    writeln!(stdin, "{}", commit_line(1, "kiro", body, false, "0", 0)).unwrap();
    stdin.flush().unwrap();
    let response = lines
        .recv_timeout(Duration::from_secs(40))
        .expect("sidecar did not answer the account commit");
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["ok"], true, "{response}");

    let path = home
        .path()
        .join(".claude-switcher")
        .join("kiro_accounts.json");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);

    writeln!(
        stdin,
        "{}",
        commit_line(
            2,
            "kiro",
            r#"{"accounts":[],"activeId":"stale"}"#,
            false,
            "0",
            0
        )
    )
    .unwrap();
    stdin.flush().unwrap();
    let rejected = lines.recv_timeout(Duration::from_secs(10)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&rejected).unwrap();
    assert_eq!(parsed["ok"], false, "{rejected}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);

    let (exists, mtime_ns, len) = file_base(&path);
    let next = r#"{"accounts":[],"activeId":"second"}"#;
    writeln!(
        stdin,
        "{}",
        commit_line(3, "kiro", next, exists, &mtime_ns, len)
    )
    .unwrap();
    stdin.flush().unwrap();
    let accepted = lines.recv_timeout(Duration::from_secs(10)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&accepted).unwrap();
    assert_eq!(parsed["ok"], true, "{accepted}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), next);

    drop(stdin);
    assert!(
        wait_for(Duration::from_secs(10), || {
            matches!(child.try_wait(), Ok(Some(status)) if status.success())
        }),
        "sidecar did not exit cleanly after stdin close"
    );
}

#[test]
fn supervisor_restarts_a_killed_sidecar_and_stops() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(EXE)
        .env("AISW_SUPERVISOR_SELFTEST", "1")
        .env("AISW_ALLOW_TEST_HOME", "1")
        .env("AISW_TEST_HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "selftest failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
