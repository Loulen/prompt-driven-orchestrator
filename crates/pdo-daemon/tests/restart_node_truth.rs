//! Layer 3a — #489 / ADR-0037: `restart_node` tells the truth.
//!
//! Sibling of `loop_command_truth.rs`, which is the same net for ADR-0025's loop
//! commands. Two properties, one per half of the issue:
//!
//! 1. **A spawn that did not happen is never a `2xx`.** Every `SpawnOutcome` is
//!    read and projected — `Throttled` to `200 {"waiting":true}` (a `NodeWaiting`
//!    *was* appended, so it is not a `noop`), everything else to its own status
//!    and slug.
//! 2. **Every knowable refusal is raised BEFORE the tmux kill.** Proved by
//!    NEGATIVE assertions: no `command_issued`, no new events at all, the node
//!    still projected as it was.
//!
//! Layer-3 coverage of `restart_node` before this file: zero.

mod common;

use std::time::Duration;

use common::TestDaemon;

const PIPELINE_NAME: &str = "restart-truth";

/// `planner` is `doc-only` (owns no sub-worktree — the positive control), `impl-1`
/// is `code-mutating` (the only class #489 broke) and `runner` is a `script` node
/// whose body a test empties to provoke `SpawnOutcome::Failed`.
const PIPELINE_YAML: &str = r#"name: restart-truth
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: planner
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: impl-1
    name: impl-1
    type: code-mutating
    inputs:
      - name: plan
    outputs:
      - name: summary
  - id: runner
    name: runner
    type: script
    inputs:
      - name: summary
    outputs:
      - name: report
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
  - source: { node: planner, port: plan }
    target: { node: impl-1, port: plan }
  - source: { node: impl-1, port: summary }
    target: { node: runner, port: summary }
  - source: { node: runner, port: report }
    target: { node: end, port: result }
"#;

const SCRIPT_BODY: &str = "#!/usr/bin/env bash\ntrue\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    for (id, body) in [
        ("planner", "You are a planner.\n"),
        ("impl-1", "You are an implementer.\n"),
        ("runner", SCRIPT_BODY),
    ] {
        std::fs::write(prompts_dir.join(format!("{id}.md")), body)?;
    }
    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
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

