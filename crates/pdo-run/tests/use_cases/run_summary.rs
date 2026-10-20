//! The Résumé de Run (ADR-0084): kept by every append in the event's own
//! transaction, equal to the replayed projection, rebuilt whole from the log,
//! untouched by a failed append.

use pdo_run::application::ports::EventStore;
use pdo_run::application::run_summary::{ensure_summaries, rebuild_summaries};
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::run_summary::{summarize, RunSummary};
use pdo_run::domain::state::RunStatus;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::fakes::*;

/// Every kind of event, as `project` routes them.
const KINDS: &[EventKind] = &[
    EventKind::NodeWaiting,
    EventKind::NodeStarted,
    EventKind::NodeAwaitingUser,
    EventKind::NodeResumed,
    EventKind::NodeCompletionReleased,
    EventKind::NodeStale,
    EventKind::NodeBlockedOnLimit,
    EventKind::NodeAutoCompleteObserved,
    EventKind::NodeInterrupted,
    EventKind::NodeCompleted,
    EventKind::NodeAutoCompleted,
    EventKind::NodeDelivered,
    EventKind::NodeFailed,
    EventKind::NodeStopped,
    EventKind::NodeInvalidated,
    EventKind::FrontmatterRetryPending,
    EventKind::MergeConflictDetected,
    EventKind::MergeResolvedInNodeFavour,
    EventKind::MergeResolverStarted,
    EventKind::MergeResolverCompleted,
    EventKind::MergeResolverFailed,
    EventKind::SwitchRouted,
    EventKind::LoopIterStarted,
    EventKind::LoopBreakReceived,
    EventKind::LoopMaxReached,
    EventKind::LoopDone,
    EventKind::ForEachStarted,
    EventKind::ForEachEmpty,
    EventKind::ForEachBreakReceived,
    EventKind::ForEachDone,
    EventKind::CollectionStarted,
    EventKind::CollectionEmpty,
    EventKind::CollectionDone,
    EventKind::PipelineLint,
    EventKind::PipelineModified,
    EventKind::ManagerStarted,
    EventKind::ManagerStopped,
    EventKind::ReviewCommentSent,
    EventKind::ReviewCommentReplied,
    EventKind::ReviewCommentResolved,
    EventKind::ReviewCommentReopened,
    EventKind::CommandIssued,
    EventKind::SandboxPrepStarted,
    EventKind::SandboxPrepReady,
    EventKind::RunRenamed,
    EventKind::RunReposEdited,
    EventKind::RunPaused,
    EventKind::RunResumed,
    EventKind::RunInterrupted,
    EventKind::RunResumed,
    EventKind::RunFailed,
    EventKind::RunResumed,
    EventKind::RunSkipped,
    EventKind::RunHalted,
    EventKind::RunCompleted,
    EventKind::RunArchived,
];

/// A `RunStarted` carrying everything a Résumé holds.
fn started(run_id: &str) -> Event {
    let mut started = run_started(run_id);
    let payload = started.payload.as_mut().unwrap();
    payload["name"] = json!("the name");
    payload["target_repo"] = json!("/repo");
    payload["triggered_by"] = json!("t1");
    payload["parent_run_id"] = json!("parent");
    payload["parent_node_id"] = json!("worker");
    payload["created_by"] = json!({ "kind": "local_operator" });
    started
}

/// What `kind` carries, so each applier has something to fold.
fn of_kind(run_id: &str, kind: EventKind) -> Event {
    let run_level = matches!(
        kind,
        EventKind::RunRenamed
            | EventKind::RunReposEdited
            | EventKind::RunPaused
            | EventKind::RunResumed
            | EventKind::RunInterrupted
            | EventKind::RunFailed
            | EventKind::RunSkipped
            | EventKind::RunHalted
            | EventKind::RunCompleted
            | EventKind::RunArchived
    );
    let mut e = event(run_id, kind, if run_level { None } else { Some("worker") });
    e.payload = Some(json!({
        "name": "renamed",
        "reason": "a reason",
        "reason_code": "session_died",
        "cause": "declared",
        "message": "a question?",
        "target_repo": "/repo",
        "loop_node_id": "worker",
        "iter": 1,
        "max_iter": 3,
    }));
    e
}

/// The Résumé the store holds for `run_id`, and the one its log projects.
async fn stored_and_replayed(w: &World, run_id: &str) -> (Option<RunSummary>, Option<RunSummary>) {
    let stored = w.store.summary(run_id).await.unwrap();
    let log = w.store.events(run_id).await.unwrap();
    (stored, summarize(&log))
}

