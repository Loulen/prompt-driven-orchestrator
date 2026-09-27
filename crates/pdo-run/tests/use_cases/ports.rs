//! What a spawn and a completion read from, and ask of, the contexts around
//! the run context: the instance settings (the session cap, the skills tier),
//! the skill bank's delivery, the session context's declared wait.

use pdo_kernel::Actor;
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::ports::InstanceDefaults;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::domain::event::EventKind;
use pdo_skill::domain::selection::SkillRef;

use crate::fakes::*;

#[tokio::test]
async fn the_stored_session_cap_throttles_a_spawn_into_waiting() {
    let w = world();
    *w.settings.defaults.lock().unwrap() = Some(InstanceDefaults {
        session_cap: Some(1),
        ..InstanceDefaults::default()
    });
    // Another Run holds the one slot.
    seed_relay(&w, "blocker").await;
    advance_run(&w.runs, "blocker").await;
    seed_relay(&w, "r1").await;

    advance_run(&w.runs, "r1").await;

    assert_eq!(
        kinds(&w, "r1"),
        vec![EventKind::RunStarted, EventKind::NodeWaiting]
    );
    assert_eq!(
        w.tmux.opened.lock().unwrap().len(),
        1,
        "only the blocker's session"
    );
}

#[tokio::test]
async fn the_instance_skills_are_delivered_into_the_tree_the_node_works_in() {
    let w = world();
    *w.settings.defaults.lock().unwrap() = Some(InstanceDefaults {
        skills: vec![SkillRef {
            id: "tdd".into(),
            name: "TDD".into(),
        }],
        ..InstanceDefaults::default()
    });
    w.skills
        .bank
        .lock()
        .unwrap()
        .insert("tdd".into(), "TDD".into());
    seed_relay(&w, "r1").await;

    advance_run(&w.runs, "r1").await;

    let delivered = w.skills.delivered.lock().unwrap().clone();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].1, vec!["tdd".to_string()]);
    assert!(delivered[0].0.ends_with("worktree"), "{:?}", delivered[0].0);
}

#[tokio::test]
async fn an_unreleased_interactive_completion_declares_the_wait_for_the_agent() {
    let w = world();
    let interactive = RELAY.replacen(
        "    name: Worker\n    type: agent\n",
        "    name: Worker\n    type: agent\n    interactive: true\n",
        1,
    );
    seed_run_of(&w, "r1", &interactive).await;
    advance_run(&w.runs, "r1").await;
    write_out(&w, "r1", "worker");

    let answer = w
        .runs
        .complete_node(
            &Actor::LocalOperator,
            CompleteNode {
                run_id: "r1".into(),
                node_id: "worker".into(),
                iter: 1,
                auto: false,
            },
        )
        .await;

    assert_eq!(answer.status(), 409);
    assert_eq!(
        *w.waits.0.lock().unwrap(),
        vec![("r1".to_string(), "worker".to_string(), 1)]
    );
    assert!(!kinds(&w, "r1").contains(&EventKind::NodeCompleted));
}
