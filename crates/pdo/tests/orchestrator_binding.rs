//! Layer 3a — la liaison forte nœud parent ↔ enfants (issue #724, ADR-0064).
//! Sibling of `run_provenance.rs` (qui pose la provenance mécanique) : ici, la
//! provenance EXISTE et elle est contractuelle pour TOUT nœud parent — agent ou
//! `script`, toggle « Orchestrator » ou non.
//!
//! - `pdo complete` sur un nœud parent est refusé tant que ses runs
//!   enfants (mêmes `parent_run_id` + `parent_node_id`) sont non terminaux,
//!   avec un message qui COMPTE (`children_pending`, `active`/`failed`) et qui
//!   nomme `pdo run wait --all` ;
//! - tous les enfants terminaux → le nœud se complète de lui-même (le watcher
//!   de la liaison passe par la même voie qu'un `pdo complete`) ;
//! - un enfant `failed` parque le nœud `awaiting_user` — l'arbitrage est à
//!   l'utilisateur : le forçage (`mark_node_done`, volontairement pas gated)
//!   et le retry de l'enfant fonctionnent ;
//! - aucun effet des gestes parent (stop du nœud, forget du run) sur les
//!   enfants ; un restart ré-adopte les enfants vivants et reprend l'attente ;
//! - un nœud SANS toggle qui a des enfants est retenu de la même façon, y
//!   compris par l'auto-complétion de fin de tour et en fin de `script` (le
//!   refus ne fait pas échouer le script) ; un nœud sans enfant, ou dont
//!   l'enfant est oublié, se complète comme avant ;
//! - un nœud parqué par la liaison n'est pas un orphelin au redémarrage du
//!   daemon, même sans session : il attend ses enfants.

use crate::common::TestDaemon;

const PIPELINE_NAME: &str = "binding-test";
const CHILD_PIPELINE_NAME: &str = "binding-child";

/// `worker` porte le toggle « Orchestrator » : sa liaison est forte (#724).
const PIPELINE_YAML: &str = r#"name: binding-test
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

/// La même pipeline SANS le toggle : le contrôle négatif (aucune retenue).
const PLAIN_PIPELINE_YAML: &str = r#"name: binding-plain
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

const CHILD_PIPELINE_YAML: &str = r#"name: binding-child
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

fn seed_pipelines(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines_dir = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines_dir)?;
    std::fs::write(
        pipelines_dir.join(format!("{PIPELINE_NAME}.yaml")),
        PIPELINE_YAML,
    )?;
    std::fs::write(
        pipelines_dir.join("binding-plain.yaml"),
        PLAIN_PIPELINE_YAML,
    )?;
    let prompts_dir = pipelines_dir.join(format!("{PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&prompts_dir)?;
    std::fs::write(prompts_dir.join("worker.md"), "You are the orchestrator.\n")?;
    let child_prompts = pipelines_dir.join(format!("{CHILD_PIPELINE_NAME}.prompts"));
    std::fs::create_dir_all(&child_prompts)?;
    std::fs::write(child_prompts.join("doer.md"), "You are a doer.\n")?;
    std::fs::write(
        pipelines_dir.join(format!("{CHILD_PIPELINE_NAME}.yaml")),
        CHILD_PIPELINE_YAML,
    )?;
    git_init_with_commit(repo)?;
    Ok(())
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_pipelines(repo)
}

fn seed_plain(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_pipelines(repo)
}

async fn create_run(
    daemon: &TestDaemon,
    pipeline: &str,
    session: Option<(&str, &str)>,
    extra: serde_json::Value,
) -> String {
    let mut body = serde_json::json!({
        "pipeline": pipeline,
        "input": "work",
        "target_repo": daemon.target_repo(),
    });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k.clone()] = v.clone();
        }
    }
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
    let body_text = resp.text().await.unwrap_or_default();
    assert_eq!(status, 201, "POST /runs should return 201: {body_text}");
    let json: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    json["run_id"].as_str().unwrap().to_string()
}

async fn get_json(daemon: &TestDaemon, path: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::get(format!("{}{}", daemon.url(), path))
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().await.unwrap();
    (status, json)
}

async fn node_status(daemon: &TestDaemon, run_id: &str, node_id: &str) -> String {
    let (status, run) = get_json(daemon, &format!("/runs/{run_id}")).await;
    assert_eq!(status, 200);
    run["nodes"][node_id]["status"]
        .as_str()
        .unwrap_or("<absent>")
        .to_string()
}

