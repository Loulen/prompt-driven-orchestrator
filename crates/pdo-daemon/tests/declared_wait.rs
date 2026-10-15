//! Layer 3a — the declared wait and the children wait (issue #793, story #588,
//! ADR-0069), over a real daemon.
//!
//! - a freshly spawned `interactive` node is `running`, its run `running`: no
//!   wait at spawn any more;
//! - `pdo wait-user`'s endpoint parks node and run `awaiting_user` with the
//!   message as `awaiting_reason` and **no** `awaiting_reason_code`;
//! - a child's wait climbs to its parent node and run **on read** — list,
//!   detail and children — over two levels, with the reason naming the child,
//!   and nothing written in the parent's log; it clears when the child resumes;
//! - the children long-poll answers `noop` without children, times out while
//!   they work, returns the settled child's verdict, and refuses a caller that
//!   is not the node's own session.

use crate::common::TestDaemon;

pub(crate) const PARENT: &str = "wait-parent";
const CHILD: &str = "wait-child";

/// `worker` is interactive AND an orchestrator: the story's FP needs both.
const PARENT_YAML: &str = r#"name: wait-parent
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
    interactive: true
    orchestrator: true
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
"#;

/// The child: a plain agent node (non-interactive) that can still declare a wait.
const CHILD_YAML: &str = r#"name: wait-child
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: doer
    name: doer
    type: agent
    orchestrator: true
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: work
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: doer, port: task }
  - source: { node: doer, port: work }
    target: { node: end, port: result }
"#;

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

pub(crate) fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(pipelines_dir.join(format!("{PARENT}.yaml")), PARENT_YAML)?;
    std::fs::write(pipelines_dir.join(format!("{CHILD}.yaml")), CHILD_YAML)?;
    let prompts = pipelines_dir.join(format!("{PARENT}.prompts"));
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(prompts.join("worker.md"), "You grill the user.\n")?;
    let child_prompts = pipelines_dir.join(format!("{CHILD}.prompts"));
    std::fs::create_dir_all(&child_prompts)?;
    std::fs::write(child_prompts.join("doer.md"), "You do.\n")?;
    git_init_with_commit(repo)?;
    Ok(())
}

pub(crate) async fn create_run(
    daemon: &TestDaemon,
    pipeline: &str,
    session: Option<(&str, &str)>,
) -> String {
    let body = serde_json::json!({
        "pipeline": pipeline,
        "input": "work",
        "name": format!("{pipeline}-run"),
        "target_repo": daemon.target_repo(),
    });
    let mut req = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&body);
    if let Some((run_id, node_id)) = session {
        req = req
            .header("X-PDO-Session-Run-Id", run_id)
            .header("X-PDO-Session-Node-Id", node_id);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert_eq!(status, 201, "POST /runs should return 201: {text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

pub(crate) async fn get_json(daemon: &TestDaemon, path: &str) -> serde_json::Value {
    let resp = reqwest::get(format!("{}{}", daemon.url(), path))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {path}");
    resp.json().await.unwrap()
}

pub(crate) async fn wait_node_status(daemon: &TestDaemon, run_id: &str, node_id: &str, want: &str) {
    for _ in 0..600 {
        let run = get_json(daemon, &format!("/runs/{run_id}")).await;
        if run["nodes"][node_id]["status"] == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("node {node_id} of run {run_id} never reached `{want}`");
}

pub(crate) async fn wait_user(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    message: Option<&str>,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/wait-user",
            daemon.url()
        ))
        .json(&serde_json::json!({ "iter": 1, "message": message }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

async fn release(daemon: &TestDaemon, run_id: &str, node_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{run_id}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "release_node_completion", "node_id": node_id, "iter": 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

fn list_entry(list: &serde_json::Value, run_id: &str) -> serde_json::Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == run_id)
        .cloned()
        .unwrap_or_else(|| panic!("run {run_id} missing from GET /runs: {list}"))
}

fn write_output(daemon: &TestDaemon, run_id: &str, node: &str, port: &str) {
    let dir = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(node)
        .join("iter-1")
        .join(port);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "done\n").unwrap();
}

async fn complete_node(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/done",
            daemon.url()
        ))
        .json(&serde_json::json!({ "iter": 1 }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    (status, json)
}

#[tokio::test]
async fn a_spawned_interactive_node_is_running_until_its_agent_declares_a_wait() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    // FP #1: the node has the colour of a working node, the run is running, no
    // banner — and no `node_awaiting_user` in the log.
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "running", "{run}");
    assert!(run["nodes"]["worker"]["awaiting"].is_null());
    assert!(run["awaiting_reason"].is_null());
    let events = get_json(&daemon, &format!("/runs/{run_id}/events")).await;
    let kinds: Vec<&str> = events
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert!(
        !kinds.contains(&"node_awaiting_user"),
        "no wait at spawn (ADR-0069): {kinds:?}"
    );
    // The interactive completion guard is untouched: it is the only marker.
    assert_eq!(run["nodes"]["worker"]["iterations"][0]["interactive"], true);

    // FP #2: the agent declares — node and run awaiting, the question on both.
    let (status, body) = wait_user(&daemon, &run_id, "worker", Some("Strip or card?")).await;
    assert_eq!(status, 200, "{body}");
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "awaiting_user");
    assert_eq!(run["awaiting_reason"], "Strip or card?");
    assert!(run["awaiting_reason_code"].is_null(), "{run}");
    assert_eq!(run["nodes"]["worker"]["status"], "awaiting_user");
    assert_eq!(run["nodes"]["worker"]["awaiting"]["cause"], "declared");
    assert_eq!(
        run["nodes"]["worker"]["awaiting"]["message"],
        "Strip or card?"
    );
    let list = get_json(&daemon, "/runs").await;
    let entry = list_entry(&list, &run_id);
    assert_eq!(entry["status"], "awaiting_user");
    assert_eq!(entry["awaiting_reason"], "Strip or card?");
    assert!(entry
        .get("awaiting_reason_code")
        .is_none_or(|v| v.is_null()));

    // FP #4: an unreleased `pdo complete` is refused and re-declares the wait.
    release(&daemon, &run_id, "worker").await; // lifts (ADR-0068)…
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let (status, body) = wait_user(&daemon, &run_id, "worker", None).await;
    assert_eq!(status, 200, "{body}");
    wait_node_status(&daemon, &run_id, "worker", "awaiting_user").await;
}

