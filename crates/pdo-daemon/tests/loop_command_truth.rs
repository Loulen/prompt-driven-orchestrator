//! Layer 3a — ADR-0025 / #327: `extend_cycle`, `bump_region`, `end_region` and
//! `resume_run` tell the truth. Validation happens against the run's pipeline
//! snapshot BEFORE any event is appended (a rejected command leaves no trace in
//! the event log), and the 200 body reports the re-scheduling's real effect
//! (`{ok,spawned:[…]}` or `{ok,noop:true,reason}`), never a blind `{ok:true}`.

mod common;

use std::process::Command;

use common::TestDaemon;

const LOOP_PIPELINE_NAME: &str = "loop-truth-test";
const LOOP_PIPELINE_YAML: &str = r#"name: loop-truth-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
    when: "iter >= max"
loops:
  - id: review_loop
    kind: bounded
    members: [worker]
    max_iter: 3
"#;

const LEGACY_PIPELINE_NAME: &str = "legacy-truth-test";
const LEGACY_PIPELINE_YAML: &str = r#"name: legacy-truth-test
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
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
"#;

/// A bounded region that exhausts after ONE lap with no route out — the shape
/// of the repo's real `review-loop`, shrunk. Once `rev` finishes lap 1 the
/// region is `exhausted — unrouted`, the run halts, and the operator's only way
/// forward is `bump_region`. That command then genuinely RE-SCHEDULES work,
/// which is the one situation exercising the `spawned:[…]` branch of ADR-0025.
const EXHAUST_PIPELINE_NAME: &str = "exhaust-truth-test";
const EXHAUST_PIPELINE_YAML: &str = r#"name: exhaust-truth-test
version: "1.0"
variables:
  max_laps: 1
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: impl
    name: impl
    type: doc-only
    outputs:
      - name: code
  - id: rev
    name: rev
    type: doc-only
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: impl, port: task }
  - source: { node: impl, port: code }
    target: { node: rev, port: code }
  - source: { node: rev, port: review }
    target: { node: impl, port: task }
    else: true
loops:
  - id: review_loop
    kind: bounded
    members: [rev, impl]
    max_iter: $max_laps
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    for (name, yaml, prompt_node) in [
        (LOOP_PIPELINE_NAME, LOOP_PIPELINE_YAML, "worker"),
        (LEGACY_PIPELINE_NAME, LEGACY_PIPELINE_YAML, "planner"),
        (EXHAUST_PIPELINE_NAME, EXHAUST_PIPELINE_YAML, "impl"),
    ] {
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml)?;
        let prompts_dir = pipelines_dir.join(format!("{name}.prompts"));
        std::fs::create_dir_all(&prompts_dir)?;
        std::fs::write(
            prompts_dir.join(format!("{prompt_node}.md")),
            "You are a test node.\n",
        )?;
    }
    git_init_with_commit(repo)?;
    Ok(())
}

fn git_init_with_commit(repo: &std::path::Path) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-b", "main"])?;
    run(&["config", "user.email", "test@test.com"])?;
    run(&["config", "user.name", "Test"])?;
    std::fs::write(repo.join("README.md"), "test")?;
    run(&["add", "."])?;
    run(&["commit", "-m", "init"])?;
    Ok(())
}

async fn create_run(daemon: &TestDaemon, pipeline: &str) -> String {
    // #470: the target repo is required at the create boundary (ADR-0033).
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "test input",
        "variables": {},
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
    assert_eq!(status, 201, "POST /runs should succeed, got body: {text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn post_command(
    daemon_url: &str,
    run_id: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{daemon_url}/runs/{run_id}/commands"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// The run's `command_issued` events matching a given `command` payload.
async fn command_events(daemon_url: &str, run_id: &str, command: &str) -> Vec<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e["kind"] == "command_issued" && e["payload"]["command"] == command)
        .collect()
}

#[tokio::test]
async fn extend_cycle_unknown_node_is_400_and_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LEGACY_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "extend_cycle", "node_id": "ghost", "additional_iter": 2 }),
    )
    .await;

    assert_eq!(status, 400, "unknown node must be rejected, got {body}");
    assert_eq!(
        body["error"], "node 'ghost' not found in pipeline",
        "message follows the start_node precedent"
    );
    // A rejection leaves NO trace: neither the extend_cycle CommandIssued nor
    // a resume_run lift may have been appended.
    assert!(
        command_events(&daemon.url(), &run_id, "extend_cycle")
            .await
            .is_empty(),
        "rejected extend_cycle must not append a CommandIssued"
    );
    assert!(
        command_events(&daemon.url(), &run_id, "resume_run")
            .await
            .is_empty(),
        "rejected extend_cycle must not lift a Halt"
    );
}

