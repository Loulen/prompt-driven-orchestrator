//! The **Résumé de Run** (ADR-0084): what the lists and the access checks
//! read of a Run, persisted beside its event log and written in the same
//! transaction as the event that changes it. Never behind the log, which
//! stays the only source of truth and rebuilds it whole ([`summarize`]).
//!
//! It holds the fields of `GET /runs` and of `GET /runs/{id}/children`, the
//! parent of a Run, its primary repository and the Projet recorded at its
//! launch (ADR-0087), which the access checks read, its activity, which
//! the date filter of the lists reads, and the status of each of its working
//! nodes, which the row of the list draws as its progress.

use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::child_awaiting::{child_awaiting_reason, AwaitingTree, ChildWait};
use crate::domain::event::Event;
use crate::domain::projection::{is_stalled, project};
use crate::domain::run_activity::{activity_of, ActivitySpan};
use crate::domain::state::{NodeStatus, RunState, RunStatus};

/// The shape of [`RunSummary`]: the Résumés written under another version
/// are rebuilt from the event log at boot. Bump it with any change of the
/// fields or of how they are derived.
pub const SUMMARY_VERSION: i64 = 5;

/// One Run, as the lists show it and the access checks read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub pipeline_name: String,
    pub status: RunStatus,
    /// The "no forward progress" overlay of the list (#180), as
    /// [`is_stalled`] derives it from the nodes.
    pub stalled: bool,
    pub name: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
    pub awaiting_reason: Option<String>,
    pub awaiting_reason_code: Option<String>,
    /// How many started nodes the Run, past `End`, still waits for (ADR-0086).
    #[serde(default)]
    pub waiting_for_started_nodes: Option<u32>,
    pub triggered_by: Option<String>,
    pub created_by: Option<Actor>,
    pub parent_run_id: Option<String>,
    pub parent_node_id: Option<String>,
    /// The primary repository the Run names; `None` for a Run older than the
    /// create boundary (ADR-0033), which works in the daemon's repo root.
    pub target_repo: Option<String>,
    /// The registry pipeline it was created from, and the number of the
    /// version it launched (ADR-0088): the history counts its Runs.
    #[serde(default)]
    pub pipeline_id: Option<String>,
    #[serde(default)]
    pub pipeline_version: Option<u32>,
    /// The Projet the Run was launched in (ADR-0087); `None` for none.
    #[serde(default)]
    pub project_id: Option<String>,
    /// The instant of its last activity; `None` for a Run with none.
    #[serde(default)]
    pub last_activity_at: Option<String>,
    /// Its spans of activity, oldest first (CONTEXT.md « Activité d'un Run »).
    #[serde(default)]
    pub activity: Vec<ActivitySpan>,
    /// Its working nodes (agent, script), in the pipeline's order, with their
    /// status: the progress the row of the list draws.
    #[serde(default)]
    pub nodes: Vec<NodeProgress>,
}

/// One working node of a Run, as the row of the list draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeProgress {
    pub id: String,
    /// Its name in the pipeline, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// `pending` until it has started.
    pub status: NodeStatus,
}

/// The working nodes of `run` (agent, script), in its pipeline's order.
fn nodes_of(run: &RunState) -> Vec<NodeProgress> {
    run.node_defs
        .iter()
        .filter(|def| def.node_type == "agent" || def.node_type == "script")
        .map(|def| NodeProgress {
            id: def.id.clone(),
            name: def.name.clone(),
            status: run
                .nodes
                .get(&def.id)
                .map_or(NodeStatus::Pending, |node| node.status.clone()),
        })
        .collect()
}

impl RunSummary {
    /// The Résumé of a projected Run whose log is `events`.
    pub fn of(run: &RunState, events: &[Event]) -> Self {
        let activity = activity_of(events);
        Self {
            run_id: run.run_id.clone(),
            pipeline_name: run.pipeline_name.clone(),
            status: run.status.clone(),
            stalled: is_stalled(run),
            name: run.name.clone(),
            started_at: run.started_at.clone(),
            completed_at: run.completed_at.clone(),
            failure_reason: run.failure_reason.clone(),
            awaiting_reason: run.awaiting_reason.clone(),
            awaiting_reason_code: run.awaiting_reason_code.clone(),
            waiting_for_started_nodes: run.waiting_for_started_nodes,
            triggered_by: run.triggered_by.clone(),
            created_by: run.created_by.clone(),
            parent_run_id: run.parent_run_id.clone(),
            parent_node_id: run.parent_node_id.clone(),
            target_repo: run.target_repo.clone(),
            pipeline_id: run.pipeline_id.clone(),
            pipeline_version: run.pipeline_version.as_ref().map(|v| v.number),
            project_id: run.project_id.clone(),
            last_activity_at: activity.last().map(|span| span.to.clone()),
            activity,
            nodes: nodes_of(run),
        }
    }
}

/// The Résumé of the Run whose whole log is `events`; `None` for a log that
/// projects no Run (no `RunStarted`).
pub fn summarize(events: &[Event]) -> Option<RunSummary> {
    project(events).map(|run| RunSummary::of(&run, events))
}

impl AwaitingTree for RunSummary {
    fn run_id(&self) -> &str {
        &self.run_id
    }

    fn parent_run_id(&self) -> Option<&str> {
        self.parent_run_id.as_deref()
    }

    fn status(&self) -> &RunStatus {
        &self.status
    }

    fn as_child_wait(&self) -> ChildWait {
        ChildWait {
            child_id: self.run_id.clone(),
            child_name: self.name.clone().unwrap_or_else(|| self.run_id.clone()),
            parent_node: self.parent_node_id.clone(),
            // A Résumé has no node: only a projected parent shows `since`.
            since: None,
        }
    }