/// 30 s budget: under the full `it` target (hundreds of tests spawning tmux
/// sessions concurrently) a retried child can take well over 10 s to settle,
/// which made `retrying_the_failed_child_lets_the_node_complete_itself` flaky.
async fn wait_node_status(daemon: &TestDaemon, run_id: &str, node_id: &str, want: &str) {
    for _ in 0..600 {
        if node_status(daemon, run_id, node_id).await == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("node {node_id} of run {run_id} never reached `{want}`");
}

/// Start the parent's `worker` and wait until its tmux session holds.
async fn start_worker(daemon: &TestDaemon, run_id: &str) {
    wait_node_status(daemon, run_id, "worker", "running").await;
}

/// Write a child's (or the worker's) declared output into the run worktree, so
/// the output validator lets the completion through.
fn write_output(daemon: &TestDaemon, run_id: &str, node: &str, port: &str) {
    let dir = daemon
        .repo_root()
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
        .join(".pdo")
        .join("artifacts")
        .join(node)
        .join("iter-1")
        .join(port);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "done\n").unwrap();
}

/// `pdo complete`'s endpoint, returning (status, body).
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
    // A granted completion answers plain `ok`; a refusal answers JSON.
    let text = resp.text().await.unwrap_or_default();
    let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    (status, json)
}

/// Drive a child to terminal: complete its `doer` (the tmux override keeps the
/// session alive, but `pdo complete`'s endpoint reaps it itself).
async fn settle_child(daemon: &TestDaemon, child_id: &str) {
    write_output(daemon, child_id, "doer", "work");
    let (status, body) = complete_node(daemon, child_id, "doer").await;
    assert_eq!(status, 200, "child completion should succeed: {body:?}");
    wait_node_status(daemon, child_id, "doer", "completed").await;
}

#[tokio::test]
async fn complete_is_refused_with_a_counter_while_children_run() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PIPELINE_NAME, None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    // Two children spawned from the worker's session (mechanical provenance).
    let child_a = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;
    let child_b = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;

    // `pdo complete` is refused with the counter in the message AND the body.
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409, "the binding refuses while children run");
    assert_eq!(body["error"], "children_pending", "{body:?}");
    assert_eq!(body["active"], 2, "both children counted: {body:?}");
    assert_eq!(body["failed"], 0);
    assert_eq!(
        body["recoverable"], true,
        "no child failed: still the agent's turn (exit 3)"
    );
    let message = body["message"].as_str().unwrap();
    assert!(message.contains('2'), "the message counts: {message}");

    // The node is parked `awaiting_user` (the binding marker), still alive.
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;

    // Children were NOT touched by the refusal.
    for child in [&child_a, &child_b] {
        let (status, run) = get_json(&daemon, &format!("/runs/{child}")).await;
        assert_eq!(status, 200);
        assert_eq!(run["status"], "running", "a child keeps living");
    }
}

#[tokio::test]
async fn the_node_completes_itself_once_every_child_is_terminal() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PIPELINE_NAME, None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child_a = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;
    let child_b = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;

    // The agent hands back control: refused, binding-parked, watcher armed.
    let (status, _) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409);

    // The children settle — NO further agent gesture happens.
    write_output(&daemon, &parent, "worker", "result");
    settle_child(&daemon, &child_a).await;
    settle_child(&daemon, &child_b).await;

    // « Le nœud se complète » : the watcher drives the SAME completion path.
    wait_node_status(&daemon, &parent, "worker", "completed").await;

    // The log says WHO decided: the binding, not the agent.
    let (status, events) = get_json(&daemon, &format!("/runs/{parent}/events")).await;
    assert_eq!(status, 200);
    let raw = serde_json::to_string(&events).unwrap();
    assert!(
        raw.contains("children_settled"),
        "the terminal event is signed by the binding: {raw}"
    );
}

