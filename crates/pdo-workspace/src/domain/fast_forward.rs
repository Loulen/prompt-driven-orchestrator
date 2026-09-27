//! Fast-forward of a local branch onto its tracking branch (#803, ADR-0070 §2).
//!
//! The ONLY write PDO ever performs on a local branch, and only on an explicit
//! click. Never a merge commit, never a rebase, never a push: either the ref moves
//! on its own (branch not checked out anywhere) or the checkout walks forward with
//! it. Both are reversible by reflog.

use serde::Serialize;
use ts_rs::TS;

use super::branches::BranchList;

/// Why a fast-forward did not happen, named rather than raw (#803).
///
/// Three of these — `diverged`, `no_upstream`, `up_to_date` — are already knowable
/// from the enriched list (#802), so the UI does not even offer the button for them;
/// they are answered here only for the RACE between the list and the click. The other
/// two need the checkout inspected at click time, which is precisely why they cannot
/// be pre-computed: a tree can get dirty between a render and a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "FastForwardReason")]
#[serde(rename_all = "snake_case")]
pub enum FastForwardRefusal {
    /// The branch is checked out HERE and tracked files are modified.
    DirtyTree,
    /// The branch is checked out in ANOTHER worktree — not ours to move.
    CheckedOutElsewhere,
    /// The branch has commits the upstream does not: a ff would drop them.
    Diverged,
    /// The branch tracks nothing, so there is nothing to fast-forward to.
    NoUpstream,
    /// The upstream has nothing this branch lacks.
    UpToDate,
}

/// A refused fast-forward, with everything the popover needs to name it (#803).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "FastForwardRefusal")]
pub struct FastForwardRefused {
    pub reason: FastForwardRefusal,
    pub message: String,
    /// The checkout the branch sits in — ours for `dirty_tree`, someone else's for
    /// `checked_out_elsewhere`. `None` for the three list-knowable reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub checkout: Option<String>,
    /// `M path` lines of the modified TRACKED files, capped (`dirty_tree` only).
    /// Untracked files never appear: they do not block a fast-forward.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub dirty_files: Vec<String>,
    /// How many tracked files are modified, uncapped — `dirty_files` may be shorter.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dirty_count: Option<u32>,
}

/// How many `git status` lines travel in a refusal. The popover shows a short
/// listing, not a diff: past a handful the count alone says it.
pub const DIRTY_FILES_SHOWN: usize = 8;

/// What a successful fast-forward moved (#803) — the S3 card's sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "FastForwardResult")]
pub struct FastForwardDone {
    pub branch: String,
    pub upstream: String,
    /// Short SHA the branch sat on before.
    pub from: String,
    /// Short SHA it sits on now.
    pub to: String,
    /// Commits gained — the number that was the `n↓` before the click.
    pub commits: u32,
    /// The new tip's commit subject, for the `from → to · subject` line.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub subject: Option<String>,
    /// The checkout whose FILES moved with the ref, when the branch was checked
    /// out in the repo we launch from. `None` = a pure ref move, nothing on disk
    /// changed — a distinction the popover states out loud before and after.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub moved_checkout: Option<String>,
}

/// The 200 of `POST /repos/fast-forward`: what moved, plus the list refreshed from
/// the same repo, so the client replaces its branches in place instead of asking
/// again (and reading a list from between the two calls).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct FastForwardOk {
    pub fast_forward: FastForwardDone,
    #[serde(flatten)]
    pub list: BranchList,
}

/// The 409 of `POST /repos/fast-forward`: the named refusal, plus the SAME refreshed
/// list. The list travels with a refusal too, because two of the five reasons are
/// races the client recovers from by simply re-reading the truth (`up_to_date`,
/// `no_upstream`): it flips to the corresponding #802 state instead of painting an
/// error for something benign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct FastForwardRefusedBody {
    pub refusal: FastForwardRefused,
    #[serde(flatten)]
    pub list: BranchList,
}
/// Where the branch to fast-forward is checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    /// In no worktree: moving the ref touches no file.
    Nowhere,
    /// In the repository we launch from, with its modified TRACKED files.
    Here { dirty: Vec<String> },
    /// In another worktree, at that path.
    Elsewhere(String),
}

impl FastForwardRefused {
    /// A refusal the enriched list could already tell: no checkout involved.
    pub fn listed(reason: FastForwardRefusal, message: String) -> Self {
        Self {
            reason,
            message,
            checkout: None,
            dirty_files: Vec::new(),
            dirty_count: None,
        }
    }
}

/// The refusal of a branch that tracks nothing.
pub fn no_upstream(branch: &str) -> FastForwardRefused {
    FastForwardRefused::listed(
        FastForwardRefusal::NoUpstream,
        format!("'{branch}' tracks no branch, so there is nothing to fast-forward to"),
    )
}

