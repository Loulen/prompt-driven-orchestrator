//! Stage 4 (ADR-0080) — the rest of the extension API against the real daemon
//! (#931): the entity attributes (stamped at creation, prefixed, returned with
//! the entity, filtered on by every list), the events (`on_event` receives the
//! JSON of `/ws` plus `OperationApplied`, asynchronously), the extensions'
//! routes under `/ext/<id>/`, their data folder, and `GET /ext`.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use pdo::extension_api::{Actor, Attributes, EntityRef, Extension, Manifest};
use serde_json::{json, Value};

use crate::common::flow::{create_run, post_json, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{self, OnEvent, Refusal, TestExtension, ACTOR_HEADER};

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

async fn daemon() -> (TestDaemon, test_extension::Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    (daemon, control)
}

/// `POST path` as the caller `actor` (the test extension identifies it).
async fn post_as(daemon: &TestDaemon, actor: &str, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .header(ACTOR_HEADER, actor)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn run_body(daemon: &TestDaemon, name: &str) -> Value {
    json!({
        "pipeline": "solo",
        "input": "go",
        "name": name,
        "target_repo": daemon.target_repo(),
    })
}

/// The ids of a list's entries, read under `key`.
fn ids(list: &Value, key: &str) -> Vec<String> {
    list.as_array()
        .unwrap_or_else(|| panic!("a list: {list}"))
        .iter()
        .map(|entry| entry[key].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_run_created_by_alice_carries_her_attribute_and_the_filter_keeps_only_hers() {
    let (daemon, _) = daemon().await;
    let (status, created) = post_as(&daemon, "alice", "/runs", run_body(&daemon, "hers")).await;
    assert_eq!(status, 201, "{created}");
    let hers = created["run_id"].as_str().unwrap().to_string();
    let mine = create_run(&daemon, "solo").await;

    let all = daemon.get_json("/runs").await;
    assert_eq!(all.as_array().unwrap().len(), 2, "{all}");
    let filtered = daemon.get_json("/runs?attr.test/started_by=alice").await;
    assert_eq!(ids(&filtered, "run_id"), vec![hers.clone()]);
    assert_eq!(
        filtered[0]["attributes"],
        json!({ "test/started_by": "alice" })
    );
    // The key is prefixed with the extension's id: the bare key filters nothing.
    let unprefixed = daemon.get_json("/runs?attr.started_by=alice").await;
    assert_eq!(unprefixed, json!([]));
    let local = daemon
        .get_json("/runs?attr.test%2Fstarted_by=local_operator")
        .await;
    assert_eq!(ids(&local, "run_id"), vec![mine]);

    let detail = daemon.get_json(&format!("/runs/{hers}")).await;
    assert_eq!(detail["attributes"], json!({ "test/started_by": "alice" }));
}

#[tokio::test]
async fn projects_and_agent_profiles_carry_their_attribute_and_filter_on_it() {
    let (daemon, _) = daemon().await;

    let (status, hers) = post_as(&daemon, "alice", "/projects", json!({ "name": "Hers" })).await;
    assert_eq!(status, 201, "{hers}");
    assert_eq!(hers["attributes"], json!({ "test/started_by": "alice" }));
    let (status, _) = post_json(&daemon, "/projects", json!({ "name": "Mine" })).await;
    assert_eq!(status, 201);
    let filtered = daemon
        .get_json("/projects?attr.test/started_by=alice")
        .await;
    assert_eq!(ids(&filtered, "id"), vec![hers["id"].as_str().unwrap()]);
    assert_eq!(
        daemon.get_json("/projects").await.as_array().unwrap().len(),
        2
    );
    let one = daemon
        .get_json(&format!("/projects/{}", hers["id"].as_str().unwrap()))
        .await;
    assert_eq!(one["attributes"], json!({ "test/started_by": "alice" }));

    let (status, profile) = post_as(
        &daemon,
        "alice",
        "/settings/agent-profiles",
        json!({ "name": "Fast", "harness": "claude" }),
    )
    .await;
    assert_eq!(status, 201, "{profile}");
    assert_eq!(profile["attributes"], json!({ "test/started_by": "alice" }));
    let filtered = daemon
        .get_json("/settings/agent-profiles?attr.test/started_by=alice")
        .await;
    assert_eq!(
        ids(&filtered["profiles"], "id"),
        vec![profile["id"].as_str().unwrap()]
    );
    let all = daemon.get_json("/settings/agent-profiles").await;
    assert_eq!(all["profiles"].as_array().unwrap().len(), 2, "{all}");
    assert!(
        all["profiles"][0].get("attributes").is_none(),
        "the reserved Default carries none: {all}"
    );
}

#[tokio::test]
async fn triggers_and_library_pipelines_carry_their_attribute_and_filter_on_it() {
    let (daemon, _) = daemon().await;

    let (status, trigger) = post_as(
        &daemon,
        "alice",
        "/triggers",
        json!({
            "name": "nightly",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
        }),
    )
    .await;
    assert_eq!(status, 201, "{trigger}");
    assert_eq!(trigger["attributes"], json!({ "test/started_by": "alice" }));
    let filtered = daemon
        .get_json("/triggers?attr.test/started_by=alice")
        .await;
    assert_eq!(ids(&filtered, "id"), vec![trigger["id"].as_str().unwrap()]);
    assert_eq!(
        daemon.get_json("/triggers?attr.test/started_by=bob").await,
        json!([])
    );

    let (status, created) =
        post_as(&daemon, "alice", "/pipelines", json!({ "name": "Hers" })).await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["attributes"], json!({ "test/started_by": "alice" }));
    let filtered = daemon
        .get_json("/pipelines?attr.test/started_by=alice")
        .await;
    assert_eq!(ids(&filtered, "id"), vec![created["id"].as_str().unwrap()]);
    assert!(
        daemon
            .get_json("/pipelines")
            .await
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}

/// A second extension stamping the very same key.
struct SameKey;
impl Extension for SameKey {
    fn manifest(&self) -> Manifest {
        Manifest::new("other", "Other", "0.0.0")
    }
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::from([("started_by".to_string(), "someone else".to_string())])
    }
}

#[tokio::test]
async fn two_extensions_setting_the_same_key_never_overwrite_each_other() {
    let (ext, _) = TestExtension::new(Refusal::Nothing);
    let mut extensions = ext.boxed();
    extensions.push(Box::new(SameKey));
    let daemon = TestDaemon::spawn_with_extensions(seed, extensions)
        .await
        .unwrap();
    let (status, project) = post_as(&daemon, "alice", "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    assert_eq!(
        project["attributes"],
        json!({ "test/started_by": "alice", "other/started_by": "someone else" })
    );
    let other = daemon
        .get_json("/projects?attr.other/started_by=someone%20else&attr.test/started_by=alice")
        .await;
    assert_eq!(other.as_array().unwrap().len(), 1, "{other}");
}

/// Wait until `check` holds on what the extension recorded (10 s budget).
async fn wait_events(
    control: &test_extension::Control,
    check: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let events = control.events();
        if check(&events) {
            return events;
        }
        assert!(Instant::now() < deadline, "never came: {events:#?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The events of `run_id` among `events`, in order.
fn events_of(events: &[Value], run_id: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e["type"] == "event" && e["event"]["run_id"] == run_id)
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_extension_receives_the_runs_events_exactly_as_ws_relays_them() {
    let (daemon, control) = daemon().await;
    let mut ws = daemon.connect_ws().await.unwrap();
    let run_id = create_run(&daemon, "solo").await;
    crate::common::flow::wait_node_status(&daemon, &run_id, "worker", "running").await;

    // What `/ws` relayed for the Run, until the worker runs.
    let mut relayed = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let next = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
        let Ok(Some(Ok(msg))) = next else {
            if !relayed.is_empty() {
                break;
            }
            continue;
        };
        if let Ok(text) = msg.to_text() {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                relayed.push(value);
            }
        }
    }
    let relayed = events_of(&relayed, &run_id);
    assert!(!relayed.is_empty(), "the Run's events reach /ws");
    let seen = wait_events(&control, |events| {
        events_of(events, &run_id).len() >= relayed.len()
    })
    .await;
    assert_eq!(events_of(&seen, &run_id)[..relayed.len()], relayed[..]);
}

#[tokio::test]
async fn a_settings_change_is_followed_by_an_operation_applied() {
    let (daemon, control) = daemon().await;
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .header(ACTOR_HEADER, "alice")
        .json(&json!({ "session_cap": 3 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let applied = |events: &[Value]| {
        events
            .iter()
            .find(|e| e["type"] == "operation_applied")
            .cloned()
    };
    let events = wait_events(&control, |events| applied(events).is_some()).await;
    assert_eq!(
        applied(&events).unwrap(),
        json!({
            "type": "operation_applied",
            "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
            "operation": { "resource": "instance_settings", "verb": "update", "target": null },
            "entity": null,
        })
    );

    // A refused mutation is no applied Operation.
    control.set(Refusal::Everything);
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&json!({ "session_cap": 4 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    control.set(Refusal::Nothing);
    let (status, project) = post_json(&daemon, "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    let events = wait_events(&control, |events| {
        events
            .iter()
            .any(|e| e["operation"]["resource"] == "project")
    })
    .await;
    let applied: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "operation_applied")
        .collect();
    assert_eq!(
        applied.len(),
        2,
        "the refused PUT is not there: {applied:#?}"
    );
    assert_eq!(
        applied[1]["entity"],
        json!({ "kind": "project", "id": project["id"] }),
        "a creation names the entity it made"
    );
}

#[tokio::test]
async fn an_extension_that_sleeps_or_panics_in_on_event_never_delays_a_response() {
    let (daemon, control) = daemon().await;
    control.on_event(OnEvent::Sleep(Duration::from_secs(30)));
    // Each creation hands an `OperationApplied` to the sleeping extension. The
    // bound is half the sleep: a loaded machine can take seconds to answer a
    // creation, never the 30 s a delayed response would.
    for name in ["one", "two", "three"] {
        let start = Instant::now();
        let (status, _) = post_json(&daemon, "/projects", json!({ "name": name })).await;
        assert_eq!(status, 201);
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "a sleeping on_event delayed the response: {:?}",
            start.elapsed()
        );
    }
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    control.on_event(OnEvent::Panic);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    for name in ["one", "two"] {
        let start = Instant::now();
        let (status, _) = post_json(&daemon, "/projects", json!({ "name": name })).await;
        assert_eq!(status, 201, "the daemon lives on");
        assert!(start.elapsed() < Duration::from_secs(15));
    }
    // Back to recording: the delivery thread survived the panics.
    control.on_event(OnEvent::Record);
    let (status, _) = post_json(&daemon, "/projects", json!({ "name": "three" })).await;
    assert_eq!(status, 201);
    wait_events(&control, |events| {
        events.iter().any(|e| e["type"] == "operation_applied")
    })
    .await;
}

#[tokio::test]
async fn the_extension_routes_answer_under_ext_and_hide_no_core_route() {
    let (daemon, control) = daemon().await;
    let about = daemon.get_json("/ext/test").await;
    assert_eq!(about["id"], "test");
    // Its data folder is `~/.pdo/extensions/test/`, the tests' home here.
    let data_dir = control.data_dir().expect("start received its data folder");
    assert!(data_dir.ends_with(".pdo/extensions/test"), "{data_dir:?}");
    assert!(data_dir.is_dir());
    assert!(
        data_dir.starts_with(daemon.repo_root()),
        "never the real $HOME"
    );
    assert_eq!(about["data_dir"], data_dir.display().to_string());

    let resp = reqwest::get(format!("{}/ext/test/ui.js", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/javascript"));
    let missing = reqwest::get(format!("{}/ext/test/nope", daemon.url()))
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    // The core's routes are all still there.
    assert!(daemon.get_json("/runs").await.is_array());
    assert!(daemon.get_json("/projects").await.is_array());

    // The trace of the events is readable through its route.
    let (status, _) = post_json(&daemon, "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    wait_events(&control, |events| !events.is_empty()).await;
    let trace = daemon.get_json("/ext/test/events").await;
    assert!(
        trace
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "operation_applied"),
        "{trace}"
    );
    let jsonl = std::fs::read_to_string(data_dir.join("events.jsonl")).unwrap();
    assert!(jsonl.contains("operation_applied"), "{jsonl}");
}

#[tokio::test]
async fn ext_lists_each_extension_and_its_ui_slots() {
    let (daemon, control) = daemon().await;
    let index = daemon.get_json("/ext").await;
    assert_eq!(index[0]["id"], "test");
    assert_eq!(index[0]["name"], "Test extension");
    let ui = &index[0]["ui"];
    assert_eq!(ui["bundle_url"], "/ext/test/ui.js");
    assert_eq!(ui["react_major"], test_extension::REACT_MAJOR);
    let slots: Vec<(&str, Option<&str>)> = ui["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["slot"].as_str().unwrap(), s["resource"].as_str()))
        .collect();
    assert_eq!(
        slots,
        vec![
            ("navigation", None),
            ("settings_panel", None),
            ("list_filter", Some("run")),
            ("detail_tab", Some("run")),
        ]
    );

    let (status, switched) = post_json(
        &daemon,
        "/ext/test/incompatible",
        json!({ "enabled": true }),
    )
    .await;
    assert_eq!(status, 200, "{switched}");
    let _ = control;
    let index = daemon.get_json("/ext").await;
    assert_eq!(
        index[0]["ui"]["react_major"],
        test_extension::REACT_MAJOR - 1
    );
}

#[tokio::test]
async fn without_extension_ext_is_empty_and_the_entities_carry_no_attribute() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    assert_eq!(daemon.get_json("/ext").await, json!([]));
    let (status, project) = post_json(&daemon, "/projects", json!({ "name": "P" })).await;
    assert_eq!(status, 201);
    assert!(project.get("attributes").is_none(), "{project}");
    let run_id = create_run(&daemon, "solo").await;
    let runs = daemon.get_json("/runs").await;
    assert!(runs[0].get("attributes").is_none(), "{runs}");
    assert_eq!(
        daemon.get_json("/runs?attr.test/started_by=alice").await,
        json!([])
    );
    assert!(daemon
        .get_json(&format!("/runs/{run_id}"))
        .await
        .get("attributes")
        .is_none());
}
