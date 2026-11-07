//! Layer 3a — ADR-0025 / #327: `extend_cycle`, `bump_region`, `end_region` and
//! `resume_run` tell the truth. Validation happens against the run's pipeline
//! snapshot BEFORE any event is appended (a rejected command leaves no trace in
//! the event log), and the 200 body reports the re-scheduling's real effect
//! (`{ok,spawned:[…]}` or `{ok,noop:true,reason}`), never a blind `{ok:true}`.

use std::process::Command;

use crate::common::TestDaemon;

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
    type: agent
    isolated_worktree: false
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
    type: agent
    isolated_worktree: false
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
    type: agent
    isolated_worktree: false
    outputs:
      - name: code
  - id: rev
    name: rev
    type: agent
    isolated_worktree: false
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

/// #1017: a `collection` region — never cost-capped (Q10), so
/// `set_region_max_cost` on it is refused.
const COLLECTION_PIPELINE_NAME: &str = "collection-truth-test";
const COLLECTION_PIPELINE_YAML: &str = r#"name: collection-truth-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: lister
    name: lister
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
        frontmatter:
          issues:
            type: list
  - id: worker
    name: worker
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
    target: { node: lister, port: task }
  - source: { node: lister, port: plan }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
loops:
  - id: per_issue
    kind: collection
    over: issues
    members: [worker]
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    for (name, yaml, prompt_node) in [
        (LOOP_PIPELINE_NAME, LOOP_PIPELINE_YAML, "worker"),
        (LEGACY_PIPELINE_NAME, LEGACY_PIPELINE_YAML, "planner"),
        (EXHAUST_PIPELINE_NAME, EXHAUST_PIPELINE_YAML, "impl"),
        (COLLECTION_PIPELINE_NAME, COLLECTION_PIPELINE_YAML, "lister"),
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
async fn set_region_max_iter_unknown_region_is_400_and_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "set_region_max_iter", "region_id": "ghost", "max_iter": 9 }),
    )
    .await;

    assert_eq!(status, 400, "unknown region must be rejected, got {body}");
    assert_eq!(body["error"], "region 'ghost' not found in pipeline");
    assert!(
        command_events(&daemon.url(), &run_id, "set_region_max_iter")
            .await
            .is_empty(),
        "rejected set_region_max_iter must not append a CommandIssued"
    );
}

#[tokio::test]
async fn set_region_max_iter_on_a_known_region_appends_the_command() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "set_region_max_iter", "region_id": "review_loop", "max_iter": 9 }),
    )
    .await;

    assert_eq!(status, 200, "known region accepted, got {body}");
    assert!(
        !command_events(&daemon.url(), &run_id, "set_region_max_iter")
            .await
            .is_empty(),
        "an accepted set_region_max_iter appends its CommandIssued"
    );
}

// ---- #1017: `set_region_max_cost` — the live Plafond de coût override ----

async fn run_json(daemon_url: &str, run_id: &str) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{daemon_url}/runs/{run_id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn set_region_max_cost_unknown_region_is_400_and_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "ghost", "max_cost_usd": 5 }),
    )
    .await;

    assert_eq!(status, 400, "unknown region must be rejected, got {body}");
    assert_eq!(body["error"], "region 'ghost' not found in pipeline");
    assert!(
        command_events(&daemon.url(), &run_id, "set_region_max_cost")
            .await
            .is_empty(),
        "rejected set_region_max_cost must not append a CommandIssued"
    );
}

#[tokio::test]
async fn set_region_max_cost_is_refused_on_a_collection_and_for_a_non_positive_cap() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, COLLECTION_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "per_issue", "max_cost_usd": 5 }),
    )
    .await;
    assert_eq!(status, 400, "a collection is never cost-capped, got {body}");
    assert!(
        body["error"].as_str().unwrap_or_default().contains("bounded"),
        "says why: {body}"
    );

    let loop_run = create_run(&daemon, LOOP_PIPELINE_NAME).await;
    for bad in [serde_json::json!(0), serde_json::json!(-1)] {
        let (status, body) = post_command(
            &daemon.url(),
            &loop_run,
            serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": bad }),
        )
        .await;
        assert_eq!(status, 400, "≤ 0 is refused, got {body}");
    }
    for run in [&run_id, &loop_run] {
        assert!(
            command_events(&daemon.url(), run, "set_region_max_cost")
                .await
                .is_empty(),
            "a refused set_region_max_cost leaves no trace"
        );
    }
}