async fn create_run(daemon: &TestDaemon) -> String {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": PIPELINE_NAME,
        "input": "test input",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert_eq!(status, 201, "POST /runs should succeed, got: {text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn post_command(
    daemon: &TestDaemon,
    run_id: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn restart(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> (reqwest::StatusCode, serde_json::Value) {
    post_command(
        daemon,
        run_id,
        serde_json::json!({ "kind": "restart_node", "node_id": node_id, "iter": iter }),
    )
    .await
}

async fn events_of(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let resp = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array().cloned().unwrap_or_default()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    let resp = reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap();
    resp.json().await.unwrap()
}

/// Every `(node_id, iter)` the run has ever started.
async fn started_pairs(daemon: &TestDaemon, run_id: &str) -> Vec<(String, i64)> {
    events_of(daemon, run_id)
        .await
        .into_iter()
        .filter(|e| e["kind"] == "node_started")
        .map(|e| {
            (
                e["node_id"].as_str().unwrap_or_default().to_string(),
                e["iter"].as_i64().unwrap_or_default(),
            )
        })
        .collect()
}

async fn node_started_count(daemon: &TestDaemon, run_id: &str, node_id: &str) -> usize {
    started_pairs(daemon, run_id)
        .await
        .into_iter()
        .filter(|(id, _)| id == node_id)
        .count()
}

async fn node_status(daemon: &TestDaemon, run_id: &str, node_id: &str) -> String {
    get_run(daemon, run_id).await["nodes"][node_id]["status"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Poll until `pred` holds or the budget runs out. Layer 3a drives a real daemon,
/// so the scheduler's work is asynchronous to the HTTP call.
async fn wait_until<F, Fut>(what: &str, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if pred().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn wait_for_node_status(daemon: &TestDaemon, run_id: &str, node_id: &str, want: &str) {
    wait_until(&format!("{node_id} to become {want}"), || async {
        node_status(daemon, run_id, node_id).await == want
    })
    .await;
}

/// **The pre-kill contract, as a reusable oracle.** A refusal that leaves any
/// trace at all has already broken ADR-0037 §3, whatever its status code.
async fn assert_no_trace(daemon: &TestDaemon, run_id: &str, before: &[serde_json::Value]) {
    let after = events_of(daemon, run_id).await;
    assert_eq!(
        after.len(),
        before.len(),
        "a pre-kill refusal must append NOTHING; new events: {:#?}",
        &after[before.len().min(after.len())..]
    );
    assert!(
        !after
            .iter()
            .skip(before.len())
            .any(|e| e["kind"] == "command_issued"),
        "not even the audit event"
    );
}

fn kill_session(daemon: &TestDaemon, run_id: &str, node_id: &str, iter: i64) {
    let _ = std::process::Command::new("tmux")
        .args([
            "-L",
            &daemon.tmux_socket(),
            "kill-session",
            "-t",
            &format!("pdo-{run_id}-{node_id}-iter-{iter}"),
        ])
        .output();
}

// ─────────────────────────────────────────────────────────────────────────────
// Spawned — the positive control
// ─────────────────────────────────────────────────────────────────────────────

/// A `doc-only` node owns no sub-worktree, so it is the one class `restart_node`
/// always worked on. It is here as the control: the new body must report the real
/// spawn, and the three sub-worktree fields must be present-and-empty rather than
/// absent, so a client reading `body.base_sha` never gets `undefined` depending on
/// the node's type.
#[tokio::test]
async fn restarting_a_doc_only_node_reports_the_spawn_it_really_did() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    let (status, body) = restart(&daemon, &run_id, "planner", 1).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(
        body["spawned"],
        serde_json::json!([{ "node_id": "planner", "iter": 1 }]),
        "no more blind {{\"ok\":true}}: {body}"
    );
    assert_eq!(body["reused_sub_worktree"], false, "{body}");
    assert!(body["base_sha"].is_null(), "{body}");
    assert_eq!(body["interrupted_git_ops"], serde_json::json!([]), "{body}");

    // The event log agrees with the body — the bidirectional proof
    // `loop_command_truth` pins for the loop commands.
    wait_until("the re-spawn", || async {
        node_started_count(&daemon, &run_id, "planner").await >= 2
    })
    .await;
    assert_eq!(node_started_count(&daemon, &run_id, "planner").await, 2);

    kill_session(&daemon, &run_id, "planner", 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Refusals raised BEFORE the kill
// ─────────────────────────────────────────────────────────────────────────────

/// A `node_id` absent from the Run's pipeline used to answer `200 {"ok":true}` —
/// after killing a tmux session and appending an audit event for work that never
/// happened. Literal violation of ADR-0025 §2, and the negative assertion is the
/// real content of this test.
#[tokio::test]
async fn an_unknown_node_is_a_400_that_touches_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    let before = events_of(&daemon, &run_id).await;
    let (status, body) = restart(&daemon, &run_id, "ghost", 1).await;

    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "node_not_found", "{body}");
    assert_eq!(body["node_id"], "ghost", "{body}");
    assert_eq!(body["recoverable"], true, "{body}");
    assert_eq!(body["session_killed"], false, "{body}");

    assert_no_trace(&daemon, &run_id, &before).await;
    // The neighbour is untouched: no session was killed and nothing re-spawned.
    assert_eq!(node_status(&daemon, &run_id, "planner").await, "running");
    assert_eq!(node_started_count(&daemon, &run_id, "planner").await, 1);

    kill_session(&daemon, &run_id, "planner", 1);
}

/// Guard refusal #1 — the Run is not live. One slug (`restart_refused`), the
/// guard's prose in `message`.
#[tokio::test]
async fn a_paused_run_refuses_the_restart_before_the_kill() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    let (status, _) =
        post_command(&daemon, &run_id, serde_json::json!({ "kind": "pause_run" })).await;
    assert_eq!(status, 200);
    wait_until("the run to be paused", || async {
        get_run(&daemon, &run_id).await["status"] == "paused"
    })
    .await;

    let before = events_of(&daemon, &run_id).await;
    let (status, body) = restart(&daemon, &run_id, "planner", 1).await;

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "restart_refused", "{body}");
    assert_eq!(body["recoverable"], true, "{body}");
    assert_eq!(body["session_killed"], false, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("resume the run"),
        "the guard's prose belongs in `message`: {body}"
    );

    assert_no_trace(&daemon, &run_id, &before).await;
    kill_session(&daemon, &run_id, "planner", 1);
}

/// Guard refusal #2 — a newer iteration of the same node is live. Same slug, and
/// that is the point: `Verdict::Reject` **now carries a typed cause**
/// (`RejectReason`, #515), but this route flattens it to #490's settled shape
/// (one slug + prose) — discrimination on the retry route is #487.
///
/// It also kills a slug that would have been FALSE: the guard tests
/// `live_iter != iter`, so a restart of iter 5 while iter 1 lives lands in the same
/// branch — a `newer_iteration_live` slug would have encoded a fact the guard
/// never checks.
#[tokio::test]
async fn a_live_newer_iteration_refuses_the_restart_of_the_old_one() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    // `node_retry` is the lever that puts the node on iter 2 (it stops, invalidates
    // and re-spawns at iter+1) — the layer-3 shape of the seeded fixture in
    // `restart_node_rejected_while_newer_iteration_is_live`.
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/planner/retry",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "node_retry should succeed");
    wait_until("planner to reach iter 2", || async {
        started_pairs(&daemon, &run_id)
            .await
            .contains(&("planner".to_string(), 2))
    })
    .await;

    let before = events_of(&daemon, &run_id).await;
    let (status, body) = restart(&daemon, &run_id, "planner", 1).await;

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "restart_refused", "{body}");
    assert_eq!(body["session_killed"], false, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("still live"),
        "{body}"
    );
    assert_no_trace(&daemon, &run_id, &before).await;

    for iter in [1, 2] {
        kill_session(&daemon, &run_id, "planner", iter);
    }
}

/// Guard refusal #3 — the iteration has already completed.
#[tokio::test]
async fn a_completed_iteration_refuses_the_restart() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/planner/iter-1/plan");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# plan\n").unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/planner/done", daemon.url()))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    wait_for_node_status(&daemon, &run_id, "planner", "completed").await;
    // The downstream `impl-1` spawns off that completion; let it settle so the
    // event count below is stable.
    wait_for_node_status(&daemon, &run_id, "impl-1", "running").await;

    let before = events_of(&daemon, &run_id).await;
    let (status, body) = restart(&daemon, &run_id, "planner", 1).await;

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "restart_refused", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("already completed"),
        "{body}"
    );
    assert_no_trace(&daemon, &run_id, &before).await;

    kill_session(&daemon, &run_id, "impl-1", 1);
}

