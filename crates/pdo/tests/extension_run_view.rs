//! API 0.12 against the real daemon, with the test extension: a Run as an
//! extension sees it.
//!
//! - **Presences**: two connections of `/ws` under two Actors show the same
//!   Run (`{"type": "show_run"}`): two Presences, read by the extension's
//!   `Host::presences` and by `GET /runs/presences`; one closes: one remains.
//!   Each arrival and departure reaches the extension's `on_event` and `/ws`.
//!   Two tabs of one Actor are one Presence; a Run the caller may not read,
//!   or that does not exist, is no Presence.
//! - **Cost**: `Host::run_cost` answers the cost `GET /runs/<id>` gives the
//!   Info tab, a lower bound and an unknown amount included.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::common::flow::git_init;
use crate::common::{ws_text, TestDaemon};
use crate::test_extension::{Refusal, TestExtension};

const RUN: &str = "20261009-100000-presence";

async fn daemon_with_a_run() -> (TestDaemon, crate::test_extension::Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    daemon.seed_run_in(RUN, "solo", &daemon.target_repo()).await;
    (daemon, control)
}

/// The extension's view of `run_id`: `{presences, cost}`.
async fn view(daemon: &TestDaemon, run_id: &str) -> Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/ext/test/runs/{run_id}/view", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json().await.unwrap()
}

/// The ids of the Actors present on `run_id`, as the extension reads them.
async fn present(daemon: &TestDaemon, run_id: &str) -> Vec<String> {
    view(daemon, run_id).await["presences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|presence| presence["actor"]["id"].as_str().unwrap_or("?").to_string())
        .collect()
}

/// Poll until the Actors present on `run_id` are `expected`.
async fn wait_present(daemon: &TestDaemon, run_id: &str, expected: &[&str]) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let now = present(daemon, run_id).await;
        if now == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "present on {run_id}: {now:?}, expected {expected:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `GET /runs/presences` as `actor`.
async fn open_runs(daemon: &TestDaemon, actor: &str) -> Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/presences", daemon.url()))
        .header("x-pdo-test-actor", actor)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json().await.unwrap()
}

fn show(run_id: Option<&str>) -> Message {
    Message::Text(
        json!({ "type": "show_run", "run_id": run_id })
            .to_string()
            .into(),
    )
}

/// The `(type, actor id)` of each Presence message `/ws` carried within
/// `window`.
async fn presence_messages<S>(ws: &mut S, window: Duration) -> Vec<(String, String)>
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
        let message: Value = serde_json::from_str(text).unwrap();
        let kind = message["type"].as_str().unwrap_or_default();
        if kind.starts_with("presence_") {
            assert_eq!(message["run_id"], RUN);
            seen.push((
                kind.to_string(),
                message["actor"]["id"].as_str().unwrap().to_string(),
            ));
        }
    }
    seen
}

