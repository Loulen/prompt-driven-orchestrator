//! The Runs, as the platform context sees them: a page mount's session claim,
//! the command it records, the live Runs an update's confirm counts.

use std::sync::Arc;

use pdo_platform::application::ports::{BoxFuture, Runs as PlatformRuns};
use pdo_run::application::session_claim::{verify_session_claim, SessionClaim};
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::Runs;

/// The run context behind the platform's port.
pub(crate) struct RunsOfPlatform(pub(crate) Arc<Runs>);

impl PlatformRuns for RunsOfPlatform {
    fn verify<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let claim = SessionClaim {
                run_id: run_id.to_string(),
                node_id: node_id.to_string(),
            };
            verify_session_claim(&self.0, &claim)
                .await
                .map(|_| ())
                .map_err(|(_, body)| body["error"].as_str().unwrap_or_default().to_string())
        })
    }

    fn record_command<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        command: &'a str,
        page_mount: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            pdo_run::application::events::append_event(
                &self.0,
                &Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: EventKind::CommandIssued,
                    node_id: Some(node_id.to_string()),
                    iter: None,
                    payload: Some(serde_json::json!({
                        "command": command,
                        "page_mount": page_mount,
                    })),
                },
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
        })
    }

    /// Live Runs (`Running` / `AwaitingUser` / `Paused`) — what the update's
    /// confirm dialog counts.
    fn count_live(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            // The Résumés (ADR-0084): no Run's log is replayed.
            let Ok(summaries) = self.0.store().summaries().await else {
                return 0;
            };
            summaries.iter().filter(|run| run.status.is_live()).count()
        })
    }
}
