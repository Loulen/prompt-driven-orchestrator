//! Layer 3 — the detached update executor against a **fixture executor** (#699).
//!
//! Drives a real daemon whose `POST /update/apply` spawns a fixture script instead
//! of `sh <generated script>`, so nothing here touches brew, systemd or the installed
//! binary. The fixture logs the plan it received (the env the daemon hands the
//! executor) — that is the AC: « the fixture executor receives the expected command
//! for `homebrew` and for `script` ». The generated script is still written beside
//! the log, so its shape (service reinstall + restart vs. kill + relaunch with the
//! same port) is asserted on disk.
//!
//! What is proven (ADR-0004 — an AC closes at layer ≥ 3):
//!   1. apply answers 202 AT ONCE with an attempt id; the log is created; the
//!      fixture received `brew update && brew upgrade Loulen/tap/pdo`; the attempt
//!      is visible in `GET /update` and ends `succeeded` with exit 0;
//!   2. `script` method, unsupervised: the installer re-run, and a script that stops
//!      the daemon by pid then relaunches `daemon --port <same port>`;
//!   3. `unknown` method: apply refused 409 with a reason, `can_apply=false` on the
//!      read, nothing journaled;
//!   4. a non-zero executor exit is recorded (`failed`, exit code) and a new apply
//!      is allowed afterwards; the log is served as text;
//!   5. active Runs never block: the count is reported, apply still answers 202;
//!   6. **really detached**: a REAL `pdo daemon` binary is SIGKILLed right after
//!      apply; the fixture finishes on its own and its late line lands in the log.

use crate::common::TestDaemon;
use pdo_daemon::update_check::{InstallMethod, Supervision};
use std::path::Path;
use std::time::{Duration, Instant};

/// A fixture executor: echo the plan (stdout is the attempt's log), then exit as
/// told. `$1` is the generated script path.
fn fixture(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             echo \"fixture-start\"\n\
             echo \"cmd=$PDO_UPDATE_COMMAND\"\n\
             echo \"method=$PDO_UPDATE_METHOD\"\n\
             echo \"supervision=$PDO_UPDATE_SUPERVISION\"\n\
             echo \"port=$PDO_UPDATE_PORT\"\n\
             echo \"relaunch=$PDO_UPDATE_RELAUNCH\"\n\
             echo \"script=$1\"\n\
             {body}\n"
        ),
    )
    .unwrap();
    format!("sh {}", path.display())
}

fn seed(_repo: &Path) -> anyhow::Result<()> {
    Ok(())
}

