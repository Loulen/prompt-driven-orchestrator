//! API 0.11 against the real daemon, with the test extension: the name of a
//! Projet an extension reads, the signal that the Triggers changed, and the
//! Triggers read in the Projets of their target repository.
//!
//! - `Host::project_name` answers « P1 » for P1's id, follows a renaming,
//!   answers nothing for an unknown id or a deleted Projet;
//! - `Host::triggers_changed` announces a `trigger_updated` without
//!   `trigger_id` on `/ws`, and so does a suspension the extension sets;
//! - a refused read of the Triggers in P2 hides them from the list, refuses
//!   their detail and their fires, and keeps what `/ws` says of them from
//!   that connection; another caller sees them; without extension, all pass.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::common::flow::{git_init, write_pipeline};
use crate::common::{ws_text, TestDaemon};
use crate::test_extension::{Refusal, TestExtension};

/// The one-node `solo` pipeline, in a git repository.
fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let yaml = r#"name: solo
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
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
    write_pipeline(repo, "solo", yaml)?;
    git_init(repo)
}

async fn send(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    actor: Option<&str>,
) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{}{path}", daemon.url()));
    if let Some(body) = body {
        request = request.json(&body);
    }
    if let Some(actor) = actor {
        request = request.header("x-pdo-test-actor", actor);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(daemon: &TestDaemon, path: &str) -> (u16, Value) {
    send(daemon, reqwest::Method::GET, path, None, None).await
}

async fn get_as(daemon: &TestDaemon, path: &str, actor: &str) -> (u16, Value) {
    send(daemon, reqwest::Method::GET, path, None, Some(actor)).await
}

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    send(daemon, reqwest::Method::POST, path, Some(body), None).await
}

/// The Projet named `name` owning `repo`: its id.
async fn projet(daemon: &TestDaemon, name: &str, repo: &str) -> String {
    let (status, project) = post(
        daemon,
        "/projects",
        json!({ "name": name, "members": [repo] }),
    )
    .await;
    assert_eq!(status, 201, "{project}");
    project["id"].as_str().unwrap().to_string()
}

/// A Trigger of `solo` working in `repo`, launching in `project`: its id.
async fn trigger_in(daemon: &TestDaemon, name: &str, repo: &str, project: Option<&str>) -> String {
    let (status, trigger) = post(
        daemon,
        "/triggers",
        json!({
            "name": name,
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": repo,
            "project_id": project,
        }),
    )
    .await;
    assert_eq!(status, 201, "{trigger}");
    trigger["id"].as_str().unwrap().to_string()
}

/// The name the test extension reads for `project_id` through its Host.
async fn name_read_by_the_extension(daemon: &TestDaemon, project_id: &str) -> Value {
    let (status, body) = get(daemon, &format!("/ext/test/projects/{project_id}/name")).await;
    assert_eq!(status, 200, "{body}");
    body["name"].clone()
}

/// The messages of `/ws` of type `kind` within `window`.
async fn messages_of<S>(ws: &mut S, kind: &str, window: Duration) -> Vec<Value>
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, ws.next()).await {
        let Some(text) = ws_text(&message) else {
            continue;
        };
        let message: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        if message["type"] == kind {
            seen.push(message);
        }
    }
    seen
}

