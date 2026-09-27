//! Reaping a node's session on its terminal transition.

use pdo_session::application::reap::snapshot_and_kill;
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
