//! Stage 4 (ADR-0080) — **a Trigger fire is an Operation** (#927): the cron
//! fire asks the Autorisation d'opération as the Acteur `Trigger`, "Run now" as
//! the caller `identify` set. Deciding that a Trigger is due asks no one.

use pdo::extension_api::{Actor, Operation, ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{post_json, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{self, Refusal, TestExtension};

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

fn fire(trigger_id: &str) -> Operation {
    Operation::new(ResourceKind::Trigger, Verb::Action("fire".into())).on(trigger_id)
}

async fn create_trigger(daemon: &TestDaemon) -> String {
    let (status, trigger) = post_json(
        daemon,
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
    trigger["id"].as_str().unwrap().to_string()
}

/// The Runs a Trigger started.
async fn runs_of(daemon: &TestDaemon, trigger_id: &str) -> Vec<Value> {
    let runs = daemon.get_json("/runs").await;
    runs.as_array()
        .unwrap()
        .iter()
        .filter(|r| r["triggered_by"] == trigger_id)
        .cloned()
        .collect()
}

#[tokio::test]
async fn a_cron_fire_asks_as_the_trigger_and_fires_when_allowed() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let id = create_trigger(&daemon).await;
    let before = control.seen().len();

    daemon.force_trigger_due(&id).await;
    daemon.run_trigger_tick().await;

    let asked: Vec<(Actor, Operation)> = control.seen().into_iter().skip(before).collect();
    assert_eq!(
        asked,
        vec![(
            Actor::Trigger {
                trigger_id: id.clone()
            },
            fire(&id)
        )],
        "the tick asks once, as the Trigger, and nothing else of its own"
    );
    assert_eq!(
        runs_of(&daemon, &id).await.len(),
        1,
        "the fire started a Run"
    );
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    assert_eq!(fires[0]["outcome"], "fired", "{fires}");
}

#[tokio::test]
async fn a_refused_cron_fire_starts_no_run_and_says_why_in_the_history() {
    let (ext, _) = TestExtension::new(Refusal::Only(
        ResourceKind::Trigger,
        Verb::Action("fire".into()),
    ));
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let id = create_trigger(&daemon).await;

    daemon.force_trigger_due(&id).await;
    daemon.run_trigger_tick().await;

    assert!(runs_of(&daemon, &id).await.is_empty(), "no Run was started");
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    let reason = test_extension::reason(&fire(&id));
    assert_eq!(fires[0]["outcome"], "error", "{fires}");
    assert_eq!(fires[0]["reason"], format!("operation refused: {reason}"));
    assert_eq!(fires[0]["source"], "cron");
    // The slot is spent: the schedule moved forward, so the next tick does not
    // ask again.
    let trigger = daemon.get_json(&format!("/triggers/{id}")).await;
    let next = trigger["next_fire_at"].as_str().unwrap();
    assert!(next > "2020-01-01T00:00:00.000Z", "{trigger}");
}

#[tokio::test]
async fn a_refused_run_now_answers_403_before_any_effect() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let id = create_trigger(&daemon).await;

    control.set(Refusal::Only(
        ResourceKind::Trigger,
        Verb::Action("fire".into()),
    ));
    let (status, body) = post_json(&daemon, &format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    assert_eq!(
        control.seen().last().cloned(),
        Some((Actor::LocalOperator, fire(&id))),
        "Run now asks as the caller identify set"
    );
    assert!(runs_of(&daemon, &id).await.is_empty());
    let fires = daemon.get_json(&format!("/triggers/{id}/fires")).await;
    assert_eq!(fires, json!([]), "no fire record");

    control.set(Refusal::Nothing);
    let (status, body) = post_json(&daemon, &format!("/triggers/{id}/fire"), json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["fired"], true, "{body}");
}