/// The sub-worktree is held by another live worktree. Refused **pre-kill**, and
/// the message names what holds it — the reaper would otherwise destroy exactly
/// the work #489 exists to save.
#[tokio::test]
async fn an_occupied_sub_worktree_refuses_the_restart_before_the_kill() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    // Somebody checked `impl-1`'s branch out somewhere else entirely. `impl-1` has
    // not spawned yet, so this is the branch ref that exists WITHOUT its own
    // worktree — the third of the three locks a re-spawn can hit.
    let borrowed = daemon.repo_root().join("borrowed");
    let sub_branch = format!("pdo/sub-{run_id}-impl-1-iter-1");
    let out = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &sub_branch])
        .arg(&borrowed)
        .arg(format!("pdo/run-{run_id}"))
        .current_dir(daemon.repo_root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fixture: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let before = events_of(&daemon, &run_id).await;
    let (status, body) = restart(&daemon, &run_id, "impl-1", 1).await;

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "sub_worktree_occupied", "{body}");
    assert_eq!(body["recoverable"], true, "{body}");
    assert_eq!(body["session_killed"], false, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("borrowed"),
        "the refusal must NAME what holds it: {body}"
    );

    assert_no_trace(&daemon, &run_id, &before).await;
    // Nothing was reaped: the other worktree is intact.
    assert!(borrowed.join(".gitignore").exists());

    kill_session(&daemon, &run_id, "planner", 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Failed → 500, Throttled → 200 {waiting}
// ─────────────────────────────────────────────────────────────────────────────

/// `SpawnOutcome::Failed` is a **panne, not a verdict**: `500`, with `run_failed`
/// re-projected rather than guessed. Pre-#489 it answered `200 {"ok":true}` on a
/// Run the same call had just moved to `failed`.
///
/// The provocation is the one `node_spawn` documents in a comment: a `script` node
/// whose body has been emptied since launch (`create_run` refuses an empty body,
/// so it has to be emptied afterwards).
#[tokio::test]
async fn a_spawn_that_fails_is_a_500_that_says_the_run_failed() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    // Empty the RUN'S OWN snapshot of the script body — the arm resolves the
    // pipeline snapshot-first (ADR-0025 §2), so editing the library copy would
    // prove nothing.
    let run_prompts = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("pipeline.prompts");
    std::fs::write(run_prompts.join("runner.md"), "").unwrap();

    let (status, body) = restart(&daemon, &run_id, "runner", 1).await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"], "spawn_failed", "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("empty body"),
        "{body}"
    );
    // `fail_spawn_before_start` appended `RunFailed`, so the caller must NOT be
    // told to go and `pdo fail` on top of it.
    assert_eq!(body["run_failed"], true, "{body}");
    assert_eq!(body["recoverable"], false, "{body}");
    assert_eq!(body["session_killed"], true, "{body}");

    wait_until("the run to be failed", || async {
        get_run(&daemon, &run_id).await["status"] == "failed"
    })
    .await;

    kill_session(&daemon, &run_id, "planner", 1);
}

