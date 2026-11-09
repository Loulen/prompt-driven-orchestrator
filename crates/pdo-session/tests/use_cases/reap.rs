//! Reaping a node's session on its terminal transition.

use pdo_session::application::reap::{snapshot_and_kill, snapshot_pane};
use pdo_session::domain::names::node_session_name;
use pdo_session::domain::pane::pane_snapshot_path;

use crate::fakes::FakeTmux;

#[test]
fn the_pane_is_frozen_before_the_session_is_killed() {
    let root = tempfile::tempdir().unwrap();
    let session = node_session_name("r1", "worker", 2);
    let tmux = FakeTmux::with_sessions(&[&session]);
    assert_eq!(
        snapshot_and_kill(&*tmux, root.path(), "r1", "worker", 2),
        session
    );
    assert_eq!(
        std::fs::read_to_string(pane_snapshot_path(root.path(), "r1", "worker", 2)).unwrap(),
        format!("pane of {session}")
    );
    assert!(!tmux.has(&session));
}

#[test]
fn a_session_already_gone_is_still_killed_and_leaves_no_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let tmux = FakeTmux::with_sessions(&[]);
    let session = snapshot_and_kill(&*tmux, root.path(), "r1", "worker", 1);
    assert_eq!(tmux.killed.lock().unwrap().as_slice(), [session]);
    assert!(!pane_snapshot_path(root.path(), "r1", "worker", 1).exists());
}

#[test]
fn a_pane_frozen_before_its_session_exits_survives_the_reap() {
    // A script's shell exits as soon as `pdo complete` answers (CORE-232): the pane
    // frozen before the answer is what the reap of a session already gone keeps.
    let root = tempfile::tempdir().unwrap();
    let session = node_session_name("r1", "worker", 1);
    let tmux = FakeTmux::with_sessions(&[&session]);
    assert!(snapshot_pane(&*tmux, root.path(), "r1", "worker", 1));
    tmux.sessions.lock().unwrap().remove(&session);
    snapshot_and_kill(&*tmux, root.path(), "r1", "worker", 1);
    assert_eq!(
        std::fs::read_to_string(pane_snapshot_path(root.path(), "r1", "worker", 1)).unwrap(),
        format!("pane of {session}")
    );
}

#[test]
fn no_session_no_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let tmux = FakeTmux::with_sessions(&[]);
    assert!(!snapshot_pane(&*tmux, root.path(), "r1", "worker", 1));
    assert!(!pane_snapshot_path(root.path(), "r1", "worker", 1).exists());
}
