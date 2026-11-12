//! **The environment and the end of a Trigger's guard**, on a real daemon with
//! the test extension: the guard reads the variables the extension gives the
//! Trigger's last editor (`X_TEST_TOKEN`), in no argv, and the extension hears
//! the guard's end whatever it is. Without variables, the guard runs as before.

use std::path::Path;

use pdo::extension_api::{Actor, GitOperationKind, GuardOutcome, NodeEnv};
use serde_json::{json, Value};

use crate::common::flow::seed_solo;
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

const SECRET: &str = "x-test-token-value-42";

fn seed(repo: &Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

async fn daemon() -> (TestDaemon, Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    (daemon, control)
}

/// Create, as alice, a Trigger guarded by `guard`: its id.
async fn trigger_of_alice(daemon: &TestDaemon, guard: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/triggers", daemon.url()))
        .header(ACTOR_HEADER, "alice")
        .json(&json!({
            "name": "guarded",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
            "guard_command": guard,
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let trigger: Value = resp.json().await.unwrap();
    assert_eq!(status, 201, "{trigger}");
    trigger["id"].as_str().unwrap().to_string()
}

/// Let the Trigger `id` fire on its tick: its last fire.
async fn fire(daemon: &TestDaemon, id: &str) -> Value {
    daemon.force_trigger_due(id).await;
    daemon.run_trigger_tick().await;
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    fires[0].clone()
}

#[tokio::test]
async fn the_guard_reads_the_token_given_its_last_editor_never_in_an_argv() {
    let (daemon, control) = daemon().await;
    control.launch.set_git_env(
        "alice",
        NodeEnv::from([("X_TEST_TOKEN".into(), SECRET.into())]),
        None,
    );
    let out = tempfile::tempdir().unwrap();
    let seen = out.path().join("seen");
    let cmdline = out.path().join("cmdline");
    let guard = format!(
        "[ -n \"$X_TEST_TOKEN\" ] && printf '%s' \"$X_TEST_TOKEN\" > '{}' \
         && tr '\\0' ' ' < /proc/$$/cmdline > '{}'",
        seen.display(),
        cmdline.display()
    );
    let id = trigger_of_alice(&daemon, &guard).await;

    let record = fire(&daemon, &id).await;

    assert_eq!(record["outcome"], "fired", "the Run starts: {record}");
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), SECRET);
    let argv = std::fs::read_to_string(&cmdline).unwrap();
    assert!(argv.contains("sh -c"), "{argv}");
    assert!(!argv.contains(SECRET), "the token sits in an argv: {argv}");

    let asked = control
        .launch
        .git_asks()
        .into_iter()
        .find(|op| op.kind == GitOperationKind::Guard)
        .expect("the guard's environment was asked");
    assert_eq!(asked.actor, alice(), "its last editor, not the Trigger");
    let ends = control.launch.guard_ends();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0].guard, asked.guard.unwrap(), "the same execution");
    assert_eq!(ends[0].guard.trigger_id.as_deref(), Some(id.as_str()));
    assert_eq!(ends[0].outcome, GuardOutcome::Passed);
    let state = daemon.get_json("/ext/test/git-env").await;
    assert_eq!(state["guard_ends"][0]["outcome"], "passed", "{state}");
}

#[tokio::test]
async fn the_extension_hears_a_skipped_guard_and_without_variables_nothing_is_added() {
    let (daemon, control) = daemon().await;
    let id = trigger_of_alice(&daemon, "[ -n \"$X_TEST_TOKEN\" ]").await;

    let record = fire(&daemon, &id).await;

    assert_eq!(record["outcome"], "guard-exit-nonzero", "{record}");
    let ends = control.launch.guard_ends();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0].outcome, GuardOutcome::Skipped);
}
