//! The declared wait: `pdo wait-user`, the daemon's own on a refused
//! completion, and the user's Enter that lifts it.

use std::sync::Arc;

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::declared_wait::{
    DeclareWait, DeclareWaitCommand, DeclareWaitError, LiftWait, TypedInput,
};
use pdo_session::domain::declared_wait::{
    AwaitingMarker, DeclareOutcome, IterationFacts, NodeWaitFacts, WaitFacts, WaitRefusal,
    CAUSE_COMPLETION_NOT_RELEASED, COMPLETION_NOT_RELEASED_MESSAGE,
};
use pdo_session::domain::names::{manager_session_name, node_session_name};

use crate::fakes::{FakeTmux, Host, Waits};

const RUN: &str = "20260912-100000-a11ce01";

fn running(awaiting: Option<AwaitingMarker>) -> WaitFacts {
    WaitFacts {
        run_live: true,
        defined: true,
        is_script: false,
        node: Some(NodeWaitFacts {
            iter: 1,
            awaiting: awaiting.is_some(),
            marker: awaiting,
            iterations: vec![IterationFacts {
                iter: 1,
                holds_session: true,
            }],
        }),
    }
}

fn command(message: Option<&str>) -> DeclareWaitCommand {
    DeclareWaitCommand {
        run_id: RUN.into(),
        node_id: "griller".into(),
        iter: 1,
        message: message.map(str::to_string),
    }
}

struct World {
    waits: Arc<Waits>,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    typed: Arc<TypedInput>,
}

impl World {
    fn new(facts: WaitFacts, host: Arc<Host>) -> Self {
        Self {
            waits: Waits::with(RUN, "griller", facts),
            tmux: FakeTmux::with_sessions(&[&node_session_name(RUN, "griller", 1)]),
            host,
            typed: Arc::new(TypedInput::default()),
        }
    }

    fn declare(&self) -> DeclareWait {
        DeclareWait::new(
            self.waits.clone(),
            self.tmux.clone(),
            self.typed.clone(),
            self.host.clone(),
        )
    }

    fn lift(&self) -> LiftWait {
        LiftWait::new(self.waits.clone(), self.typed.clone(), self.host.clone())
    }
}

#[tokio::test]
async fn a_declaration_is_authorized_on_the_node_then_recorded_with_its_message() {
    let world = World::new(running(None), Host::allowing());
    let outcome = world
        .declare()
        .execute(&Actor::LocalOperator, command(Some("  Which\nlayout? ")))
        .await;
    assert_eq!(outcome, Ok(DeclareOutcome::Declared));
    let asked = world.host.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].resource, ResourceKind::Node);
    assert_eq!(asked[0].verb, Verb::Action("wait_user".into()));
    assert_eq!(
        asked[0].target.as_deref(),
        Some(format!("{RUN}/griller").as_str())
    );
    let awaiting = world.waits.awaiting.lock().unwrap().clone();
    assert_eq!(
        awaiting,
        vec![(
            RUN.to_string(),
            "griller".to_string(),
            1,
            serde_json::json!({ "cause": "declared", "message": "Which layout?" })
        )]
    );
}

