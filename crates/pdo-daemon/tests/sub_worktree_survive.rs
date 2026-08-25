//! Layer 3a — sub-worktree survival integration tests for issues #32 and #489.
//!
//! Spawns a real TestDaemon with a code-mutating pipeline, creates a run,
//! marks the code-mutating node done, then asserts:
//! - the sub-worktree directory still exists on disk
//! - GET /runs/{run_id}/nodes/{node_id}/prompt?iter=1 returns 200
//!
//! #489 extends the file to the other half of "survives": a `restart_node` on the
//! same node must re-spawn it **without destroying the dead session's uncommitted
//! work**. That is the regression the whole issue is about, and this pipeline
//! already has the only node class that can see it (`code-mutating`).

mod common;

use common::TestDaemon;

const PIPELINE_NAME: &str = "cm-survive-test";
const NODE_ID: &str = "impl-1";
const PIPELINE_YAML: &str = r#"name: cm-survive-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: impl-1
    name: impl-1
    type: code-mutating
    inputs:
      - name: task
    outputs:
      - name: summary
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: impl-1, port: task }
"#;

const ROLE_PROMPT: &str = "You are an implementer. Do the task.\n";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;

    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join(format!("{NODE_ID}.md")), ROLE_PROMPT)?;

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
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().await.unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

// ── #489 helpers ─────────────────────────────────────────────────────────────

