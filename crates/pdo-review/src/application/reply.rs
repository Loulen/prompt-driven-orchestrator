//! `POST /runs/{run_id}/review/comments/{comment_id}/reply` (#751, ADR-0067 §4;
//! CONTEXT.md « Réponse de review »): an agent — the manager or any node of the
//! Run — answers a sent comment.

use pdo_kernel::Actor;
use pdo_run::domain::event::EventKind;
use serde_json::{json, Value};
use tracing::info;

use super::failure::{Failure, Outcome};
use super::operations;
use super::review::{find_comment, Review};
use crate::domain::comments::decision::{proposes_resolution, ReplyOutcome};
use crate::domain::comments::reply_author;

/// A reply, as `pdo review reply` sends it.
#[derive(Debug, Clone, Default)]
pub struct ReplyCommand {
    pub text: String,
    /// The agent considers the comment addressed: a **proposal** unless the
    /// instance setting lets agents resolve directly.
    pub resolved: bool,
    /// Fallback author when the call comes from no session.
    pub author: Option<String>,
}

/// The node of the session the actor speaks from: the agent CLI's.
pub(crate) fn session_node(actor: &Actor) -> Option<&str> {
    match actor {
        Actor::Agent { node_id, .. } => Some(node_id.as_str()),
        _ => None,
    }
}

impl Review {
    /// The author is **deduced from the session** that calls (the manager
    /// session reads `manager`, a node session its node id). `resolved: true`
    /// is a **proposal** by default (`review_comment_replied` with
    /// `proposes_resolution`); under `review_agent_can_resolve` it also
    /// appends `review_comment_resolved` by the same author — the human keeps
    /// Reopen either way. Replies are welcome on a resolved comment too (the
    /// thread stays open to clarification); `resolved` is then just a reply.
    pub async fn reply(
        &self,
        actor: &Actor,
        run_id: &str,
        comment_id: &str,
        command: ReplyCommand,
    ) -> Outcome<Value> {
        self.authorize(actor, &operations::on_comment("reply", run_id, comment_id))?;
        let run = self.run(run_id).await?;
        let text = command.text.trim_end().to_string();
        if text.trim().is_empty() {
            return Err(Failure::error(400, "empty reply text"));
        }
        let status = find_comment(&run, comment_id)?.status;
        let author = reply_author(session_node(actor), command.author.as_deref());
        let can_resolve = command.resolved && self.agent_can_resolve().await;
        let outcome = ReplyOutcome::of(command.resolved, status, can_resolve);

        self.append_review_event(
            run_id,
            EventKind::ReviewCommentReplied,
            json!({
                "id": comment_id,
                "author": author,
                "text": text,
                "proposes_resolution": proposes_resolution(command.resolved, status),
            }),
        )
        .await?;
        if outcome == ReplyOutcome::Resolved {
            self.append_review_event(
                run_id,
                EventKind::ReviewCommentResolved,
                json!({ "id": comment_id, "by": author }),
            )
            .await?;
        }
        info!(
            "run {run_id}: {author} replied to review comment {comment_id} ({})",
            outcome.describe()
        );
        let comment = self.projected_comment(run_id, comment_id).await?;
        Ok(json!({
            "comment": comment,
            "author": author,
            "outcome": outcome,
        }))
    }
}