#[tokio::test]
async fn a_refused_operation_writes_nothing() {
    let world = World::new(running(None), Host::refusing());
    let outcome = world
        .declare()
        .execute(&Actor::LocalOperator, command(Some("q?")))
        .await;
    assert!(matches!(outcome, Err(DeclareWaitError::Refused(d)) if d.reason == "admins only"));
    assert!(world.waits.awaiting.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_repeat_is_a_noop_and_a_new_message_refreshes() {
    let marker = AwaitingMarker {
        cause: "declared".into(),
        message: Some("q?".into()),
    };
    let world = World::new(running(Some(marker)), Host::allowing());
    let declare = world.declare();
    assert_eq!(
        declare
            .execute(&Actor::LocalOperator, command(Some("q?")))
            .await,
        Ok(DeclareOutcome::Noop)
    );
    assert!(world.waits.awaiting.lock().unwrap().is_empty());
    assert_eq!(
        declare
            .execute(&Actor::LocalOperator, command(Some("another?")))
            .await,
        Ok(DeclareOutcome::Refreshed)
    );
}

/// The projection still says `running`, tmux says the pane is gone (FP #793,
/// finding 3): the refusal is `node_session_not_live`, nothing is written.
#[tokio::test]
async fn a_node_whose_pane_died_cannot_wait() {
    let world = World::new(running(None), Host::allowing());
    world.tmux.sessions.lock().unwrap().clear();
    assert_eq!(
        world
            .declare()
            .execute(&Actor::LocalOperator, command(Some("anyone there?")))
            .await,
        Err(DeclareWaitError::Wait(WaitRefusal::NodeSessionNotLive))
    );
    assert!(world.waits.awaiting.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unreadable_or_unknown_run_is_not_found() {
    let world = World::new(running(None), Host::allowing());
    *world.waits.unreadable.lock().unwrap() = true;
    assert_eq!(
        world
            .declare()
            .execute(&Actor::LocalOperator, command(None))
            .await,
        Err(DeclareWaitError::Wait(WaitRefusal::RunNotFound))
    );
    let world = World::new(running(None), Host::allowing());
    let mut other = command(None);
    other.run_id = "ghost".into();
    assert_eq!(
        world.declare().execute(&Actor::LocalOperator, other).await,
        Err(DeclareWaitError::Wait(WaitRefusal::RunNotFound))
    );
}

#[tokio::test]
async fn a_write_the_run_refuses_reads_as_a_run_that_is_not_live() {
    let world = World::new(running(None), Host::allowing());
    *world.waits.refuse_writes.lock().unwrap() = true;
    assert_eq!(
        world
            .declare()
            .execute(&Actor::LocalOperator, command(None))
            .await,
        Err(DeclareWaitError::Wait(WaitRefusal::RunNotLive))
    );
}

/// The daemon's own declaration is an internal transition: never authorized,
/// and it carries its own sentence, whole.
#[tokio::test]
async fn the_daemon_declares_a_refused_completion_without_asking_the_extensions() {
    let world = World::new(running(None), Host::refusing());
    world
        .declare()
        .on_completion_not_released(RUN, "griller", 1)
        .await;
    assert!(world.host.asked().is_empty());
    let awaiting = world.waits.awaiting.lock().unwrap().clone();
    assert_eq!(awaiting[0].3["cause"], CAUSE_COMPLETION_NOT_RELEASED);
    assert_eq!(awaiting[0].3["message"], COMPLETION_NOT_RELEASED_MESSAGE);
}

#[tokio::test]
async fn only_an_enter_after_typed_bytes_lifts_the_wait() {
    let marker = AwaitingMarker {
        cause: "declared".into(),
        message: None,
    };
    let world = World::new(running(Some(marker)), Host::allowing());
    let lift = world.lift();
    let session = node_session_name(RUN, "griller", 1);
    let actor = Actor::LocalOperator;

    // An Enter on an empty line (a click to focus the pane): nothing.
    lift.note_input(&actor, &session, b"\r").await;
    // Arrows, scroll, mouse: nothing, and they do not arm the rule either.
    lift.note_input(&actor, &session, b"\x1b[A\x1b[<65;3;4M")
        .await;
    lift.note_input(&actor, &session, b"\r").await;
    assert!(world.waits.resumed.lock().unwrap().is_empty());

    // The manager's session never lifts a node's wait.
    lift.note_input(&actor, &manager_session_name(RUN), b"hello\r")
        .await;
    assert!(world.waits.resumed.lock().unwrap().is_empty());

    lift.note_input(&actor, &session, b"the strip").await;
    lift.note_input(&actor, &session, b"\r").await;
    let resumed = world.waits.resumed.lock().unwrap().clone();
    assert_eq!(
        resumed,
        vec![(
            RUN.to_string(),
            "griller".to_string(),
            1,
            serde_json::json!({ "source": "pty_enter" })
        )]
    );
    let asked = world.host.asked();
    assert_eq!(asked.last().unwrap().verb, Verb::Action("resume".into()));
}

#[tokio::test]
async fn typed_bytes_before_a_declaration_do_not_count() {
    let world = World::new(running(None), Host::allowing());
    let session = node_session_name(RUN, "griller", 1);
    world
        .lift()
        .note_input(&Actor::LocalOperator, &session, b"typed before")
        .await;
    world
        .declare()
        .execute(&Actor::LocalOperator, command(Some("q?")))
        .await
        .unwrap();
    *world.waits.facts.lock().unwrap() = [(
        (RUN.to_string(), "griller".to_string()),
        running(Some(AwaitingMarker {
            cause: "declared".into(),
            message: Some("q?".into()),
        })),
    )]
    .into();
    world
        .lift()
        .note_input(&Actor::LocalOperator, &session, b"\r")
        .await;
    assert!(world.waits.resumed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_children_pending_marker_is_never_lifted_by_a_keystroke() {
    let marker = AwaitingMarker {
        cause: "children_pending".into(),
        message: None,
    };
    let world = World::new(running(Some(marker)), Host::allowing());
    let session = node_session_name(RUN, "griller", 1);
    world
        .lift()
        .note_input(&Actor::LocalOperator, &session, b"hi\r")
        .await;
    assert!(world.waits.resumed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_resume_lifts_nothing() {
    let marker = AwaitingMarker {
        cause: "declared".into(),
        message: None,
    };
    let world = World::new(running(Some(marker)), Host::refusing());
    let lifted = world
        .lift()
        .lift(&Actor::LocalOperator, RUN, "griller", 1)
        .await;
    assert!(lifted.unwrap_err().contains("admins only"));
    assert!(world.waits.resumed.lock().unwrap().is_empty());
}
