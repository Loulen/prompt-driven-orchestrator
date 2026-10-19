//! The OTLP export of the real daemon (ADR-0090), against a local collector
//! that records what it receives: off by default, nothing leaves; switched on
//! or pointed elsewhere by `PUT /settings`, without a restart; a Run's
//! measures named by the repository's remote, never by its path; a collector
//! that does not answer is said in the status and leaves the Runs alone.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::Uri;
use axum::Router;
use serde_json::{json, Value};

use crate::common::TestDaemon;

/// A collector that keeps every request it receives: its path and its body.
#[derive(Clone, Default)]
struct Collector {
    received: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    url: String,
}

impl Collector {
    async fn start() -> Self {
        let received: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::default();
        let kept = received.clone();
        let app = Router::new().fallback(move |uri: Uri, body: Bytes| {
            let kept = kept.clone();
            async move {
                kept.lock()
                    .unwrap()
                    .push((uri.path().to_string(), body.to_vec()));
                ""
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self { received, url }
    }

    fn requests(&self) -> Vec<(String, Vec<u8>)> {
        self.received.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.received.lock().unwrap().clear();
    }

    /// Every body received on `path`, as lossy text: protobuf strings are raw
    /// UTF-8, so an attribute's value reads as itself.
    fn text_on(&self, path: &str) -> String {
        self.requests()
            .into_iter()
            .filter(|(p, _)| p == path)
            .map(|(_, body)| String::from_utf8_lossy(&body).into_owned())
            .collect()
    }
}

async fn put_settings(daemon: &TestDaemon, body: Value) -> Value {
    let resp = reqwest::Client::new()
        .put(format!("{}/settings", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "PUT /settings {body}");
    resp.json().await.unwrap()
}

/// `POST /telemetry/test-export`: its status code and body.
async fn test_export(daemon: &TestDaemon) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/telemetry/test-export", daemon.url()))
        .send()
        .await
        .unwrap();
    (resp.status().as_u16(), resp.json().await.unwrap())
}

/// The reload is detached from the `PUT`: wait for the status to follow.
async fn wait_exporting(daemon: &TestDaemon, want: bool) {
    for _ in 0..100 {
        let status = daemon.get_json("/telemetry/status").await;
        if status["exporting"] == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the export never turned {want}");
}

#[tokio::test]
async fn off_by_default_nothing_leaves_even_with_a_collector_named() {
    let collector = Collector::start().await;
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    let status = daemon.get_json("/telemetry/status").await;
    assert_eq!(status["exporting"], false);
    assert!(status["sent"]["metrics"]
        .as_array()
        .unwrap()
        .contains(&json!("pdo.runs.started")));

    let page = put_settings(&daemon, json!({ "telemetry_otlp_endpoint": collector.url })).await;
    assert_eq!(page["telemetry_otlp"]["effective"], false);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        daemon.get_json("/telemetry/status").await["exporting"],
        false
    );
    let (code, _) = test_export(&daemon).await;
    assert_eq!(code, 409);
    assert!(
        collector.requests().is_empty(),
        "nothing leaves while the switch is off"
    );
}

#[tokio::test]
async fn the_switch_and_the_collector_take_effect_without_a_restart() {
    let first = Collector::start().await;
    let second = Collector::start().await;
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();

    put_settings(
        &daemon,
        json!({
            "telemetry_otlp": true,
            "telemetry_otlp_endpoint": first.url,
            "telemetry_otlp_headers": [{ "name": "X-Api-Key", "value": "k3y" }],
        }),
    )
    .await;
    wait_exporting(&daemon, true).await;
    let (code, status) = test_export(&daemon).await;
    assert_eq!(code, 200, "{status}");
    assert_eq!(status["last_export"]["ok"], true, "{status}");
    assert!(
        first.text_on("/v1/traces").contains("pdo.telemetry.test"),
        "the test span reached the collector"
    );

    put_settings(&daemon, json!({ "telemetry_otlp_endpoint": second.url })).await;
    for _ in 0..100 {
        test_export(&daemon).await;
        if !second.requests().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !second.requests().is_empty(),
        "the new collector receives without a restart"
    );

    put_settings(&daemon, json!({ "telemetry_otlp": false })).await;
    wait_exporting(&daemon, false).await;
    first.clear();
    second.clear();
    reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap();
    let (code, _) = test_export(&daemon).await;
    assert_eq!(code, 409);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(first.requests().is_empty() && second.requests().is_empty());
}

#[tokio::test]
async fn an_unreachable_collector_is_said_in_the_status() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let daemon = TestDaemon::spawn(|_| Ok(())).await.unwrap();
    put_settings(
        &daemon,
        json!({ "telemetry_otlp": true, "telemetry_otlp_endpoint": format!("http://127.0.0.1:{port}") }),
    )
    .await;
    wait_exporting(&daemon, true).await;
    let (code, status) = test_export(&daemon).await;
    assert_eq!(code, 200);
    assert_eq!(status["last_export"]["ok"], false, "{status}");
    // The daemon still answers: the export runs beside it, never in its way.
    assert_eq!(
        daemon.get_json("/telemetry/status").await["exporting"],
        true
    );
}

const PIPELINE_YAML: &str = r#"name: telemetry-test
version: "1.0"
prompt_required: false
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
"#;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(pipelines.join("telemetry-test.prompts"))?;
    std::fs::write(pipelines.join("telemetry-test.yaml"), PIPELINE_YAML)?;
    std::fs::write(
        pipelines.join("telemetry-test.prompts").join("worker.md"),
        "Do the task.\n",
    )?;
    let git = |args: &[&str]| -> anyhow::Result<()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["config", "user.email", "test@example.com"])?;
    git(&["config", "user.name", "Test"])?;
    git(&["config", "commit.gpgsign", "false"])?;
    // A token in the remote must never leave with the metrics.
    git(&[
        "remote",
        "add",
        "origin",
        "https://someone:t0ken@git.example.org/org/app.git",
    ])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    git(&["add", "."])?;
    git(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

#[tokio::test]
async fn a_run_is_measured_with_its_remote_and_never_its_path() {
    let collector = Collector::start().await;
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    put_settings(
        &daemon,
        json!({ "telemetry_otlp": true, "telemetry_otlp_endpoint": collector.url }),
    )
    .await;
    wait_exporting(&daemon, true).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/runs", daemon.url()))
        .json(&json!({
            "pipeline": "telemetry-test",
            "input": "",
            "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // The recorder reads the bus beside the Run: export until it got there.
    let mut metrics = String::new();
    for _ in 0..100 {
        test_export(&daemon).await;
        metrics = collector.text_on("/v1/metrics");
        if metrics.contains("pdo.runs.started") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(metrics.contains("pdo.runs.started"), "no Run measure");
    assert!(metrics.contains("https://git.example.org/org/app.git"));
    assert!(metrics.contains("telemetry-test"), "the pipeline is named");
    assert!(metrics.contains("main"), "the branch is named");
    let everything: String = collector
        .requests()
        .into_iter()
        .map(|(_, body)| String::from_utf8_lossy(&body).into_owned())
        .collect();
    assert!(
        !everything.contains("t0ken"),
        "the remote's credentials leaked"
    );
    assert!(
        !everything.contains(&daemon.target_repo()),
        "a local path leaked"
    );
}
