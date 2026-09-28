//! The reads of the Runs as a list: every Run as the list shows it
//! (`GET /runs`), the terminal ones whose disk can be reclaimed ([`reapable`]),
//! and a Run's children ([`children`]).

pub mod children;
pub mod reapable;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::attributes::{AttributeFilter, AttributedList, Attributes};
use pdo_kernel::ResourceKind;
use serde::Serialize;
use ts_rs::TS;

use crate::application::paths::effective_repo_root;
use crate::domain::child_awaiting;
use crate::domain::projection::{is_stalled, project};
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
    /// though `status` stays `running`. Derived per read by `crate::domain::projection::is_stalled`.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// Provenance: the Trigger that created this Run, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub triggered_by: Option<String>,
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
    /// `GET /runs`: every Run carrying the attributes `filter` asks for, as the
    /// list shows it, with its attributes.
    pub async fn list_runs(&self, filter: AttributeFilter) -> Response {
        let state = self;
        let run_ids = match state.store().run_ids().await {
            Ok(ids) => ids,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };

        let mut states = Vec::new();
        for run_id in run_ids {
            let events = match state.store().events(&run_id).await {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(run_state) = project(&events) {
                states.push(run_state);
            }
        }
        // #588 / ADR-0069 §4: a child awaiting its user lifts its parent on read.
        child_awaiting::overlay(&mut states);
        let attributed = AttributedList::read(self.extensions(), ResourceKind::Run, filter).await;
        states.retain(|run_state| attributed.keeps(&run_state.run_id));

        let mut runs = Vec::new();
        for run_state in states {
            {
                let stalled = is_stalled(&run_state);
                // Must be computed before `run_state`'s fields move into the literal:
                // `effective_repo_root` borrows `&run_state`.
                let effective_repo = effective_repo_root(state, &run_state)
                    .to_string_lossy()
                    .into_owned();
                let attributes = attributed.of(&run_state.run_id);
                runs.push(RunListEntry {
                    run_id: run_state.run_id,
                    pipeline_name: run_state.pipeline_name,
                    status: run_state.status,
                    stalled,
                    started_at: run_state.started_at,
                    failure_reason: run_state.failure_reason,
                    awaiting_reason: run_state.awaiting_reason,
                    awaiting_reason_code: run_state.awaiting_reason_code,
                    name: run_state.name,
                    triggered_by: run_state.triggered_by,
                    parent_run_id: run_state.parent_run_id,
                    parent_node_id: run_state.parent_node_id,
                    effective_repo,
                    attributes,
                });
            }
        }

        Json(runs).into_response()
    }
}