#[tokio::test]
async fn a_childs_wait_climbs_to_its_parent_on_read_and_clears_when_it_resumes() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &parent, "worker", "running").await;
    let child = create_run(&daemon, CHILD, Some((&parent, "worker"))).await;
    wait_node_status(&daemon, &child, "doer", "running").await;
    // Two levels: the child orchestrates a grandchild.
    let grandchild = create_run(&daemon, CHILD, Some((&child, "doer"))).await;
    wait_node_status(&daemon, &grandchild, "doer", "running").await;

    // The grandchild's agent asks its user something.
    let (status, _) = wait_user(&daemon, &grandchild, "doer", Some("Which repo?")).await;
    assert_eq!(status, 200);

    // Detail of the middle run: derived from its child, reason naming it.
    let mid = get_json(&daemon, &format!("/runs/{child}")).await;
    assert_eq!(mid["status"], "awaiting_user", "{mid}");
    assert_eq!(
        mid["awaiting_reason"],
        format!("child run {CHILD}-run is awaiting you")
    );
    assert!(mid["awaiting_reason_code"].is_null());
    assert_eq!(mid["nodes"]["doer"]["status"], "awaiting_user");
    // The latest iteration agrees with the rollup — the node detail gates its
    // banner on the iteration on screen (FP #793, finding 1).
    assert_eq!(
        mid["nodes"]["doer"]["iterations"][0]["status"], "awaiting_user",
        "{mid}"
    );
    assert_eq!(mid["nodes"]["doer"]["awaiting"]["cause"], "child_awaiting");
    assert_eq!(mid["nodes"]["doer"]["awaiting"]["child_run_id"], grandchild);

    // …and the root, one level higher, derived through the middle run.
    let root = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert_eq!(root["status"], "awaiting_user", "{root}");
    assert_eq!(root["nodes"]["worker"]["status"], "awaiting_user");
    assert_eq!(root["nodes"]["worker"]["awaiting"]["child_run_id"], child);

    // The list agrees with the detail; the children read tints the child row.
    let list = get_json(&daemon, "/runs").await;
    assert_eq!(list_entry(&list, &parent)["status"], "awaiting_user");
    assert_eq!(list_entry(&list, &child)["status"], "awaiting_user");
    let children = get_json(&daemon, &format!("/runs/{parent}/children")).await;
    assert_eq!(
        children["nodes"][0]["children"][0]["status"],
        "awaiting_user"
    );

    // Nothing was written on the parents: their logs carry no awaiting event.
    for run in [&parent, &child] {
        let events = get_json(&daemon, &format!("/runs/{run}/events")).await;
        assert!(
            !events
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "node_awaiting_user"),
            "derived, never written: {run}"
        );
    }

    // The wait lifts on the grandchild (here via a re-spawn-free path: the
    // release is refused on a non-interactive node, so drive the grandchild to
    // terminal instead — the parents come back to running at once).
    write_output(&daemon, &grandchild, "doer", "work");
    let (status, body) = complete_node(&daemon, &grandchild, "doer").await;
    assert_eq!(status, 200, "{body:?}");
    wait_node_status(&daemon, &grandchild, "doer", "completed").await;
    let mid = get_json(&daemon, &format!("/runs/{child}")).await;
    assert_eq!(mid["status"], "running", "{mid}");
    assert!(mid["awaiting_reason"].is_null());
    assert_eq!(mid["nodes"]["doer"]["status"], "running");
    let root = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert_eq!(root["status"], "running");
}

