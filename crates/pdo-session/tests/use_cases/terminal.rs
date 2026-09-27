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

#[test]
fn attaching_a_terminal_is_an_operation_on_the_session() {
    let host = Host::allowing();
    let t = terminals(host.clone());
    assert!(t
        .authorize_attach(&Actor::LocalOperator, "pdo-mgr-r1")
        .is_ok());
    let asked = host.asked();
    assert_eq!(asked[0].verb, Verb::Action("attach".into()));
    assert_eq!(asked[0].target.as_deref(), Some("pdo-mgr-r1"));
    assert!(terminals(Host::refusing())
        .authorize_attach(&Actor::LocalOperator, "pdo-mgr-r1")
        .is_err());
}

#[test]
fn a_spectator_takes_control_unless_it_is_refused() {
    for (host, expected) in [(Host::allowing(), true), (Host::refusing(), false)] {
        let t = terminals(host);
        let (pilot_tx, _pilot_rx) = mpsc::unbounded_channel();
        let (spectator_tx, _spectator_rx) = mpsc::unbounded_channel();
        let _pilot = t.registry().join("s", Some("A".into()), pilot_tx);
        let spectator = t.registry().join("s", Some("B".into()), spectator_tx);
        assert_eq!(
            t.take_control(&Actor::LocalOperator, "s", spectator.id),
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

#[test]
fn the_bridge_attaches_through_the_tmux_port() {
    let t = terminals(Host::allowing());
    assert_eq!(
        t.attach_argv("s", true),
        ["tmux", "attach", "-t", "s", "ignore-size"]
    );
}
