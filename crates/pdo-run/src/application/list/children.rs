//! `GET /runs/{run_id}/children` (ADR-0064): every Run whose mechanical
//! `parent_run_id` is this Run, grouped by parent node, with a cost derived on
//! read (ADR-0052) and, for a child that has its own children, its tree cost.

use std::collections::HashMap;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use serde::Serialize;
use ts_rs::TS;

use crate::application::events::reload_run_state_with;
use crate::application::tree_cost::TreeCosts;
use crate::domain::child_awaiting;
use crate::domain::projection::project;
use crate::domain::state::{CostStat, RunStatus, TreeCost};
use crate::Runs;

/// One child Run of `GET /runs/{run_id}/children` — the read model the
/// Orchestration view consumes. Ordinary projection fields, plus a cost
/// **derived on read** through the cost cache, never persisted (ADR-0052):
/// `None` means unavailable and the surface renders "—", never `$0`.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunChildEntry {
    pub run_id: String,
    pub pipeline_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub status: RunStatus,
    /// Display-only "no forward progress" overlay (#180, #783): true when the
    /// child has no running/waiting node and a stale node, even though `status`
    /// stays `running`. Derived per read by `is_stalled`, exactly as
    /// on `GET /runs`, so the Orchestration tab's stale pill and the run list's
    /// amber dot can never disagree about the same child.
    pub stalled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completed_at: Option<String>,
    /// Derived on read via the cost cache (ADR-0052) — never persisted. Absent
    /// (JSON `null`/omitted) when the cost is unavailable: the surface renders
    /// "—", never `$0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost: Option<CostStat>,
    /// The child's Coût de l'arbre (ADR-0101): its own cost
    /// plus every one of its descendants, grandchildren included. Absent for a
    /// child without descendant: its tree cost is `cost`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tree_cost: Option<TreeCost>,
}

/// One group of `GET /runs/{run_id}/children`: all children spawned by ONE
/// node's sessions of the parent Run. `node_id: None` groups the (theoretically
/// impossible but tolerated) children whose `RunStarted` carried no node id —
/// grouped last, never dropped.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunChildrenGroup {
    pub node_id: Option<String>,
    pub children: Vec<RunChildEntry>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunChildrenResponse {
    pub run_id: String,
    pub nodes: Vec<RunChildrenGroup>,
}

impl Runs {
    /// `GET /runs/{run_id}/children` — the dedicated children read (ADR-0064):
    /// every Run whose mechanical `parent_run_id` is this Run, grouped by parent
    /// node, in the parent's node-definition order (unknown/absent node last),
    /// children chronological within a group. The cost lives HERE and on
    /// `GET /runs/costs`, not on `GET /runs` — the global list stays cheap
    /// (ADR-0052: derived on read, never persisted).
    ///
    /// Only the children `actor` may read (CORE-5). The children are found
    /// in the Résumés (ADR-0084); only a child's own log is replayed, for its
    /// cost.
    pub async fn run_children(&self, actor: &Actor, run_id: &str) -> Response {
        let state = self;
        let run_id = run_id.to_string();
        // The parent must exist, so a typo'd id is a 404 — not a silent empty list
        // (which would be indistinguishable from a childless Run).
        let Some((_, parent_state)) = reload_run_state_with(state, &run_id).await else {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        };

        let mut summaries = match state.store().summaries().await {
            Ok(summaries) => summaries,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        // #588 / ADR-0069 §4: a grandchild's wait shows on the child row too, so
        // the Orchestration tab and the run list agree about the same child.
        child_awaiting::overlay(&mut summaries);
        // The tree of every Run, read before the list narrows to the children:
        // a child's tree cost reaches its grandchildren (ADR-0101),
        // readable by this caller or not, as an aggregate.
        let mut tree_costs =
            TreeCosts::new(state.store().as_ref(), self.live_facts.as_ref(), &summaries);
        summaries.retain(|child| child.parent_run_id.as_deref() == Some(run_id.as_str()));
        self.readable(actor).retain(&mut summaries).await;

        let mut by_node: HashMap<Option<String>, Vec<RunChildEntry>> = HashMap::new();
        for child in summaries {
            let cost = tree_costs.cost(&child.run_id).await;
            let tree_cost = tree_costs.tree_cost(&child.run_id).await;
            let entry = RunChildEntry {
                run_id: child.run_id,
                pipeline_name: child.pipeline_name,
                name: child.name,
                status: child.status,
                stalled: child.stalled,
                started_at: child.started_at,
                completed_at: child.completed_at,
                cost,
                tree_cost,
            };
            by_node.entry(child.parent_node_id).or_default().push(entry);
        }

        // Group order: the parent's own node-definition order, so the response reads
        // like the pipeline; a group keyed by an unknown or absent node id lands last.
        let mut nodes: Vec<RunChildrenGroup> = Vec::new();
        let mut seen: std::collections::HashSet<Option<String>> = Default::default();
        let mut push_group = |node_id: Option<String>, by_node: &mut HashMap<_, _>| {
            if !seen.insert(node_id.clone()) {
                return;
            }
            let mut children: Vec<RunChildEntry> = by_node.remove(&node_id).unwrap_or_default();
            if children.is_empty() {
                return;
            }
            children.sort_by(|a, b| match (&a.started_at, &b.started_at) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
            nodes.push(RunChildrenGroup { node_id, children });
        };
        for def in &parent_state.node_defs {
            push_group(Some(def.id.clone()), &mut by_node);
        }
        let mut leftover_keys: Vec<Option<String>> = by_node.keys().cloned().collect();
        leftover_keys.sort();
        for key in leftover_keys {
            push_group(key, &mut by_node);
        }

        Json(RunChildrenResponse { run_id, nodes }).into_response()
    }

    /// The cost of the Run `run_id`, derived on read from its log
    /// (ADR-0052): the one log a read of costs replays, the Run's own.
    pub(crate) async fn cost_of(&self, run_id: &str) -> Option<CostStat> {
        let events = self.store().events(run_id).await.ok()?;
        let child = project(&events)?;
        self.live_facts.cost(&child, &events).await
    }
}