async fn children_wait(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    session: Option<(&str, &str)>,
    query: &[(&str, String)],
) -> (u16, serde_json::Value) {
    let mut req = reqwest::Client::new()
        .get(format!(
            "{}/runs/{run_id}/nodes/{node_id}/children/wait",
            daemon.url()
        ))
        .query(query);
    if let Some((r, n)) = session {
        req = req
            .header("X-PDO-Session-Run-Id", r)
            .header("X-PDO-Session-Node-Id", n);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

#[tokio::test]
async fn the_children_long_poll_times_out_then_returns_the_settled_child() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &parent, "worker", "running").await;
    let session = Some((parent.as_str(), "worker"));

    // Out of the node's own session: refused by name.
    let (status, body) = children_wait(&daemon, &parent, "worker", None, &[]).await;
    assert_eq!(status, 403);
    assert_eq!(body["error"], "not_in_node_session");
    let (status, body) = children_wait(
        &daemon,
        &parent,
        "worker",
        Some(("other-run", "worker")),
        &[],
    )
    .await;
    assert_eq!(status, 403, "{body}");

    // No child yet: an honest no-op (ADR-0025).
    let (status, body) = children_wait(&daemon, &parent, "worker", session, &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["noop"], true, "{body}");

    let child = create_run(&daemon, CHILD, session).await;
    wait_node_status(&daemon, &child, "doer", "running").await;

    // The child works: the poll holds for its budget, then says so — nothing
    // settled, and the daemon's `since` comes back for the next call.
    let started = std::time::Instant::now();
    let (status, body) = children_wait(
        &daemon,
        &parent,
        "worker",
        session,
        &[("timeout_secs", "1".to_string())],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["timeout"], true, "{body}");
    assert!(started.elapsed() >= std::time::Duration::from_millis(900));
    let since = body["since"].as_str().unwrap().to_string();

    // A child that declares a wait does NOT wake the poll (it is not terminal)…
    let (status, _) = wait_user(&daemon, &child, "doer", Some("q?")).await;
    assert_eq!(status, 200);
    let (_, body) = children_wait(
        &daemon,
        &parent,
        "worker",
        session,
        &[("timeout_secs", "1".to_string()), ("since", since.clone())],
    )
    .await;
    assert_eq!(body["timeout"], true, "{body}");
    // …but the parent shows it.
    let root = get_json(&daemon, &format!("/runs/{parent}")).await;
    assert_eq!(root["status"], "awaiting_user");

    // The child settles: the poll returns its verdict with the child's name.
    write_output(&daemon, &child, "doer", "work");
    let (status, body) = complete_node(&daemon, &child, "doer").await;
    assert_eq!(status, 200, "{body:?}");
    wait_node_status(&daemon, &child, "doer", "completed").await;
    let (status, body) = children_wait(
        &daemon,
        &parent,
        "worker",
        session,
        &[("timeout_secs", "5".to_string()), ("since", since)],
    )
    .await;
    assert_eq!(status, 200);
    let settled = body["children"].as_array().unwrap();
    assert_eq!(settled.len(), 1, "{body}");
    assert_eq!(settled[0]["run_id"], child);
    assert_eq!(settled[0]["name"], format!("{CHILD}-run"));
    assert_eq!(settled[0]["status"], "completed");
    assert_eq!(settled[0]["children_active"], 0);
    assert!(settled[0].get("reason").is_none_or(|v| v.is_null()));

    // A fresh call (new `since`) has nothing active any more: no-op again.
    let (_, body) = children_wait(&daemon, &parent, "worker", session, &[]).await;
    assert_eq!(body["noop"], true, "{body}");
}

