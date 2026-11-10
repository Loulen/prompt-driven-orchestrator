//! The terminals the PTY bridge pilots.

use std::sync::Arc;

use pdo_kernel::{Actor, Verb};
use pdo_session::application::declared_wait::{LiftWait, TypedInput};
use pdo_session::application::terminal::Terminals;
use tokio::sync::mpsc;

use crate::fakes::{FakeTmux, Host, Waits};

fn terminals(host: Arc<Host>) -> Terminals {
    let waits = Arc::new(Waits::default());
    Terminals::new(
        Arc::new(FakeTmux::default()),
        LiftWait::new(waits, Arc::new(TypedInput::default()), host.clone()),
        host,
    )
}

#[tokio::test]
async fn attaching_a_terminal_is_an_operation_on_the_session() {
    let host = Host::allowing();
    let t = terminals(host.clone());
    assert!(t
        .authorize_attach(&Actor::LocalOperator, "pdo-mgr-r1")
        .await
        .is_ok());
    let asked = host.asked();
    assert_eq!(asked[0].verb, Verb::Action("attach".into()));
    assert_eq!(asked[0].target.as_deref(), Some("pdo-mgr-r1"));
    assert!(terminals(Host::refusing())
        .authorize_attach(&Actor::LocalOperator, "pdo-mgr-r1")
        .await
        .is_err());
}

#[tokio::test]
async fn a_spectator_takes_control_unless_it_is_refused() {
    for (host, expected) in [(Host::allowing(), true), (Host::refusing(), false)] {
        let t = terminals(host);
        let (pilot_tx, _pilot_rx) = mpsc::unbounded_channel();
        let (spectator_tx, _spectator_rx) = mpsc::unbounded_channel();
        let _pilot = t
            .registry()
            .join("s", Some("A".into()), Actor::LocalOperator, pilot_tx);
        let spectator =
            t.registry()
                .join("s", Some("B".into()), Actor::LocalOperator, spectator_tx);
        assert_eq!(
            t.take_control(&Actor::LocalOperator, "s", spectator.id)
                .await,
            expected
        );
        assert_eq!(
            spectator
                .spectator
                .load(std::sync::atomic::Ordering::SeqCst),
            !expected
        );
    }
}

/// CORE-61: a take-over that changed the pilot is announced with who took
/// it; a refused one, or one without effect (the pilot's own), is not.
#[tokio::test]
async fn a_take_over_is_announced_only_when_it_changes_the_pilot() {
    const SESSION: &str = "pdo-20260506-143000-a3f1b2c-worker-iter-1";
    let bob = Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    };
    for (host, announced) in [(Host::allowing(), 1), (Host::refusing(), 0)] {
        let t = terminals(host.clone());
        let (pilot_tx, _pilot_rx) = mpsc::unbounded_channel();
        let (spectator_tx, _spectator_rx) = mpsc::unbounded_channel();
        let pilot = t
            .registry()
            .join(SESSION, Some("A".into()), Actor::LocalOperator, pilot_tx);
        let spectator = t.registry().join(
            SESSION,
            Some("B".into()),
            Actor::LocalOperator,
            spectator_tx,
        );
        t.take_control(&Actor::LocalOperator, SESSION, pilot.id)
            .await;
        t.take_control(&bob, SESSION, spectator.id).await;
        let events = host.events();
        assert_eq!(events.len(), announced);
        if announced == 1 {
            assert!(events[0].0, "a session of a Run goes on the bus");
            assert_eq!(events[0].1["type"], "session_taken_over");
            assert_eq!(events[0].1["actor"]["id"], "bob");
            assert_eq!(events[0].1["session"]["id"], SESSION);
            assert_eq!(events[0].1["session"]["node_id"], "worker");
            assert_eq!(events[0].1["session"]["run_id"], "20260506-143000-a3f1b2c");
        }
    }
}

#[tokio::test]
async fn the_first_to_drive_a_terminal_is_announced() {
    let host = Host::allowing();
    let t = terminals(host.clone());
    t.joined_as_driver(&Actor::LocalOperator, "pdo-mgr-20260506-143000-a3f1b2c")
        .await;
    let events = host.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["type"], "session_taken_over");
    assert_eq!(events[0].1["session"]["kind"], "manager");
    t.joined_as_driver(&Actor::LocalOperator, "not-a-pdo-session")
        .await;
    assert_eq!(
        host.events().len(),
        1,
        "a name PDO does not give is ignored"
    );
}

#[test]
fn the_bridge_attaches_through_the_tmux_port() {
    let t = terminals(Host::allowing());
    assert_eq!(
        t.attach_argv("s", true),
        ["tmux", "attach", "-t", "s", "ignore-size"]
    );
}
