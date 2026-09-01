//! Layer 3a — the delivery of a NodeRun's work onto the Run's branch (#654 /
//! ADR-0060).
//!
//! Every test here drives a **real temporary git repository**: the delivery is a
//! sequence of `git add -A` / `git commit` / `git merge`, and a fake would only
//! prove the fake. `script` nodes carry most of the matrix because they are the
//! one node type that runs end to end in CI with zero stubbing (their body IS
//! deterministic bash — see `script_node.rs`); the `agent` arms drive
//! `POST …/nodes/:id/done` against a node whose session is the default
//! `exec sleep 600` override, which is exactly what a live agent's `pdo complete`
//! does.
//!
//! Covered, one dimension per test: both node types, both isolations, new files,
//! deleted files, ignored files, a node's own pre-existing commits, the no-op, a
//! git error, concurrency in the shared worktree, and the per-NodeRun diff.

use std::process::Command;
use std::time::Duration;

use crate::common::{ensure_pdo_on_path, TestDaemon};

// ── fixture ──────────────────────────────────────────────────────────────────

/// A repo with one tracked file, one file the repo *ignores*, and a `.gitignore`
/// that covers PDO's run directory — the shape every target repo is expected to
/// have (ADR-0060: `.gitignore` is the single exclusion policy).
fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
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
    run(&["config", "user.email", "delivery@test.local"])?;
    run(&["config", "user.name", "Delivery Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    // PDO's runtime paths are ignored BY THE TARGET REPO, which is the whole
    // exclusion policy (ADR-0060): the delivery itself excludes nothing. Inside a
    // run's worktree the blackboard sits at `.pdo/artifacts/`, so a repo that only
    // ignores `.pdo/runs/` would have every delivery carry its own plumbing —
    // mirrors this project's own `.gitignore`.
    std::fs::write(
        repo.join(".gitignore"),
        ".pdo/runs/\n.pdo/artifacts/\n.pdo/prompts/\nscratch/\n",
    )?;
    std::fs::write(repo.join("tracked.txt"), "original\n")?;
    std::fs::write(repo.join("doomed.txt"), "delete me\n")?;
    run(&["add", "-A"])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

fn git_out(repo: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A `start → worker → end` pipeline whose single worker carries the given type
/// and isolation. The worker declares no output port: these tests are about the
/// delivery, not about output validation.
fn one_worker_pipeline(name: &str, node_type: &str, isolated: bool) -> String {
    format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  \
         - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  \
         - id: worker\n    name: worker\n    type: {node_type}\n    isolated_worktree: {isolated}\n  \
         - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\
         edges:\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: worker, port: in }}\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: end, port: result }}\n"
    )
}

/// Seed a pipeline YAML plus one prompt file per node (a `script` node's prompt
/// slot IS its bash body) and initialise the git repo.
fn seed(
    yaml: String,
    name: &'static str,
    prompts: Vec<(&'static str, String)>,
) -> impl FnOnce(&std::path::Path) -> anyhow::Result<()> {
    move |repo: &std::path::Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), &yaml)?;
        let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts_dir)?;
        for (node, body) in &prompts {
            std::fs::write(prompts_dir.join(format!("{node}.md")), body)?;
        }
        git_init_with_commit(repo)?;
        Ok(())
    }
}