#[tokio::test]
async fn wait_user_is_refused_when_the_tmux_session_is_gone() {
    // The projection still says `running` (the sweep has not judged the dead
    // pane yet); tmux is the source of truth for the declared wait (FP #793,
    // finding 3): named `409 node_session_not_live`, nothing written.
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let session = pdo_daemon::tmux_session_manager::node_session_name(&run_id, "worker", 1);
    let killed = std::process::Command::new("tmux")
        .args(["-L", &daemon.tmux_socket(), "kill-session", "-t", &session])
        .status()
        .unwrap();
    assert!(killed.success(), "kill-session {session}");

    let (status, body) = wait_user(&daemon, &run_id, "worker", Some("anyone?")).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "node_session_not_live");
    assert_eq!(body["recoverable"], true);
    let events = get_json(&daemon, &format!("/runs/{run_id}/events")).await;
    assert!(
        !events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "node_awaiting_user"),
        "nothing written on refusal"
    );
}

async fn post_wait_user(
    daemon: &TestDaemon,
    run_id: &str,
    node_id: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/wait-user",
            daemon.url()
        ))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap())
}

async fn reply(daemon: &TestDaemon, run_id: &str, node_id: &str, text: &str) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/{node_id}/reply",
            daemon.url()
        ))
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

/// « Reply »: an interactive node's claude session carries the turn-end wait hook
/// even with auto-completion off (the default).
#[tokio::test]
async fn an_interactive_node_is_armed_with_the_turn_end_wait_hook() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let settings = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("worktree/.pdo/prompts/worker-iter-1.settings.json");
    let body = std::fs::read_to_string(&settings).expect("the wait hook settings file");
    assert!(body.contains("pdo wait-user --turn-end"), "{body}");
    assert!(
        !body.contains("pdo complete --auto"),
        "auto-completion is off: {body}"
    );
}