#[tokio::test]
async fn extend_cycle_on_region_member_is_409_naming_the_region() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    // `worker` is both a member AND the entry/head of `review_loop` — the head
    // is a member like any other, so it must be 409 too (grilling decision 1).
    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "extend_cycle", "node_id": "worker", "additional_iter": 2 }),
    )
    .await;

    assert_eq!(status, 409, "region member must be rejected, got {body}");
    let msg = body["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("'review_loop'") && msg.contains("bump_region"),
        "409 must name the region and redirect to bump_region, got: {msg}"
    );
    assert!(
        command_events(&daemon.url(), &run_id, "extend_cycle")
            .await
            .is_empty(),
        "rejected extend_cycle must not append a CommandIssued"
    );
}

#[tokio::test]
async fn extend_cycle_on_legacy_pipeline_is_200_with_truthful_body() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LEGACY_PIPELINE_NAME).await;

    // No `loops:` block → legacy cycles keep working through extend_cycle.
    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "extend_cycle", "node_id": "planner", "additional_iter": 2 }),
    )
    .await;

    assert_eq!(
        status, 200,
        "legacy extend_cycle must stay accepted: {body}"
    );
    assert_eq!(body["ok"], true);
    // Truthful body: either something spawned, or an explicit noop with reason —
    // never a bare `{ok:true}`.
    let truthful =
        body["spawned"].is_array() || (body["noop"] == true && body["reason"].is_string());
    assert!(truthful, "body must report the real effect, got: {body}");
    // And the command IS recorded this time.
    assert_eq!(
        command_events(&daemon.url(), &run_id, "extend_cycle")
            .await
            .len(),
        1,
        "accepted extend_cycle must append its CommandIssued"
    );
}

#[tokio::test]
async fn bump_region_unknown_region_is_400_and_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "bump_region", "region_id": "ghost", "additional_iter": 2 }),
    )
    .await;

    assert_eq!(status, 400, "unknown region must be rejected, got {body}");
    assert_eq!(body["error"], "region 'ghost' not found in pipeline");
    assert!(
        command_events(&daemon.url(), &run_id, "bump_region")
            .await
            .is_empty(),
        "rejected bump_region must not append a CommandIssued"
    );
}

#[tokio::test]
async fn end_region_unknown_region_is_400() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "end_region", "region_id": "ghost" }),
    )
    .await;

    assert_eq!(status, 400, "unknown region must be rejected, got {body}");
    assert_eq!(body["error"], "region 'ghost' not found in pipeline");
    assert!(
        command_events(&daemon.url(), &run_id, "end_region")
            .await
            .is_empty(),
        "rejected end_region must not append a CommandIssued"
    );
}

#[tokio::test]
async fn bump_region_with_live_iteration_is_noop_with_reason() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    // `worker` iter 1 is live (the harmless sleep session): nothing is eligible
    // to re-spawn, so the truthful body is an explicit noop with a reason.
    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "bump_region", "region_id": "review_loop", "additional_iter": 2 }),
    )
    .await;

    assert_eq!(status, 200, "valid bump_region must be accepted: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["noop"], true,
        "no spawn is possible while the lap is live, got: {body}"
    );
    assert!(
        body["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "noop must carry a non-empty reason, got: {body}"
    );
    // The command IS recorded (it applies when the lap finishes).
    assert_eq!(
        command_events(&daemon.url(), &run_id, "bump_region")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn resume_run_reports_noop_when_nothing_to_spawn() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LEGACY_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "resume_run" }),
    )
    .await;

    assert_eq!(status, 200, "resume_run stays accepted: {body}");
    assert_eq!(body["ok"], true);
    let truthful =
        body["spawned"].is_array() || (body["noop"] == true && body["reason"].is_string());
    assert!(truthful, "body must report the real effect, got: {body}");
}

