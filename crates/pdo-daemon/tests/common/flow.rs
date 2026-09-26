//! Small building blocks for tests that drive a Run end to end over HTTP: seed a
//! git repository and its pipelines, launch a Run, wait for a node, complete it.
//!
//! Every file of `tests/` used to carry its own copy of these helpers. The route
//! tests of the refactor net (#914) and the temporary snapshot suite share this
//! one instead, so a change of the HTTP contract has one place to be read from.

use std::path::Path;
use std::time::Duration;

use super::TestDaemon;

/// A three-node pipeline, `start → worker → end`, with `worker` a non-isolated
/// agent working in the Run's worktree. `interactive` flips the worker's
/// completion gate (ADR-0068).
pub fn solo_pipeline_yaml(name: &str, interactive: bool) -> String {
    format!(
        r#"name: {name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    interactive: {interactive}
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: worker, port: task }}
  - source: {{ node: worker, port: out }}
    target: {{ node: end, port: result }}
"#
    )
}

/// Run `git <args>` in `dir`, failing loudly with git's stderr.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `git init` on `main` with one commit (a `.gitignore` for the Run dirs and a
/// `lib.rs` a review can comment on).
pub fn git_init(repo: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(repo)?;
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    std::fs::write(repo.join("lib.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n")?;
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
    Ok(())
}

/// Write `<name>.yaml` (and a prompt for `worker`) into the instance pipeline
/// directory a test daemon reads: `<repo>/.pdo/pipelines/`.
pub fn write_pipeline(repo: &Path, name: &str, yaml: &str) -> anyhow::Result<()> {
    let dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{name}.yaml")), yaml)?;
    let prompts = dir.join(format!("{name}.prompts"));
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(prompts.join("worker.md"), "Do the work.\n")?;
    Ok(())
}

/// Seed a daemon repo with a git history and the given `(name, interactive)`
/// solo pipelines.
pub fn seed_solo(repo: &Path, pipelines: &[(&str, bool)]) -> anyhow::Result<()> {
    for (name, interactive) in pipelines {
        write_pipeline(repo, name, &solo_pipeline_yaml(name, *interactive))?;
    }
    git_init(repo)
}

/// `POST /runs` for `pipeline` in the daemon's own repo; returns the run id.
pub async fn create_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&serde_json::json!({
            "pipeline": pipeline,
            "input": "go",
            "name": format!("{pipeline} run"),
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert_eq!(status, 201, "POST /runs should create the run: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

/// Poll `GET /runs/{run_id}` until `check` accepts it (10 s budget).
pub async fn wait_run<F>(
    daemon: &TestDaemon,
    run_id: &str,
    what: &str,
    check: F,
) -> serde_json::Value
where
    F: Fn(&serde_json::Value) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let run = daemon.get_json(&format!("/runs/{run_id}")).await;
        if check(&run) {
            return run;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "run {run_id} never reached: {what}; last state: {run}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait until `node_id` of `run_id` reads `status`.
pub async fn wait_node_status(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    status: &str,
) -> serde_json::Value {
    wait_run(daemon, run_id, &format!("{node_id} is {status}"), |run| {
        run["nodes"][node_id]["status"] == status
    })
    .await
}

/// Wait until the Run itself reads `status`.
pub async fn wait_run_status(daemon: &TestDaemon, run_id: &str, status: &str) -> serde_json::Value {
    wait_run(daemon, run_id, &format!("run is {status}"), |run| {
        run["status"] == status
    })
    .await
}

/// The Run's pipeline worktree (the cwd of every non-isolated node).
pub fn worktree(daemon: &TestDaemon, run_id: &str) -> std::path::PathBuf {
    daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

/// Write the artifact `node_id`'s `out` port expects, where the agent would.
pub fn write_out_artifact(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    write_out_artifact_at(daemon, run_id, node_id, 1);
}

/// [`write_out_artifact`] for iteration `iter`.
pub fn write_out_artifact_at(daemon: &TestDaemon, run_id: &str, node_id: &str, iter: i64) {
    let dir = worktree(daemon, run_id)
        .join(".pdo")
        .join("artifacts")
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "artifact\n").unwrap();
}

/// `POST {path}` with a JSON body; answers the status and the body parsed as
/// JSON (`Null` when the body is not JSON).
pub async fn post_json(
    daemon: &TestDaemon,
    path: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// Complete `node_id` (iteration 1) the way `pdo complete` does: write its
/// artifact, then `POST …/done`. Answers the `done` status and body.
pub async fn complete_node(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
) -> (u16, serde_json::Value) {
    write_out_artifact(daemon, run_id, node_id);
    post_json(
        daemon,
        &format!("/runs/{run_id}/nodes/{node_id}/done"),
        serde_json::json!({ "iter": 1 }),
    )
    .await
}

/// Launch a solo Run and drive it to `completed`; answers the run id.
pub async fn completed_solo_run(daemon: &TestDaemon, pipeline: &str) -> String {
    let run_id = create_run(daemon, pipeline).await;
    wait_node_status(daemon, &run_id, "worker", "running").await;
    let (status, body) = complete_node(daemon, &run_id, "worker").await;
    assert_eq!(status, 200, "worker completes: {body}");
    wait_run_status(daemon, &run_id, "completed").await;
    run_id
}

/// Is a tmux session up on this daemon's private socket?
pub fn tmux_session_exists(daemon: &TestDaemon, session: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", &daemon.tmux_socket(), "has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