#[tokio::test]
async fn a_turn_end_waits_on_the_user_without_replacing_a_declared_question() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    // The hook's call: the node waits, cause `turn_ended`, no message.
    let (status, body) = post_wait_user(
        &daemon,
        &run_id,
        "worker",
        serde_json::json!({ "iter": 1, "turn_end": true }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["status"], "awaiting_user", "{run}");
    assert_eq!(run["nodes"]["worker"]["awaiting"]["cause"], "turn_ended");
    assert!(run["nodes"]["worker"]["awaiting"]["message"].is_null());

    // A reply lifts it like any other wait.
    reply(&daemon, &run_id, "worker", "A web app for invoices").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    // The agent asks its own question, then its turn ends: the question stays.
    let (status, _) = wait_user(&daemon, &run_id, "worker", Some("Strip or card?")).await;
    assert_eq!(status, 200);
    let (status, body) = post_wait_user(
        &daemon,
        &run_id,
        "worker",
        serde_json::json!({ "iter": 1, "turn_end": true }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["noop"], true, "{body}");
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(run["nodes"]["worker"]["awaiting"]["cause"], "declared");
    assert_eq!(
        run["nodes"]["worker"]["awaiting"]["message"],
        "Strip or card?"
    );
}

#[tokio::test]
async fn a_declared_wait_carries_its_options_normalized() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let options: Vec<String> = ["PDF invoices", "  ", "Images", "PDF invoices"]
        .into_iter()
        .map(str::to_string)
        .chain((1..=10).map(|i| format!("extra {i}")))
        .collect();
    let (status, body) = post_wait_user(
        &daemon,
        &run_id,
        "worker",
        serde_json::json!({ "iter": 1, "message": "What should v1 accept?", "options": options, "multi": true }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    let awaiting = &run["nodes"]["worker"]["awaiting"];
    assert_eq!(awaiting["message"], "What should v1 accept?");
    assert_eq!(awaiting["multi"], true);
    let got: Vec<&str> = awaiting["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_str().unwrap())
        .collect();
    assert_eq!(got.len(), 8, "capped: {got:?}");
    assert_eq!(
        &got[..3],
        ["PDF invoices", "Images", "extra 1"],
        "trimmed, deduped: {got:?}"
    );

    // Without options the wire carries neither field.
    reply(&daemon, &run_id, "worker", "PDF invoices, Images").await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let (status, _) = wait_user(&daemon, &run_id, "worker", Some("Anything else?")).await;
    assert_eq!(status, 200);
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert!(
        run["nodes"]["worker"]["awaiting"].get("options").is_none(),
        "{run}"
    );
    assert!(
        run["nodes"]["worker"]["awaiting"].get("multi").is_none(),
        "{run}"
    );
}

/// ADR-0081: the agent's own question tool is answered from the panel. The real
/// `pdo ask-user` hook declares the wait, holds the question, and hands the
/// panel's answers to the tool as `updatedInput.answers`.
#[tokio::test]
async fn ask_user_hands_the_panels_answers_to_the_question_tool() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let questions = serde_json::json!([{
        "question": "What kind of product is it?",
        "header": "Product",
        "multiSelect": false,
        "options": [{"label": "Web app", "description": "In a browser"}]
    }]);
    let hook_input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_use_id": "toolu_q1",
        "tool_input": {"questions": questions},
    });
    let url = daemon.url();
    let rid = run_id.clone();
    let hook = std::thread::spawn(move || {
        use std::io::Write;
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_pdo"))
            .arg("ask-user")
            .env("PDO_DAEMON_URL", url)
            .env("PDO_RUN_ID", rid)
            .env("PDO_NODE_ID", "worker")
            .env("PDO_NODE_ITER", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(hook_input.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    });

    // The hook declared the wait with the first question on the banner.
    wait_node_status(&daemon, &run_id, "worker", "awaiting_user").await;
    let run = get_json(&daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(
        run["nodes"]["worker"]["awaiting"]["message"],
        "What kind of product is it?"
    );

    // An empty answer is refused; a real one is accepted and lifts the wait.
    let answer = |answers: serde_json::Value| {
        reqwest::Client::new()
            .post(format!(
                "{}/runs/{run_id}/nodes/worker/questions/toolu_q1/answer",
                daemon.url()
            ))
            .json(&serde_json::json!({ "answers": answers }))
            .send()
    };
    let resp = answer(serde_json::json!({ "What kind of product is it?": " " }))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    // The hook checks in on its first poll; until then nobody holds the question.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let resp = answer(serde_json::json!({ "What kind of product is it?": "Web app" }))
            .await
            .unwrap();
        if resp.status() == 200 {
            break;
        }
        assert_eq!(
            resp.status(),
            409,
            "only `question_not_held` may delay the answer"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "the hook never held the question"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    wait_node_status(&daemon, &run_id, "worker", "running").await;

    let out = tokio::task::spawn_blocking(move || hook.join().unwrap())
        .await
        .unwrap();
    assert!(out.status.success());
    let decision: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let hso = &decision["hookSpecificOutput"];
    assert_eq!(hso["hookEventName"], "PreToolUse");
    assert_eq!(hso["permissionDecision"], "allow");
    assert_eq!(hso["updatedInput"]["questions"], questions);
    assert_eq!(
        hso["updatedInput"]["answers"]["What kind of product is it?"],
        "Web app"
    );
}

/// Outside a node session (no identity), the hook decides nothing: Claude's
/// own menu shows, as it always did.
#[test]
fn ask_user_without_a_node_identity_decides_nothing() {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_pdo"))
        .arg("ask-user")
        .env_remove("PDO_RUN_ID")
        .env_remove("PDO_NODE_ID")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_use_id":"x","tool_input":{"questions":[]}}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

/// A question no `pdo ask-user` hook holds (a session launched before the hook)
/// is refused by name: the panel never says « sent » for an answer nobody reads.
#[tokio::test]
async fn an_answer_nobody_holds_is_refused() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let run_id = create_run(&daemon, PARENT, None).await;
    wait_node_status(&daemon, &run_id, "worker", "running").await;
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{run_id}/nodes/worker/questions/toolu_old/answer",
            daemon.url()
        ))
        .json(&serde_json::json!({ "answers": { "Strip or card?": "Card" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "question_not_held");
}