#[tokio::test]
async fn set_region_max_cost_sets_raises_lowers_and_removes_the_override_and_survives_a_reopen() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;
    let url = daemon.url();

    // Set, then raise, then lower (no floor: any positive amount is accepted).
    for cap in [1.0, 5.0, 0.01] {
        let (status, body) = post_command(
            &url,
            &run_id,
            serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": cap }),
        )
        .await;
        assert_eq!(status, 200, "cap {cap} accepted, got {body}");
        assert_eq!(body["ok"], true, "truthful body (ADR-0025): {body}");
        let run = run_json(&url, &run_id).await;
        assert_eq!(
            run["region_max_cost_overrides"]["review_loop"].as_f64(),
            Some(cap),
            "the override is the last write: {run}"
        );
    }

    // `null` removes the cap — explicitly, on the wire too.
    let (status, body) = post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": null }),
    )
    .await;
    assert_eq!(status, 200, "null removal accepted, got {body}");
    let run = run_json(&url, &run_id).await;
    assert!(
        run["region_max_cost_overrides"]
            .as_object()
            .is_some_and(|o| o.contains_key("review_loop") && o["review_loop"].is_null()),
        "removal is an explicit null override: {run}"
    );
    assert_eq!(
        command_events(&url, &run_id, "set_region_max_cost").await.len(),
        4,
        "every accepted edit is in the event log"
    );

    // The override lives in the log: re-cap, reopen, it is still there.
    post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": 2.5 }),
    )
    .await;
    let _ = post_command(&url, &run_id, serde_json::json!({ "kind": "reopen_run" })).await;
    let run = run_json(&url, &run_id).await;
    assert_eq!(
        run["region_max_cost_overrides"]["review_loop"].as_f64(),
        Some(2.5),
        "the override survives a reopen re-projection: {run}"
    );
}

#[tokio::test]
async fn force_route_unknown_source_is_400_and_leaves_no_trace() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "force_route", "from": "ghost", "target": "end" }),
    )
    .await;

    assert_eq!(status, 400, "unknown source must be rejected, got {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("neither a node nor a region"),
        "error names the bad source: {body}"
    );
    assert!(
        command_events(&daemon.url(), &run_id, "force_route")
            .await
            .is_empty(),
        "rejected force_route must not append a CommandIssued"
    );
}

