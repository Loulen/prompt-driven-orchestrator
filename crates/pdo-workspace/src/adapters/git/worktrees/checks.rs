//! Worktree probes, the Run worktree itself, and the read-only snapshots of
//! secondary repositories (ADR-0042).

use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

/// Is `dir` an existing directory inside a git working tree?
///
/// The precondition of a delivery (#654): a directory git knows nothing about has
/// nothing to deliver, and that is not the same thing as a git operation failing
/// inside a real worktree. Probed with git's own question rather than guessed from
/// an error string.
pub fn is_git_worktree(dir: &std::path::Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output()
        .is_ok_and(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true"
        })
}

pub fn worktree_has_tracked_changes(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .context("git status failed")?;

    let status = String::from_utf8_lossy(&output.stdout);
    Ok(status.lines().any(|line| !line.starts_with("??")))
}

pub fn create_worktree(
    repo_root: &std::path::Path,
    worktree_dir: &std::path::Path,
    branch_name: &str,
    source_ref: &str,
) -> Result<()> {
    std::fs::create_dir_all(worktree_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name])
        .arg(worktree_dir)
        .arg(source_ref)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    info!("Created worktree at {}", worktree_dir.display());
    Ok(())
}

/// Materialise a secondary repo's **read-only snapshot** (#465, ADR-0042).
///
/// Mirror of [`create_worktree`], with one load-bearing difference: `--detach`
/// instead of `-b <branch>`. A detached worktree has **no branch**, so:
/// - there is no ref to `branch -D` at cleanup — only a registration to `prune`;
/// - it sidesteps the #498 class (a sub-worktree branch colliding with another
///   Run's), because two Runs pinning the same secondary create two detached
///   worktrees at different paths and no shared branch name.
///
/// `sha` is the frozen commit (resolved by [`rev_parse_verified`] at Run start);
/// `dest_dir` is [`secondary_snapshot_path`]. Runs with `current_dir` set to the
/// **secondary** repo (that is the repo the worktree registers into), and shares
/// its object store — the snapshot costs no full clone.
pub fn create_secondary_snapshot(secondary_repo: &Path, dest_dir: &Path, sha: &str) -> Result<()> {
    std::fs::create_dir_all(dest_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(dest_dir)
        .arg(sha)
        .current_dir(secondary_repo)
        .output()
        .context("failed to run git worktree add --detach for a secondary snapshot")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add --detach (secondary) failed: {stderr}");
    }

    info!(
        "Created secondary snapshot at {} (detached @ {sha}) from {}",
        dest_dir.display(),
        secondary_repo.display()
    );
    Ok(())
}

/// Tear down one secondary snapshot at cleanup (#465, ADR-0042).
///
/// Best-effort like [`reap_orphan_sub_worktree`] and `cleanup_run` — a missing
/// snapshot / secondary is fine — but the **`prune` is never skipped**: the
/// worktree registration lives in the *secondary* repo's `.git`, OUTSIDE
/// `repo_root`, so the Run's `remove_dir_all(<run_dir>)` cannot clear it. Without
/// the prune, every multi-repo Run leaves a dangling `--detach` registration in
/// the secondary (the #498 class), and a future `worktree add` at the same path
/// fails 255.
///
/// `current_dir` is the **secondary** repo, since that is where the registration
/// and `prune` operate.
pub fn remove_secondary_snapshot(secondary_repo: &Path, snapshot_dir: &Path) {
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(snapshot_dir)
        .current_dir(secondary_repo)
        .output();
    // The prune is the part `cleanup_run` cannot do from the primary: it clears the
    // dangling registration in the secondary's .git even if the dir is already gone.
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(secondary_repo)
        .output();
    if snapshot_dir.exists() {
        let _ = std::fs::remove_dir_all(snapshot_dir);
    }
    info!(
        "Reaped secondary snapshot {} from {} (worktree remove + prune, #465/#498)",
        snapshot_dir.display(),
        secondary_repo.display()
    );
}
