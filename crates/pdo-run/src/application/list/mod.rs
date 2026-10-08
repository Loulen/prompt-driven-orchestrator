//! The reads of the Runs as a list: every Run as the list shows it
//! (`GET /runs`), the terminal ones whose disk can be reclaimed ([`reapable`]),
//! and a Run's children ([`children`]). The list and the children read the
//! Résumés de Run (ADR-0084), never a Run's log.

pub mod children;
pub mod reapable;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::attributes::{AttributeFilter, AttributedList, Attributes};
use pdo_kernel::{Actor, ResourceKind};
use serde::Serialize;
use ts_rs::TS;

use crate::application::paths::effective_repo_of;
use crate::domain::child_awaiting;
use crate::domain::state::RunStatus;
use crate::Runs;

#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunListEntry {
    pub run_id: String,
    pub pipeline_name: String,
    pub status: RunStatus,
    /// Display-only "no forward progress" overlay: true when the run has no
    /// running/waiting node and a stale node, so its dot renders amber even
    /// though `status` stays `running`. Derived by `crate::domain::projection::is_stalled`
    /// into the Résumé.
    pub stalled: bool,
    pub started_at: Option<String>,
    /// Why the Run ended non-green. Carried on the *list* entry so the red dot has
    /// something to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub failure_reason: Option<String>,
    /// Why the Run is parked `AwaitingUser` on an **incident** (ADR-0049), and its
    /// machine slug. On the list entry for the same reason as `failure_reason`.
    /// `None` for an interactive wait, which carries no incident reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub awaiting_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub awaiting_reason_code: Option<String>,
    /// How many started nodes the Run, past `End`, still waits for (ADR-0086):
    /// the list says why a Run that reached `End` is still `Running`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waiting_for_started_nodes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// Provenance: the Trigger that created this Run, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub triggered_by: Option<String>,
    /// Who created the Run (API 0.8), as `RunState::created_by`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_by: Option<pdo_kernel::Actor>,
    /// Provenance: the parent Run + node whose session created this Run
    /// (ADR-0064) — the key the "orchestrated" badge and the "root runs only"
    /// filter read. `None` on a root run. Mechanical: never caller-declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_node_id: Option<String>,
    /// Resolved target repo for "group by project": the run's `target_repo`, or
    /// the daemon's `repo_root` when unset. Only runs predating the hardened
    /// create boundary (ADR-0033) can be unset — this is a READ, so keep the
    /// fallback forever.
    pub effective_repo: String,
    /// The entity attributes the extensions stamped on it (CONTEXT.md §
    /// *Extensions*); absent when none.
    #[serde(skip_serializing_if = "Attributes::is_empty")]
    #[ts(as = "Option<Attributes>", optional)]
    pub attributes: Attributes,
}

impl Runs {
    /// `GET /runs`: every Run `actor` may read (CORE-5) carrying the
    /// attributes `filter` asks for, as the list shows it, with its
    /// attributes. Read from the Résumés (ADR-0084).
    pub async fn list_runs(&self, actor: &Actor, filter: AttributeFilter) -> Response {
        let mut summaries = match self.store().summaries().await {
            Ok(summaries) => summaries,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        // #588 / ADR-0069 §4: a child awaiting its user lifts its parent on read.
        child_awaiting::overlay(&mut summaries);
        let attributed = AttributedList::read(self.extensions(), ResourceKind::Run, filter).await;
        summaries.retain(|summary| attributed.keeps(&summary.run_id));
        self.readable(actor).retain(&mut summaries).await;

        let runs: Vec<RunListEntry> = summaries
            .into_iter()
            .map(|summary| {
                let effective_repo = effective_repo_of(self, summary.target_repo.as_deref())
                    .to_string_lossy()
                    .into_owned();
                let attributes = attributed.of(&summary.run_id);
                RunListEntry {
                    run_id: summary.run_id,
                    pipeline_name: summary.pipeline_name,
                    status: summary.status,
                    stalled: summary.stalled,
                    started_at: summary.started_at,
                    failure_reason: summary.failure_reason,
                    awaiting_reason: summary.awaiting_reason,
                    awaiting_reason_code: summary.awaiting_reason_code,
                    waiting_for_started_nodes: summary.waiting_for_started_nodes,
                    name: summary.name,
                    triggered_by: summary.triggered_by,
                    created_by: summary.created_by,
                    parent_run_id: summary.parent_run_id,
                    parent_node_id: summary.parent_node_id,
                    effective_repo,
                    attributes,
                }
            })
            .collect();

        Json(runs).into_response()
    }
}
