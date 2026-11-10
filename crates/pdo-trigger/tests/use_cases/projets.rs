//! A Trigger is in the Projet it launches its Runs in (ADR-0087): its
//! Operations name it, its reads are asked there, and what it announces is
//! guarded by its read in it. Its Projet holds its repository.

use pdo_kernel::event_bus::READ_GUARD;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::TriggerError;
use serde_json::json;

use crate::fakes::{request, world, World, REPO};

/// The second repository, held by P2; [`REPO`] is held by P1 and P2.
const OTHER: &str = "/repos/other";

fn projets(w: &World) {
    let mut members = w.projets.members.lock().unwrap();
    for (path, project) in [(REPO, "p1"), (REPO, "p2"), (OTHER, "p2")] {
        members.push((path.into(), project.into()));
    }
}

/// A Trigger working in `repo`, launching in `project`: its id.
async fn trigger_in(w: &World, repo: &str, project: &str) -> String {
    w.create(request(
        json!({ "target_repo": repo, "project_id": project }),
    ))
    .await
}

/// The Projets each Operation on a Trigger asked was in, with its verb.
fn asked(w: &World) -> Vec<(Verb, Option<String>, Vec<String>)> {
    w.host
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, op)| op.resource == ResourceKind::Trigger)
        .map(|(_, op)| (op.verb.clone(), op.target.clone(), op.projects.clone()))
        .collect()
}

#[tokio::test]
async fn every_operation_on_a_trigger_is_in_its_projet() {
    let w = world();
    projets(&w);
    let t1 = trigger_in(&w, REPO, "p1").await;
    let t2 = trigger_in(&w, OTHER, "p2").await;
    w.triggers
        .update(
            &Actor::LocalOperator,
            ActorHint::Ui,
            &t1,
            serde_json::from_value(json!({ "name": "renamed" })).unwrap(),
        )
        .await
        .unwrap();
    w.triggers
        .run_now(&Actor::LocalOperator, &t2)
        .await
        .unwrap();
    w.triggers
        .delete(&Actor::LocalOperator, ActorHint::Ui, &t2)
        .await
        .unwrap();

    let fire = Verb::Action("fire".into());
    let p = |id: &str| vec![id.to_string()];
    assert_eq!(
        asked(&w),
        vec![
            (Verb::Create, None, p("p1")),
            (Verb::Create, None, p("p2")),
            (Verb::Update, Some(t1.clone()), p("p1")),
            (fire, Some(t2.clone()), p("p2")),
            (Verb::Delete, Some(t2.clone()), p("p2")),
        ]
    );
}

#[tokio::test]
async fn a_trigger_launching_in_no_projet_is_in_none() {
    let w = world();
    projets(&w);
    // Its repository is in two Projets: the Trigger is in neither.
    let outside = w.create(request(json!({}))).await;
    w.triggers
        .delete(&Actor::LocalOperator, ActorHint::Ui, &outside)
        .await
        .unwrap();
    let projects: Vec<_> = asked(&w)
        .into_iter()
        .map(|(_, _, projects)| projects)
        .collect();
    assert_eq!(projects, vec![Vec::<String>::new(), Vec::new()]);
}