#[tokio::test]
async fn a_failed_child_parks_the_node_and_the_user_decides() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PIPELINE_NAME, None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        // auto_fail on the CHILD: an agent `pdo fail` terminalises it to
        // `failed` instead of parking it awaiting confirmation.
        serde_json::json!({ "auto_fail": true }),
    )
    .await;

    // First refusal: the child is running (active=1, failed=0).
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409);
    assert_eq!(body["active"], 1);
    assert_eq!(body["failed"], 0);

    // The child fails.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{child}/nodes/doer/fail", daemon.url()))
        .json(&serde_json::json!({ "reason": "broken", "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the child fails");
    for _ in 0..100 {
        let (status, run) = get_json(&daemon, &format!("/runs/{child}")).await;
        assert_eq!(status, 200);
        if run["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // Second refusal: the counter now names the failure and the decision is
    // NOT the agent's any more (exit 4 — the user tranches).
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409, "a failed child keeps the node held");
    assert_eq!(body["error"], "children_pending");
    assert_eq!(body["failed"], 1, "{body:?}");
    assert_eq!(
        body["recoverable"], false,
        "a failed child hands the decision to the user"
    );
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("1 failed"), "{message}");
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;

    // Le forçage utilisateur (`mark_node_done`) is deliberately NOT gated:
    // it is the user's tranching gesture. (The worker's declared output is
    // written so the completion validator lets the force through.)
    write_output(&daemon, &parent, "worker", "result");
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{parent}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "mark_node_done",
            "node_id": "worker",
            "iter": 1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "forcing works despite the failed child");
    wait_node_status(&daemon, &parent, "worker", "completed").await;
}