fn ids(list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn an_extension_reads_the_name_of_a_projet_and_follows_its_renaming() {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let p1 = projet(&daemon, "P1", &daemon.target_repo()).await;
    assert_eq!(name_read_by_the_extension(&daemon, &p1).await, "P1");
    assert_eq!(
        name_read_by_the_extension(&daemon, "prj-unknown").await,
        Value::Null,
        "an unknown id has no name"
    );

    let (status, body) = send(
        &daemon,
        reqwest::Method::PATCH,
        &format!("/projects/{p1}"),
        Some(json!({ "name": "Front" })),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        name_read_by_the_extension(&daemon, &p1).await,
        "Front",
        "the answer of the rename comes once the name is current"
    );

    let (status, _) = send(
        &daemon,
        reqwest::Method::DELETE,
        &format!("/projects/{p1}"),
        None,
        None,
    )
    .await;
    assert!(status < 300, "{status}");
    assert_eq!(name_read_by_the_extension(&daemon, &p1).await, Value::Null);
}

#[tokio::test]
async fn the_names_are_loaded_before_the_daemon_serves() {
    let root = tempfile::tempdir().unwrap();
    seed(root.path()).unwrap();
    let repo = root.path().to_string_lossy().into_owned();
    let p1 = {
        let daemon = TestDaemon::spawn_at(root.path()).await.unwrap();
        projet(&daemon, "P1", &repo).await
    };
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_at_with_extensions(root.path(), ext.boxed())
        .await
        .unwrap();
    assert_eq!(name_read_by_the_extension(&daemon, &p1).await, "P1");
}

#[tokio::test]
async fn the_signal_of_the_triggers_reaches_ws_as_a_trigger_updated_without_id() {
    let (ext, _control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let trigger = trigger_in(&daemon, "nightly", &daemon.target_repo(), None).await;
    let mut ws = daemon.connect_ws().await.unwrap();

    let (status, body) = post(&daemon, "/ext/test/triggers-changed", json!({})).await;
    assert_eq!(status, 200, "{body}");
    let window = Duration::from_millis(1000);
    assert_eq!(
        messages_of(&mut ws, "trigger_updated", window).await,
        vec![json!({ "type": "trigger_updated" })]
    );

    // A suspension the extension sets is signalled: the list read again
    // shows it.
    let (status, _) = send(
        &daemon,
        reqwest::Method::PUT,
        "/ext/test/suspensions",
        Some(json!({ "trigger_id": trigger, "reason": "suspended by the test extension" })),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        messages_of(&mut ws, "trigger_updated", window).await,
        vec![json!({ "type": "trigger_updated" })]
    );
    let (_, list) = get(&daemon, "/triggers").await;
    assert_eq!(
        list[0]["suspension"]["reason"], "suspended by the test extension",
        "{list}"
    );
}

/// P1 owns the daemon's repository, P2 another; a Trigger in each.
struct World {
    daemon: TestDaemon,
    control: crate::test_extension::Control,
    p2: String,
    t1: String,
    t2: String,
    _other: tempfile::TempDir,
}

async fn world() -> World {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    git_init(other.path()).unwrap();
    let other_repo = other.path().to_string_lossy().into_owned();
    let p1 = projet(&daemon, "P1", &daemon.target_repo()).await;
    let p2 = projet(&daemon, "P2", &other_repo).await;
    let t1 = trigger_in(&daemon, "in P1", &daemon.target_repo(), Some(&p1)).await;
    let t2 = trigger_in(&daemon, "in P2", &other_repo, Some(&p2)).await;
    World {
        daemon,
        control,
        p2,
        t1,
        t2,
        _other: other,
    }
}

#[tokio::test]
async fn a_reader_refused_in_a_projet_sees_none_of_its_triggers_nor_their_fires() {
    let w = world().await;
    let (status, body) = post(&w.daemon, &format!("/triggers/{}/fire", w.t2), json!({})).await;
    assert_eq!(status, 200, "{body}");
    w.control
        .set(Refusal::parse(&format!("trigger:read@{}~bob", w.p2)).unwrap());

    let (status, as_bob) = get_as(&w.daemon, "/triggers", "bob").await;
    assert_eq!(status, 200);
    assert_eq!(ids(&as_bob), std::slice::from_ref(&w.t1));
    let (_, as_alice) = get_as(&w.daemon, "/triggers", "alice").await;
    let mut both = vec![w.t1.clone(), w.t2.clone()];
    both.sort();
    assert_eq!(ids(&as_alice), both);

    for path in [
        format!("/triggers/{}", w.t2),
        format!("/triggers/{}/fires", w.t2),
    ] {
        let (status, body) = get_as(&w.daemon, &path, "bob").await;
        assert_eq!(status, 403, "{path}: {body}");
        assert_eq!(body["error"], "operation_refused");
        assert_eq!(body["reason"], "the test extension refuses read trigger");
        let (status, body) = get_as(&w.daemon, &path, "alice").await;
        assert_eq!(status, 200, "{path}: {body}");
    }
    let (_, fires) = get_as(&w.daemon, &format!("/triggers/{}/fires", w.t2), "alice").await;
    assert_eq!(fires.as_array().unwrap().len(), 1, "{fires}");
}

#[tokio::test]
async fn ws_says_nothing_of_a_trigger_to_a_connection_that_may_not_read_it() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("trigger:read@{}~bob", w.p2)).unwrap());
    let mut bob = w.daemon.connect_ws_as("bob").await.unwrap();
    let mut alice = w.daemon.connect_ws_as("alice").await.unwrap();

    for trigger in [&w.t2, &w.t1] {
        let (status, body) = send(
            &w.daemon,
            reqwest::Method::PATCH,
            &format!("/triggers/{trigger}"),
            Some(json!({ "name": "renamed" })),
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }
    let (status, _) = post(&w.daemon, &format!("/triggers/{}/fire", w.t2), json!({})).await;
    assert_eq!(status, 200);

    let window = Duration::from_millis(1500);
    // What a message says of a Trigger: its announcements and the Operations
    // on it (the events of the Run it fired are the Run's, read as a Run).
    let about = |seen: &[Value], id: &str| -> Vec<String> {
        seen.iter()
            .filter(|m| m["trigger_id"] == id || m["operation"]["target"] == id)
            .map(|m| m["type"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    let collect = |ws| async move {
        let mut ws = ws;
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + window;
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout_at(deadline, StreamExt::next(&mut ws)).await
        {
            if let Some(text) = ws_text(&message) {
                seen.push(serde_json::from_str::<Value>(text).unwrap_or(Value::Null));
            }
        }
        seen
    };
    let (bob_seen, alice_seen) = tokio::join!(collect(&mut bob), collect(&mut alice));

    assert!(
        about(&bob_seen, &w.t2).is_empty(),
        "bob may not read t2: {bob_seen:?}"
    );
    assert!(about(&bob_seen, &w.t1).contains(&"trigger_updated".to_string()));
    let alice_t2 = about(&alice_seen, &w.t2);
    for kind in ["trigger_updated", "trigger_fired", "operation_applied"] {
        assert!(
            alice_t2.contains(&kind.to_string()),
            "{kind}: {alice_seen:?}"
        );
    }
    assert!(
        alice_seen.iter().all(|m| m.get("read_guard").is_none()),
        "the guard never leaves the daemon: {alice_seen:?}"
    );
}

#[tokio::test]
async fn without_extension_every_trigger_and_message_passes_unguarded() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let trigger = trigger_in(&daemon, "nightly", &daemon.target_repo(), None).await;
    let mut ws = daemon.connect_ws().await.unwrap();
    let (status, _) = send(
        &daemon,
        reqwest::Method::PATCH,
        &format!("/triggers/{trigger}"),
        Some(json!({ "name": "renamed" })),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        messages_of(&mut ws, "trigger_updated", Duration::from_millis(1000)).await,
        vec![json!({ "type": "trigger_updated", "trigger_id": trigger })]
    );
    let (_, list) = get(&daemon, "/triggers").await;
    assert_eq!(ids(&list), vec![trigger.clone()]);
    let (status, _) = get(&daemon, &format!("/triggers/{trigger}/fires")).await;
    assert_eq!(status, 200);
}
