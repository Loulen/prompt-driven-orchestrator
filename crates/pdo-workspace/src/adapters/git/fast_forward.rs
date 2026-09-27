//! Fast-forward a local branch onto its tracking branch (#803, ADR-0070 §2): the
//! git side of the rules in [`crate::domain::fast_forward`].

use std::path::Path;

use crate::domain::fast_forward::{
    check_checkout, check_gap, no_upstream, Checkout, FastForwardDone, FastForwardRefusal,
    FastForwardRefused,
};

use super::repository::{
    ahead_behind, dirty_tracked_files, resolve_commit, rev_parse_short, same_checkout, tip_summary,
    upstream_of, worktree_holding_branch,
};

/// Fast-forward `branch` onto its tracking branch, if and only if it is lossless
/// (#803, ADR-0070 §2).
///
/// The conditions are cumulative and checked in the order a person would read them:
/// a tracking branch exists → the branch is strictly behind (no commit of its own to
/// drop) → nobody else's checkout holds it → if OUR checkout holds it, its tracked
/// files are unmodified. Only then does anything move, and what moves is decided by
/// where the branch lives:
///
/// - checked out **here** → `git merge --ff-only` in that checkout, so the working
///   tree walks forward with the ref (`update-ref` alone would leave every fetched
///   commit staged as a deletion — the one way this could destroy work);
/// - checked out **nowhere** → `git update-ref` with the old value as a guard, a pure
///   ref move that touches no file at all.
///
/// Never a `git pull`: that is a fetch *and* a merge, and the merge is exactly the
/// thing ADR-0070 forbids.
pub fn fast_forward_branch(
    repo: &Path,
    branch: &str,
) -> Result<FastForwardDone, FastForwardRefused> {
    let refused = FastForwardRefused::listed;

    let Some(upstream) = upstream_of(repo, branch) else {
        return Err(no_upstream(branch));
    };
    let behind = check_gap(branch, &upstream, ahead_behind(repo, branch, &upstream))?;

    let checkout = match worktree_holding_branch(repo, branch) {
        None => Checkout::Nowhere,
        Some(holder) if same_checkout(&holder, repo) => Checkout::Here {
            dirty: dirty_tracked_files(repo),
        },
        Some(elsewhere) => Checkout::Elsewhere(elsewhere.display().to_string()),
    };
    let here = check_checkout(branch, &repo.display().to_string(), checkout)?;

    let from = rev_parse_short(repo, &format!("refs/heads/{branch}")).unwrap_or_default();
    let target_full = format!("refs/remotes/{upstream}");
    let Some(target_sha) = resolve_commit(repo, &target_full) else {
        return Err(refused(
            FastForwardRefusal::NoUpstream,
            format!("'{upstream}' does not resolve any more — fetch and try again"),
        ));
    };

    let outcome = if here {
        // `--ff-only` can only ever move the ref; it refuses rather than create a
        // merge commit, which is the guarantee ADR-0070 §2 asks for in git's own
        // words rather than in ours.
        std::process::Command::new("git")
            .args(["merge", "--ff-only", "--quiet", &target_full])
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
    } else {
        let old = resolve_commit(repo, &format!("refs/heads/{branch}")).unwrap_or_default();
        // The old value is passed as a guard: if anything moved the branch between
        // our check and this call, git refuses instead of overwriting the winner.
        std::process::Command::new("git")
            .args([
                "update-ref",
                &format!("refs/heads/{branch}"),
                &target_sha,
                &old,
            ])
            .current_dir(repo)
            .output()
    };

    match outcome {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            return Err(refused(
                FastForwardRefusal::Diverged,
                if stderr.is_empty() {
                    format!("git refused to fast-forward '{branch}'")
                } else {
                    stderr
                },
            ));
        }
        Err(e) => {
            return Err(refused(
                FastForwardRefusal::Diverged,
                format!("failed to run git: {e}"),
            ));
        }
    }

    let (to, subject) = tip_summary(repo, &format!("refs/heads/{branch}"));
    Ok(FastForwardDone {
        branch: branch.to_string(),
        upstream,
        from,
        to,
        commits: behind,
        subject,
        moved_checkout: here.then(|| repo.display().to_string()),
    })
}