/// Every `(node_id, iter)` the run has ever started.
async fn started_pairs(daemon_url: &str, run_id: &str) -> Vec<(String, i64)> {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json.as_array()
        .cloned()
        .unwrap_or_default()
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

/// How many times a given node has started, whatever the iteration.
async fn node_started_count(daemon_url: &str, run_id: &str, node_id: &str) -> usize {
    started_pairs(daemon_url, run_id)
        .await
        .into_iter()
        .filter(|(id, _)| id == node_id)
        .count()
}

async fn run_status(daemon_url: &str, run_id: &str) -> String {
    let resp = reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["status"].as_str().unwrap_or_default().to_string()
}

/// Poll until `pred` holds or the budget runs out. Layer 3a drives a real
/// daemon, so the scheduler's work is asynchronous to the HTTP call.
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

#[tokio::test]
async fn bump_region_that_actually_reschedules_reports_the_spawn() {
    // The `spawned:[{node_id,iter}]` branch of ADR-0025. Until this test it was
    // pinned NOWHERE: the three sibling assertions accept the disjunction
    // `spawned.is_array() || noop`, so a regression that answered `noop`
    // forever would have kept the whole suite green.
    //
    // Getting a real spawn out of a command needs a region whose current lap is
    // FINISHED and whose next lap only becomes admissible because of the
    // command. `tight_loop` has `max_iter: 1` and no exit edge, so once worker
    // iter 1 completes the run halts `exhausted — unrouted` — the operator
    // situation `bump_region` exists for.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, EXHAUST_PIPELINE_NAME).await;
    let url = daemon.url();

    // Worker iter 1 must be up before it can be completed.
    // Entry node `impl` is spawned by run creation.
    wait_until("impl iter 1 to start", || async {
        node_started_count(&url, &run_id, "impl").await >= 1
    })
    .await;

    // Drive lap 1 to its end. `mark_node_done` validates declared outputs, so
    // write each node's artifact first — through the command surface, not
    // behind it.
    for (node_id, port) in [("impl", "code"), ("rev", "review")] {
        wait_until(&format!("{node_id} iter 1 to start"), || async {
            node_started_count(&url, &run_id, node_id).await >= 1
        })
        .await;
        let (status, body) = post_command(
            &url,
            &run_id,
            serde_json::json!({
                "kind": "inject_artifact",
                "path": format!("{node_id}/iter-1/{port}/output.md"),
                "content": "# done\n",
            }),
        )
        .await;
        assert_eq!(status, 200, "inject_artifact for {node_id}: {body}");

        let (status, body) = post_command(
            &url,
            &run_id,
            serde_json::json!({ "kind": "mark_node_done", "node_id": node_id, "iter": 1 }),
        )
        .await;
        assert_eq!(status, 200, "mark_node_done for {node_id}: {body}");
    }

    // The region is now exhausted with nowhere to route: the run halts.
    wait_until("the run to halt exhausted-unrouted", || async {
        run_status(&url, &run_id).await == "halted"
    })
    .await;

    let before = started_pairs(&url, &run_id).await;

    let (status, body) = post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "bump_region", "region_id": "review_loop", "additional_iter": 1 }),
    )
    .await;
    assert_eq!(status, 200, "bump_region must be accepted: {body}");
    assert_eq!(body["ok"], true);

    // THE assertion this test exists for: a real spawn, reported as one.
    let spawned = body["spawned"]
        .as_array()
        .unwrap_or_else(|| panic!("expected spawned:[…], got: {body}"));
    assert!(!spawned.is_empty(), "spawned must not be empty: {body}");
    assert!(
        body.get("noop").is_none(),
        "a run that spawned is not a noop: {body}"
    );

    let mut reported: Vec<(String, i64)> = spawned
        .iter()
        .map(|e| {
            (
                e["node_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("each entry carries a node_id: {body}"))
                    .to_string(),
                e["iter"]
                    .as_i64()
                    .unwrap_or_else(|| panic!("each entry carries an iter: {body}")),
            )
        })
        .collect();
    reported.sort();
    assert_eq!(
        reported,
        vec![("impl".to_string(), 2), ("rev".to_string(), 2)],
        "the bump buys lap 2 for the region's members: {body}"
    );

    // Proof obligation inherited from #108: the body may not claim a spawn the
    // event log does not show. One `node_started` per reported pair, no more.
    let after = started_pairs(&url, &run_id).await;
    let mut fresh: Vec<(String, i64)> = after
        .iter()
        .filter(|pair| !before.contains(pair))
        .cloned()
        .collect();
    fresh.sort();
    assert_eq!(
        fresh, reported,
        "event log and response body must agree (before={before:?}, after={after:?}): {body}"
    );
    assert_eq!(
        after.len() - before.len(),
        spawned.len(),
        "no unreported spawn either: {body}"
    );

    // And the Halt was lifted, as the arm promises.
    assert_eq!(
        command_events(&url, &run_id, "resume_run").await.len(),
        1,
        "routing a halted region lifts the Halt"
    );
}