#[tokio::test]
async fn a_trigger_names_an_existing_projet_holding_its_repository() {
    let w = world();
    projets(&w);
    let refusal = |result: Result<_, TriggerError>| match result {
        Err(TriggerError::Invalid(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };
    let unknown = w
        .triggers
        .create(
            &Actor::LocalOperator,
            ActorHint::Ui,
            request(json!({ "project_id": "nope" })),
        )
        .await;
    assert!(refusal(unknown).contains("unknown project"));
    let outside = w
        .triggers
        .create(
            &Actor::LocalOperator,
            ActorHint::Ui,
            request(json!({ "target_repo": OTHER, "project_id": "p1" })),
        )
        .await;
    assert!(refusal(outside).contains("not a member"));
    assert!(w.store.all().is_empty(), "nothing stored");

    // An edit is checked against the resulting Projet and repository.
    let t1 = trigger_in(&w, REPO, "p1").await;
    let moved = w
        .triggers
        .update(
            &Actor::LocalOperator,
            ActorHint::Ui,
            &t1,
            serde_json::from_value(json!({ "target_repo": OTHER })).unwrap(),
        )
        .await;
    assert!(refusal(moved).contains("not a member"));
}

#[tokio::test]
async fn moving_a_trigger_to_another_projet_is_an_update_there_too() {
    let mut w = world();
    projets(&w);
    let t1 = trigger_in(&w, REPO, "p1").await;
    w.announced();
    w.triggers
        .update(
            &Actor::LocalOperator,
            ActorHint::Ui,
            &t1,
            serde_json::from_value(json!({ "project_id": "p2" })).unwrap(),
        )
        .await
        .unwrap();
    let updates: Vec<_> = asked(&w)
        .into_iter()
        .filter(|(verb, _, _)| *verb == Verb::Update)
        .map(|(_, _, projects)| projects)
        .collect();
    assert_eq!(
        updates,
        vec![vec!["p1".to_string()], vec!["p2".to_string()]]
    );
    // Told where it is now, and where it was: a reader of P1 drops it.
    let told: Vec<_> = w
        .announced()
        .into_iter()
        .map(|m| m[READ_GUARD]["projects"].clone())
        .collect();
    assert_eq!(told, vec![json!(["p2"]), json!(["p1"])]);
}

#[tokio::test]
async fn a_refused_read_hides_the_trigger_and_its_fires() {
    let w = world();
    projets(&w);
    let t1 = trigger_in(&w, REPO, "p1").await;
    let t2 = trigger_in(&w, OTHER, "p2").await;
    *w.host.refuse_reads_in.lock().unwrap() = Some("p2".into());

    let listed: Vec<String> = w
        .triggers
        .list(&Actor::LocalOperator, Default::default())
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.trigger.id)
        .collect();
    assert_eq!(listed, vec![t1.clone()], "the Trigger of P2 is hidden");
    assert!(w
        .triggers
        .attributed(&Actor::LocalOperator, &t1)
        .await
        .is_ok());
    for refused in [
        w.triggers
            .attributed(&Actor::LocalOperator, &t2)
            .await
            .map(|_| ()),
        w.triggers
            .fires(&Actor::LocalOperator, &t2)
            .await
            .map(|_| ()),
    ] {
        let Err(TriggerError::Refused(denied)) = refused else {
            panic!("refused: {refused:?}");
        };
        assert_eq!(denied.reason, "the test host refuses this read");
    }
    let reads: Vec<_> = asked(&w)
        .into_iter()
        .filter(|(verb, target, _)| *verb == Verb::Read && target.as_deref() == Some(&t2))
        .map(|(_, _, projects)| projects)
        .collect();
    assert!(reads.iter().all(|projects| projects == &["p2".to_string()]));
}

#[tokio::test]
async fn what_a_trigger_announces_is_guarded_by_its_read_in_its_projets() {
    let mut w = world();
    projets(&w);
    let t2 = trigger_in(&w, OTHER, "p2").await;
    w.triggers
        .run_now(&Actor::LocalOperator, &t2)
        .await
        .unwrap();
    w.triggers
        .delete(&Actor::LocalOperator, ActorHint::Ui, &t2)
        .await
        .unwrap();

    let read = json!({ "resource": "trigger", "verb": "read", "target": t2, "projects": ["p2"] });
    let announced = w.announced();
    let kinds: Vec<&str> = announced
        .iter()
        .map(|m| m["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec!["trigger_created", "trigger_fired", "trigger_deleted"]
    );
    for message in &announced {
        assert_eq!(message[READ_GUARD], read, "{message}");
    }
}
