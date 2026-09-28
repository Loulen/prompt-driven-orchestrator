//! Layer 3a — the exit codes of `pdo wait-user` and `pdo run wait` (ADR-0069),
//! through the real binary: part of the same public contract as
//! `pdo complete`'s (`cli_complete_does_not_panic.rs`).
//!
//! `3` — `wait-user` refused, still your turn · `2` — `run wait` timed out, not
//! a failure (nothing on stdout).

use std::process::Command;

use crate::common::TestDaemon;
use crate::declared_wait::{create_run, seed, wait_node_status, CHILD, PARENT};

/// Run the real binary as the agent of `run_id`/`node_id`, on a blocking task
/// so the host runtime keeps serving the daemon the binary is waiting on.
async fn pdo_as_node(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    args: &[&str],
) -> (Option<i32>, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (url, run_id, node_id) = (daemon.url(), run_id.to_string(), node_id.to_string());
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_pdo"))
            .args(&args)
            .env("PDO_RUN_ID", &run_id)
            .env("PDO_NODE_ID", &node_id)
            .env("PDO_NODE_ITER", "1")
            .env("PDO_DAEMON_URL", &url)
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

/// Exit `3`: the node's tmux session is gone, so the daemon refuses the wait
/// (`409 node_session_not_live`) and the node is still the caller's.
#[tokio::test]
async fn pdo_wait_user_exits_3_on_a_refusal() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let session = pdo_session::domain::names::node_session_name(&run_id, "worker", 1);
    let killed = Command::new("tmux")
        .args(["-L", &daemon.tmux_socket(), "kill-session", "-t", &session])
        .status()
        .unwrap();
    assert!(killed.success(), "kill-session {session}");

    let (code, _, stderr) = pdo_as_node(
        &daemon,
        &run_id,
        "worker",
        &["wait-user", "--message", "anyone?"],
    )
    .await;
    assert_eq!(code, Some(3), "stderr=\n{stderr}");
    assert!(stderr.contains("REFUSED"), "stderr=\n{stderr}");
    assert!(
        stderr.contains("node_session_not_live"),
        "stderr=\n{stderr}"
    );
    assert!(
        stderr.contains("Do NOT run `pdo fail`"),
        "stderr=\n{stderr}"
    );
}

/// Exit `2`: a child is still working when `--timeout` elapses. Not a failure:
/// the agent loops and calls again, and stdout stays empty.
#[tokio::test]
async fn pdo_run_wait_exits_2_on_a_timeout() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &parent, "worker", "running").await;
    let child = create_run(&daemon, CHILD, Some((parent.as_str(), "worker"))).await;
    wait_node_status(&daemon, &child, "doer", "running").await;

    let (code, stdout, stderr) = pdo_as_node(
        &daemon,
        &parent,
        "worker",
        &["run", "wait", "--timeout", "1"],
    )
    .await;
    assert_eq!(code, Some(2), "stdout=\n{stdout}\nstderr=\n{stderr}");
    assert!(
        stdout.trim().is_empty(),
        "a timeout prints nothing: {stdout}"
    );
}
