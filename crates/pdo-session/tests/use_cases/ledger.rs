//! The session ledger (CORE-61): who started each session, and its end,
//! observed in tmux and announced.

use std::sync::Arc;

use pdo_extension_api::{SessionKind, SessionRef};
use pdo_kernel::Actor;
use pdo_session::application::session_ledger::SessionLedger;
use pdo_session::domain::names::{manager_session_name, LIBASSIST_SESSION_NAME};

use crate::fakes::{FakeTmux, Host};

const RUN: &str = "20260506-143000-a3f1b2c";

fn bob() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    }
}

fn types(host: &Host) -> Vec<String> {
    host.events()
        .iter()
        .map(|(_, e)| e["type"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_session_gone_from_tmux_is_announced_ended_once_with_who_started_it() {
    let manager = manager_session_name(RUN);
    let tmux = FakeTmux::with_sessions(&[manager.as_str()]);
    let host = Host::allowing();
    let ledger = SessionLedger::new(tmux.clone(), host.clone());
    ledger.started_named(&manager, Some(bob()));
    assert_eq!(ledger.started_by(&manager), Some(bob()));

    ledger.reconcile();
    assert_eq!(types(&host), ["session_started"], "still alive");

    tmux.sessions.lock().unwrap().remove(&manager);
    ledger.reconcile();
    ledger.reconcile();
    assert_eq!(types(&host), ["session_started", "session_ended"]);
    let (on_the_bus, ended) = host.events()[1].clone();
    assert!(on_the_bus);
    assert_eq!(ended["actor"]["id"], "bob");
    assert_eq!(ended["session"]["kind"], "manager");
    assert_eq!(ended["session"]["run_id"], RUN);
    assert_eq!(ledger.started_by(&manager), None);
}

#[test]
fn a_session_started_again_under_its_name_ended_first() {
    let manager = manager_session_name(RUN);
    let tmux = FakeTmux::with_sessions(&[manager.as_str()]);
    let host = Host::allowing();
    let ledger = SessionLedger::new(tmux, host.clone());
    ledger.started_named(&manager, Some(bob()));
    ledger.started_named(&manager, Some(Actor::LocalOperator));
    assert_eq!(
        types(&host),
        ["session_started", "session_ended", "session_started"]
    );
    assert_eq!(host.events()[1].1["actor"]["id"], "bob");
    assert_eq!(ledger.started_by(&manager), Some(Actor::LocalOperator));
}

#[test]
fn the_end_of_a_session_outside_any_run_reaches_only_the_extensions() {
    let tmux = FakeTmux::with_sessions(&[LIBASSIST_SESSION_NAME]);
    let host = Host::allowing();
    let ledger = SessionLedger::new(tmux.clone(), host.clone());
    ledger.started(
        SessionRef::new(LIBASSIST_SESSION_NAME, SessionKind::LibraryAssistant),
        Some(Actor::LocalOperator),
    );
    tmux.sessions.lock().unwrap().clear();
    ledger.reconcile();
    let events = host.events();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|(on_the_bus, _)| !on_the_bus));
    assert_eq!(events[1].1["type"], "session_ended");
}

#[test]
fn the_sessions_a_previous_daemon_started_end_with_nobody_known() {
    let manager = manager_session_name(RUN);
    let tmux = FakeTmux::with_sessions(&[manager.as_str(), "pdo-garbage", "other"]);
    let host = Host::allowing();
    let ledger = Arc::new(SessionLedger::new(tmux.clone(), host.clone()));
    ledger.adopt_live();
    assert!(host.events().is_empty(), "adopting announces nothing");
    tmux.sessions.lock().unwrap().clear();
    ledger.reconcile();
    let events = host.events();
    assert_eq!(events.len(), 1, "only the names PDO gives");
    assert_eq!(events[0].1["type"], "session_ended");
    assert_eq!(events[0].1["actor"], serde_json::Value::Null);
}

#[test]
fn a_name_pdo_does_not_give_is_never_announced() {
    let host = Host::allowing();
    let ledger = SessionLedger::new(Arc::new(FakeTmux::default()), host.clone());
    ledger.started_named("someone-else", Some(bob()));
    ledger.taken_over(&bob(), "someone-else");
    assert!(host.events().is_empty());
}

#[test]
fn without_an_extension_the_ledger_keeps_nothing() {
    let manager = manager_session_name(RUN);
    let tmux = FakeTmux::with_sessions(&[manager.as_str()]);
    let ledger = SessionLedger::new(
        tmux.clone(),
        Arc::new(pdo_kernel::extension_host::NoExtensions),
    );
    ledger.adopt_live();
    ledger.started_named(&manager, Some(bob()));
    assert_eq!(ledger.started_by(&manager), None);
    tmux.sessions.lock().unwrap().clear();
    ledger.reconcile();
}
