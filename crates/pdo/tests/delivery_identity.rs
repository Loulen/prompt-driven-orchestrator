//! CORE-43 — **the delivery of a Node under its identity**, on a real daemon
//! with the test extension: the git identity an extension puts in a Node's
//! environment is frozen on its `NodeStarted` and signs every commit of its
//! delivery, isolated or not; nothing else of that environment reaches the
//! event log; and a daemon with no git identity of its own finishes the Run.
//!
//! The last case runs in a **child process** (this test binary, re-run on
//! [`a_run_without_daemon_identity_child`] alone): a daemon "without identity"
//! needs an environment with no `GIT_AUTHOR_*` / `GIT_COMMITTER_*` and no global
//! git configuration, and the test process cannot drop them for itself without
//! changing them for every other test running beside it.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::common::flow::{git, wait_node_status, wait_run_status};
use crate::common::{ensure_pdo_on_path, TestDaemon};
use crate::test_extension::{Control, Refusal, TestExtension};

const ALICE: &str = "Alice alice@example.com / Alice alice@example.com";

/// What the test extension puts in a Node's environment: `alice (test)`'s git
/// identity, and a secret that must never be frozen.
fn alice_env() -> pdo::extension_api::NodeEnv {
    [
        ("GIT_AUTHOR_NAME", "Alice"),
        ("GIT_AUTHOR_EMAIL", "alice@example.com"),
        ("GIT_COMMITTER_NAME", "Alice"),
        ("GIT_COMMITTER_EMAIL", "alice@example.com"),
        ("FIXTURE_SECRET", "s3cr3t-never-logged"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// `start → worker → end`, the worker a shared script writing `alice.rs`.
fn one_script(name: &str) -> String {
    format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  \
         - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  \
         - id: worker\n    name: worker\n    type: script\n    isolated_worktree: false\n    outputs:\n      - name: out\n  \
         - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\
         edges:\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: worker, port: in }}\n  \
         - source: {{ node: worker, port: out }}\n    target: {{ node: end, port: result }}\n"
    )
}

const WORKER: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\
    printf 'pub fn alice() {}\\n' > alice.rs\nprintf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n";

/// `start → {shared, forked} → end`: `shared` delivers in place at once, so when
/// the isolated `forked` comes back the Run's branch has moved and its
/// merge-back is a true merge commit.
fn shared_and_forked(name: &str) -> String {
    format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  \
         - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  \
         - id: shared\n    name: shared\n    type: script\n    isolated_worktree: false\n    outputs:\n      - name: out\n  \
         - id: forked\n    name: forked\n    type: script\n    isolated_worktree: true\n    outputs:\n      - name: out\n  \
         - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\
         edges:\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: shared, port: in }}\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: forked, port: in }}\n  \
         - source: {{ node: shared, port: out }}\n    target: {{ node: end, port: result }}\n  \
         - source: {{ node: forked, port: out }}\n    target: {{ node: end, port: result }}\n"
    )
}

/// Seed `.pdo/pipelines/<name>` with its scripts, then the repository: `.gitignore`
/// over PDO's directories, one commit. `identity: false` leaves the repository
/// with no `user.name` / `user.email` and forbids git to guess one.
fn seed(
    name: &'static str,
    yaml: String,
    scripts: Vec<(&'static str, String)>,
    identity: bool,
) -> impl FnOnce(&Path) -> anyhow::Result<()> {
    move |repo| {
        let dir = repo.join(".pdo/pipelines");
        std::fs::create_dir_all(dir.join(format!("{name}.prompts")))?;
        std::fs::write(dir.join(format!("{name}.yaml")), yaml)?;
        for (node, body) in scripts {
            std::fs::write(dir.join(format!("{name}.prompts/{node}.md")), body)?;
        }
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "commit.gpgsign", "false"]);
        if identity {
            git(repo, &["config", "user.name", "Daemon"]);
            git(repo, &["config", "user.email", "daemon@test.local"]);
        } else {
            git(repo, &["config", "user.useConfigOnly", "true"]);
        }
        std::fs::write(
            repo.join(".gitignore"),
            ".pdo/runs/\n.pdo/artifacts/\n.pdo/prompts/\n",
        )?;
        git(repo, &["add", "-A"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Seed",
                "-c",
                "user.email=seed@test.local",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        );
        Ok(())
    }
}