async fn start_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "deliver",
        "target_repo": daemon.target_repo(),
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /runs should create the run");
    resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn get_run(daemon: &TestDaemon, run_id: &str) -> serde_json::Value {
    reqwest::get(format!("{}/runs/{run_id}", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// Poll until `nodes[node_id].status` reaches `expected`, or give up. Returns the
/// last run JSON either way, so the assertion that follows can print it.
async fn wait_for_node_status(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    expected: &str,
) -> serde_json::Value {
    let started = std::time::Instant::now();
    let mut last = serde_json::Value::Null;
    while started.elapsed() < Duration::from_secs(30) {
        let run = get_run(daemon, run_id).await;
        if run["nodes"][node_id]["status"] == expected {
            return run;
        }
        last = run;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    last
}

async fn complete(daemon: &TestDaemon, run_id: &str, node_id: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/done",
            daemon.url()
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap()
        .status()
}

fn run_worktree(daemon: &TestDaemon, run_id: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
}

fn sub_worktree(daemon: &TestDaemon, run_id: &str, node_id: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join("iter-1")
}

// ── script × shared worktree ─────────────────────────────────────────────────

/// A non-isolated `script` modifies a tracked file, deletes another and creates a
/// new one. All three land in one commit on the Run's branch, under the
/// deterministic message — and the ignored file does not.
#[tokio::test]
async fn a_shared_script_delivers_edits_deletions_and_new_files() {
    ensure_pdo_on_path();
    let name = "deliver-shared-script";
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'rewritten\\n' > tracked.txt\n\
        rm doomed.txt\n\
        printf 'pub fn added() {}\\n' > added.rs\n\
        mkdir -p scratch && printf 'noise\\n' > scratch/ignored.txt\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", false),
        "deliver-shared-script",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["nodes"]["worker"]["status"], "completed", "run: {run}");

    let repo = daemon.repo_root();
    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git_out(repo, &["log", "--format=%s", "-1", &branch]),
        "worker iter-1: completed",
        "PDO writes its own deterministic message"
    );
    let files = git_out(repo, &["show", "--name-status", "--format=", &branch]);
    assert!(files.contains("M\ttracked.txt"), "modified: {files}");
    assert!(files.contains("D\tdoomed.txt"), "deleted: {files}");
    assert!(files.contains("A\tadded.rs"), "added: {files}");
    assert!(
        !files.contains("ignored.txt"),
        "an ignored file is never delivered — `.gitignore` is the only policy: {files}"
    );

    // The projection carries the delivery, so the per-node diff is answerable for
    // a node that never owned a branch.
    let delivery = &run["nodes"]["worker"]["delivery"];
    assert!(
        delivery["before"].is_string() && delivery["after"].is_string(),
        "a delivering NodeRun records both tips: {run}"
    );
    let diff: String = reqwest::get(format!("{}/runs/{run_id}/nodes/worker/diff", daemon.url()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        diff.contains("added.rs") && diff.contains("tracked.txt"),
        "the diff of a non-isolated NodeRun shows what it delivered: {diff}"
    );
}

/// A NodeRun that leaves nothing behind takes no commit — empty or otherwise.
#[tokio::test]
async fn a_node_that_leaves_nothing_takes_no_commit() {
    ensure_pdo_on_path();
    let name = "deliver-noop";
    let body = "#!/usr/bin/env bash\nset -euo pipefail\ntrue\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", false),
        "deliver-noop",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["nodes"]["worker"]["status"], "completed", "run: {run}");

    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git_out(repo, &["log", "--format=%s", "-1", &branch]),
        "init",
        "nothing to deliver ⇒ the run's branch never moved"
    );
    assert!(
        run["nodes"]["worker"]["delivery"].is_null(),
        "a no-op delivery records nothing: {run}"
    );
}

/// A node that commits its own work keeps those commits, and PDO tops them with
/// exactly one commit for what was left uncommitted.
#[tokio::test]
async fn pre_existing_commits_are_kept_and_topped_by_one_delivery_commit() {
    ensure_pdo_on_path();
    let name = "deliver-own-commits";
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'mine\\n' > mine.txt\n\
        git add mine.txt\n\
        git commit -q -m 'worker: my own commit'\n\
        printf 'leftover\\n' > leftover.txt\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", false),
        "deliver-own-commits",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["nodes"]["worker"]["status"], "completed", "run: {run}");

    let branch = format!("pdo/run-{run_id}");
    let subjects = git_out(repo, &["log", "--format=%s", "-3", &branch]);
    let subjects: Vec<&str> = subjects.lines().collect();
    assert_eq!(
        subjects,
        vec!["worker iter-1: completed", "worker: my own commit", "init"],
        "the node's own commit survives, PDO adds one on top"
    );
}

/// A node that committed *everything* itself leaves nothing to commit: its
/// commits are kept and no second one is written.
#[tokio::test]
async fn a_node_that_committed_everything_gets_no_extra_commit() {
    ensure_pdo_on_path();
    let name = "deliver-all-own";
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'mine\\n' > mine.txt\n\
        git add -A\n\
        git commit -q -m 'worker: everything, by hand'\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", false),
        "deliver-all-own",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["nodes"]["worker"]["status"], "completed", "run: {run}");

    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git_out(repo, &["log", "--format=%s", "-1", &branch]),
        "worker: everything, by hand",
        "a clean tree ⇒ no empty commit on top"
    );
}

// ── script × its own sub-worktree ────────────────────────────────────────────