    fn lift(&mut self, child: &ChildWait) {
        self.status = RunStatus::AwaitingUser;
        // Awaiting is not stalled: the list derived `stalled` after the
        // overlay, on a Run no longer `Running`.
        self.stalled = false;
        if self.awaiting_reason.is_none() {
            self.awaiting_reason = Some(child_awaiting_reason(&child.child_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::child_awaiting::overlay;
    use crate::domain::event::EventKind;
    use pretty_assertions::assert_eq;

    fn ev(run: &str, kind: EventKind, node: Option<&str>, payload: serde_json::Value) -> Event {
        Event {
            id: None,
            run_id: run.into(),
            ts: "2026-10-07T10:00:00Z".into(),
            kind,
            node_id: node.map(str::to_string),
            iter: node.map(|_| 1),
            payload: Some(payload),
        }
    }

    fn started(run: &str, parent: Option<&str>) -> Event {
        let mut payload =
            serde_json::json!({ "pipeline_name": "p", "name": run, "target_repo": "/repo" });
        if let Some(parent) = parent {
            payload["parent_run_id"] = serde_json::json!(parent);
            payload["parent_node_id"] = serde_json::json!("worker");
        }
        ev(run, EventKind::RunStarted, None, payload)
    }

    #[test]
    fn a_log_without_run_started_has_no_summary() {
        assert_eq!(summarize(&[]), None);
        assert_eq!(
            summarize(&[ev(
                "r",
                EventKind::NodeStarted,
                Some("n"),
                serde_json::json!({})
            )]),
            None
        );
    }

    #[test]
    fn a_summary_carries_what_the_list_reads() {
        let summary = summarize(&[started("r", Some("parent"))]).unwrap();
        assert_eq!(summary.run_id, "r");
        assert_eq!(summary.pipeline_name, "p");
        assert_eq!(summary.name.as_deref(), Some("r"));
        assert_eq!(summary.status, RunStatus::Running);
        assert_eq!(summary.parent_run_id.as_deref(), Some("parent"));
        assert_eq!(summary.parent_node_id.as_deref(), Some("worker"));
        assert_eq!(summary.target_repo.as_deref(), Some("/repo"));
        assert_eq!(summary.project_id, None);
        assert!(!summary.stalled);
    }

    #[test]
    fn a_summary_carries_the_working_nodes_in_order_with_their_status() {
        let mut start = started("r", None);
        start.payload.as_mut().unwrap()["node_defs"] = serde_json::json!([
            { "id": "start", "node_type": "start", "view_x": null, "view_y": null, "inputs": [], "outputs": [] },
            { "id": "impl", "name": "Implement", "node_type": "agent", "view_x": null, "view_y": null, "inputs": [], "outputs": [] },
            { "id": "check", "node_type": "script", "view_x": null, "view_y": null, "inputs": [], "outputs": [] },
            { "id": "end", "node_type": "end", "view_x": null, "view_y": null, "inputs": [], "outputs": [] },
        ]);
        let summary = summarize(&[
            start,
            ev(
                "r",
                EventKind::NodeStarted,
                Some("impl"),
                serde_json::json!({}),
            ),
        ])
        .unwrap();
        assert_eq!(
            summary.nodes,
            vec![
                NodeProgress {
                    id: "impl".into(),
                    name: Some("Implement".into()),
                    status: NodeStatus::Running,
                },
                NodeProgress {
                    id: "check".into(),
                    name: None,
                    status: NodeStatus::Pending,
                },
            ]
        );
    }

    #[test]
    fn a_summary_carries_the_projet_recorded_at_launch() {
        let mut event = started("r", None);
        event.payload.as_mut().unwrap()["project_id"] = serde_json::json!("prj-1");
        assert_eq!(
            summarize(&[event]).unwrap().project_id.as_deref(),
            Some("prj-1")
        );
    }

    #[test]
    fn a_summary_carries_the_activity_of_the_run() {
        let mut resumed = ev(
            "r",
            EventKind::NodeStarted,
            Some("n"),
            serde_json::json!({}),
        );
        resumed.ts = "2026-10-09T08:00:00Z".into();
        let summary = summarize(&[started("r", None), resumed]).unwrap();
        assert_eq!(
            summary.last_activity_at.as_deref(),
            Some("2026-10-09T08:00:00.000Z")
        );
        assert_eq!(
            summary.activity,
            vec![
                ActivitySpan {
                    from: "2026-10-07T10:00:00.000Z".into(),
                    to: "2026-10-07T10:00:00.000Z".into(),
                },
                ActivitySpan {
                    from: "2026-10-09T08:00:00.000Z".into(),
                    to: "2026-10-09T08:00:00.000Z".into(),
                },
            ]
        );
    }

    #[test]
    fn an_awaiting_child_lifts_its_parent_s_summary() {
        let child = vec![
            started("child", Some("parent")),
            ev(
                "child",
                EventKind::NodeStarted,
                Some("worker"),
                serde_json::json!({}),
            ),
            ev(
                "child",
                EventKind::NodeAwaitingUser,
                Some("worker"),
                serde_json::json!({ "cause": "declared", "message": "q?" }),
            ),
        ];
        let parent = vec![
            started("parent", None),
            ev(
                "parent",
                EventKind::NodeStarted,
                Some("worker"),
                serde_json::json!({}),
            ),
        ];
        let mut summaries = vec![summarize(&parent).unwrap(), summarize(&child).unwrap()];
        overlay(&mut summaries);
        assert_eq!(summaries[0].status, RunStatus::AwaitingUser);
        assert_eq!(
            summaries[0].awaiting_reason.as_deref(),
            Some("child run child is awaiting you")
        );
        assert!(!summaries[0].stalled);
    }
}
