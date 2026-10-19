//! Steps 1–2 of a send: the Run branch must still exist, and every draft is
//! validated and anchored before anything is touched.

use pdo_kernel::Actor;
use pdo_run::domain::run_refs::{self, Resolved};
use pdo_run::domain::state::RunStatus;
use pdo_workspace::adapters::git::revisions::is_safe_path;
use serde_json::json;

use super::DraftComment;
use crate::application::failure::{Failure, Outcome};
use crate::application::ports::ReviewedRun;
use crate::application::review::{default_to_id, Review};
use crate::domain::comments::{excerpt, ReviewComment, ReviewCommentStatus, ReviewSide};

/// Context lines kept on each side of the anchored line in the excerpt.
const EXCERPT_CONTEXT: usize = 2;

impl Review {
    /// 1. Branch gone ⇒ sending is disabled, with the reason: an archived Run,
    ///    or a `pdo/run-<id>` branch that is gone, answers `409
    ///    run_branch_gone` (nothing for the manager to act on).
    pub(super) fn refuse_gone_branch(&self, run_id: &str, run: &ReviewedRun) -> Outcome<()> {
        let tip = run_refs::tip_branch(run_id);
        let gone = run.state.status == RunStatus::Archived
            || self.git.rev_parse(&run.repo, &tip).is_none();
        if !gone {
            return Ok(());
        }
        Err(Failure::Json(
            409,
            json!({
                "error": "run_branch_gone",
                "recoverable": false,
                "message": format!(
                    "Run branch {tip} no longer exists — nothing for the manager to act on. Comments stay readable; sending is disabled."
                ),
            }),
        ))
    }

    /// 2. Every anchor validated (safe path, `old|new`, line ≥ 1, non-empty
    ///    text, known ref ids — `400` names the offending index) and resolved:
    ///    its SHAs and its excerpt. An anchor on the working tree freezes its
    ///    `snapshot` tree id (#835). The ids are assigned once the manager is up.
    ///    Each comment keeps the `actor` that sends it (CORE-63).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &self,
        actor: &Actor,
        run_id: &str,
        run: &ReviewedRun,
        drafts: &[DraftComment],
        snapshot: Option<&str>,
        sent_at: &str,
        batch_id: &str,
    ) -> Outcome<Vec<ReviewComment>> {
        let sha_of = |r: &str| self.git.rev_parse_object(&run.repo, r);
        let resolve = |id: &str| -> Option<String> {
            match run_refs::resolve_id(run_id, &run.state, &run.events, id) {
                Resolved::Git(r) => Some(r),
                Resolved::Worktree => snapshot.map(str::to_string),
                _ => None,
            }
        };
        let bad = |i: usize, why: String| Failure::error(400, format!("comment #{}: {why}", i + 1));
        let mut prepared = Vec::with_capacity(drafts.len());
        for (i, c) in drafts.iter().enumerate() {
            if !is_safe_path(&c.path) {
                return Err(bad(i, format!("invalid path {:?}", c.path)));
            }
            let Some(side) = ReviewSide::parse(&c.side) else {
                return Err(bad(
                    i,
                    format!("side must be `old` or `new`, got {:?}", c.side),
                ));
            };
            if c.line < 1 {
                return Err(bad(i, "line must be ≥ 1".into()));
            }
            if c.text.trim().is_empty() {
                return Err(bad(i, "empty text".into()));
            }
            let from_id = c.from.as_deref().unwrap_or(run_refs::FORK_ID);
            let to_id = c.to.as_deref().unwrap_or(default_to_id(run));
            let Some(from_git) = resolve(from_id) else {
                return Err(bad(i, format!("unknown run ref {from_id:?}")));
            };
            let Some(to_git) = resolve(to_id) else {
                return Err(bad(i, format!("unknown run ref {to_id:?}")));
            };
            // The excerpt reads the anchored side's file at its ref.
            // Best-effort: a path that no longer resolves sends with an empty
            // excerpt.
            let anchored_ref = match side {
                ReviewSide::Old => &from_git,
                ReviewSide::New => &to_git,
            };
            let excerpt = self
                .git
                .file_at_ref(&run.repo, anchored_ref, &c.path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|content| excerpt(&content, c.line, EXCERPT_CONTEXT))
                .unwrap_or_default();
            prepared.push(ReviewComment {
                id: String::new(),
                path: c.path.clone(),
                side,
                line: c.line,
                from_ref: from_id.to_string(),
                to_ref: to_id.to_string(),
                from_sha: sha_of(&from_git),
                to_sha: sha_of(&to_git),
                text: c.text.trim_end().to_string(),
                excerpt,
                author: "user".to_string(),
                actor: Some(actor.clone()),
                sent_at: sent_at.to_string(),
                batch_id: Some(batch_id.to_string()),
                status: ReviewCommentStatus::Sent,
                replies: Vec::new(),
                proposal_pending: false,
                resolved_by: None,
                resolved_at: None,
                resolved_by_actor: None,
                reopened_by: None,
                reopened_at: None,
                reopened_by_actor: None,
                proposal_declined: false,
            });
        }
        Ok(prepared)
    }
}
