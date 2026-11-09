//! `POST /runs/{run_id}/review/comments/resend` (CORE-239): « Send again » —
//! hand comments the manager never answered to it once more, as written.

use std::collections::HashSet;

use pdo_kernel::Actor;
use pdo_run::domain::event::EventKind;
use pdo_run::domain::review_comment::ReviewDeliveryState;
use serde_json::{json, Value};
use tracing::info;

use super::labels;
use crate::application::delivery::{self, Handover};
use crate::application::failure::{Failure, Outcome};
use crate::application::operations;
use crate::application::review::{find_comment, Review};
use crate::domain::comments::delivery::DELIVERY_AWAITING;
use crate::domain::comments::message::batch_message;
use crate::domain::comments::{ReviewComment, ReviewCommentStatus};

impl Review {
    /// Hand `ids` to the Pipeline Manager again, without rewriting them: the
    /// manager is started on demand (the batch in its launch prompt) or the
    /// batch is typed into the running one, a `review_comment_delivery`
    /// `awaiting` puts them back on their way, and the delivery follows them
    /// as it follows a send. The same right as a send: it hands comments to
    /// the manager. A comment already answered, or resolved, is refused
    /// (`409`): there is nothing to send again; so is one still on its way to
    /// a running manager (it would reach it twice).
    pub async fn resend_comments(
        &self,
        actor: &Actor,
        run_id: &str,
        ids: Vec<String>,
    ) -> Outcome<Value> {
        self.authorize(actor, &operations::send(run_id)).await?;
        let run = self.run(run_id).await?;
        if ids.is_empty() {
            return Err(Failure::error(400, "no comments to send again"));
        }
        self.refuse_gone_branch(run_id, &run)?;
        let manager_up = self.runs.manager_screen(run_id).is_some();
        let mut seen = HashSet::new();
        let mut comments: Vec<ReviewComment> = Vec::new();
        for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
            let c = find_comment(&run, id)?;
            if c.status == ReviewCommentStatus::Resolved || !c.replies.is_empty() {
                return Err(Failure::error(
                    409,
                    format!("{id} was already answered — nothing to send again"),
                ));
            }
            // A comment still on its way to a running manager would reach it twice.
            let lost = c
                .delivery
                .as_ref()
                .is_some_and(|d| d.state == ReviewDeliveryState::Exited);
            if manager_up && !lost {
                return Err(Failure::error(
                    409,
                    format!("{id} is on its way to the running manager — nothing to send again"),
                ));
            }
            comments.push(c.clone());
        }
        let snapshot = run
            .worktree
            .as_ref()
            .map(|dir| self.git.snapshot_worktree(dir).ok());
        let (from_label, to_label) = labels(self, run_id, &run, snapshot.as_ref(), &comments[0]);
        let message = batch_message(run_id, &from_label, &to_label, &comments);
        let created = self
            .runs
            .ensure_manager(run_id, &run.state, actor, &message)
            .await
            .map_err(|(status, body)| Failure::Json(status, body))?;

        let ids: Vec<String> = comments.iter().map(|c| c.id.clone()).collect();
        let actor_json = serde_json::to_value(actor).unwrap_or(Value::Null);
        self.append_review_event(
            run_id,
            EventKind::ReviewCommentDelivery,
            json!({ "ids": ids, "state": DELIVERY_AWAITING, "actor": actor_json }),
        )
        .await?;

        delivery::spawn(
            self.runs.clone(),
            Handover {
                run_id: run_id.to_string(),
                ids: ids.clone(),
                paste: (!created).then_some(message),
            },
        );
        info!(
            "run {run_id}: review comment(s) {ids:?} sent to the manager again{}",
            if created { ", manager started" } else { "" }
        );
        Ok(json!({ "resent": ids, "manager_started": created }))
    }
}