async fn update_json(daemon: &TestDaemon) -> serde_json::Value {
    reqwest::get(format!("{}/update", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn apply(daemon: &TestDaemon) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/update/apply", daemon.url()))
        .send()
        .await
        .unwrap()
}

async fn wait_for_status(daemon: &TestDaemon, id: &str, status: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let body = update_json(daemon).await;
        let a = &body["last_attempt"];
        if a["attempt_id"] == id && a["status"] == status {
            return a.clone();
        }
        assert!(
            Instant::now() < deadline,
            "attempt {id} never reached `{status}`: {body}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn read_log(daemon: &TestDaemon, id: &str) -> String {
    std::fs::read_to_string(daemon.repo_root().join(format!(".pdo/update/{id}.log")))
        .unwrap_or_default()
}

fn read_script(daemon: &TestDaemon, id: &str) -> String {
    std::fs::read_to_string(daemon.repo_root().join(format!(".pdo/update/{id}.sh"))).unwrap()
}

#[tokio::test]
async fn apply_homebrew_answers_at_once_and_the_fixture_receives_the_brew_command() {
    let daemon = spawn(InstallMethod::Homebrew, Supervision::Systemd, "exit 0").await;

    // Before: the read reports the gate open and no attempt yet.
    let before = update_json(&daemon).await;
    assert_eq!(before["install_method"], "homebrew");
    assert_eq!(before["can_apply"], true);
    assert_eq!(before["apply_blocked_reason"], serde_json::Value::Null);
    assert_eq!(before["last_attempt"], serde_json::Value::Null);
    assert_eq!(before["active_runs"], 0);

    let t0 = Instant::now();
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202, "apply answers immediately");
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "apply must not wait for the executor"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    let id = body["attempt_id"].as_str().unwrap().to_string();
    assert!(!id.is_empty());
    assert_eq!(body["status"], "running");
    assert_eq!(
        body["command"],
        "brew update && brew upgrade Loulen/tap/pdo"
    );
    assert_eq!(body["from_version"], env!("CARGO_PKG_VERSION"));

    // The attempt ends: exit 0 recorded by the daemon that outlived the fixture.
    let attempt = wait_for_status(&daemon, &id, "succeeded").await;
    assert_eq!(attempt["exit_code"], 0);
    assert_eq!(attempt["method"], "homebrew");
    assert_eq!(attempt["supervision"], "systemd");
    assert!(attempt["finished_at"].as_str().is_some());

    // The journal: the fixture got the homebrew command, the port, the plan.
    let log = read_log(&daemon, &id);
    assert!(
        log.contains("fixture-start"),
        "log created and written: {log}"
    );
    assert!(
        log.contains("cmd=brew update && brew upgrade Loulen/tap/pdo"),
        "the executor receives the method's command: {log}"
    );
    assert!(log.contains("method=homebrew"));
    assert!(log.contains("supervision=systemd"));
    assert!(log.contains(&format!("port={}", daemon.addr.port())));

    // The generated script (what production runs) reinstalls the unit through the
    // binary path, then restarts the service — no kill/relaunch when supervised.
    let script = read_script(&daemon, &id);
    assert!(script.contains("brew update && brew upgrade Loulen/tap/pdo"));
    assert!(
        script.contains(&format!("service install --port {}", daemon.addr.port())),
        "idempotent service install with the daemon's port:\n{script}"
    );
    assert!(script.contains("systemctl --user restart pdo"));
    assert!(!script.contains("kill -TERM"));

    // Served as text for the Settings « View log ».
    let resp = reqwest::get(format!("{}/update/attempts/{id}/log", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    assert!(resp.text().await.unwrap().contains("cmd=brew update"));
}

/// Spawn with a fixture whose path is known before the daemon boots.
async fn spawn(method: InstallMethod, supervision: Supervision, body: &str) -> TestDaemon {
    let fixture_dir = tempfile::tempdir().unwrap();
    let cmd = fixture(fixture_dir.path(), "executor.sh", body);
    // Keep the fixture alive for the daemon's whole life.
    Box::leak(Box::new(fixture_dir));
    TestDaemon::spawn_with_update_executor(seed, cmd, method, supervision)
        .await
        .unwrap()
}

#[tokio::test]
async fn apply_script_method_unsupervised_relaunches_the_daemon_with_the_same_port() {
    let daemon = spawn(InstallMethod::Script, Supervision::None, "exit 0").await;
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202);
    let id = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();
    wait_for_status(&daemon, &id, "succeeded").await;

    let log = read_log(&daemon, &id);
    assert!(
        log.contains("cmd=curl --proto '=https' --tlsv1.2 -LsSf"),
        "the executor receives the installer re-run: {log}"
    );
    assert!(log.contains("pdo-daemon-installer.sh | sh"));
    assert!(log.contains("method=script"));
    assert!(log.contains("supervision=none"));
    let port = daemon.addr.port();
    assert!(
        log.contains(&format!("daemon --port {port}")),
        "relaunch carries the same port: {log}"
    );

    let script = read_script(&daemon, &id);
    assert!(script.contains(&format!("kill -TERM {}", std::process::id())));
    assert!(
        script.contains(&format!("daemon --port {port}")),
        "same arguments (port):\n{script}"
    );
    assert!(
        !script.contains("service install"),
        "no unit when unsupervised"
    );
    assert!(!script.contains("systemctl"));
}

#[tokio::test]
async fn unknown_method_is_refused_with_the_reason_and_the_manual_command() {
    let daemon = spawn(InstallMethod::Unknown, Supervision::None, "exit 0").await;
    let before = update_json(&daemon).await;
    assert_eq!(before["install_method"], "unknown");
    assert_eq!(before["can_apply"], false);
    assert!(before["apply_blocked_reason"]
        .as_str()
        .unwrap()
        .contains("Install method not detected"));

    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Install method not detected"));
    assert_eq!(body["install_method"], "unknown");
    assert!(body["manual_command"]
        .as_str()
        .unwrap()
        .starts_with("Build from source"));
    assert!(
        !daemon.repo_root().join(".pdo/update").exists()
            || std::fs::read_dir(daemon.repo_root().join(".pdo/update"))
                .unwrap()
                .next()
                .is_none(),
        "nothing journaled on a refusal"
    );
    assert_eq!(
        update_json(&daemon).await["last_attempt"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn a_failed_executor_is_recorded_with_its_exit_code_and_does_not_block_a_retry() {
    let daemon = spawn(
        InstallMethod::Homebrew,
        Supervision::None,
        "echo 'Error: brew upgrade failed' >&2\nexit 3",
    )
    .await;
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202);
    let id = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();
    let attempt = wait_for_status(&daemon, &id, "failed").await;
    assert_eq!(attempt["exit_code"], 3);
    assert!(attempt["finished_at"].as_str().is_some());
    let log = read_log(&daemon, &id);
    assert!(
        log.contains("brew upgrade failed"),
        "stderr in the journal: {log}"
    );

    // The gate reopens: a failed attempt is not a running one.
    let body = update_json(&daemon).await;
    assert_eq!(body["can_apply"], true);
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202);
    let id2 = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(id, id2);
    wait_for_status(&daemon, &id2, "failed").await;

    // Unknown attempt id: 404, not a panic; bad id: 400.
    let resp = reqwest::get(format!("{}/update/attempts/nope-000000/log", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let resp = reqwest::get(format!("{}/update/attempts/..%2Fx/log", daemon.url()))
        .await
        .unwrap();
    assert!(resp.status() == 400 || resp.status() == 404);
}

#[tokio::test]
async fn a_running_attempt_makes_a_second_apply_answer_409() {
    let daemon = spawn(
        InstallMethod::Homebrew,
        Supervision::None,
        "sleep 2\nexit 0",
    )
    .await;
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202);
    let id = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();
    let body = update_json(&daemon).await;
    assert_eq!(body["can_apply"], false);
    assert!(body["apply_blocked_reason"].as_str().unwrap().contains(&id));
    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 409);
    wait_for_status(&daemon, &id, "succeeded").await;
    assert_eq!(update_json(&daemon).await["can_apply"], true);
}

const PIPELINE_YAML: &str = r#"
name: update-live
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: solo
    name: solo
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: solo, port: in }
"#;

fn seed_pipeline(repo: &Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(pipelines_dir.join("update-live.yaml"), PIPELINE_YAML)?;
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "test@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

#[tokio::test]
async fn active_runs_are_counted_and_never_block_apply() {
    let fixture_dir = tempfile::tempdir().unwrap();
    let cmd = fixture(fixture_dir.path(), "executor.sh", "exit 0");
    let daemon = TestDaemon::spawn_with_update_executor(
        seed_pipeline,
        cmd,
        InstallMethod::Homebrew,
        Supervision::Systemd,
    )
    .await
    .unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": "update-live",
            "input": "hello",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let body = update_json(&daemon).await;
    assert_eq!(body["active_runs"], 1, "the confirm dialog's count: {body}");
    assert_eq!(body["can_apply"], true, "active Runs warn, never block");

    let resp = apply(&daemon).await;
    assert_eq!(resp.status(), 202, "never refused for active Runs");
    let id = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();
    wait_for_status(&daemon, &id, "succeeded").await;
}

/// The one test that needs a REAL `pdo daemon` process (in-process daemons share the
/// test process, whose death cannot be staged): SIGKILL the daemon right after apply
/// and prove the executor, spawned in its own session, finishes and journals anyway.
#[tokio::test]
async fn the_executor_survives_the_daemon_death() {
    let home = tempfile::tempdir().unwrap();
    let cmd = fixture(
        home.path(),
        "executor.sh",
        "sleep 1.5\necho \"detached-done\"\nexit 0",
    );
    // A free port for the real daemon.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_pdo"))
        .args(["daemon", "--port", &port.to_string()])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("PDO_UPDATE_EXECUTOR", &cmd)
        .env("PDO_INSTALL_METHOD", "homebrew")
        // No egress and a passive daemon: this test is about process lifetime.
        .env("PDO_UPDATE_CHECK", "0")
        .env("PDO_PRICE_SYNC", "off")
        .env("PDO_DAEMON_NO_CLEANUP", "1")
        .env_remove("PDO_NODE_ID")
        .env_remove("INVOCATION_ID")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the real pdo binary");

    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(r) = reqwest::get(format!("{base}/sessions")).await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(Instant::now() < deadline, "the real daemon never answered");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let status = reqwest::get(format!("{base}/update"))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        status["install_method"], "homebrew",
        "PDO_INSTALL_METHOD honoured"
    );

    let resp = reqwest::Client::new()
        .post(format!("{base}/update/apply"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    let id = resp.json::<serde_json::Value>().await.unwrap()["attempt_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The daemon dies NOW — before the fixture's 1.5 s sleep ends.
    child.kill().unwrap();
    let _ = child.wait();

    let log_path = home.path().join(format!(".pdo/update/{id}.log"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        if log.contains("detached-done") {
            assert!(log.contains("cmd=brew update && brew upgrade Loulen/tap/pdo"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the executor did not survive the daemon: {log}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The record the daemon wrote at spawn time is still there (`running`: the
    // fixture, unlike the real script, does not close it — production's script
    // owns the end state precisely because the daemon is gone).
    let record: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.path().join(".pdo/update/last-attempt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["attempt_id"], id);
}
