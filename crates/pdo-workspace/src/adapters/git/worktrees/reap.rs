//! Reap what a spawn that aborted left behind: a sub-worktree, its branch, its
//! registration.

use tracing::info;

/// Reap a sub-worktree + branch left orphaned by a spawn that aborted before
/// `NodeStarted` (#279), or a registration that has already lost all value
/// ([`SubWorktreeState::Recyclable`], #489). Best-effort throughout (mirrors
/// `cleanup_run`): a missing dir / branch is fine.
///
/// **`worktree prune` before `branch -D`, and the `remove` is unconditional**
/// (#489 / #498). The pre-#489 form skipped `worktree remove` when the directory
/// was already gone, which left git's *registration* in place — and a registered
/// worktree pins its branch:
///
/// ```text
/// $ git branch -D pdo/sub-A3
/// error: cannot delete branch 'pdo/sub-A3' used by worktree at '…/nodes/A3/iter-1'   # exit 1
/// ```
///
/// So the old reap left **both** locks standing and the next `worktree add -b`
/// still failed 255. Measured: either `worktree prune` or an unconditional
/// `worktree remove --force` clears the registration; doing the prune too is free
/// and covers the dir-already-deleted case the `remove` cannot.
pub fn reap_orphan_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
) {
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(sub_worktree_dir)
        .current_dir(repo_root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", sub_branch])
        .current_dir(repo_root)
        .output();
    // `worktree remove` unlinks the directory it knew about; a directory git never
    // registered (or one left behind by a failed remove) is ours to clear.
    if sub_worktree_dir.exists() {
        let _ = std::fs::remove_dir_all(sub_worktree_dir);
    }

    info!(
        "Reaped sub-worktree {} (branch {sub_branch}) — nothing of value was there (#279/#489)",
        sub_worktree_dir.display()
    );
}
