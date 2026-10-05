//! `POST /runs/{run_id}/review/comments/{comment_id}/resolve` and `…/reopen`
//! (#751): the human's verbs on a sent comment.

use pdo_kernel::Actor;
use pdo_run::domain::event::EventKind;
use serde_json::{json, Value};
use tracing::info;

use super::failure::Outcome;
use super::operations;
use super::reply::session_node;
use super::review::{find_comment, Review};
use crate::domain::comments::decision::{decider, Decision};
use crate::domain::comments::reply_author;

impl Review {
    /// Resolve (accept a proposal, or close a comment outright) or reopen
    /// (back to `sent`: on a resolved comment whoever resolved it — the human
    /// keeps the last word, ADR-0067 §4 —, on a pending proposal it
    /// **declines** it and keeps the comment open for the agent). A decision
    /// that changes nothing answers `changed: false` and appends no event. A
    /// session (agent) decides as itself; the Review page decides as `by`, or
    /// `user`.
    pub async fn decide(
        &self,
        actor: &Actor,
        run_id: &str,
        comment_id: &str,
        decision: Decision,
        by: Option<&str>,
    ) -> Outcome<Value> {
        self.authorize(
            actor,
            &operations::on_comment(decision.action(), run_id, comment_id),
        )
        .await?;
        let run = self.run(run_id).await?;
        let comment = find_comment(&run, comment_id)?.clone();
        let by = match session_node(actor) {
            Some(node) => reply_author(Some(node), None),
            None => decider(by),
        };
        if !decision.changes(&comment) {
            return Ok(json!({ "comment": comment, "changed": false }));
        }
        let kind = match decision {
            Decision::Resolve => EventKind::ReviewCommentResolved,
            Decision::Reopen => EventKind::ReviewCommentReopened,
        };
        let kind_str = format!("{kind:?}");
        self.append_review_event(run_id, kind, json!({ "id": comment_id, "by": by }))
            .await?;
        info!("run {run_id}: review comment {comment_id} {kind_str} by {by}");
        let comment = self.projected_comment(run_id, comment_id).await?;
        Ok(json!({ "comment": comment, "changed": true }))
    }
}