#[tokio::test]
async fn force_route_unknown_target_is_400() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, LOOP_PIPELINE_NAME).await;

    let (status, body) = post_command(
        &daemon.url(),
        &run_id,
        serde_json::json!({ "kind": "force_route", "from": "worker", "target": "ghost" }),
    )
    .await;

    assert_eq!(status, 400, "unknown target must be rejected, got {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not a node"),
        "error names the bad target: {body}"
    );
    assert!(
        command_events(&daemon.url(), &run_id, "force_route")
            .await
            .is_empty(),
        "rejected force_route must not append a CommandIssued"
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

    // Entry node `impl` is spawned by run creation, not by a command.
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

    // The region is now exhausted with nowhere to route: since résilience
    // (ADR-0049) an `unrouted` convergence parks the run `AwaitingUser` (a human
    // routes it), instead of the pre-résilience terminal `halted`.
    wait_until(
        "the run to park awaiting-user (exhausted-unrouted)",
        || async { run_status(&url, &run_id).await == "awaiting_user" },
    )
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
    // The bump buys lap 2, and lap 2 restarts at the FRONTIER HEAD alone (#626,
    // shipped in 1.36.1). This assertion used to expect both members at iter 2 at
    // once, and that expectation was the bug: re-firing every member of a finished
    // region forked the loop into two branches racing on one worktree. The
    // freshness guards now let only `impl` resume; `rev` follows turn by turn when
    // `impl` completes, exactly as it does on a nominal lap.
    assert_eq!(
        reported,
        vec![("impl".to_string(), 2)],
        "the bump resumes lap 2 at the region's head, one resume point: {body}"
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

// ---- #1017: resume after « budget reached » (ADR-0083), end to end ----

const BUDGET_PIPELINE_NAME: &str = "budget-truth-test";
/// A single-member bounded self-loop capped at $0.01: one lap of `worker` costs
/// far more (its planted transcript), so the re-entry after lap 1 parks the Run
/// « budget reached ». No exit edge: at max_iter the region parks
/// `region_exhausted`, which tells the two bounds apart.
const BUDGET_PIPELINE_YAML: &str = r#"name: budget-truth-test
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
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
    else: true
loops:
  - id: review_loop
    kind: bounded
    members: [worker]
    max_iter: 3
    max_cost_usd: 0.01
"#;

fn seed_budget(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{BUDGET_PIPELINE_NAME}.yaml")),
        BUDGET_PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{BUDGET_PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are a test node.\n")?;
    git_init_with_commit(repo)
}

/// Plant `worker`'s Claude transcript for its latest session (1M input tokens of
/// a priced model ⇒ several dollars), under the daemon's home override.
async fn plant_worker_transcript(daemon: &TestDaemon, run_id: &str) {
    let sid = daemon.pinned_session_id(run_id, "worker").await;
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_daemon::stale_detector::encode_working_dir(&worktree);
    let proj = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join(format!("{sid}.jsonl")),
        format!(
            "{{\"type\":\"assistant\",\"requestId\":\"{sid}\",\"message\":{{\"id\":\"{sid}\",\
             \"model\":\"claude-opus-4-8\",\"usage\":{{\"input_tokens\":1000000,\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
}

/// Finish `worker`'s lap `iter` through the command surface.
async fn finish_worker_lap(url: &str, run_id: &str, iter: i64) {
    let (status, body) = post_command(
        url,
        run_id,
        serde_json::json!({
            "kind": "inject_artifact",
            "path": format!("worker/iter-{iter}/result/output.md"),
            "content": "# done\n",
        }),
    )
    .await;
    assert_eq!(status, 200, "inject_artifact lap {iter}: {body}");
    let (status, body) = post_command(
        url,
        run_id,
        serde_json::json!({ "kind": "mark_node_done", "node_id": "worker", "iter": iter }),
    )
    .await;
    assert_eq!(status, 200, "mark_node_done lap {iter}: {body}");
}

async fn worker_started(url: &str, run_id: &str, iter: i64) -> bool {
    started_pairs(url, run_id)
        .await
        .contains(&("worker".to_string(), iter))
}

async fn awaiting_reason_code(url: &str, run_id: &str) -> Option<String> {
    run_json(url, run_id).await["awaiting_reason_code"]
        .as_str()
        .map(str::to_string)
}

#[tokio::test]
async fn raising_the_cost_cap_resumes_the_next_lap_lowering_it_stops_at_the_next_reentry_and_removing_it_runs_to_max_iter(
) {
    let daemon = TestDaemon::spawn_with_home_override(seed_budget, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let url = daemon.url();
    let run_id = create_run(&daemon, BUDGET_PIPELINE_NAME).await;

    // Lap 1 spends dollars against a $0.01 cap ⇒ the re-entry parks the Run.
    wait_until("worker lap 1 to start", || async { worker_started(&url, &run_id, 1).await }).await;
    plant_worker_transcript(&daemon, &run_id).await;
    finish_worker_lap(&url, &run_id, 1).await;
    wait_until("the Run to park « budget reached »", || async {
        awaiting_reason_code(&url, &run_id).await.as_deref() == Some("region_budget_exhausted")
    })
    .await;
    assert_eq!(run_status(&url, &run_id).await, "awaiting_user");
    assert!(!worker_started(&url, &run_id, 2).await, "lap 2 must not start on budget");

    // FP 1 — raise the cap: the command itself resumes the Run at lap 2.
    let (status, body) = post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": 100 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let spawned = body["spawned"]
        .as_array()
        .unwrap_or_else(|| panic!("raising the cap must report the lap-2 spawn: {body}"));
    assert!(
        spawned
            .iter()
            .any(|s| s["node_id"] == "worker" && s["iter"] == 2),
        "lap 2 spawned: {body}"
    );
    wait_until("lap 2 to start", || async { worker_started(&url, &run_id, 2).await }).await;
    assert_eq!(run_status(&url, &run_id).await, "running");
    let run = run_json(&url, &run_id).await;
    assert_eq!(run["loop_costs"]["review_loop"]["cap_usd"].as_f64(), Some(100.0), "effective cap served: {run}");

    // FP 2 — lower the cap under the spend DURING lap 2: accepted, nothing is
    // killed, the Run keeps running until the next re-entry.
    let (status, body) = post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": 1 }),
    )
    .await;
    assert_eq!(status, 200, "a cap under the spend is accepted (no floor): {body}");
    assert_eq!(run_status(&url, &run_id).await, "running", "the lap in progress is not stopped");
    let run = run_json(&url, &run_id).await;
    assert_eq!(run["nodes"]["worker"]["status"], "running", "lap 2 is not killed: {run}");
    plant_worker_transcript(&daemon, &run_id).await;
    finish_worker_lap(&url, &run_id, 2).await;
    wait_until("the Run to park again at the next re-entry", || async {
        awaiting_reason_code(&url, &run_id).await.as_deref() == Some("region_budget_exhausted")
    })
    .await;
    assert!(!worker_started(&url, &run_id, 3).await, "lap 3 must not start on budget");

    // FP 3 — remove the cap: the loop resumes and runs to max_iter.
    let (status, body) = post_command(
        &url,
        &run_id,
        serde_json::json!({ "kind": "set_region_max_cost", "region_id": "review_loop", "max_cost_usd": null }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    wait_until("lap 3 to start", || async { worker_started(&url, &run_id, 3).await }).await;
    finish_worker_lap(&url, &run_id, 3).await;
    // Uncapped, the loop runs to max_iter: lap 3/3 ends on the max_iter
    // exhaustion (no exit is wired here), never on the budget.
    wait_until("the loop to run out at max_iter", || async {
        awaiting_reason_code(&url, &run_id).await.as_deref() == Some("region_exhausted")
    })
    .await;
    assert!(!worker_started(&url, &run_id, 4).await, "max_iter still bounds the loop");
}
