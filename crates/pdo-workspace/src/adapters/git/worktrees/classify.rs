//! Classify a node's sub-worktree without mutating anything (#489-B).

use super::*;

/// Classify a node's sub-worktree **without mutating anything** (#489-B).
///
/// Pure read path: safe to call from the pre-kill probes of `restart_node`, which
/// is the whole point — an `Occupied` verdict must be knowable before a session is
/// destroyed.
pub fn classify_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
) -> SubWorktreeState {
    let want_ref = format!("refs/heads/{sub_branch}");
    let target = comparable(sub_worktree_dir);
    let records = registered_worktrees(repo_root);

    // The branch checked out somewhere ELSE, and that somewhere is still live.
    if let Some(other) = records
        .iter()
        .find(|r| r.branch.as_deref() == Some(&want_ref) && comparable(&r.path) != target)
    {
        if other.prunable.is_none() {
            return SubWorktreeState::Occupied {
                detail: format!(
                    "branch {sub_branch} is checked out in another worktree at {}",
                    other.path.display()
                ),
            };
        }
    }

    match records.iter().find(|r| comparable(&r.path) == target) {
        Some(own) => {
            if let Some(reason) = own.prunable.as_deref() {
                return SubWorktreeState::Recyclable {
                    detail: format!(
                        "git reports the worktree registration for {} prunable: {reason}",
                        sub_worktree_dir.display()
                    ),
                };
            }
            match own.branch.as_deref() {
                Some(branch) if branch == want_ref => SubWorktreeState::Reusable {
                    has_work: has_any_change(sub_worktree_dir),
                    base_moved: !base_is_ancestor(repo_root, base_branch, &want_ref),
                    interrupted_git_ops: interrupted_git_ops_in(sub_worktree_dir),
                },
                // Detached HEAD at our own path: only PDO ever creates a worktree
                // there, so its branch was deleted from under it. Nothing to keep.
                None => SubWorktreeState::Recyclable {
                    detail: format!(
                        "worktree at {} is detached, not on {sub_branch}",
                        sub_worktree_dir.display()
                    ),
                },
                // A live worktree at OUR path on a DIFFERENT named branch — the
                // agent ran `git checkout -b feature/…` following a project
                // git-flow (ADR-0050 §3 / #599). This IS the node's work, so it is
                // `Reusable`, never `Occupied`: PDO encaisses the agent's git
                // (Sharp tool, ADR-0001) rather than refusing it. `Occupied` is
                // reserved for a branch checked out in ANOTHER live worktree
                // (handled at the top of this function). The merge-back follows the
                // real HEAD (`node_tip`), not the `pdo/sub-*` name, so the commits
                // on `feature/…` still land (ADR-0036 amended). `base_moved` is read
                // against the worktree's actual HEAD here, since that — not the
                // stale `pdo/sub-*` ref — is what carries the work.
                Some(_other) => SubWorktreeState::Reusable {
                    has_work: has_any_change(sub_worktree_dir),
                    base_moved: !head_descends_from_base(sub_worktree_dir, base_branch),
                    interrupted_git_ops: interrupted_git_ops_in(sub_worktree_dir),
                },
            }
        }
        None => {
            if sub_worktree_dir.exists() && !dir_is_empty(sub_worktree_dir) {
                return SubWorktreeState::Occupied {
                    detail: format!(
                        "{} exists and is not a registered git worktree",
                        sub_worktree_dir.display()
                    ),
                };
            }
            if branch_ref_exists(repo_root, &want_ref) {
                // The #498 shape: the branch ref outlives its worktree, and
                // `worktree add -b` refuses it (exit 255) for ever.
                return SubWorktreeState::Recyclable {
                    detail: format!(
                        "branch {sub_branch} exists with no worktree registered at {}",
                        sub_worktree_dir.display()
                    ),
                };
            }
            SubWorktreeState::Absent
        }
    }
}

/// Anything at all in the tree, **untracked included** — the opposite polarity to
/// [`worktree_has_tracked_changes`], which deliberately ignores `??` lines.
pub fn has_any_change(worktree_dir: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

pub fn base_is_ancestor(repo_root: &std::path::Path, base_branch: &str, sub_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", base_branch, sub_ref])
        .current_dir(repo_root)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Is the sub-worktree's **actual HEAD** still a descendant of the cut base?
///
/// Run inside the worktree so `HEAD` peels to whatever branch the agent switched
/// it onto (`feature/…`), not the stale `pdo/sub-*` ref. `base_branch` resolves in
/// a linked worktree — it shares the repo's refs (#599 / ADR-0050 §3).
pub fn head_descends_from_base(sub_worktree_dir: &std::path::Path, base_branch: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", base_branch, "HEAD"])
        .current_dir(sub_worktree_dir)
        .output()
        .is_ok_and(|out| out.status.success())
}

pub fn branch_ref_exists(repo_root: &std::path::Path, full_ref: &str) -> bool {
    std::process::Command::new("git")
        .args(["show-ref", "--verify", "--quiet", full_ref])
        .current_dir(repo_root)
        .output()
        .is_ok_and(|out| out.status.success())
}
