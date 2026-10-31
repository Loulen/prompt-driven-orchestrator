//! Layer 3a — `pdo run status` through the real binary, against a real daemon:
//! one line for a found Run (exit 0), a compact JSON object with `--json`, and
//! exit `4` for an id the daemon does not know.

use std::process::Command;

use crate::common::TestDaemon;
use crate::declared_wait::{create_run, seed, wait_node_status, CHILD, PARENT};

/// Run the real binary out of any node session, on a blocking task so the host
/// runtime keeps serving the daemon it talks to.
async fn pdo(daemon: &TestDaemon, args: &[&str]) -> (Option<i32>, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let url = daemon.url();
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        cmd.args(&args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("PDO_DAEMON_URL", &url)
            .output()
            .expect("failed to spawn pdo")
    })
    .await
    .expect("blocking task panicked");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("panicked"), "pdo panicked: {stderr}");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr,
    )
}

#[tokio::test]
async fn a_found_run_is_one_line_and_its_json_one_compact_object() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &parent, "worker", "running").await;
    let child = create_run(&daemon, CHILD, Some((parent.as_str(), "worker"))).await;
    wait_node_status(&daemon, &child, "doer", "running").await;

    let (code, stdout, stderr) = pdo(&daemon, &["run", "status", &parent]).await;
    assert_eq!(code, Some(0), "stdout=\n{stdout}\nstderr=\n{stderr}");
    assert_eq!(stdout.lines().count(), 1, "one line: {stdout}");
    let line = stdout.trim_end();
    assert!(
        line.starts_with(&format!(
            "{parent} running · nodes 0/1 done, 1 running · children 1 active · $"
        )),
        "{line}"
    );

    let (code, stdout, stderr) = pdo(&daemon, &["run", "status", &parent, "--json"]).await;
    assert_eq!(code, Some(0), "stdout=\n{stdout}\nstderr=\n{stderr}");
    assert_eq!(stdout.lines().count(), 1, "one compact line: {stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["run_id"], parent.as_str());
    assert_eq!(json["status"], "running");
    assert_eq!(json["nodes_done"], 0);
    assert_eq!(json["nodes_total"], 1);
    assert_eq!(json["nodes_running"], 1);
    assert_eq!(json["nodes_awaiting_user"], 0);
    assert_eq!(json["nodes_failed"], 0);
    assert_eq!(json["children_active"], 1);
    assert!(json.get("cost_usd").is_some() && json.get("cost_partial").is_some());

    // The child has no children of its own.
    let (code, stdout, _) = pdo(&daemon, &["run", "status", &child]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("· children 0 active ·"), "{stdout}");
}

#[tokio::test]
async fn an_unknown_run_exits_4_with_a_clear_message() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (code, stdout, stderr) = pdo(&daemon, &["run", "status", "no-such-run"]).await;
    assert_eq!(code, Some(4), "stdout=\n{stdout}\nstderr=\n{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("no run no-such-run"), "{stderr}");
}

#[test]
fn an_unreachable_daemon_exits_1() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("PDO_") {
            cmd.env_remove(key);
        }
    }
    let output = cmd
        .args(["run", "status", "r1"])
        .env("PDO_DAEMON_URL", "http://127.0.0.1:1")
        .output()
        .expect("failed to spawn pdo");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("failed to reach daemon"), "{stderr}");
}