/// `git <args>` in `dir`, stdout trimmed. Panics on a non-zero exit: every use
/// below is a fact the assertions depend on, so a silent empty string would turn a
/// real regression into a green run.
fn git_out(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

/// The worktree's full dirty state, untracked included.
fn porcelain(dir: &std::path::Path) -> String {
    git_out(dir, &["status", "--porcelain"])
}

/// Is `dir` a worktree git has REGISTERED? The authoritative probe, and the one
/// `branch_exists` (`spawn_abort_recovery.rs`) cannot answer: a branch ref is only
/// one of the three locks a re-spawn can hit. Matched on the absolute path, never
/// the basename — `.git/worktrees/` is named by basename, so every node collides on
/// `iter-1` and git disambiguates to `iter-11`, `iter-12`…
fn worktree_registered(repo_root: &std::path::Path, dir: &std::path::Path) -> bool {
    let want = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    git_out(repo_root, &["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .any(|p| {
            let p = std::path::Path::new(p.trim());
            std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()) == want
        })
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

async fn events_of(daemon: &TestDaemon, run_id: &str) -> Vec<serde_json::Value> {
    let resp = reqwest::get(format!("{}/runs/{run_id}/events", daemon.url()))
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array().cloned().unwrap_or_default()
}

/// Every `node_started` for `node_id`, in log order.
async fn node_starts(daemon: &TestDaemon, run_id: &str, node_id: &str) -> Vec<serde_json::Value> {
    events_of(daemon, run_id)
        .await
        .into_iter()
        .filter(|e| e["kind"] == "node_started" && e["node_id"] == node_id)
        .collect()
}

/// Poll until `pred` holds or the budget runs out. Layer 3a drives a real daemon,
/// so the spawn is asynchronous to the HTTP call.
async fn wait_until<F, Fut>(what: &str, mut pred: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if pred().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

/// **THE #489 REGRESSION.** A `restart_node` on a `code-mutating` node whose
/// sub-worktree already exists must genuinely re-spawn it, and must not touch the
/// work in flight.
///
/// Before the fix this test failed on its first assertion: the arm answered
/// `200 {"ok":true}` while `git worktree add -b pdo/sub-…-iter-1` had exited 255
/// (`a branch named … already exists`), no second `node_started` was ever written,
/// and the liveness sweep filed a false `session_died` 30 s later.
///
/// The oracle is deliberately the **event log**, not tmux: `node_spawn` swallows a
/// tmux spawn failure and returns `Spawned` anyway (a separate, filed bug), and
/// every tmux-based test in this suite early-returns behind `tmux_available()` —
/// such an oracle would skip in silence on a runner without tmux.
#[tokio::test]
async fn restart_node_reuses_the_sub_worktree_and_keeps_the_work_in_flight() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");

    wait_until("the entry node to spawn", || async {
        !node_starts(&daemon, &run_id, NODE_ID).await.is_empty()
    })
    .await;
    assert!(sub_wt_dir.exists());
    assert!(worktree_registered(daemon.repo_root(), &sub_wt_dir));

    let first_starts = node_starts(&daemon, &run_id, NODE_ID).await;
    let first_base = first_starts[0]["payload"]["base_sha"]
        .as_str()
        .expect("the first spawn records its base (#503)")
        .to_string();
    assert!(!first_base.is_empty());

    // What a wedged agent leaves behind. FOUR assertions hang off this, not one:
    // an untracked file, a TRACKED file modified, the porcelain snapshot and HEAD.
    // The tracked leg is the load-bearing one — it is the only thing that tells
    // "the restart reused the worktree" from "the restart committed first, then
    // re-cut it".
    std::fs::write(sub_wt_dir.join("scratch.txt"), "half-written thought\n").unwrap();
    std::fs::write(sub_wt_dir.join(".gitignore"), ".pdo/runs/\n# touched\n").unwrap();
    let porcelain_before = porcelain(&sub_wt_dir);
    let head_before = git_out(&sub_wt_dir, &["rev-parse", "HEAD"]);
    assert!(
        porcelain_before.contains("scratch.txt") && porcelain_before.contains(".gitignore"),
        "fixture must be dirty on both legs, or it tests nothing: {porcelain_before}"
    );

    let (status, body) = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "restart_node", "node_id": NODE_ID, "iter": 1 }),
    )
    .await;
    assert_eq!(status, 200, "restart_node: {body}");

    // AC1/AC3 — it really restarted. A second `node_started` for the same iter.
    wait_until("the restart to re-spawn the node", || async {
        node_starts(&daemon, &run_id, NODE_ID).await.len() >= 2
    })
    .await;
    let starts = node_starts(&daemon, &run_id, NODE_ID).await;
    assert_eq!(starts.len(), 2, "expected exactly one re-spawn: {starts:?}");
    assert_eq!(starts[1]["iter"], 1, "the restart stays on the same iter");

    // …and it says so on the wire, which is the other half of #489.
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(
        body["spawned"],
        serde_json::json!([{ "node_id": NODE_ID, "iter": 1 }]),
        "{body}"
    );
    assert_eq!(
        body["reused_sub_worktree"], true,
        "the sub-worktree was reused in place: {body}"
    );

    // AC4 / the #503 rider — the base is CARRIED OVER, non-null and identical. A
    // missing base would disable ADR-0036's adoption escape hatch for every
    // restarted node; a re-derived one would arm it falsely.
    assert_eq!(
        starts[1]["payload"]["base_sha"].as_str(),
        Some(first_base.as_str()),
        "the re-spawn must report the ORIGINAL base: {starts:?}"
    );
    assert_eq!(
        body["base_sha"].as_str(),
        Some(first_base.as_str()),
        "{body}"
    );

    // AC3 — the work in flight is untouched, on all four legs.
    assert_eq!(
        std::fs::read_to_string(sub_wt_dir.join("scratch.txt")).unwrap(),
        "half-written thought\n",
        "the untracked file must survive the restart"
    );
    assert_eq!(
        std::fs::read_to_string(sub_wt_dir.join(".gitignore")).unwrap(),
        ".pdo/runs/\n# touched\n",
        "the modified tracked file must survive the restart"
    );
    assert_eq!(
        porcelain(&sub_wt_dir),
        porcelain_before,
        "nothing was committed, staged or reverted"
    );
    assert_eq!(
        git_out(&sub_wt_dir, &["rev-parse", "HEAD"]),
        head_before,
        "HEAD must not move: a reuse cuts nothing"
    );
    assert!(worktree_registered(daemon.repo_root(), &sub_wt_dir));

    let session_name = format!("pdo-{run_id}-{NODE_ID}-iter-1");
    let _ = std::process::Command::new("tmux")
        .args([
            "-L",
            &daemon.tmux_socket(),
            "kill-session",
            "-t",
            &session_name,
        ])
        .output();
}