/// `Throttled` stays a `2xx` — and it is **not** a `noop`. A `NodeWaiting` was
/// appended, it flipped the node to `waiting`, and the admission sweep genuinely
/// owns the retry (ADR-0037 §2). The second half proves that claim rather than
/// asserting it: freeing a slot with `kill_node` really does spawn the queued node.
///
/// The cap is set through the **stored** tier (`PUT /settings`), not
/// `PDO_SESSION_CAP`: the env var is process-global and would race every sibling
/// test in this binary (which is why `session_cap_admission.rs` is a single-test
/// file). Stored beats env, so this is also hermetic against a runner that exports
/// one.
#[tokio::test]
async fn a_throttled_restart_answers_waiting_and_is_really_picked_back_up() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "session_cap": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "PUT /settings session_cap=1");

    // Run A takes the only slot.
    let run_a = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_a, "planner", "running").await;

    // Run B's entry node finds none, and is throttled into `waiting`.
    let run_b = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_b, "planner", "waiting").await;

    // Restarting a `waiting` node is legal (the guard treats `Waiting` as the live
    // iteration at the same iter) and is throttled again — deterministically, before
    // AND after the #489-C self-slot exclusion, because a `waiting` node holds no
    // session for the exclusion to take back.
    let (status, body) = restart(&daemon, &run_b, "planner", 1).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["waiting"], true, "{body}");
    assert!(body["reason"].is_string(), "{body}");
    assert!(
        body.get("noop").is_none(),
        "a reservation that flipped the node's status is not a no-op: {body}"
    );
    assert!(body.get("spawned").is_none(), "{body}");

    // …and `waiting:true` is honest. #489-C: `kill_node` frees the slot AND now
    // re-drives the throttled nodes — before it, nothing did, and the queued node
    // starved for ever (`retry_waiting_nodes` has no timer of its own).
    let (status, _) = post_command(
        &daemon,
        &run_a,
        serde_json::json!({ "kind": "kill_node", "node_id": "planner", "iter": 1 }),
    )
    .await;
    assert_eq!(status, 200);
    wait_for_node_status(&daemon, &run_b, "planner", "running").await;

    kill_session(&daemon, &run_b, "planner", 1);
}