#[tokio::test]
async fn two_actors_showing_a_run_are_two_presences_until_one_leaves() {
    let (daemon, control) = daemon_with_a_run().await;
    let mut watcher = daemon.connect_ws_as("carol").await.unwrap();
    let mut alice = daemon.connect_ws_as("alice").await.unwrap();
    let mut bob = daemon.connect_ws_as("bob").await.unwrap();
    assert!(present(&daemon, RUN).await.is_empty(), "nobody yet");

    alice.send(show(Some(RUN))).await.unwrap();
    wait_present(&daemon, RUN, &["alice"]).await;
    bob.send(show(Some(RUN))).await.unwrap();
    wait_present(&daemon, RUN, &["alice", "bob"]).await;

    let open = open_runs(&daemon, "carol").await;
    assert_eq!(open["runs"].as_array().unwrap().len(), 1, "{open}");
    assert_eq!(open["runs"][0]["run_id"], RUN);
    assert_eq!(open["runs"][0]["presences"][1]["actor"]["id"], "bob");
    assert!(open["runs"][0]["presences"][0]["since"].is_string());

    bob.close(None).await.unwrap();
    wait_present(&daemon, RUN, &["alice"]).await;
    alice.send(show(None)).await.unwrap();
    wait_present(&daemon, RUN, &[]).await;

    let expected = vec![
        ("presence_arrived".to_string(), "alice".to_string()),
        ("presence_arrived".to_string(), "bob".to_string()),
        ("presence_left".to_string(), "bob".to_string()),
        ("presence_left".to_string(), "alice".to_string()),
    ];
    assert_eq!(
        presence_messages(&mut watcher, Duration::from_millis(500)).await,
        expected,
        "/ws announces each arrival and each departure"
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let seen: Vec<(String, String)> = control
            .events()
            .iter()
            .filter(|event| {
                event["type"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("presence_"))
            })
            .map(|event| {
                (
                    event["type"].as_str().unwrap().to_string(),
                    event["actor"]["id"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        if seen == expected {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the extension's on_event saw {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn two_tabs_of_one_actor_are_one_presence_until_both_closed() {
    let (daemon, _control) = daemon_with_a_run().await;
    let mut first = daemon.connect_ws_as("alice").await.unwrap();
    let mut second = daemon.connect_ws_as("alice").await.unwrap();
    first.send(show(Some(RUN))).await.unwrap();
    second.send(show(Some(RUN))).await.unwrap();
    wait_present(&daemon, RUN, &["alice"]).await;
    first.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        present(&daemon, RUN).await,
        ["alice"],
        "her other tab still shows it"
    );
    drop(second);
    wait_present(&daemon, RUN, &[]).await;
}

#[tokio::test]
async fn a_run_the_caller_may_not_read_or_that_does_not_exist_is_no_presence() {
    let (daemon, control) = daemon_with_a_run().await;
    control.set(Refusal::parse("run:read~bob").unwrap());
    let mut bob = daemon.connect_ws_as("bob").await.unwrap();
    let mut alice = daemon.connect_ws_as("alice").await.unwrap();
    bob.send(show(Some(RUN))).await.unwrap();
    alice.send(show(Some("no-such-run"))).await.unwrap();
    alice.send(show(Some(RUN))).await.unwrap();
    wait_present(&daemon, RUN, &["alice"]).await;
    assert!(present(&daemon, "no-such-run").await.is_empty());
    assert_eq!(
        open_runs(&daemon, "bob").await,
        json!({ "runs": [] }),
        "Bob may not read the Run: he sees nobody on it"
    );
    assert!(
        presence_messages(&mut bob, Duration::from_millis(300))
            .await
            .is_empty(),
        "nor its Presences on /ws"
    );
}

/// Plant a Claude Code transcript of `model` for the Run's worktree, under
/// the daemon's home.
fn plant_transcript(daemon: &TestDaemon, model: &str, input: u64) {
    let worktree = std::path::Path::new(&daemon.target_repo())
        .join(".pdo/runs")
        .join(RUN)
        .join("worktree");
    let encoded = pdo_harness::claude_transcript::encode_working_dir(&worktree);
    let home = daemon.repo_root().join("home");
    let project = home.join(".claude/projects").join(encoded);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("s.jsonl"),
        format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r1\",\"message\":{{\"id\":\"m1\",\
             \"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
        ),
    )
    .unwrap();
}

/// The cost `GET /runs/<id>` gives the Info tab.
async fn info_cost(daemon: &TestDaemon) -> Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/runs/{RUN}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["cost"].clone()
}

#[tokio::test]
async fn the_host_reads_the_cost_the_info_tab_shows() {
    let (daemon, _control) = daemon_with_a_run().await;

    // No session left a cost: unknown on both sides, never $0.
    assert_eq!(info_cost(&daemon).await, Value::Null);
    let unknown = view(&daemon, RUN).await["cost"].clone();
    assert_eq!(unknown["usd"], Value::Null, "{unknown}");
    assert_eq!(unknown["partial"], false);

    // A model no tier prices: a lower bound naming it.
    plant_transcript(&daemon, "claude-mystery-9", 1_000_000);
    let info = info_cost(&daemon).await;
    let read = view(&daemon, RUN).await["cost"].clone();
    assert!(info.is_object(), "the Info tab has a cost now: {info}");
    assert_eq!(read["usd"], info["usd"], "{read} vs {info}");
    assert_eq!(read["partial"], info["partial"]);
    assert_eq!(read["partial"], true);
    assert_eq!(read["unpriced_models"], info["unpriced_models"]);
    assert_eq!(read["uncosted_harnesses"], json!([]));

    // A Run the daemon does not know has no cost at all.
    assert_eq!(view(&daemon, "no-such-run").await["cost"], Value::Null);
}
