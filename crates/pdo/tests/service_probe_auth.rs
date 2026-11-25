//! Layer 3 (CORE-326) — `pdo service install`'s port probe recognises a PDO
//! daemon whose API an identity extension gates: the anonymous `GET /sessions`
//! gets the core's `401 {"error": "unauthenticated"}`, which still means "a PDO
//! daemon owns this port". The one-click Update runs `service install` against
//! the running daemon; before this fix the probe classed it foreign, the step
//! refused and the Update rolled back. A non-PDO `401` stays foreign.
//!
//! Dry-run only: systemd / launchd are never touched.

use std::io::{Read, Write};
use std::process::Command;

use pdo::extension_api::Extension;

use crate::common::{flow, TestDaemon};
use crate::test_extension::gate::Gate;
use crate::test_extension::{Refusal, TestExtension};

const ALREADY_ANSWERING: &str = "a PDO daemon is already answering";
const WOULD_REFUSE: &str = "a real install would REFUSE";

async fn daemon(gate: Gate) -> TestDaemon {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    control.set_gate(gate);
    let extensions: Vec<Box<dyn Extension>> = vec![Box::new(ext)];
    TestDaemon::spawn_with_extensions(|repo| flow::seed_solo(repo, &[("solo", false)]), extensions)
        .await
        .unwrap()
}

/// `pdo service install --dry-run` on `127.0.0.1:<port>`, anonymous (every
/// `PDO_*` removed, `PDO_TOKEN` included), in a throwaway home.
async fn dry_run_install(port: u16) -> String {
    tokio::task::spawn_blocking(move || {
        let home = tempfile::tempdir().expect("tempdir");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        let out = cmd
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .args(["service", "install", "--dry-run", "--bind", "127.0.0.1"])
            .args(["--port", &port.to_string()])
            .output()
            .expect("run pdo service install --dry-run");
        assert!(
            out.status.success(),
            "dry-run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("dry-run output is UTF-8")
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_daemon_gated_by_an_identity_extension_is_recognised() {
    let daemon = daemon(Gate::token("s3cret")).await;
    let anonymous = reqwest::get(format!("{}/sessions", daemon.url()))
        .await
        .unwrap();
    assert_eq!(anonymous.status().as_u16(), 401, "the API is gated");

    let plan = dry_run_install(daemon.addr.port()).await;
    assert!(plan.contains(ALREADY_ANSWERING), "{plan}");
    assert!(!plan.contains(WOULD_REFUSE), "{plan}");
}

#[tokio::test]
async fn an_ungated_daemon_is_still_recognised() {
    let daemon = daemon(Gate::default()).await;
    let plan = dry_run_install(daemon.addr.port()).await;
    assert!(plan.contains(ALREADY_ANSWERING), "{plan}");
    assert!(!plan.contains(WOULD_REFUSE), "{plan}");
}

#[tokio::test]
async fn a_foreign_401_is_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let body = "<html><body>401 unauthenticated</body></html>";
            let reply = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });

    let plan = dry_run_install(port).await;
    assert!(
        plan.contains("already bound by a non-PDO process"),
        "{plan}"
    );
    assert!(plan.contains(WOULD_REFUSE), "{plan}");
}
