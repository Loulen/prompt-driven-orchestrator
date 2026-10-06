//! API 0.9 — **the creation of a Run, open to the extensions**, on a real
//! daemon with the test extension: the data a `POST /runs` carries for an
//! extension (checked before anything exists, received before the first
//! Node, frozen in `RunStarted`), the `--ext` flag of `pdo run create`, and a
//! Run an extension creates through its `Host` on behalf of an Actor.

use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

use crate::common::flow::{post_json, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// A daemon with the test extension and the `solo` pipeline (one agent,
/// `worker`).
async fn daemon() -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_configured(
        |repo| seed_solo(repo, &[("solo", false)]),
        move |root, config| {
            config.sandbox_home_override = Some(root.join("home"));
            pdo::compose(config, ext.boxed(), None).unwrap();
        },
    )
    .await
    .unwrap();
    (daemon, control)
}

fn create_body(daemon: &TestDaemon, extensions: Value) -> Value {
    json!({
        "pipeline": "solo",
        "input": "go",
        "target_repo": daemon.target_repo(),
        "extensions": extensions,
    })
}

async fn runs(daemon: &TestDaemon) -> Vec<Value> {
    daemon
        .get_json("/runs")
        .await
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The asks of `authorize` about `run` × `create`.
fn run_creates(control: &Control) -> usize {
    control
        .seen()
        .iter()
        .filter(|(_, op)| op.resource.to_string() == "run" && op.verb.name() == "create")
        .count()
}

/// The `RunStarted` payload of `run_id`.
async fn run_started(daemon: &TestDaemon, run_id: &str) -> Value {
    let events = daemon.get_json(&format!("/runs/{run_id}/events")).await;
    events
        .as_array()
        .and_then(|events| events.iter().find(|e| e["kind"] == "run_started"))
        .map(|e| e["payload"].clone())
        .expect("a run_started")
}

/// Wait until the test extension was asked for the launch of `run_id`'s
/// `worker`; the timeline then.
async fn timeline_once_launched(control: &Control, run_id: &str) -> Vec<String> {
    let launched = format!("node_env {run_id}/worker");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let timeline = control.creation.timeline();
        if timeline.contains(&launched) {
            return timeline;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker never launched: {timeline:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_value_the_extension_refuses_creates_nothing() {
    let (daemon, control) = daemon().await;
    let (status, body) = post_json(
        &daemon,
        "/runs",
        create_body(&daemon, json!({ "test": { "cle": "refuse-me" } })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body,
        json!({
            "error": "extension `test` refused the creation: the test extension refuses cle=refuse-me",
            "extension": "test",
            "reason": "the test extension refuses cle=refuse-me",
        })
    );
    assert_eq!(run_creates(&control), 1, "checked after the authorization");
    assert!(runs(&daemon).await.is_empty(), "no Run");
    assert!(control.creation.creations().is_empty(), "nothing stamped");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        control.events().iter().all(|e| e.get("run_id").is_none()),
        "no event of a Run: {:?}",
        control.events()
    );
}

#[tokio::test]
async fn an_extension_nobody_installed_is_refused_like_an_unknown_field() {
    let (daemon, control) = daemon().await;
    let (status, body) = post_json(
        &daemon,
        "/runs",
        create_body(&daemon, json!({ "ghost": { "k": "v" } })),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        "unknown extension `ghost` in `extensions`; installed extensions: test"
    );
    assert_eq!(run_creates(&control), 0, "refused before the authorization");
    assert!(runs(&daemon).await.is_empty());
}

#[tokio::test]
async fn an_accepted_value_is_stamped_before_the_first_node_and_frozen() {
    let (daemon, control) = daemon().await;
    let (status, body) = post_json(
        &daemon,
        "/runs",
        create_body(&daemon, json!({ "test": { "cle": "valeur" } })),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();

    let timeline = timeline_once_launched(&control, &run_id).await;
    let stamped = timeline
        .iter()
        .position(|t| *t == format!("on_create {run_id}"))
        .expect("stamped");
    let launched = timeline
        .iter()
        .position(|t| *t == format!("node_env {run_id}/worker"))
        .unwrap();
    assert!(
        stamped < launched,
        "the Run is stamped before its first Node: {timeline:?}"
    );
    let creation = control
        .creation
        .creations()
        .into_iter()
        .find(|c| c["id"] == run_id.as_str())
        .unwrap();
    assert_eq!(creation["data"], json!({ "cle": "valeur" }));
    assert_eq!(creation["actor"], json!({ "kind": "local_operator" }));

    assert_eq!(
        run_started(&daemon, &run_id).await["extensions"],
        json!({ "test": { "cle": "valeur" } })
    );
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(run["attributes"]["test/data.cle"], "valeur", "{run}");
    assert_eq!(run["attributes"]["test/started_by"], "local_operator");
}

#[tokio::test]
async fn a_run_without_data_keeps_its_shape() {
    let (daemon, control) = daemon().await;
    let run_id = crate::common::flow::create_run(&daemon, "solo").await;
    assert!(run_started(&daemon, &run_id).await.get("extensions").is_none());
    timeline_once_launched(&control, &run_id).await;
    let creation = control
        .creation
        .creations()
        .into_iter()
        .find(|c| c["id"] == run_id.as_str())
        .unwrap();
    assert_eq!(creation["data"], Value::Null, "the hook receives no value");
}

/// `pdo run create … --ext …` against `daemon`, on a blocking task.
async fn pdo_run_create(daemon: &TestDaemon, args: &[&str]) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
    cmd.args(["run", "create"]).args(args);
    cmd.env("PDO_DAEMON_URL", daemon.url());
    cmd.env_remove("PDO_RUN_ID");
    cmd.env_remove("PDO_NODE_ID");
    cmd.env_remove("PDO_TOKEN");
    let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test]
async fn the_ext_flag_sends_the_same_request() {
    let (daemon, control) = daemon().await;
    let repo = daemon.target_repo();
    let (ok, stdout, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &repo,
            "--ext",
            "test.cle=valeur",
            "--ext",
            "test.autre=a=b",
        ],
    )
    .await;
    assert!(ok, "stdout={stdout} stderr={stderr}");
    let run_id = stdout
        .trim()
        .strip_prefix("Run ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    assert_eq!(
        run_started(&daemon, &run_id).await["extensions"],
        json!({ "test": { "cle": "valeur", "autre": "a=b" } })
    );
    timeline_once_launched(&control, &run_id).await;

    let before = runs(&daemon).await.len();
    let (ok, _, stderr) = pdo_run_create(
        &daemon,
        &[
            "solo",
            "--input",
            "go",
            "--target-repo",
            &repo,
            "--ext",
            "test.cle=refuse-me",
        ],
    )
    .await;
    assert!(!ok);
    assert!(
        stderr.contains("the test extension refuses cle=refuse-me"),
        "{stderr}"
    );
    assert_eq!(runs(&daemon).await.len(), before, "no Run");
}

#[tokio::test]
async fn an_extension_creates_a_run_through_its_host_on_behalf_of_an_actor() {
    let (daemon, control) = daemon().await;
    control.set(Refusal::parse("run:create~bob").unwrap());
    let client = reqwest::Client::new();
    let create = |actor: &'static str, extensions: Value| {
        let request = client
            .post(format!("{}/ext/test/runs", daemon.url()))
            .header(ACTOR_HEADER, actor)
            .json(&json!({
                "pipeline": "solo",
                "input": "from the backend",
                "repo": daemon.target_repo(),
                "name": "Backend run",
                "extensions": extensions,
            }));
        async move {
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            (status, response.json::<Value>().await.unwrap())
        }
    };

    let (status, body) = create("alice", json!({ "test": { "cle": "valeur" } })).await;
    assert_eq!(status, 201, "{body}");
    let run_id = body["run_id"].as_str().unwrap().to_string();
    let run = daemon.get_json(&format!("/runs/{run_id}")).await;
    assert_eq!(
        run["created_by"],
        json!({ "kind": "identified", "namespace": "test", "id": "alice" })
    );
    assert_eq!(run["name"], "Backend run");
    assert_eq!(run["attributes"]["test/started_by"], "alice");
    assert_eq!(run["attributes"]["test/data.cle"], "valeur");
    timeline_once_launched(&control, &run_id).await;

    let (status, body) = create("bob", json!({})).await;
    assert_eq!(status, 403, "refused as over HTTP: {body}");
    assert_eq!(body["error"], "operation_refused");
    let (status, body) = create("alice", json!({ "test": { "cle": "refuse" } })).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["reason"], "the test extension refuses cle=refuse");
    assert_eq!(runs(&daemon).await.len(), 1, "only alice's first Run");
}
