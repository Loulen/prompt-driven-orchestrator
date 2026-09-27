//! A node's pane: live, from its snapshot, or re-attached in place.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::ports::RunLookup;
use pdo_session::application::read_pane::{ReadPane, ReadPaneError};
use pdo_session::domain::names::node_session_name;
use pdo_session::domain::pane::{pane_snapshot_path, PaneSource, PaneTarget, ReattachOutcome};

use crate::fakes::{FakeTmux, Host, Panes};

const RUN: &str = "20260506-143000-a3f1b2c";

fn target(root: PathBuf, latest_iter: bool, terminal: bool) -> PaneTarget {
    PaneTarget {
        latest_iter,
        terminal,
        pending: false,
        repo_root: root,
    }
}

fn read_pane(
    target: Result<Option<PaneTarget>, RunLookup>,
    reattach: ReattachOutcome,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
) -> (ReadPane, Arc<Panes>) {
    let panes = Arc::new(Panes {
        target,
        reattach,
        reattached: Mutex::new(Vec::new()),
        tmux: Some(tmux.clone()),
    });
    (ReadPane::new(panes.clone(), tmux, host), panes)
}

#[tokio::test]
async fn a_live_session_is_captured() {
    let session = node_session_name(RUN, "worker", 1);
    let tmux = FakeTmux::with_sessions(&[&session]);
    let root = tempfile::tempdir().unwrap();
    let (uc, panes) = read_pane(
        Ok(Some(target(root.path().into(), true, false))),
        ReattachOutcome::Resumed,
        tmux,
        Host::allowing(),
    );
    let (name, pane) = uc
        .execute(&Actor::LocalOperator, RUN, "worker", 1)
        .await
        .unwrap();
    assert_eq!(name, session);
    assert_eq!(pane.source, PaneSource::Live);
    assert_eq!(pane.content, format!("pane of {session}"));
    assert!(!pane.stale && !pane.resumed);
    assert!(panes.reattached.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_terminal_iteration_serves_its_snapshot_and_is_never_resurrected() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = pane_snapshot_path(root.path(), RUN, "worker", 1);
    std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
    std::fs::write(&snapshot, "the last screen").unwrap();
    let (uc, panes) = read_pane(
        Ok(Some(target(root.path().into(), false, true))),
        ReattachOutcome::Resumed,
        Arc::new(FakeTmux::default()),
        Host::allowing(),
    );
    let (_, pane) = uc
        .execute(&Actor::LocalOperator, RUN, "worker", 1)
        .await
        .unwrap();
    assert_eq!(pane.source, PaneSource::Snapshot);
    assert_eq!(pane.content, "the last screen");
    assert!(pane.stale, "not the latest iteration");
    assert!(panes.reattached.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_dead_latest_session_is_resumed_once_authorized() {
    let root = tempfile::tempdir().unwrap();
    let host = Host::allowing();
    let (uc, panes) = read_pane(
        Ok(Some(target(root.path().into(), true, false))),
        ReattachOutcome::Resumed,
        Arc::new(FakeTmux::default()),
        host.clone(),
    );
    let (_, pane) = uc
        .execute(&Actor::LocalOperator, RUN, "worker", 1)
        .await
        .unwrap();
    assert_eq!(pane.source, PaneSource::Resumed);
    assert!(pane.resumed);
    assert_eq!(pane.content, "resumed pane");
    assert_eq!(panes.reattached.lock().unwrap().len(), 1);
    let asked = host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Action("resume".into()));
}

#[tokio::test]
async fn a_refused_resurrection_resumes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (uc, panes) = read_pane(
        Ok(Some(target(root.path().into(), true, false))),
        ReattachOutcome::Resumed,
        Arc::new(FakeTmux::default()),
        Host::refusing(),
    );
    assert!(matches!(
        uc.execute(&Actor::LocalOperator, RUN, "worker", 1).await,
        Err(ReadPaneError::Refused(_))
    ));
    assert!(panes.reattached.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_reattach_outcome_is_rendered_for_the_operator() {
    let root = tempfile::tempdir().unwrap();
    let cases = [
        (
            ReattachOutcome::ScriptNode,
            "Script session no longer available",
        ),
        (ReattachOutcome::CannotResume, "Session no longer available"),
        (
            ReattachOutcome::FrozenHarnessGone {
                harness: "opencode".into(),
            },
            "Session cannot be resumed: this node's harness 'opencode' no longer resolves. \
             Restore its descriptor, or retry the node to start fresh.",
        ),
        (
            ReattachOutcome::TraceRefused {
                error: "run is terminal".into(),
            },
            "Session no longer available",
        ),
        (
            ReattachOutcome::CapReached { live: 4, cap: 4 },
            "Session cannot be resumed: session cap reached (4/4)",
        ),
        (
            ReattachOutcome::ResumeFailed {
                error: "tmux gone".into(),
            },
            "Session no longer available",
        ),
        (
            ReattachOutcome::WorkingDirMissing,
            "Session no longer available",
        ),
    ];
    for (outcome, content) in cases {
        let (uc, _) = read_pane(
            Ok(Some(target(root.path().into(), true, false))),
            outcome.clone(),
            Arc::new(FakeTmux::default()),
            Host::allowing(),
        );
        let (_, pane) = uc
            .execute(&Actor::LocalOperator, RUN, "worker", 1)
            .await
            .unwrap();
        assert_eq!(pane.content, content, "{outcome:?}");
        assert_eq!(pane.source, PaneSource::Unavailable, "{outcome:?}");
    }
}

#[tokio::test]
async fn a_script_node_without_resume_falls_back_to_its_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = pane_snapshot_path(root.path(), RUN, "worker", 1);
    std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
    std::fs::write(&snapshot, "script output").unwrap();
    let (uc, _) = read_pane(
        Ok(Some(target(root.path().into(), true, false))),
        ReattachOutcome::ScriptNode,
        Arc::new(FakeTmux::default()),
        Host::allowing(),
    );
    let (_, pane) = uc
        .execute(&Actor::LocalOperator, RUN, "worker", 1)
        .await
        .unwrap();
    assert_eq!(pane.source, PaneSource::Snapshot);
    assert_eq!(pane.content, "script output");
}

#[tokio::test]
async fn an_unknown_run_or_node_is_not_found() {
    let (uc, _) = read_pane(
        Err(RunLookup::NotFound),
        ReattachOutcome::Resumed,
        Arc::new(FakeTmux::default()),
        Host::allowing(),
    );
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN, "worker", 1).await,
        Err(ReadPaneError::Run(RunLookup::NotFound))
    );
    let (uc, _) = read_pane(
        Ok(None),
        ReattachOutcome::Resumed,
        Arc::new(FakeTmux::default()),
        Host::allowing(),
    );
    assert_eq!(
        uc.execute(&Actor::LocalOperator, RUN, "worker", 1).await,
        Err(ReadPaneError::NodeNotFound)
    );
}
