use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_settings::application::agent_profiles::ProfileRequest;
use pdo_settings::application::SettingsError;
use pdo_settings::domain::agent_profile::DEFAULT_PROFILE_ID;

use crate::fakes::world;

fn request(name: &str, harness: &str) -> ProfileRequest {
    ProfileRequest {
        name: name.into(),
        harness: harness.into(),
        model: None,
        effort: None,
    }
}

fn op(verb: Verb, id: Option<&str>) -> (Actor, Operation) {
    let operation = Operation::new(ResourceKind::AgentProfile, verb);
    let operation = match id {
        Some(id) => operation.on(id),
        None => operation,
    };
    (Actor::LocalOperator, operation)
}

#[tokio::test]
async fn a_create_asks_first_then_stores_the_normalised_profile() {
    let w = world();
    let created = w
        .settings
        .create_agent_profile(
            &Actor::LocalOperator,
            ProfileRequest {
                name: " Fast Reviewer ".into(),
                harness: "claude".into(),
                model: Some("opus".into()),
                effort: Some("  ".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(created.name, "Fast Reviewer");
    assert_eq!(created.model.as_deref(), Some("opus"));
    assert_eq!(created.effort, None, "a blank effort is unset");
    assert_eq!(w.asked(), vec![op(Verb::Create, None)]);
}

#[tokio::test]
async fn a_refused_create_stores_nothing() {
    let w = world();
    w.refuse();
    let err = w
        .settings
        .create_agent_profile(&Actor::LocalOperator, request("X", "claude"))
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Refused(_)));
    assert_eq!(w.profiles.ids(), vec![DEFAULT_PROFILE_ID.to_string()]);
}

#[tokio::test]
async fn names_are_unique_case_insensitively_and_the_refusal_names_the_clash() {
    let w = world();
    w.settings
        .create_agent_profile(&Actor::LocalOperator, request("Reviewer", "claude"))
        .await
        .unwrap();
    for name in ["REVIEWER", "default"] {
        let err = w
            .settings
            .create_agent_profile(&Actor::LocalOperator, request(name, "opencode"))
            .await
            .unwrap_err();
        match err {
            SettingsError::Conflict(message) => assert!(message.contains("already exists")),
            other => panic!("expected a 409, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_blank_name_or_harness_is_a_400() {
    let w = world();
    for (name, harness) in [("  ", "claude"), ("X", " ")] {
        let err = w
            .settings
            .create_agent_profile(&Actor::LocalOperator, request(name, harness))
            .await
            .unwrap_err();
        assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");
    }
}

#[tokio::test]
async fn an_edit_asks_about_the_profile_and_keeps_its_id() {
    let w = world();
    let a = w
        .settings
        .create_agent_profile(&Actor::LocalOperator, request("Alpha", "claude"))
        .await
        .unwrap();
    // Its own name, case changed, is no clash.
    let edited = w
        .settings
        .update_agent_profile(&Actor::LocalOperator, &a.id, request("ALPHA", "opencode"))
        .await
        .unwrap();
    assert_eq!(
        (edited.id.as_str(), edited.name.as_str()),
        (a.id.as_str(), "ALPHA")
    );
    assert_eq!(edited.harness, "opencode");
    assert_eq!(w.asked().last().unwrap(), &op(Verb::Update, Some(&a.id)));
}

#[tokio::test]
async fn the_reserved_default_is_editable_and_renamable() {
    let w = world();
    let edited = w
        .settings
        .update_agent_profile(
            &Actor::LocalOperator,
            DEFAULT_PROFILE_ID,
            request("House Default", "opencode"),
        )
        .await
        .unwrap();
    assert_eq!(edited.id, DEFAULT_PROFILE_ID);
    assert_eq!(edited.name, "House Default");
}

#[tokio::test]
async fn renaming_onto_another_profile_is_a_409_and_an_unknown_id_a_404() {
    let w = world();
    let a = w
        .settings
        .create_agent_profile(&Actor::LocalOperator, request("Alpha", "claude"))
        .await
        .unwrap();
    w.settings
        .create_agent_profile(&Actor::LocalOperator, request("Bravo", "claude"))
        .await
        .unwrap();
    let err = w
        .settings
        .update_agent_profile(&Actor::LocalOperator, &a.id, request("bravo", "claude"))
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Conflict(_)), "{err:?}");
    let err = w
        .settings
        .update_agent_profile(&Actor::LocalOperator, "agp-nope", request("X", "claude"))
        .await
        .unwrap_err();
    assert_eq!(err, SettingsError::NotFound("no such agent profile".into()));
}

#[tokio::test]
async fn a_refused_edit_leaves_the_profile_alone() {
    let w = world();
    w.refuse();
    let err = w
        .settings
        .update_agent_profile(
            &Actor::LocalOperator,
            DEFAULT_PROFILE_ID,
            request("X", "pi"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Refused(_)));
    assert!(w.profiles.named("Default").is_some());
}

#[tokio::test]
async fn a_delete_asks_about_the_profile_and_the_default_is_never_deleted() {
    let w = world();
    let temp = w
        .settings
        .create_agent_profile(&Actor::LocalOperator, request("Temp", "claude"))
        .await
        .unwrap();
    w.settings
        .delete_agent_profile(&Actor::LocalOperator, &temp.id)
        .await
        .unwrap();
    assert_eq!(w.asked().last().unwrap(), &op(Verb::Delete, Some(&temp.id)));
    assert_eq!(w.profiles.ids(), vec![DEFAULT_PROFILE_ID.to_string()]);

    let err = w
        .settings
        .delete_agent_profile(&Actor::LocalOperator, DEFAULT_PROFILE_ID)
        .await
        .unwrap_err();
    match err {
        SettingsError::Invalid(message) => assert!(message.contains("reserved")),
        other => panic!("expected a 400, got {other:?}"),
    }
    let err = w
        .settings
        .delete_agent_profile(&Actor::LocalOperator, "agp-nope")
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::NotFound(_)));
}

#[tokio::test]
async fn a_refused_delete_keeps_the_profile() {
    let w = world();
    let temp = w
        .settings
        .create_agent_profile(&Actor::LocalOperator, request("Temp", "claude"))
        .await
        .unwrap();
    w.refuse();
    let err = w
        .settings
        .delete_agent_profile(&Actor::LocalOperator, &temp.id)
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Refused(_)));
    assert!(w.profiles.named("Temp").is_some());
}

#[tokio::test]
async fn the_reads_ask_no_one() {
    let w = world();
    assert_eq!(
        w.settings
            .agent_profiles(Default::default())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.settings
            .agent_profile(DEFAULT_PROFILE_ID)
            .await
            .unwrap()
            .name,
        "Default"
    );
    assert!(matches!(
        w.settings.agent_profile("agp-nope").await.unwrap_err(),
        SettingsError::NotFound(_)
    ));
    let snapshot = w.settings.agent_profile_snapshot().await.unwrap();
    assert_eq!(snapshot[DEFAULT_PROFILE_ID].harness, "claude");
    assert!(w.asked().is_empty());
}
