//! Where a Run's worktrees live, and what their branches are called: the
//! canonical path math every other context relies on (ADR-0009).

use std::path::{Path, PathBuf};

pub fn worktree_dir_for_run(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

pub fn sub_worktree_path(
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("iter-{iter}"))
}

pub fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo/sub-{run_id}-{node_id}-iter-{iter}")
}

/// On-disk location of a secondary repo's read-only snapshot (#465, ADR-0042).
///
/// A **third sibling** of `worktree/` and `nodes/` under the Run directory:
/// `<repo_root>/.pdo/runs/<run_id>/repos/<alias>/`. Living under `repo_root`
/// (the primary) is deliberate — the sandbox already bind-mounts `repo_root` at
/// an identical path (invariant D3), so the snapshot is visible in-sandbox with
/// **no new mount**, and `remove_dir_all(<run_dir>)` at cleanup reclaims it.
///
/// `alias` MUST already be disambiguated by the caller (two secondaries with the
/// same basename would otherwise collide here) — the path never re-derives it
/// from the repo basename.
pub fn secondary_snapshot_path(repo_root: &Path, run_id: &str, alias: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("repos")
        .join(alias)
}

/// The message PDO puts on the commit it creates for a NodeRun's leftover changes
/// (#654 / ADR-0060). Deterministic, and the **only** thing PDO writes on it: no
/// trailer, no author override — the delivery uses the configured git identity, so
/// a repo's own hooks and conventions see an ordinary commit.
pub fn delivery_commit_message(node_id: &str, iter: i64) -> String {
    format!("{node_id} iter-{iter}: completed")
}

/// The run's artifact store, relative to its worktree root. Written by PDO for
/// every node's ports (`<node>/iter-<N>/<port>/…`) and read back by the inspector;
/// never a deliverable, so the delivery keeps it out of the Run branch whatever
/// the target repo's `.gitignore` says (#796).
pub const RUN_ARTIFACTS_RELATIVE: &str = ".pdo/artifacts";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_dir_for_run_follows_canonical_schema() {
        assert_eq!(
            worktree_dir_for_run(Path::new("/repo"), "r1"),
            PathBuf::from("/repo/.pdo/runs/r1/worktree")
        );
    }

    #[test]
    fn sub_worktree_path_follows_canonical_schema() {
        let path = sub_worktree_path(Path::new("/repo"), "20260101-120000-abc", "impl-1", 1);
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/runs/20260101-120000-abc/nodes/impl-1/iter-1")
        );
    }

    #[test]
    fn sub_worktree_branch_name() {
        let branch = sub_worktree_branch("20260101-120000-abc", "impl-1", 1);
        assert_eq!(branch, "pdo/sub-20260101-120000-abc-impl-1-iter-1");
    }

    #[test]
    fn secondary_snapshots_sit_beside_the_worktree() {
        assert_eq!(
            secondary_snapshot_path(Path::new("/repo"), "r1", "api"),
            PathBuf::from("/repo/.pdo/runs/r1/repos/api")
        );
    }

    #[test]
    fn the_delivery_commit_names_the_node_and_iteration() {
        assert_eq!(delivery_commit_message("impl", 2), "impl iter-2: completed");
    }
}
