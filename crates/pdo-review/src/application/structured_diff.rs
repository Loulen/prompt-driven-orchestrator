//! `GET /runs/{run_id}/diff/structured[?from=<ref>&to=<ref>]` (#748, ADR-0067).

use pdo_run::domain::run_refs;

use super::failure::{Failure, Outcome};
use super::review::{default_to_id, resolve_ref, RefTarget, Review};
use crate::domain::diff::StructuredDiff;

impl Review {
    /// Files → hunks → lines for a pair of Run refs, computed in the Run's
    /// **effective** repository. With no pair it is fork → worktree while the
    /// Run's worktree exists (#835: commits *and* uncommitted edits, so a
    /// running node needs no commit to be reviewed), fork → tip once it is
    /// gone; either way the range is three-dot with `.pdo/` excluded — the
    /// bounds of the LOC stat, so "counted" and "shown" agree. An explicit pair
    /// is compared two-dot (two arbitrary Run refs: a node's `before`/`after`, …).
    pub async fn structured_diff(
        &self,
        run_id: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Outcome<StructuredDiff> {
        let run = self.run(run_id).await?;
        // #749: a side is a stable Run ref id (`fork`, `tip`, `worktree`,
        // `node:<id>:<iter>:before`, `live:<id>`) resolved against the log, or —
        // compatibility — a raw git ref.
        let from_ref = match resolve_ref(run_id, &run, from, run_refs::FORK_ID)? {
            RefTarget::Git(r) => r,
            RefTarget::Worktree => {
                return Err(Failure::Text(
                    400,
                    "the working tree can only be the destination of a diff".into(),
                ));
            }
        };
        let to_ref = resolve_ref(run_id, &run, to, default_to_id(&run))?;
        // Three-dot only for the fork → tip/worktree default (the LOC bounds);
        // any other pair is two arbitrary Run refs compared two-dot.
        let three_dot = run_refs::is_default_pair(from, to);
        let result = match to_ref {
            RefTarget::Git(to_ref) => self
                .git
                .structured(&run.repo, &from_ref, &to_ref, three_dot),
            RefTarget::Worktree => {
                let Some(dir) = run.worktree.as_ref() else {
                    return Err(Failure::Text(404, "run worktree not found".into()));
                };
                self.git
                    .structured_worktree(dir, &from_ref, run_refs::WORKTREE_ID, three_dot)
            }
        };
        result.map_err(|e| {
            if e.unknown_revision {
                Failure::Text(404, "run branch not found".into())
            } else {
                Failure::Text(500, e.message)
            }
        })
    }
}
