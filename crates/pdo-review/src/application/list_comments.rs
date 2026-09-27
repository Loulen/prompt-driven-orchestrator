//! `GET /runs/{run_id}/review/comments[?state=&from=&to=]` (#750, #751, #752).

use pdo_run::domain::run_refs;
use pdo_workspace::adapters::git::revisions::{is_safe_path, is_safe_ref};
use serde_json::{json, Value};

use super::failure::{Failure, Outcome};
use super::ports::ReviewedRun;
use super::review::{default_to_id, resolve_ref, RefTarget, Review};
use crate::domain::comments::anchor::map_comment;
use crate::domain::comments::{ReviewComment, StateFilter};

/// The list's query.
#[derive(Debug, Default)]
pub struct ListComments<'a> {
    /// `open` | `resolved` | `all` (default: the UI wants everything, the
    /// resolved ones collapse client-side).
    pub state: Option<&'a str>,
    /// The displayed pair to **re-map** every comment onto (stable Run ref
    /// ids, defaults `fork` / the destination when only one side is given).
    /// Absent both: no mapping, the comments come back as written.
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
}

impl Review {
    /// The Run's **sent** review comments as projected from the log — the same
    /// list `GET /runs/<id>` carries under `review_comments`, with each
    /// comment's thread — and whether `--resolved` resolves
    /// (`agent_can_resolve`). With a pair, every comment is re-mapped onto it:
    /// `outdated` / `mapped_line` / `moved` are added, computed on read.
    pub async fn list_comments(&self, run_id: &str, q: ListComments<'_>) -> Outcome<Value> {
        let filter = match q.state {
            None => StateFilter::All,
            Some(raw) => StateFilter::parse(raw).ok_or_else(|| {
                Failure::error(
                    400,
                    format!("state must be `open`, `resolved` or `all`, got {raw:?}"),
                )
            })?,
        };
        let run = self.run(run_id).await?;
        let comments: Vec<&ReviewComment> = run
            .state
            .review_comments
            .iter()
            .filter(|c| filter.keeps(c))
            .collect();
        let can_resolve = self.agent_can_resolve().await;
        if q.from.is_none() && q.to.is_none() {
            return Ok(json!({
                "comments": comments,
                "agent_can_resolve": can_resolve,
            }));
        }
        let from = resolve_ref(run_id, &run, q.from, run_refs::FORK_ID)?;
        let to = resolve_ref(run_id, &run, q.to, default_to_id(&run))?;
        let from_sha = self.sha_of(&run, &from);
        let to_sha = self.sha_of(&run, &to);
        Ok(json!({
            "comments": self.mapped(&run, &comments, from_sha.as_deref(), to_sha.as_deref()),
            "agent_can_resolve": can_resolve,
            "from_sha": from_sha,
            "to_sha": to_sha,
        }))
    }

    /// The object a side stands for. #835: the working tree's "SHA" is its
    /// snapshot tree id, a real object of the repository — the two-dot re-map
    /// diffs against it like any ref.
    fn sha_of(&self, run: &ReviewedRun, target: &RefTarget) -> Option<String> {
        match target {
            RefTarget::Git(r) => self.git.rev_parse_object(&run.repo, r),
            RefTarget::Worktree => run
                .worktree
                .as_ref()
                .and_then(|dir| self.git.snapshot_worktree(dir).ok()),
        }
    }

    /// #752 (ADR-0067 §5): each anchor re-mapped onto the pair, on read. A
    /// comment whose anchored side is unchanged between the SHA it was written
    /// against and the displayed one is *reported* (`mapped_line`, possibly
    /// shifted); otherwise it is `outdated`. Nothing is written back.
    fn mapped(
        &self,
        run: &ReviewedRun,
        comments: &[&ReviewComment],
        from_sha: Option<&str>,
        to_sha: Option<&str>,
    ) -> Vec<Value> {
        let diff_path = |orig: &str, target: &str, path: &str| {
            if !is_safe_path(path) || !is_safe_ref(orig) || !is_safe_ref(target) {
                return None;
            }
            self.git.path_diff(&run.repo, orig, target, path).ok()
        };
        comments
            .iter()
            .map(|c| {
                let mut v = serde_json::to_value(c).expect("ReviewComment serializes");
                if let Some(m) = map_comment(c, from_sha, to_sha, &diff_path) {
                    if let (Some(obj), Value::Object(extra)) = (
                        v.as_object_mut(),
                        serde_json::to_value(m).expect("AnchorMapping serializes"),
                    ) {
                        obj.extend(extra);
                    }
                }
                v
            })
            .collect()
    }
}