/// Check the gap between `branch` and its `upstream`: the number of commits a
/// fast-forward takes, or why there is none to take.
///
/// Order matters: "behind 0" is `up_to_date` even when ahead > 0 — there is
/// genuinely nothing upstream to take. Divergence is only a refusal when a
/// fast-forward would have had somewhere to go and local commits in the way.
pub fn check_gap(
    branch: &str,
    upstream: &str,
    gap: Option<(u32, u32)>,
) -> Result<u32, FastForwardRefused> {
    let Some((ahead, behind)) = gap else {
        return Err(FastForwardRefused::listed(
            FastForwardRefusal::NoUpstream,
            format!("the gap between '{branch}' and '{upstream}' could not be read"),
        ));
    };
    if behind == 0 {
        return Err(FastForwardRefused::listed(
            FastForwardRefusal::UpToDate,
            format!("'{branch}' already has everything '{upstream}' has"),
        ));
    }
    if ahead > 0 {
        return Err(FastForwardRefused::listed(
            FastForwardRefusal::Diverged,
            format!(
                "'{branch}' has {ahead} commit(s) '{upstream}' does not: a fast-forward \
                 would drop them"
            ),
        ));
    }
    Ok(behind)
}

/// Check where the branch sits: `Ok(true)` when the checkout HERE moves with the
/// ref, `Ok(false)` for a pure ref move, or why nothing may move.
pub fn check_checkout(
    branch: &str,
    repo: &str,
    checkout: Checkout,
) -> Result<bool, FastForwardRefused> {
    match checkout {
        Checkout::Nowhere => Ok(false),
        Checkout::Elsewhere(elsewhere) => Err(FastForwardRefused {
            reason: FastForwardRefusal::CheckedOutElsewhere,
            message: format!(
                "'{branch}' is checked out in another worktree; PDO only fast-forwards a \
                 branch in the repository you launch from"
            ),
            checkout: Some(elsewhere),
            dirty_files: Vec::new(),
            dirty_count: None,
        }),
        Checkout::Here { dirty } if dirty.is_empty() => Ok(true),
        Checkout::Here { dirty } => Err(FastForwardRefused {
            reason: FastForwardRefusal::DirtyTree,
            message: format!(
                "'{branch}' is checked out here and {} tracked file(s) are modified; \
                 PDO will not touch them",
                dirty.len()
            ),
            checkout: Some(repo.to_string()),
            dirty_count: Some(dirty.len() as u32),
            dirty_files: dirty.into_iter().take(DIRTY_FILES_SHOWN).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_branch_is_up_to_date_even_when_ahead() {
        let refusal = check_gap("main", "origin/main", Some((2, 0))).unwrap_err();
        assert_eq!(refusal.reason, FastForwardRefusal::UpToDate);
    }

    #[test]
    fn a_branch_ahead_and_behind_has_diverged() {
        let refusal = check_gap("main", "origin/main", Some((1, 3))).unwrap_err();
        assert_eq!(refusal.reason, FastForwardRefusal::Diverged);
        assert_eq!(check_gap("main", "origin/main", Some((0, 3))), Ok(3));
    }

    #[test]
    fn an_unreadable_gap_is_no_upstream() {
        let refusal = check_gap("main", "origin/main", None).unwrap_err();
        assert_eq!(refusal.reason, FastForwardRefusal::NoUpstream);
        assert_eq!(no_upstream("main").reason, FastForwardRefusal::NoUpstream);
    }

    #[test]
    fn a_dirty_checkout_here_is_refused_with_a_capped_listing() {
        let dirty: Vec<String> = (0..10).map(|i| format!(" M f{i}")).collect();
        let refusal = check_checkout("main", "/repo", Checkout::Here { dirty }).unwrap_err();
        assert_eq!(refusal.reason, FastForwardRefusal::DirtyTree);
        assert_eq!(refusal.dirty_count, Some(10));
        assert_eq!(refusal.dirty_files.len(), DIRTY_FILES_SHOWN);
        assert_eq!(refusal.checkout.as_deref(), Some("/repo"));
    }

    #[test]
    fn a_clean_checkout_moves_and_nowhere_is_a_ref_move() {
        assert_eq!(
            check_checkout("main", "/repo", Checkout::Here { dirty: vec![] }),
            Ok(true)
        );
        assert_eq!(
            check_checkout("main", "/repo", Checkout::Nowhere),
            Ok(false)
        );
        let refusal =
            check_checkout("main", "/repo", Checkout::Elsewhere("/other".into())).unwrap_err();
        assert_eq!(refusal.reason, FastForwardRefusal::CheckedOutElsewhere);
        assert_eq!(refusal.checkout.as_deref(), Some("/other"));
    }
}
