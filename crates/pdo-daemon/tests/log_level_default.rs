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
        // #427: this harness runs the REAL binary, so it goes through
        // `DaemonConfig::from_env()` where the boot price refresh defaults to armed.
        // It would still make no request (a fresh tempdir has no `fetched.json`, and
        // the boot pass refreshes rather than seeds), but a test must not depend on
        // the network being irrelevant — it must not reach for it at all.
        .env("PDO_PRICE_SYNC", "off")
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

    // Poll for the line rather than sleeping a fixed 2 s: under the full suite
    // (thousands of tests in parallel) a cold daemon can take longer than that to
    // bind and flush, and the fixed window made this test flake on load alone.
    // Return as soon as the line shows up; give up after a generous deadline.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let seen = buf
            .lock()
            .map(|g| g.contains("INFO") || g.contains("listening"))
            .unwrap_or(false);
        if seen || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    let collected = buf.lock().map(|g| g.clone()).unwrap_or_default();
    assert!(
        collected.contains("INFO") || collected.contains("listening"),
        "expected an INFO-level startup line in stderr; got:\n---\n{collected}\n---"
    );
}