#[tokio::test]
async fn the_summary_equals_the_replayed_projection_after_each_kind_of_event() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&started("r1")).await.unwrap();
    let (stored, replayed) = stored_and_replayed(&w, "r1").await;
    assert_eq!(stored, replayed);
    let stored = stored.unwrap();
    assert_eq!(stored.name.as_deref(), Some("the name"));
    assert_eq!(stored.target_repo.as_deref(), Some("/repo"));
    assert_eq!(stored.triggered_by.as_deref(), Some("t1"));
    assert_eq!(stored.parent_run_id.as_deref(), Some("parent"));

    for kind in KINDS {
        // The transition guard may refuse one: the Résumé must agree with
        // the log either way.
        let _ = append.execute(&of_kind("r1", kind.clone())).await;
        let (stored, replayed) = stored_and_replayed(&w, "r1").await;
        assert_eq!(stored, replayed, "after {kind:?}");
    }
    assert_eq!(
        w.store.summary("r1").await.unwrap().unwrap().status,
        RunStatus::Archived
    );
}

#[tokio::test]
async fn the_summaries_are_rebuilt_whole_from_the_log() {
    let w = world();
    let append = w.runs.append_event();
    for run_id in ["r1", "r2", "r3"] {
        append.execute(&started(run_id)).await.unwrap();
        append
            .execute(&event(run_id, EventKind::NodeStarted, Some("worker")))
            .await
            .unwrap();
    }
    append
        .execute(&of_kind("r2", EventKind::RunFailed))
        .await
        .unwrap();
    let kept = w.store.summaries().await.unwrap();
    assert_eq!(
        kept.iter().map(|s| s.run_id.as_str()).collect::<Vec<_>>(),
        ["r3", "r2", "r1"],
        "newest first"
    );

    // Lost, or written by an older shape: not current.
    w.store.summaries.lock().unwrap().clear();
    assert!(ensure_summaries(w.store.as_ref()).await.unwrap(), "rebuilt");
    assert_eq!(w.store.summaries().await.unwrap(), kept);
    assert!(
        !ensure_summaries(w.store.as_ref()).await.unwrap(),
        "current: kept"
    );

    // A stale Résumé is overwritten by the log's.
    w.store
        .summaries
        .lock()
        .unwrap()
        .get_mut("r2")
        .unwrap()
        .status = RunStatus::Running;
    assert_eq!(rebuild_summaries(w.store.as_ref()).await.unwrap(), 3);
    assert_eq!(w.store.summaries().await.unwrap(), kept);
}

#[tokio::test]
async fn a_failed_append_changes_neither_the_log_nor_the_summary() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&started("r1")).await.unwrap();
    let before = w.store.summary("r1").await.unwrap();

    *w.store.failing_summary.lock().unwrap() = true;
    assert!(append
        .execute(&of_kind("r1", EventKind::RunFailed))
        .await
        .is_err());
    assert_eq!(w.store.events("r1").await.unwrap().len(), 1);
    assert_eq!(w.store.summary("r1").await.unwrap(), before);
    assert!(
        w.bus.published.lock().unwrap().len() == 1,
        "nothing published"
    );

    // A forgotten Run refuses the append, and has no Résumé any more.
    w.store.forget("r1").await.unwrap();
    assert!(append.execute(&started("r1")).await.is_err());
    assert_eq!(w.store.summary("r1").await.unwrap(), None);
}

#[tokio::test]
async fn each_append_keeps_the_activity_of_its_run_and_the_rebuild_gives_it_back() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&started("r1")).await.unwrap();
    let mut resumed = event("r1", EventKind::NodeStarted, Some("worker"));
    resumed.ts = "2026-09-29T08:00:00Z".into();
    append.execute(&resumed).await.unwrap();

    let kept = w.store.summary("r1").await.unwrap().unwrap();
    assert_eq!(
        kept.last_activity_at.as_deref(),
        Some("2026-09-29T08:00:00.000Z")
    );
    assert_eq!(
        kept.activity
            .iter()
            .map(|span| span.from.as_str())
            .collect::<Vec<_>>(),
        ["2026-09-27T12:00:00.000Z", "2026-09-29T08:00:00.000Z"],
        "active on the 27th and on the 29th, not on the 28th"
    );

    w.store.summaries.lock().unwrap().clear();
    assert!(ensure_summaries(w.store.as_ref()).await.unwrap(), "rebuilt");
    assert_eq!(w.store.summary("r1").await.unwrap(), Some(kept));
}