/// An isolated `script` works in its own sub-worktree and PDO merges it back —
/// the same full cycle an isolated agent gets.
#[tokio::test]
async fn an_isolated_script_works_and_delivers_from_its_sub_worktree() {
    ensure_pdo_on_path();
    let name = "deliver-isolated-script";
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'pub fn from_the_sub_worktree() {}\\n' > sub.rs\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", true),
        "deliver-isolated-script",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "completed").await;
    assert_eq!(run["nodes"]["worker"]["status"], "completed", "run: {run}");

    // It really did work in its own directory, not in the Run's.
    assert!(
        sub_worktree(&daemon, &run_id, "worker")
            .join("sub.rs")
            .exists(),
        "an isolated script works in its sub-worktree"
    );
    let branch = format!("pdo/run-{run_id}");
    let files = git_out(repo, &["show", "--name-only", "--format=", &branch]);
    assert!(
        files.contains("sub.rs"),
        "…and PDO merges that work into the run's branch: {files}"
    );
    assert!(
        run_worktree(&daemon, &run_id).join("sub.rs").exists(),
        "the merge lands in the run's worktree too"
    );
    assert!(
        !run["nodes"]["worker"]["delivery"].is_null(),
        "an isolated delivery is recorded like any other: {run}"
    );
}

// ── the Feature Path: shared upstream, isolated downstream ───────────────────

/// The FP of #654: a non-isolated node's work is on the Run's branch **before**
/// the downstream node is dispatched, so an isolated successor forked afterwards
/// sees it. Both deliveries end up on the branch, in order.
#[tokio::test]
async fn an_isolated_downstream_node_sees_an_upstream_shared_node_work() {
    ensure_pdo_on_path();
    let name = "deliver-handoff";
    let yaml = format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  \
         - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  \
         - id: shared\n    name: shared\n    type: script\n    isolated_worktree: false\n    outputs:\n      - name: out\n  \
         - id: forked\n    name: forked\n    type: script\n    isolated_worktree: true\n    outputs:\n      - name: out\n  \
         - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\
         edges:\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: shared, port: in }}\n  \
         - source: {{ node: shared, port: out }}\n    target: {{ node: forked, port: in }}\n  \
         - source: {{ node: forked, port: out }}\n    target: {{ node: end, port: result }}\n"
    );
    // The downstream node PROVES it sees the upstream work: it reads the file and
    // fails (exit 1) if it is not there.
    let shared_body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'from the shared node\\n' > handoff.txt\n\
        printf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n";
    let forked_body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        grep -q 'from the shared node' handoff.txt\n\
        printf 'seen\\n' > forked.txt\n\
        printf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n";
    let daemon = TestDaemon::spawn(seed(
        yaml,
        "deliver-handoff",
        vec![
            ("shared", shared_body.to_string()),
            ("forked", forked_body.to_string()),
        ],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "forked", "completed").await;
    assert_eq!(
        run["nodes"]["forked"]["status"], "completed",
        "the downstream node must find the upstream file in its fresh fork; run: {run}"
    );

    let branch = format!("pdo/run-{run_id}");
    let files = git_out(repo, &["log", "--name-only", "--format=", &branch]);
    assert!(
        files.contains("handoff.txt") && files.contains("forked.txt"),
        "both deliveries are on the run's branch: {files}"
    );
    // Both NodeRuns have a diff, whatever their isolation.
    for node in ["shared", "forked"] {
        assert!(
            !run["nodes"][node]["delivery"].is_null(),
            "{node} delivered and must say so: {run}"
        );
    }
}

// ── agent × both isolations, driven through `pdo complete` ───────────────────

/// The `agent` half of the matrix, non-isolated: the node makes no git call at
/// all — the test writes files into its working directory exactly as an agent
/// would — and `POST …/done` delivers them.
#[tokio::test]
async fn a_shared_agent_delivers_what_it_left_in_the_run_worktree() {
    let name = "deliver-shared-agent";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "agent", false),
        "deliver-shared-agent",
        vec![("worker", "You are a worker.\n".to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "running").await;
    assert_eq!(run["nodes"]["worker"]["status"], "running", "run: {run}");

    let wt = run_worktree(&daemon, &run_id);
    std::fs::write(wt.join("tracked.txt"), "edited by the agent\n").unwrap();
    std::fs::write(wt.join("agent_new.rs"), "pub fn agent() {}\n").unwrap();

    assert_eq!(complete(&daemon, &run_id, "worker").await, 200);

    let branch = format!("pdo/run-{run_id}");
    assert_eq!(
        git_out(repo, &["log", "--format=%s", "-1", &branch]),
        "worker iter-1: completed"
    );
    let files = git_out(repo, &["show", "--name-only", "--format=", &branch]);
    assert!(
        files.contains("tracked.txt") && files.contains("agent_new.rs"),
        "a non-isolated agent's edits reach the run's branch: {files}"
    );
}

