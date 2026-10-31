//! `pdo run wait`: the long-poll on a node's child runs (a read).

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::events::reload_run_state_with;
use crate::application::session_claim::SessionClaim;
use crate::domain::state;
use crate::Runs;

#[derive(Deserialize, Default)]
pub struct ChildrenWaitQuery {
    #[serde(default)]
    pub all: Option<bool>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// One settled child as `pdo run wait` prints it.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct SettledChild {
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub status: state::RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    /// The child's own live children — a settled child may leave grandchildren
    /// running (they are ordinary runs, ADR-0064).
    pub children_active: usize,
}

/// One look at the children of `(run, node)`: which are live, which settled
/// after `since`. Pure over the projected store.
pub struct ChildrenWaitSnapshot {
    pub active: usize,
    pub settled_after: Vec<SettledChild>,
    pub total: usize,
}

pub fn children_wait_snapshot(
    runs: &[state::RunState],
    run_id: &str,
    node_id: &str,
    since: &str,
) -> ChildrenWaitSnapshot {
    let mut snapshot = ChildrenWaitSnapshot {
        active: 0,
        settled_after: Vec::new(),
        total: 0,
    };
    for child in runs {
        if child.parent_run_id.as_deref() != Some(run_id)
            || child.parent_node_id.as_deref() != Some(node_id)
        {
            continue;
        }
        snapshot.total += 1;
        if child.status.is_live() {
            snapshot.active += 1;
            continue;
        }
        // Terminal *after* the call started: `completed_at` is ISO-8601 with a
        // fixed width, so the string order is the time order. A terminal child
        // with no timestamp (an archived legacy run) counts as old.
        let after = child.completed_at.as_deref().is_some_and(|t| t >= since);
        if !after {
            continue;
        }
        let children_active = runs
            .iter()
            .filter(|r| r.parent_run_id.as_deref() == Some(child.run_id.as_str()))
            .filter(|r| r.status.is_live())
            .count();
        snapshot.settled_after.push(SettledChild {
            run_id: child.run_id.clone(),
            name: child.name.clone(),
            status: child.status.clone(),
            reason: child.failure_reason.clone(),
            children_active,
        });
    }
    snapshot
}

/// `GET /runs/{run_id}/nodes/{node_id}/children/wait` — the children long-poll
/// (#588 / ADR-0069 §3). Holds the response until a child of this node becomes
/// terminal after `since` (every child with `all=true`), or `timeout_secs`
/// (server-capped at [`CHILDREN_WAIT_SERVER_MAX_SECS`]) elapses — then answers
/// `{"timeout":true,"since":…}` and the CLI calls again. No active child at the
/// call ⇒ `{"noop":true}` (ADR-0025). Only the node's own session may ask: the
/// session headers must name this run and node (`not_in_node_session`).
pub async fn children_wait(
    state: &Arc<Runs>,
    run_id: String,
    node_id: String,
    claim: Option<SessionClaim>,
    query: ChildrenWaitQuery,
) -> Response {
    if claim
        .as_ref()
        .is_none_or(|c| c.run_id != run_id || c.node_id != node_id)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "not_in_node_session",
                "message": "`pdo run wait` must be run from the node session it waits for",
            })),
        )
            .into_response();
    }
    if reload_run_state_with(state, &run_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "run_not_found" })),
        )
            .into_response();
    }
    let all = query.all.unwrap_or(false);
    let since = query.since.unwrap_or_else(pdo_kernel::clock::now_iso);
    let budget = query
        .timeout_secs
        .unwrap_or(CHILDREN_WAIT_SERVER_MAX_SECS)
        .clamp(1, CHILDREN_WAIT_SERVER_MAX_SECS);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(budget);

    let mut first = true;
    loop {
        let runs =
            crate::application::projections::project_all_overlaid(state.store.as_ref()).await;
        let snapshot = children_wait_snapshot(&runs, &run_id, &node_id, &since);
        if first {
            first = false;
            if snapshot.active == 0 && snapshot.settled_after.is_empty() {
                return Json(serde_json::json!({
                    "noop": true,
                    "reason": format!("node {node_id} of run {run_id} has no active child run"),
                    "children": [],
                    "since": since,
                }))
                .into_response();
            }
        }
        let ready = if all {
            snapshot.active == 0 && !snapshot.settled_after.is_empty()
        } else {
            !snapshot.settled_after.is_empty()
        };
        if ready {
            return Json(serde_json::json!({
                "children": snapshot.settled_after,
                "all": all,
                "since": since,
                "children_total": snapshot.total,
            }))
            .into_response();
        }
        if tokio::time::Instant::now() >= deadline {
            return Json(serde_json::json!({
                "timeout": true,
                "children": [],
                "since": since,
            }))
            .into_response();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
/// The daemon caps one long-poll at this many seconds; the CLI loops.
pub const CHILDREN_WAIT_SERVER_MAX_SECS: u64 = 25;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    #[test]
    fn run_wait_settled_children_carry_the_reason_of_a_failed_or_skipped_child() {
        // `pdo run wait` prints `reason` for a `failed` / `skipped` child (the
        // agent reads *why*); a `completed` child has none and the key is
        // omitted rather than `null`.
        fn run(
            id: &str,
            parent: Option<&str>,
            end: Option<(event_log::EventKind, &str)>,
        ) -> event_log::RunState {
            let mut started = serde_json::json!({ "pipeline_name": "p", "name": id });
            if let Some(p) = parent {
                started["parent_run_id"] = serde_json::json!(p);
                started["parent_node_id"] = serde_json::json!("worker");
            }
            let mut events = vec![event_log::Event {
                id: None,
                run_id: id.into(),
                ts: "2026-09-12T10:00:00Z".into(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(started),
            }];
            if let Some((kind, reason)) = end {
                events.push(event_log::Event {
                    id: None,
                    run_id: id.into(),
                    ts: "2026-09-12T10:05:00Z".into(),
                    kind,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "reason": reason })),
                });
            }
            event_log::project(&events).unwrap()
        }
        let runs = vec![
            run("parent", None, None),
            run(
                "failed-child",
                Some("parent"),
                Some((event_log::EventKind::RunFailed, "build broke")),
            ),
            run(
                "skipped-child",
                Some("parent"),
                Some((event_log::EventKind::RunSkipped, "nothing to do")),
            ),
            run(
                "completed-child",
                Some("parent"),
                Some((event_log::EventKind::RunCompleted, "")),
            ),
            // Archived while it ran: it ended at its archive, after the wait began.
            run(
                "archived-child",
                Some("parent"),
                Some((event_log::EventKind::RunArchived, "")),
            ),
        ];
        let snapshot = children_wait_snapshot(&runs, "parent", "worker", "2026-09-12T10:00:00Z");
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.total, 4);
        let by_id: HashMap<&str, serde_json::Value> = snapshot
            .settled_after
            .iter()
            .map(|c| (c.run_id.as_str(), serde_json::to_value(c).unwrap()))
            .collect();
        assert_eq!(by_id["failed-child"]["status"], "failed");
        assert_eq!(by_id["failed-child"]["reason"], "build broke");
        assert_eq!(by_id["skipped-child"]["status"], "skipped");
        assert_eq!(by_id["skipped-child"]["reason"], "nothing to do");
        assert_eq!(by_id["completed-child"]["status"], "completed");
        assert_eq!(by_id["archived-child"]["status"], "archived");
        assert!(
            by_id["completed-child"].get("reason").is_none(),
            "no reason ⇒ key omitted: {}",
            by_id["completed-child"]
        );
    }
}
