//! The events a gesture on a sent comment appends, and the comment they
//! project to.

use pdo_run::domain::event::{Event, EventKind};
use serde_json::Value;
use tracing::error;

use super::failure::{Failure, Outcome};
use super::review::{find_comment, Review};
use crate::domain::comments::ReviewComment;

impl Review {
    /// Append one review event (`replied` / `resolved` / `reopened`). Every
    /// append broadcasts, so the Review page sees it inline without polling.
    pub(crate) async fn append_review_event(
        &self,
        run_id: &str,
        kind: EventKind,
        payload: Value,
    ) -> Outcome<()> {
        let event = Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: None,
            iter: None,
            payload: Some(payload),
        };
        self.runs.append(&event).await.map_err(|e| {
            error!("run {run_id}: failed to append {:?}: {e}", event.kind);
            Failure::error(500, format!("failed to record the event: {e}"))
        })
    }

    /// The comment as projected after the appends, for the response body.
    pub(crate) async fn projected_comment(
        &self,
        run_id: &str,
        comment_id: &str,
    ) -> Outcome<ReviewComment> {
        let run = self.run(run_id).await?;
        find_comment(&run, comment_id).cloned()
    }
}
