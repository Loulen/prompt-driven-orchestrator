//! `POST /runs/{run_id}/review/comments/send` (#750, ADR-0067 §2–3; CONTEXT.md
//! « Envoi au manager »): turn a batch of browser drafts into `sent` comments.

mod prepare;

use pdo_kernel::Actor;
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::run_refs;
use serde_json::{json, Value};
use tracing::{error, info};

use super::failure::{Failure, Outcome};
use super::operations;
use super::ports::ReviewedRun;
use super::review::Review;
use crate::domain::comments::message::batch_message;
use crate::domain::comments::{comment_id, ReviewComment};

/// One draft the browser hands over. `from`/`to` are stable Run ref ids
/// (default `fork` → the destination), never SHAs.
#[derive(Debug, Clone, Default)]
pub struct DraftComment {
    pub path: String,
    pub side: String,
    pub line: i64,
    pub from: Option<String>,
    pub to: Option<String>,
    pub text: String,
}

impl Review {
    /// Order of operations, each step a reason the drafts **stay drafts**:
    /// 1. the Run branch must still exist (`409 run_branch_gone`);
    /// 2. every anchor is validated (`400` names the offending index);
    /// 3. the Pipeline Manager is started **on demand** if it is not running
    ///    (its refusals pass through);
    /// 4. one `review_comment_sent` event per comment lands in the log (ids
    ///    `rc-NNN` continue the Run's sequence; the batch shares a `batch_id`);
    /// 5. **one** message for the whole batch is pasted into the manager's
    ///    session, detached — after a short grace when this call spawned it.
    ///
    /// The answer carries the sent comments so the browser can drop its
    /// drafts without waiting for the WebSocket round-trip (which also
    /// arrives: every append broadcasts).
    pub async fn send_comments(
        &self,
        actor: &Actor,
        run_id: &str,
        drafts: Vec<DraftComment>,
    ) -> Outcome<Value> {
        self.authorize(actor, &operations::send(run_id)).await?;
        let run = self.run(run_id).await?;
        if drafts.is_empty() {
            return Err(Failure::error(400, "no comments to send"));
        }
        self.refuse_gone_branch(run_id, &run)?;
        let snapshot = run
            .worktree
            .as_ref()
            .map(|dir| self.git.snapshot_worktree(dir).ok());
        let now = pdo_kernel::clock::now_iso();
        let batch_id = uuid::Uuid::new_v4().to_string();
        let mut prepared = self.prepare(
            run_id,
            &run,
            &drafts,
            snapshot.clone().flatten().as_deref(),
            &now,
            &batch_id,
        )?;

        // 3. Manager on demand — before any event, so a refusal leaves the
        //    drafts intact.
        let created = self
            .runs
            .ensure_manager(run_id, &run.state)
            .await
            .map_err(|(status, body)| Failure::Json(status, body))?;

        // 4. Durable first: one event per comment, ids continuing the Run's
        //    sequence.
        let base = run.state.review_comments.len();
        for (i, c) in prepared.iter_mut().enumerate() {
            c.id = comment_id(base + i + 1);
            self.record_sent(run_id, &now, c, i).await?;
        }

        // 5. One message for the batch, handed over off the request.
        let (from_label, to_label) = labels(self, run_id, &run, snapshot.as_ref(), &prepared[0]);
        let message = batch_message(run_id, &from_label, &to_label, &prepared);
        self.runs.hand_to_manager(run_id, message, created);
        info!(
            "run {run_id}: {} review comment(s) sent to the manager as one message (batch {batch_id}{})",
            prepared.len(),
            if created { ", manager started" } else { "" }
        );
        Ok(json!({
            "sent": prepared,
            "batch_id": batch_id,
            "manager_started": created,
        }))
    }

    async fn record_sent(
        &self,
        run_id: &str,
        now: &str,
        c: &ReviewComment,
        recorded: usize,
    ) -> Outcome<()> {
        let event = Event {
            id: None,
            run_id: run_id.to_string(),
            ts: now.to_string(),
            kind: EventKind::ReviewCommentSent,
            node_id: None,
            iter: None,
            payload: Some(serde_json::to_value(c).expect("ReviewComment serializes")),
        };
        self.runs.append(&event).await.map_err(|e| {
            error!(
                "run {run_id}: failed to append review_comment_sent {}: {e}",
                c.id
            );
            Failure::error(
                500,
                format!(
                    "failed to record comment {} ({recorded} of the batch recorded): {e}",
                    c.id
                ),
            )
        })
    }
}

/// The labels of the batch's pair, as the Run's refs name them (the UI and the
/// CLI agree on them).
fn labels(
    review: &Review,
    run_id: &str,
    run: &ReviewedRun,
    snapshot: Option<&Option<String>>,
    first: &ReviewComment,
) -> (String, String) {
    let sha_of = |r: &str| review.git.rev_parse_object(&run.repo, r);
    let refs = run_refs::collect(
        run_id,
        &run.state,
        &run.events,
        &sha_of,
        snapshot.map(Option::as_deref),
    );
    let label_of = |id: &str| -> String {
        refs.refs
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    (label_of(&first.from_ref), label_of(&first.to_ref))
}