#[tokio::test]
async fn retrying_the_failed_child_lets_the_node_complete_itself() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PIPELINE_NAME, None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({ "auto_fail": true }),
    )
    .await;

    let (status, _) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409);

    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{child}/nodes/doer/fail", daemon.url()))
        .json(&serde_json::json!({ "reason": "broken", "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // `node_fail` answers before its detached tail reaps the session and
    // appends `RunFailed`. The retry below is about a FAILED child (ADR-0049
    // re-open), so wait for that state instead of racing the tail — under the
    // full parallel suite the reap can otherwise collide with the delivery.
    for _ in 0..600 {
        let (status, run) = get_json(&daemon, &format!("/runs/{child}")).await;
        assert_eq!(status, 200);
        if run["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // The orchestrator's own output is on disk BEFORE its child can settle, as
    // in `the_node_completes_itself_once_every_child_is_terminal`: the binding's
    // watcher drives the completion the moment the retried child settles, and
    // under the full parallel suite that can land before the test's next line
    // runs — an output written after it was refused once, never retried.
    write_output(&daemon, &parent, "worker", "result");

    // The user tranches by RETRYING the failed child: a targeted
    // `mark_node_done` re-opens the failed run and completes it (ADR-0049).
    write_output(&daemon, &child, "doer", "work");
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{child}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "mark_node_done",
            "node_id": "doer",
            "iter": 1,
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert_eq!(status, 200, "the retried child completes: {body}");
    wait_node_status(&daemon, &child, "doer", "completed").await;

    // The retried child settled → the orchestrator completes itself.
    wait_node_status(&daemon, &parent, "worker", "completed").await;
}

/// The binding follows the REAL children, not the toggle: a node WITHOUT the
/// Orchestrator toggle that created a child run is held exactly like an
/// orchestrator, with a generic refusal that names `pdo run wait --all`, and
/// completes itself (`children_settled`) once its child is terminal.
#[tokio::test]
async fn a_node_without_the_toggle_is_held_by_its_children() {
    let daemon = TestDaemon::spawn(seed_plain).await.unwrap();
    let parent = create_run(&daemon, "binding-plain", None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;

    write_output(&daemon, &parent, "worker", "result");
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409, "a parent node is held by its child: {body:?}");
    assert_eq!(body["error"], "children_pending", "{body:?}");
    assert_eq!(body["active"], 1, "{body:?}");
    assert_eq!(body["recoverable"], true, "still the agent's turn (exit 3)");
    let message = body["message"].as_str().unwrap();
    assert_eq!(
        message,
        "node `worker` is held by its child runs: 1 active — wait for them with \
         `pdo run wait --all`, then complete again"
    );
    assert!(!message.contains("orchestrator"), "{message}");
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;

    // The child settles → the parent completes itself, signed by the binding.
    settle_child(&daemon, &child).await;
    wait_node_status(&daemon, &parent, "worker", "completed").await;
    let (status, events) = get_json(&daemon, &format!("/runs/{parent}/events")).await;
    assert_eq!(status, 200);
    let raw = serde_json::to_string(&events).unwrap();
    assert!(raw.contains("children_settled"), "{raw}");
}

/// A non-toggle parent node whose child FAILED is parked awaiting the user
/// (the failed variant of the refusal, without « orchestrator »), and the
/// force (`mark_node_done`) stays ungated.
#[tokio::test]
async fn a_failed_child_parks_a_node_without_the_toggle() {
    let daemon = TestDaemon::spawn(seed_plain).await.unwrap();
    let parent = create_run(&daemon, "binding-plain", None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({ "auto_fail": true }),
    )
    .await;
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{child}/nodes/doer/fail", daemon.url()))
        .json(&serde_json::json!({ "reason": "broken", "iter": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the child fails");
    wait_run_status(&daemon, &child, "failed").await;

    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409, "{body:?}");
    assert_eq!(body["error"], "children_pending");
    assert_eq!(body["failed"], 1, "{body:?}");
    assert_eq!(body["recoverable"], false, "the user decides (exit 4)");
    let message = body["message"].as_str().unwrap();
    assert!(message.starts_with("node `worker` is held"), "{message}");
    assert!(message.contains("force-complete the node"), "{message}");
    assert!(!message.contains("orchestrator"), "{message}");
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;

    write_output(&daemon, &parent, "worker", "result");
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{parent}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "mark_node_done",
            "node_id": "worker",
            "iter": 1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "forcing stays ungated");
    wait_node_status(&daemon, &parent, "worker", "completed").await;
}

/// The turn-end auto-completion (`pdo complete --auto`, the Stop hook source)
/// of a parent node goes through the same gate: held, not completed.
#[tokio::test]
async fn the_turn_end_auto_completion_of_a_parent_node_is_held() {
    let daemon = TestDaemon::spawn(seed_plain).await.unwrap();
    let parent = create_run(&daemon, "binding-plain", None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;
    let _child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;

    write_output(&daemon, &parent, "worker", "result");
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{parent}/nodes/worker/done", daemon.url()))
        .json(&serde_json::json!({ "iter": 1, "auto": true }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 409, "the Stop hook is held too: {body:?}");
    assert_eq!(body["error"], "children_pending");
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;
}

/// Non-regression: a node without children completes as before — with or
/// without the toggle.
#[tokio::test]
async fn a_node_without_children_completes_as_before() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    for pipeline in ["binding-plain", PIPELINE_NAME] {
        let run = create_run(&daemon, pipeline, None, serde_json::json!({})).await;
        start_worker(&daemon, &run).await;
        write_output(&daemon, &run, "worker", "result");
        let (status, body) = complete_node(&daemon, &run, "worker").await;
        assert_eq!(status, 200, "{pipeline}: no child, no retention: {body:?}");
        wait_node_status(&daemon, &run, "worker", "completed").await;
    }
}

/// A forgotten (tombstoned) child never holds its parent node (ADR-0024).
#[tokio::test]
async fn a_forgotten_child_does_not_hold_the_parent_node() {
    let daemon = TestDaemon::spawn(seed_plain).await.unwrap();
    let parent = create_run(&daemon, "binding-plain", None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;
    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;
    // Archive, then forget the still-running child.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{child}/commands", daemon.url()))
        .json(&serde_json::json!({ "kind": "cleanup_run" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "archive the child");
    let resp = reqwest::Client::new()
        .delete(format!("{}/runs/{child}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "forget the child");

    write_output(&daemon, &parent, "worker", "result");
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 200, "a forgotten child holds nothing: {body:?}");
    wait_node_status(&daemon, &parent, "worker", "completed").await;
}

/// A node parked by the binding waits on its children, not on its session: a
/// daemon restart that finds its session gone leaves it awaiting (no
/// `interrupted`), and it still completes itself once its child settles.
#[tokio::test]
async fn a_held_parent_node_survives_the_boot_recovery_without_its_session() {
    let daemon = TestDaemon::spawn(seed_plain).await.unwrap();
    let parent = create_run(&daemon, "binding-plain", None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;
    let child = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;
    write_output(&daemon, &parent, "worker", "result");
    let (status, _) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(status, 409);
    wait_node_status(&daemon, &parent, "worker", "awaiting_user").await;

    let session = pdo_session::domain::names::node_session_name(&parent, "worker", 1);
    crate::common::tmux(&daemon.tmux_socket()).kill(&session);
    daemon.run_boot_recovery_tick().await;
    assert_eq!(
        node_status(&daemon, &parent, "worker").await,
        "awaiting_user",
        "a held parent node is not an orphan"
    );

    settle_child(&daemon, &child).await;
    wait_node_status(&daemon, &parent, "worker", "completed").await;
}

/// A `script` parent node: its body creates a child run (`pdo run create`), so
/// it is held; its output feeds `end`, so the run only completes once the held
/// node does.
const SCRIPT_PIPELINE_YAML: &str = r#"name: binding-script
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: spawner
    name: spawner
    type: script
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: spawner, port: in }
  - source: { node: spawner, port: out }
    target: { node: end, port: result }
"#;

/// A `script` node that created a child run is held like any parent node: the
/// `children_pending` refusal of its end-of-script `pdo complete` (exit 3)
/// does NOT fail it; PDO completes it once the child is terminal.
#[tokio::test]
async fn a_script_node_with_a_child_is_held_then_completed_without_failing() {
    crate::common::ensure_pdo_on_path();
    let daemon = TestDaemon::spawn(|repo: &std::path::Path| {
        let pipelines_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir)?;
        std::fs::write(
            pipelines_dir.join("binding-script.yaml"),
            SCRIPT_PIPELINE_YAML,
        )?;
        let prompts = pipelines_dir.join("binding-script.prompts");
        std::fs::create_dir_all(&prompts)?;
        std::fs::write(
            prompts.join("spawner.md"),
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n\
                 pdo run create {CHILD_PIPELINE_NAME} --input work --target-repo '{}' \
                 > child.json\n\
                 printf 'spawned\\n' > \"$PDO_OUTPUT_OUT\"\n",
                repo.display()
            ),
        )?;
        seed_pipelines(repo)
    })
    .await
    .unwrap();
    let parent = create_run(&daemon, "binding-script", None, serde_json::json!({})).await;

    // The script ran, created its child, and its `pdo complete` was refused:
    // the node is parked by the binding — NOT failed.
    wait_node_status(&daemon, &parent, "spawner", "awaiting_user").await;
    let child = find_child_run_id(&daemon, &parent).await;
    // Give the tail time to have acted on the refusal: still held, not failed.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        node_status(&daemon, &parent, "spawner").await,
        "awaiting_user",
        "the refusal at the end of the script must not fail the node"
    );

    settle_child(&daemon, &child).await;
    wait_node_status(&daemon, &parent, "spawner", "completed").await;
    wait_run_status(&daemon, &parent, "completed").await;
}

/// The run id of the (single) child run of `parent`, from its children read.
async fn find_child_run_id(daemon: &TestDaemon, parent: &str) -> String {
    for _ in 0..600 {
        let (status, body) = get_json(daemon, &format!("/runs/{parent}/children")).await;
        assert_eq!(status, 200);
        if let Some(child) = body["nodes"][0]["children"][0]["run_id"].as_str() {
            return child.to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("no child run of {parent} appeared");
}

/// Wait until the Run reaches `want` (30 s, as [`wait_node_status`]).
async fn wait_run_status(daemon: &TestDaemon, run_id: &str, want: &str) {
    let mut last = serde_json::Value::Null;
    for _ in 0..600 {
        let (status, run) = get_json(daemon, &format!("/runs/{run_id}")).await;
        assert_eq!(status, 200);
        if run["status"] == want {
            return;
        }
        last = run["status"].clone();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("run {run_id} never reached `{want}` (last: {last})");
}

#[tokio::test]
async fn stop_and_forget_of_the_parent_leave_the_children_intact() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let parent = create_run(&daemon, PIPELINE_NAME, None, serde_json::json!({})).await;
    start_worker(&daemon, &parent).await;

    let child_a = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;
    let child_b = create_run(
        &daemon,
        CHILD_PIPELINE_NAME,
        Some((&parent, "worker")),
        serde_json::json!({}),
    )
    .await;

    // Stop the orchestrator node: no death propagates DOWN.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{parent}/nodes/worker/stop", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the node stops");
    for child in [&child_a, &child_b] {
        let (status, run) = get_json(&daemon, &format!("/runs/{child}")).await;
        assert_eq!(status, 200);
        assert_eq!(run["status"], "running", "stop of the node keeps children");
    }

    // Restart the node: the living children are re-adopted, the wait resumes.
    let resp = reqwest::Client::new()
        .post(format!("{}/runs/{parent}/commands", daemon.url()))
        .json(&serde_json::json!({
            "kind": "restart_node",
            "node_id": "worker",
            "iter": 1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "the node restarts");
    start_worker(&daemon, &parent).await;
    let (status, body) = complete_node(&daemon, &parent, "worker").await;
    assert_eq!(
        status, 409,
        "re-adoption: the binding holds again (children still live)"
    );
    assert_eq!(body["active"], 2, "{body:?}");
}