async fn daemon_with<F>(setup: F) -> (TestDaemon, Control)
where
    F: FnOnce(&Path) -> anyhow::Result<()>,
{
    ensure_pdo_on_path();
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(setup, move |root, config| {
        config.sandbox_home_override = Some(root.join("home"));
        pdo::compose(config, ext.boxed(), None).unwrap();
    })
    .await
    .unwrap();
    (daemon, control)
}

async fn start_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": pipeline,
            "input": "deliver",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Poll until the Run reads `status` (script nodes on a loaded host are slow).
async fn wait_run_settles(daemon: &TestDaemon, run_id: &str, status: &str) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let run = daemon.get_json(&format!("/runs/{run_id}")).await;
        if run["status"] == status || std::time::Instant::now() > deadline {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// `author / committer` of `rev`, as `git log` prints them.
fn signed_by(repo: &Path, rev: &str) -> String {
    git(repo, &["log", "-1", "--format=%an %ae / %cn %ce", rev])
}

#[tokio::test]
async fn the_delivery_commit_is_signed_by_the_identity_the_node_was_given() {
    let name = "identity-shared";
    let (daemon, control) = daemon_with(seed(
        name,
        one_script(name),
        vec![("worker", WORKER.to_string())],
        true,
    ))
    .await;
    control.launch.set_env(alice_env());

    let run_id = start_run(&daemon, name).await;
    let run = wait_run_settles(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "{run}");

    let repo = daemon.repo_root();
    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git(repo, &["log", "-1", "--format=%s", &branch]),
        "worker iter-1: completed"
    );
    assert_eq!(signed_by(repo, &branch), ALICE);

    // Only the identity is frozen: the secret beside it never reaches the log.
    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    let started = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "node_started" && e["node_id"] == "worker")
        .expect("the worker started");
    assert_eq!(
        started["payload"]["git_identity"],
        serde_json::json!({
            "GIT_AUTHOR_NAME": "Alice",
            "GIT_AUTHOR_EMAIL": "alice@example.com",
            "GIT_COMMITTER_NAME": "Alice",
            "GIT_COMMITTER_EMAIL": "alice@example.com",
        })
    );
    let log = events.to_string();
    assert!(
        !log.contains("s3cr3t-never-logged") && !log.contains("FIXTURE_SECRET"),
        "no other variable of the Node's environment is frozen: {log}"
    );
}

#[tokio::test]
async fn an_isolated_node_commit_and_its_merge_back_are_signed_by_its_identity() {
    let name = "identity-isolated";
    let shared = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'pub fn shared() {}\\n' > shared.rs\nprintf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n";
    let forked = "#!/usr/bin/env bash\nset -euo pipefail\nsleep 3\n\
        printf 'pub fn forked() {}\\n' > forked.rs\nprintf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n";
    let (daemon, control) = daemon_with(seed(
        name,
        shared_and_forked(name),
        vec![
            ("shared", shared.to_string()),
            ("forked", forked.to_string()),
        ],
        true,
    ))
    .await;
    control.launch.set_env(alice_env());

    let run_id = start_run(&daemon, name).await;
    let run = wait_run_settles(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "{run}");

    let repo = daemon.repo_root();
    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git(repo, &["log", "-1", "--format=%s", &format!("{branch}^2")]),
        "forked iter-1: completed",
        "the Run's branch ends on the merge-back of the isolated node"
    );
    assert_eq!(signed_by(repo, &format!("{branch}^2")), ALICE, "its commit");
    assert_eq!(signed_by(repo, &branch), ALICE, "the merge commit");
    assert_eq!(signed_by(repo, &format!("{branch}^1")), ALICE, "shared");
}