/// The `agent` half, isolated: it works in its sub-worktree, PDO commits what is
/// left and merges the branch back.
#[tokio::test]
async fn an_isolated_agent_delivers_from_its_sub_worktree() {
    let name = "deliver-isolated-agent";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "agent", true),
        "deliver-isolated-agent",
        vec![("worker", "You are a worker.\n".to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "running").await;
    assert_eq!(run["nodes"]["worker"]["status"], "running", "run: {run}");

    let sub = sub_worktree(&daemon, &run_id, "worker");
    assert!(sub.is_dir(), "an isolated agent gets a sub-worktree");
    std::fs::write(sub.join("forked.rs"), "pub fn forked() {}\n").unwrap();

    assert_eq!(complete(&daemon, &run_id, "worker").await, 200);

    let branch = format!("pdo/run-{run_id}");
    let files = git_out(repo, &["log", "--name-only", "--format=", &branch]);
    assert!(files.contains("forked.rs"), "merged back: {files}");
}

// ── a git error is loud, and destroys nothing ────────────────────────────────

/// A staging failure interrupts the NodeRun, parks the Run `awaiting_user`, and
/// leaves the work exactly where the node left it.
#[tokio::test]
async fn a_git_failure_interrupts_the_node_and_keeps_the_work() {
    let name = "deliver-git-error";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "agent", false),
        "deliver-git-error",
        vec![("worker", "You are a worker.\n".to_string())],
    ))
    .await
    .unwrap();

    let run_id = start_run(&daemon, name).await;
    let run = wait_for_node_status(&daemon, &run_id, "worker", "running").await;
    assert_eq!(run["nodes"]["worker"]["status"], "running", "run: {run}");

    let wt = run_worktree(&daemon, &run_id);
    std::fs::write(wt.join("precious.rs"), "pub fn precious() {}\n").unwrap();
    // A leftover `index.lock` is the real-world shape of this failure (#489): it
    // makes `git add -A` exit non-zero without touching the working tree.
    let git_dir = git_out(&wt, &["rev-parse", "--absolute-git-dir"]);
    std::fs::write(std::path::Path::new(&git_dir).join("index.lock"), "").unwrap();

    let status = complete(&daemon, &run_id, "worker").await;
    assert_eq!(
        status, 500,
        "a delivery that cannot run is a panne, not a verdict"
    );

    let run = wait_for_node_status(&daemon, &run_id, "worker", "interrupted").await;
    assert_eq!(
        run["nodes"]["worker"]["status"], "interrupted",
        "a git failure interrupts the NodeRun, it never fails it: {run}"
    );
    assert_eq!(run["status"], "awaiting_user", "the run parks: {run}");
    assert_eq!(
        run["awaiting_reason_code"], "delivery_failed",
        "the cause is named in the run state: {run}"
    );
    assert!(
        wt.join("precious.rs").exists(),
        "the work stays on disk, untouched"
    );
}

// ── two shared NodeRuns at once ──────────────────────────────────────────────

