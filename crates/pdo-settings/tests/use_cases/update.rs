use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_settings::application::SettingsError;
use serde_json::json;

use crate::fakes::world;

fn invalid(error: SettingsError) -> String {
    match error {
        SettingsError::Invalid(message) => message,
        other => panic!("expected a 400, got {other:?}"),
    }
}

#[tokio::test]
async fn an_edit_asks_the_instance_settings_update_and_answers_the_new_page() {
    let w = world();
    let page = w
        .settings
        .update(&Actor::LocalOperator, json!({ "session_cap": 4 }))
        .await
        .unwrap();
    assert_eq!(page["session_cap"]["stored"], 4, "the page follows: {page}");
    assert_eq!(w.row().session_cap, Some(4));
    assert_eq!(
        w.asked(),
        vec![(
            Actor::LocalOperator,
            Operation::new(ResourceKind::InstanceSettings, Verb::Update)
        )]
    );
}

#[tokio::test]
async fn a_refused_edit_stores_nothing_and_validates_nothing() {
    let w = world();
    w.refuse();
    // Even an invalid body: the refusal comes first, before the body is read.
    let err = w
        .settings
        .update(&Actor::LocalOperator, json!({ "session_cap": 0 }))
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Refused(_)), "{err:?}");
    assert_eq!(*w.config.writes.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_retired_field_rejects_the_whole_edit_naming_itself() {
    let w = world();
    let err = w
        .settings
        .update(
            &Actor::LocalOperator,
            json!({ "session_cap": 5, "image_source": "dockerfile" }),
        )
        .await
        .unwrap_err();
    assert!(invalid(err).contains("`image_source` is no longer"));
    assert_eq!(w.row().session_cap, None, "the sibling edit is not applied");
}

#[tokio::test]
async fn an_out_of_range_knob_is_refused_before_anything_is_stored() {
    let w = world();
    for body in [
        json!({ "session_cap": 0 }),
        json!({ "reaper_ttl_secs": 0 }),
        json!({ "guard_timeout_secs": 601 }),
        json!({ "max_attachments_mb": 0 }),
    ] {
        let err = w
            .settings
            .update(&Actor::LocalOperator, body.clone())
            .await
            .unwrap_err();
        invalid(err);
    }
    assert_eq!(*w.config.writes.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_type_error_is_a_named_refusal() {
    let w = world();
    let err = w
        .settings
        .update(&Actor::LocalOperator, json!({ "session_cap": "many" }))
        .await
        .unwrap_err();
    assert!(invalid(err).starts_with("invalid settings payload"));
}

#[tokio::test]
async fn the_default_sandbox_must_name_a_staging_profile_or_clear() {
    let w = world();
    let err = w
        .settings
        .update(&Actor::LocalOperator, json!({ "default_sandbox": "vm" }))
        .await
        .unwrap_err();
    assert!(invalid(err).contains("unknown sandbox profile `vm`"));
    for ok in ["team", ""] {
        w.settings
            .update(&Actor::LocalOperator, json!({ "default_sandbox": ok }))
            .await
            .unwrap();
    }
    assert_eq!(w.row().default_sandbox, None, "\"\" clears it");
}

#[tokio::test]
async fn the_skills_selection_only_gains_skills_of_the_bank() {
    let w = world();
    let err = w
        .settings
        .update(
            &Actor::LocalOperator,
            json!({ "skills": [{ "id": "tdd" }, { "id": "no-such-skill" }], "session_cap": 3 }),
        )
        .await
        .unwrap_err();
    assert!(
        invalid(err).contains("not a skill of the bank: no-such-skill"),
        "the refusal names the skill"
    );
    assert_eq!(
        *w.config.writes.lock().unwrap(),
        0,
        "a refusal writes nothing"
    );

    w.settings
        .update(
            &Actor::LocalOperator,
            json!({ "skills": [{ "id": "tdd" }] }),
        )
        .await
        .unwrap();
    let ids = |w: &crate::fakes::World| -> Vec<String> {
        w.row().skills.iter().map(|s| s.id.clone()).collect()
    };
    assert_eq!(ids(&w), ["tdd"]);

    // A skill already selected stays, even once the bank no longer holds it;
    // emptying the selection is never refused.
    w.config.row.lock().unwrap().skills =
        serde_json::from_value(json!([{ "id": "gone" }])).unwrap();
    w.settings
        .update(
            &Actor::LocalOperator,
            json!({ "skills": [{ "id": "gone" }, { "id": "plan" }] }),
        )
        .await
        .unwrap();
    assert_eq!(ids(&w), ["gone", "plan"]);
    w.settings
        .update(&Actor::LocalOperator, json!({ "skills": [] }))
        .await
        .unwrap();
    assert!(ids(&w).is_empty());
}

#[tokio::test]
async fn the_manager_pin_must_name_an_agentic_profile() {
    let w = world();
    let err = w
        .settings
        .update(&Actor::LocalOperator, json!({ "manager_profile": "ghost" }))
        .await
        .unwrap_err();
    let message = invalid(err);
    assert!(
        message.contains("unknown manager profile `ghost`"),
        "{message}"
    );
    assert!(message.ends_with("Known profiles: Default"), "{message}");
    assert_eq!(w.row().manager_profile, None);

    // Case-insensitively, and `""` goes back to « Follow the Run ».
    w.settings
        .update(
            &Actor::LocalOperator,
            json!({ "manager_profile": "default" }),
        )
        .await
        .unwrap();
    assert_eq!(w.row().manager_profile.as_deref(), Some("default"));
    w.settings
        .update(&Actor::LocalOperator, json!({ "manager_profile": "" }))
        .await
        .unwrap();
    assert_eq!(w.row().manager_profile, None);
}

#[tokio::test]
async fn the_default_harness_must_resolve() {
    let w = world();
    let err = w
        .settings
        .update(&Actor::LocalOperator, json!({ "default_harness": "ghost" }))
        .await
        .unwrap_err();
    assert!(invalid(err).contains("unknown default harness `ghost`"));
    w.settings
        .update(
            &Actor::LocalOperator,
            json!({ "default_harness": "opencode" }),
        )
        .await
        .unwrap();
    assert_eq!(w.row().default_harness.as_deref(), Some("opencode"));
}

#[tokio::test]
async fn re_enabling_the_update_check_runs_one_now_and_disabling_does_not() {
    let w = world();
    w.settings
        .update(&Actor::LocalOperator, json!({ "update_check": false }))
        .await
        .unwrap();
    assert_eq!(*w.checks.now.lock().unwrap(), 0);
    w.settings
        .update(&Actor::LocalOperator, json!({ "update_check": true }))
        .await
        .unwrap();
    assert_eq!(*w.checks.now.lock().unwrap(), 1);
    assert_eq!(w.row().update_check, Some(1));
}

#[tokio::test]
async fn an_unknown_field_is_ignored_and_an_empty_edit_writes_nothing() {
    let w = world();
    w.settings
        .update(&Actor::LocalOperator, json!({ "no_such_knob": 1 }))
        .await
        .unwrap();
    assert_eq!(*w.config.writes.lock().unwrap(), 0);
}

#[tokio::test]
async fn an_edit_of_the_telemetry_export_reloads_it_and_no_other_edit_does() {
    // ADR-0090: switching the export or changing its collector takes effect
    // without a restart.
    let w = world();
    w.settings
        .update(&Actor::LocalOperator, json!({ "session_cap": 3 }))
        .await
        .unwrap();
    assert_eq!(*w.checks.telemetry_reloads.lock().unwrap(), 0);
    w.settings
        .update(
            &Actor::LocalOperator,
            json!({ "telemetry_otlp": true, "telemetry_otlp_endpoint": "http://127.0.0.1:4318" }),
        )
        .await
        .unwrap();
    assert_eq!(*w.checks.telemetry_reloads.lock().unwrap(), 1);
    assert_eq!(w.row().telemetry_otlp, Some(1));
    w.settings
        .update(&Actor::LocalOperator, json!({ "telemetry_otlp": false }))
        .await
        .unwrap();
    assert_eq!(*w.checks.telemetry_reloads.lock().unwrap(), 2);
}

#[tokio::test]
async fn a_refused_telemetry_edit_reloads_nothing() {
    let w = world();
    let refusal = w
        .settings
        .update(
            &Actor::LocalOperator,
            json!({ "telemetry_otlp_endpoint": "ftp://collector" }),
        )
        .await
        .unwrap_err();
    assert!(invalid(refusal).contains("telemetry_otlp_endpoint"));
    assert_eq!(*w.checks.telemetry_reloads.lock().unwrap(), 0);
}
