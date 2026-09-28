//! The heartbeat's tick: the Trigger is the Acteur of a cron fire; deciding
//! that it is due asks no one.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_trigger::TriggersOptions;

use crate::fakes::{world, world_with, World};

fn make_due(w: &World, id: &str) {
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|t| t.id == id)
        .for_each(|t| t.next_fire_at = Some("2020-01-01T00:00:00.000Z".into()));
}

#[tokio::test]
async fn a_due_trigger_fires_as_itself_and_its_schedule_moves_forward() {
    let w = world();
    let id = w.create_standard().await;
    make_due(&w, &id);
    w.host.asked.lock().unwrap().clear();

    w.triggers.tick().await;

    assert_eq!(
        w.host.asked.lock().unwrap().clone(),
        vec![(
            Actor::Trigger {
                trigger_id: id.clone()
            },
            Operation::new(ResourceKind::Trigger, Verb::Action("fire".into())).on(id.clone())
        )],
        "one question per fire, asked as the Trigger"
    );
    assert_eq!(w.runs.started.lock().unwrap().len(), 1);
    let fire = &w.store.fires_of(&id)[0];
    assert_eq!(fire.outcome, "fired");
    assert_eq!(fire.source.as_deref(), Some("cron"));
    let next = w.store.one(&id).unwrap().next_fire_at.unwrap();
    assert!(
        next.as_str() > pdo_kernel::clock::now_iso().as_str(),
        "{next}"
    );
    assert!(w.triggers.last_tick_ms() > 0, "the liveness advanced");
}

#[tokio::test]
async fn a_trigger_that_is_not_due_is_neither_asked_about_nor_fired() {
    let w = world();
    w.create_standard().await;
    w.host.asked.lock().unwrap().clear();
    w.triggers.tick().await;
    assert!(w.host.asked.lock().unwrap().is_empty());
    assert!(w.runs.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_cron_fire_spends_the_slot_and_records_why() {
    let w = world();
    let id = w.create_standard().await;
    make_due(&w, &id);
    w.refuse();

    w.triggers.tick().await;

    assert!(w.runs.started.lock().unwrap().is_empty(), "no Run");
    assert!(w.guard.ran.lock().unwrap().is_empty(), "no guard");
    let fire = &w.store.fires_of(&id)[0];
    assert_eq!(fire.outcome, "error");
    assert_eq!(
        fire.reason.as_deref(),
        Some("operation refused: the test host refuses everything")
    );
    let next = w.store.one(&id).unwrap().next_fire_at.unwrap();
    assert!(
        next.as_str() > "2020-01-01T00:00:00.000Z",
        "the slot is spent"
    );
}

#[tokio::test]
async fn while_paused_nothing_fires_and_the_schedule_stays_frozen() {
    let w = world();
    let id = w.create_standard().await;
    make_due(&w, &id);
    *w.settings.paused.lock().unwrap() = true;

    w.triggers.tick().await;

    assert!(w.runs.started.lock().unwrap().is_empty());
    assert_eq!(
        w.store.one(&id).unwrap().next_fire_at.as_deref(),
        Some("2020-01-01T00:00:00.000Z"),
        "frozen, for the one-shot catch-up on unpause"
    );
    assert!(w.triggers.last_tick_ms() > 0, "paused is not dead");
}

#[tokio::test]
async fn a_dangling_trigger_goes_dormant_with_an_error_record() {
    let w = world();
    let id = w.create_standard().await;
    make_due(&w, &id);
    w.pipelines.known.lock().unwrap().clear();

    w.triggers.tick().await;

    let fire = &w.store.fires_of(&id)[0];
    assert_eq!(fire.outcome, "error");
    assert_eq!(
        fire.reason.as_deref(),
        Some("pipeline not found: watch-pipe")
    );
    assert_eq!(w.store.one(&id).unwrap().next_fire_at, None);
    assert!(w.guard.ran.lock().unwrap().is_empty());
}

#[tokio::test]
#[should_panic(expected = "PDO_DEBUG_PANIC_TRIGGER")]
async fn the_fault_injection_panics_on_the_named_trigger() {
    let w = world_with(TriggersOptions {
        repo_root: "/daemon/root".into(),
        panic_on_trigger_name: Some("T".into()),
    });
    let id = w.create_standard().await;
    make_due(&w, &id);
    w.triggers.tick().await;
}