/// Without an extension that gives an identity, the delivery keeps the
/// daemon's: nothing is frozen, nothing changes.
#[tokio::test]
async fn without_a_given_identity_the_delivery_keeps_the_daemon_one() {
    let name = "identity-none";
    let (daemon, _control) = daemon_with(seed(
        name,
        one_script(name),
        vec![("worker", WORKER.to_string())],
        true,
    ))
    .await;

    let run_id = start_run(&daemon, name).await;
    let run = wait_run_settles(&daemon, &run_id, "completed").await;
    assert_eq!(run["status"], "completed", "{run}");

    let branch = format!("pdo/run-{run_id}");
    assert_ne!(signed_by(daemon.repo_root(), &branch), ALICE);
    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    assert!(
        !events.to_string().contains("git_identity"),
        "nothing frozen: {events}"
    );
}

// ── a daemon with no git identity of its own ─────────────────────────────────

/// Which case the child runs: `given` (the extension gives Alice) or `none`.
const CASE_ENV: &str = "PDO_TEST_NO_DAEMON_IDENTITY_CASE";
/// The child's last stdout line: what the Run ended as, then the branch's
/// signature and the awaiting reason.
const VERDICT: &str = "no-daemon-identity verdict: ";

/// The child's side: a daemon whose environment and configuration hold no git
/// identity, a Run of one shared script. A no-op when run without a case.
#[test]
#[ignore = "the child process of the tests below"]
fn a_run_without_daemon_identity_child() {
    let Ok(case) = std::env::var(CASE_ENV) else {
        return;
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let name = "identity-daemonless";
            let (daemon, control) = daemon_with(seed(
                name,
                one_script(name),
                vec![("worker", WORKER.to_string())],
                false,
            ))
            .await;
            if case == "given" {
                control.launch.set_env(alice_env());
            }
            let run_id = start_run(&daemon, name).await;
            let run = if case == "given" {
                wait_run_status_long(&daemon, &run_id, "completed").await
            } else {
                wait_node_status_long(&daemon, &run_id, "worker", "interrupted").await
            };
            let branch = format!("pdo/run-{run_id}");
            println!(
                "{VERDICT}{} | {} | {}",
                run["status"].as_str().unwrap_or("?"),
                signed_by(daemon.repo_root(), &branch),
                run["awaiting_reason_code"].as_str().unwrap_or("-"),
            );
        });
}

async fn wait_run_status_long(daemon: &TestDaemon, run_id: &str, status: &str) -> Value {
    let run = wait_run_settles(daemon, run_id, status).await;
    if run["status"] == status {
        return run;
    }
    wait_run_status(daemon, run_id, status).await
}

async fn wait_node_status_long(
    daemon: &TestDaemon,
    run_id: &str,
    node: &str,
    status: &str,
) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let run = daemon.get_json(&format!("/runs/{run_id}")).await;
        if run["nodes"][node]["status"] == status || std::time::Instant::now() > deadline {
            return wait_node_status(daemon, run_id, node, status).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Run the child on `case` with no git identity anywhere; its verdict line.
fn run_child(case: &str) -> String {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "delivery_identity::a_run_without_daemon_identity_child",
        "--ignored",
        "--nocapture",
    ]);
    for (key, _) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if key_text.starts_with("PDO_") || key_text.starts_with("GIT_") {
            cmd.env_remove(&key);
        }
    }
    let output = cmd
        .env_remove("EMAIL")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(CASE_ENV, case)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the child failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .find_map(|l| l.strip_prefix(VERDICT))
        .unwrap_or_else(|| panic!("the child gave no verdict:\n{stdout}"))
        .trim()
        .to_string()
}

/// The fixture is genuine: with no identity given, the delivery cannot commit,
/// and the Run parks on `delivery_failed` — the failure CORE-43 removes.
#[test]
fn a_daemon_without_identity_cannot_deliver_alone() {
    let verdict = run_child("none");
    assert!(verdict.starts_with("awaiting_user |"), "{verdict}");
    assert!(verdict.ends_with("| delivery_failed"), "{verdict}");
}

#[test]
fn a_daemon_without_identity_finishes_the_run_the_extension_signs() {
    assert_eq!(run_child("given"), format!("completed | {ALICE} | -"));
}