/// Two non-isolated NodeRuns run side by side with no serialisation added by the
/// runtime, and the first one to complete commits every non-ignored change then
/// visible — under its own name. The test asserts the *state*, never who wrote
/// which file: the sharp-tool posture is precisely that PDO does not pretend to
/// attribute it (ADR-0060).
#[tokio::test]
async fn two_shared_nodes_run_concurrently_and_the_state_is_delivered_whole() {
    ensure_pdo_on_path();
    let name = "deliver-concurrent";
    let yaml = format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  \
         - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  \
         - id: left\n    name: left\n    type: script\n    isolated_worktree: false\n    outputs:\n      - name: out\n  \
         - id: right\n    name: right\n    type: script\n    isolated_worktree: false\n    outputs:\n      - name: out\n  \
         - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\
         edges:\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: left, port: in }}\n  \
         - source: {{ node: start, port: user_prompt }}\n    target: {{ node: right, port: in }}\n  \
         - source: {{ node: left, port: out }}\n    target: {{ node: end, port: result }}\n  \
         - source: {{ node: right, port: out }}\n    target: {{ node: end, port: result }}\n"
    );
    let body_for = |file: &str| {
        format!(
            "#!/usr/bin/env bash\nset -euo pipefail\n\
             printf 'concurrent\\n' > {file}\n\
             printf 'ok\\n' > \"$PDO_OUTPUT_OUT\"\n"
        )
    };
    let daemon = TestDaemon::spawn(seed(
        yaml,
        "deliver-concurrent",
        vec![
            ("left", body_for("left.txt")),
            ("right", body_for("right.txt")),
        ],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    for node in ["left", "right"] {
        let run = wait_for_node_status(&daemon, &run_id, node, "completed").await;
        assert_eq!(
            run["nodes"][node]["status"], "completed",
            "{node} must complete — the runtime serialises nothing; run: {run}"
        );
    }

    // Both files are on the branch. WHICH commit carries which is deliberately
    // not asserted: whoever completed first took whatever was there.
    let branch = format!("pdo/run-{run_id}");
    let files = git_out(repo, &["log", "--name-only", "--format=", &branch]);
    assert!(
        files.contains("left.txt") && files.contains("right.txt"),
        "the shared worktree's whole state reaches the branch: {files}"
    );
    let subjects = git_out(repo, &["log", "--format=%s", &branch]);
    assert!(
        subjects
            .lines()
            .any(|s| s == "left iter-1: completed" || s == "right iter-1: completed"),
        "at least one delivery commit carries its own node's name: {subjects}"
    );
}

// ── recovery: an isolated script keeps its directory and its work ────────────

/// An isolated `script` gets the whole cycle, recovery included: killing its
/// session and restarting the same iteration REUSES its sub-worktree in place, so
/// the work it had already written is still there for the second attempt to
/// deliver. Nothing here is agent-specific — the sub-worktree is a property of the
/// isolation, not of the node type (#654 / ADR-0060).
#[tokio::test]
async fn restarting_an_interrupted_isolated_script_reuses_its_directory_and_work() {
    ensure_pdo_on_path();
    let name = "deliver-script-restart";
    // Write first, then hang: the kill lands with real work on disk.
    let body = "#!/usr/bin/env bash\nset -euo pipefail\n\
        printf 'half done\\n' > partial.txt\n\
        sleep 120\n";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "script", true),
        "deliver-script-restart",
        vec![("worker", body.to_string())],
    ))
    .await
    .unwrap();

    let run_id = start_run(&daemon, name).await;
    wait_for_node_status(&daemon, &run_id, "worker", "running").await;
    let sub = sub_worktree(&daemon, &run_id, "worker");
    // The body has to have run far enough to have written its file.
    let started = std::time::Instant::now();
    while !sub.join("partial.txt").exists() && started.elapsed() < Duration::from_secs(30) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        sub.join("partial.txt").exists(),
        "the script must have written its partial work before the kill"
    );

    let _ = Command::new("tmux")
        .args([
            "-L",
            &daemon.tmux_socket(),
            "kill-session",
            "-t",
            &format!("pdo-{run_id}-worker-iter-1"),
        ])
        .output();

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "restart_node", "node_id": "worker", "iter": 1
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "restart_node: {body}");
    assert_eq!(
        body["reused_sub_worktree"], true,
        "the restart reuses the directory rather than cutting a new one: {body}"
    );
    assert!(
        sub.join("partial.txt").exists(),
        "…and the work the first attempt left is still there"
    );
}

// ── the manual completion delivers too ───────────────────────────────────────

/// AC1: the human's *Mark complete* (`mark_node_done`) is a completion path like
/// any other and goes through the same delivery. Pre-#654 this arm ran neither
/// the merge-back nor any worktree handling, so an interactive node marked done
/// from the UI stranded its work.
#[tokio::test]
async fn the_manual_completion_delivers_like_every_other() {
    let name = "deliver-manual";
    let daemon = TestDaemon::spawn(seed(
        one_worker_pipeline(name, "agent", true),
        "deliver-manual",
        vec![("worker", "You are a worker.\n".to_string())],
    ))
    .await
    .unwrap();

    let repo = daemon.repo_root();
    let run_id = start_run(&daemon, name).await;
    wait_for_node_status(&daemon, &run_id, "worker", "running").await;

    let sub = sub_worktree(&daemon, &run_id, "worker");
    std::fs::write(sub.join("by_hand.rs"), "pub fn by_hand() {}\n").unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "mark_node_done", "node_id": "worker", "iter": 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "mark_node_done should complete the node"
    );

    let branch = format!("pdo/run-{run_id}");
    let files = git_out(repo, &["log", "--name-only", "--format=", &branch]);
    assert!(
        files.contains("by_hand.rs"),
        "the manual completion merged the sub-worktree back: {files}"
    );
    let run = get_run(&daemon, &run_id).await;
    assert!(
        !run["nodes"]["worker"]["delivery"].is_null(),
        "…and recorded the delivery: {run}"
    );
}
