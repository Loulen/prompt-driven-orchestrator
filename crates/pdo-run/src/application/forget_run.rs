//! `DELETE /runs/{run_id}`: permanently forget an archived Run (ADR-0024) —
//! its log purged behind a tombstone, its sessions reaped, its durable outputs
//! removed. A gesture of an actor: it asks the Autorisation d'opération
//! (`run` × `delete`).

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{error, info, warn};

use crate::application::events::load_projected;
use crate::application::paths::durable_run_dir;
use crate::domain::state::RunStatus;
use crate::Runs;

impl Runs {
    /// DELETE /runs/{run_id} — permanently forget an archived run, removing every event
    /// log row. Only allowed once the run is `Archived`, i.e. `cleanup_run` has already
    /// torn down its worktrees and branches, or deleting the events would strand the
    /// on-disk state.
    pub async fn forget_run(self: &Arc<Self>, actor: &Actor, run_id: &str) -> Response {
        if let Err(denied) = self
            .authorize(
                actor,
                &Operation::new(ResourceKind::Run, Verb::Delete).on(run_id),
            )
            .await
        {
            return denied.into_response();
        }
        let state = self;
        let (_, run_state) = match load_projected(state, run_id).await {
            Ok(t) => t,
            Err(resp) => return *resp,
        };

        if run_state.status != RunStatus::Archived {
            return (
                StatusCode::CONFLICT,
                "run must be archived (cleanup_run) before it can be forgotten",
            )
                .into_response();
        }

        // Tombstone + purge in ONE step: a late writer must never observe "events
        // gone, no tombstone yet" and resurrect the run (ADR-0024).
        if let Err(e) = state.store().forget(run_id).await {
            error!("failed to delete events for {run_id}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response();
        }

        // A forgotten run has nothing left to recover: reap its manager/shell sessions so
        // they can't keep POSTing commands.
        let tmux = state.tmux();
        for session in [
            pdo_session::domain::names::manager_session_name(run_id),
            pdo_session::domain::names::shell_session_name(run_id),
        ] {
            tmux.kill(&session);
        }

        // Forget also reclaims the durable output store, or the outputs preserved on
        // archive leak forever and contradict forget's "the run vanishes entirely"
        // contract. A missing dir is not an error.
        if let Some(durable) = durable_run_dir(run_id) {
            if let Err(e) = std::fs::remove_dir_all(&durable) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        "#315: failed to remove durable dir {}: {e}",
                        durable.display()
                    );
                }
            }
        }

        info!("Run {run_id} forgotten (event log purged)");

        state.forget_presences(&run_id);
        // Piggy-back on the pipeline-broadcast channel: the event channel only carries
        // typed `event_log::Event` records, and every event for this run was just deleted.
        state.bus().publish(serde_json::json!({
            "type": "run_forgotten",
            "run_id": run_id,
        }));

        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "forgotten" })),
        )
            .into_response()
    }
}