/// #489-C — the auto-throttle. At `live == cap`, a `restart_node` on a **live**
/// iteration used to lose its own slot to itself: the arm kills the session but
/// appends no lifecycle event, so the node still projects `running` and the count
/// includes the very session the restart just destroyed.
///
/// The freeze that followed was permanent: `retry_waiting_nodes` has no timer,
/// `resume_run` treats a throttled node as owned by the sweep, boot recovery only
/// looks at `Running`/`AwaitingUser`, and Stop `409`s because `node_stop` requires
/// `Running`. `cap = 1` is the minimal repro, not the scope — the real condition is
/// `live == cap`, so 19 live sessions plus one restart froze just as hard.
#[tokio::test]
async fn a_restart_at_a_full_cap_no_longer_throttles_against_itself() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&serde_json::json!({ "session_cap": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    // live == cap == 1, and the one live session is this node's own.
    let (status, body) = restart(&daemon, &run_id, "planner", 1).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["spawned"],
        serde_json::json!([{ "node_id": "planner", "iter": 1 }]),
        "the restart must SPAWN, not queue behind the session it just killed: {body}"
    );
    assert!(body.get("waiting").is_none(), "{body}");

    wait_until("the re-spawn", || async {
        node_started_count(&daemon, &run_id, "planner").await >= 2
    })
    .await;
    assert_eq!(node_status(&daemon, &run_id, "planner").await, "running");

    kill_session(&daemon, &run_id, "planner", 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// #516 — interrupted git ops are inventoried IN FULL and routed to the preamble
// ─────────────────────────────────────────────────────────────────────────────

/// A linked worktree's private gitdir, read from its `.git` pointer — never
/// derived from the basename (git disambiguates colliding `iter-1` basenames to
/// `iter-11`, `iter-12`…). Mirrors `worktree_ops::private_gitdir`, crate-private.
fn private_gitdir(sub_worktree_dir: &std::path::Path) -> std::path::PathBuf {
    let pointer = std::fs::read_to_string(sub_worktree_dir.join(".git"))
        .expect("a linked worktree has a .git pointer file");
    let raw = pointer
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("gitdir:"))
        .expect("the .git pointer starts with `gitdir:`")
        .trim();
    std::path::PathBuf::from(raw)
}

/// **THE #516 end-to-end proof.** A `code-mutating` node killed mid git-operation
/// leaves BOTH an `index.lock` and a `MERGE_HEAD` in its sub-worktree's private
/// gitdir. `restart_node` must:
///
/// (a) inventory **both** markers on the wire, in scan order — never just the
///     first, which once masked the `MERGE_HEAD` and let `pdo complete` take a
///     silent two-parent merge commit; and
/// (b) route a differentiated notice naming both markers into the re-spawned
///     node's **own** preamble, not merely the manager-facing response body — the
///     fresh agent no longer depends on the manager relaying the instruction.
#[tokio::test]
async fn a_restart_inventories_every_interrupted_git_op_and_routes_it_to_the_preamble() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;
    wait_for_node_status(&daemon, &run_id, "planner", "running").await;

    // Complete `planner` so `impl-1` (the only code-mutating node) spawns and cuts
    // its per-iteration sub-worktree.
    let plan_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts/planner/iter-1/plan");
    std::fs::create_dir_all(&plan_dir).unwrap();
    std::fs::write(plan_dir.join("output.md"), "# plan\n").unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/nodes/planner/done", daemon.url()))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    wait_for_node_status(&daemon, &run_id, "impl-1", "running").await;

    // The dead session mid git-op: kill it, then plant the markers a SIGKILL
    // during the `git commit` that concludes a merge leaves behind, plus an
    // uncommitted file (the work the reuse exists to protect).
    kill_session(&daemon, &run_id, "impl-1", 1);
    let sub_wt = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes/impl-1/iter-1");
    let gitdir = private_gitdir(&sub_wt);
    std::fs::write(gitdir.join("index.lock"), "").unwrap();
    std::fs::write(gitdir.join("MERGE_HEAD"), "").unwrap();
    std::fs::write(sub_wt.join("scratch.rs"), "fn main() {}\n").unwrap();

    // (a) The wire inventories BOTH markers, in scan order, and reports the reuse.
    let (status, body) = restart(&daemon, &run_id, "impl-1", 1).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["reused_sub_worktree"], true, "{body}");
    assert_eq!(
        body["interrupted_git_ops"],
        serde_json::json!(["index.lock", "MERGE_HEAD"]),
        "both markers, in scan order — the first must not mask the second: {body}"
    );

    // (b) The re-spawn wrote a fresh prompt file; its preamble carries the notice,
    // naming both markers with their differentiated instructions.
    wait_until("the impl-1 re-spawn", || async {
        node_started_count(&daemon, &run_id, "impl-1").await >= 2
    })
    .await;
    let resp = reqwest::get(format!(
        "{}/runs/{run_id}/nodes/impl-1/prompt?iter=1",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "GET /prompt should serve the re-spawn prompt"
    );
    let prompt = resp.text().await.unwrap();
    assert!(
        prompt.contains("REUSED from a previous attempt"),
        "the reuse notice must be in the preamble: {prompt}"
    );
    assert!(prompt.contains("index.lock"), "{prompt}");
    assert!(prompt.contains("MERGE_HEAD"), "{prompt}");
    assert!(
        prompt.contains("remove `.git/index.lock`"),
        "index.lock gets the remove-first instruction: {prompt}"
    );
    assert!(
        prompt.contains("git merge --abort"),
        "MERGE_HEAD gets the finish-or-abort instruction: {prompt}"
    );
    assert!(
        prompt.contains("nobody intended") && prompt.contains("**silently**"),
        "the silent-merge warning must reach the agent directly: {prompt}"
    );

    kill_session(&daemon, &run_id, "impl-1", 1);
}