/// #489-B / #279: a spawn that ABORTS on a **reused** worktree must destroy
/// nothing. `orphan_to_reap` is `Some(...)` only on the create path, so
/// `fail_spawn_before_start` no longer reaches `worktree remove --force` — which
/// succeeds on a dirty tree and would take the agent's work with it.
///
/// The Run goes terminal with the work intact; `resume_run` reopens it and the next
/// classification answers `Reusable`. Idempotent and self-healing, where the
/// residue used to condemn the node for the Run's whole life.
#[tokio::test]
async fn a_panic_on_a_reused_sub_worktree_destroys_nothing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");
    wait_until("the entry node to spawn", || async {
        !node_starts(&daemon, &run_id, NODE_ID).await.is_empty()
    })
    .await;

    std::fs::write(sub_wt_dir.join("precious.txt"), "the only copy\n").unwrap();
    let head_before = git_out(&sub_wt_dir, &["rev-parse", "HEAD"]);

    // #279's one-shot fault injection, armed for the restart's spawn.
    daemon.arm_spawn_panic();
    let (status, body) = post_command(
        &daemon,
        &run_id,
        serde_json::json!({ "kind": "restart_node", "node_id": NODE_ID, "iter": 1 }),
    )
    .await;

    // The panic is caught and reported loud — a 500, never a 200 (#489-A).
    assert_eq!(
        status, 500,
        "a caught spawn panic is a panne, not a success: {body}"
    );
    assert_eq!(body["error"], "spawn_failed", "{body}");
    // ADR-0049: the abort parks the run `AwaitingUser` (via `NodeInterrupted`),
    // it never fails it — so `run_failed` re-projects to `false` and the
    // situation is recoverable (reopen re-drives the reused work).
    assert_eq!(body["run_failed"], false, "{body}");
    assert_eq!(body["recoverable"], true, "{body}");
    assert_eq!(body["session_killed"], true, "{body}");

    // And the whole point: the worktree, its branch and its work are all still here.
    assert!(
        sub_wt_dir.exists(),
        "the reused sub-worktree must survive an aborted re-spawn"
    );
    assert!(worktree_registered(daemon.repo_root(), &sub_wt_dir));
    assert_eq!(
        std::fs::read_to_string(sub_wt_dir.join("precious.txt")).unwrap(),
        "the only copy\n"
    );
    assert_eq!(git_out(&sub_wt_dir, &["rev-parse", "HEAD"]), head_before);
}

/// Layer 3a: after marking a code-mutating node done, the sub-worktree
/// directory must still exist on disk and the prompt endpoint must return 200.
#[tokio::test]
async fn sub_worktree_survives_node_completion() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    // The daemon creates the sub-worktree at spawn time. Verify it exists.
    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");

    assert!(
        sub_wt_dir.exists(),
        "sub-worktree should exist after run creation: {}",
        sub_wt_dir.display()
    );

    // Write a code change in the sub-worktree so merge has something to commit
    std::fs::write(sub_wt_dir.join("implementation.rs"), "fn main() {}\n").unwrap();

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1")
        .join("summary");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Summary\nDone.\n").unwrap();

    // Mark node done — triggers commit_and_merge_sub_worktree
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/nodes/{}/done",
            daemon.url(),
            run_id,
            NODE_ID,
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree directory must still exist after node completion (refs #32)
    assert!(
        sub_wt_dir.exists(),
        "sub-worktree must survive after merge for inspection (refs #32): {}",
        sub_wt_dir.display()
    );

    // Prompt endpoint must return 200 for the completed iter
    let resp = reqwest::get(format!(
        "{}/runs/{}/nodes/{}/prompt?iter=1",
        daemon.url(),
        run_id,
        NODE_ID,
    ))
    .await
    .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "prompt endpoint must return 200 for completed code-mutating node"
    );

    let body = resp.text().await.unwrap();
    assert!(!body.is_empty(), "prompt response body must be non-empty");

    // Cleanup tmux session
    let session_name = format!("pdo-{run_id}-{NODE_ID}-iter-1");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session_name])
        .output();
}

/// Layer 3a: cleanup_run must still remove all sub-worktrees even though
/// they now survive merge.
#[tokio::test]
async fn cleanup_run_removes_surviving_sub_worktrees() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon).await;

    let sub_wt_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("nodes")
        .join(NODE_ID)
        .join("iter-1");

    // Write a code change and mark done
    std::fs::write(sub_wt_dir.join("implementation.rs"), "fn main() {}\n").unwrap();

    let port_dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/artifacts")
        .join(NODE_ID)
        .join("iter-1")
        .join("summary");
    std::fs::create_dir_all(&port_dir).unwrap();
    std::fs::write(port_dir.join("output.md"), "# Summary\nDone.\n").unwrap();

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/nodes/{}/done",
            daemon.url(),
            run_id,
            NODE_ID,
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree survives merge
    assert!(sub_wt_dir.exists());

    // Run cleanup
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{}/commands", daemon.url(), run_id))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Sub-worktree must be gone after cleanup
    assert!(
        !sub_wt_dir.exists(),
        "cleanup_run must remove sub-worktree directory"
    );

    // Run directory must be gone
    let run_dir = daemon.repo_root().join(".pdo/runs").join(&run_id);
    assert!(
        !run_dir.exists(),
        "cleanup_run must remove the run directory"
    );
}
