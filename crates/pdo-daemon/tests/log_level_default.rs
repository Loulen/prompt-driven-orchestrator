//! Layer 3a — proves Bug C (#18) is fixed: when `RUST_LOG` is unset, the daemon
//! emits at least one INFO-level line to stderr instead of silently swallowing
//! diagnostics. The hours we burned debugging Bug A came from this very
//! near-silent default.
//!
//! Spawns the `pdo` binary in a subprocess with `RUST_LOG` removed so the
//! tracing subscriber falls through to the default filter. The startup banner
//! `PDO daemon listening on http://...` is emitted at INFO from `serve()`.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn info_logs_emitted_when_rust_log_is_unset() {
    let bin = env!("CARGO_BIN_EXE_pdo");

    // Daemon writes `.pdo/pdo.db` under CWD; point it at a tempdir so
    // it doesn't pollute the package directory when run under cargo test.
    let tempdir = tempfile::tempdir().expect("tempdir");

    let mut child = std::process::Command::new(bin)
        .current_dir(tempdir.path())
        .args(["daemon", "--port", "0"])
        .env_remove("RUST_LOG")
        .env_remove("PDO_TMUX_CMD_OVERRIDE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn pdo daemon");

    // Pipe reads block, and the daemon stays up indefinitely — drain stderr
    // on a worker thread into a shared buffer and let the main thread observe
    // it after a fixed window.
    let stderr = child.stderr.take().expect("stderr pipe missing");
    let buf = Arc::new(Mutex::new(String::new()));
    let buf_w = Arc::clone(&buf);

    let reader = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut g) = buf_w.lock() {
                g.push_str(&String::from_utf8_lossy(&chunk[..n]));
            }
        }
    });

    // 2s gives the runtime time to bind, log, and flush even on slow CI.
    std::thread::sleep(Duration::from_secs(2));

    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    let collected = buf.lock().map(|g| g.clone()).unwrap_or_default();
    assert!(
        collected.contains("INFO") || collected.contains("listening"),
        "expected an INFO-level startup line in stderr; got:\n---\n{collected}\n---"
    );
}
