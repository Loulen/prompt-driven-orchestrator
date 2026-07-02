//! Pure git/fs worktree lifecycle helpers.
//!
//! Carved out of the `lib.rs` god-file (issue #276, Slice-1), mirroring the
//! `run_advance` carve (#235/#275). These are the effect substrate *below*
//! layer 1 of ADR-0009: canonical path math for run/sub worktrees plus the
//! `git worktree add` / `git merge` shell-outs that create, merge, validate and
//! reap them. No `AppState`, no async, no event log, no tmux — only `&Path` /
//! `&str` / `i64` in, path math or a shell-out to `git`/`std::fs` out.
//!
//! Keep this module a pure worktree-lifecycle surface: `MergeResult` (the git
//! *effect*) belongs here; it is deliberately distinct from `MergeOutcome`
//! (`merge_action.rs` — the pure merge *decision* type). Do not conflate them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

/// Reap a sub-worktree + branch left orphaned by a spawn that aborted before
/// `NodeStarted` (#279). The worktree was created at the pipeline branch's tip
/// with no agent run, so removing it loses no work. Best-effort throughout
/// (mirrors `cleanup_run`): a missing dir / branch is fine.
pub(crate) fn reap_orphan_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
) {
    if sub_worktree_dir.exists() {
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(sub_worktree_dir)
            .current_dir(repo_root)
            .output();
    }
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", sub_branch])
        .current_dir(repo_root)
        .output();
    info!(
        "Reaped orphaned sub-worktree {} (branch {sub_branch}) after aborted spawn (#279)",
        sub_worktree_dir.display()
    );
}

pub(crate) fn worktree_dir_for_run(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

pub(crate) fn sub_worktree_path(
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

pub(crate) fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo/sub-{run_id}-{node_id}-iter-{iter}")
}

pub(crate) fn create_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
) -> Result<()> {
    std::fs::create_dir_all(
        sub_worktree_dir
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", sub_branch])
        .arg(sub_worktree_dir)
        .arg(base_branch)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add for sub-worktree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add (sub) failed: {stderr}");
    }

    info!("Created sub-worktree at {}", sub_worktree_dir.display());
    Ok(())
}

pub(crate) enum MergeResult {
    Success,
    Conflict(String),
    ConflictPendingResolution(String),
}

#[cfg(test)]
pub(crate) fn commit_and_merge_sub_worktree(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
) -> Result<MergeResult> {
    commit_and_merge_sub_worktree_inner(
        sub_worktree_dir,
        pipeline_worktree_dir,
        sub_branch,
        node_id,
        iter,
        false,
    )
}

pub(crate) fn commit_and_merge_sub_worktree_inner(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
    keep_conflict: bool,
) -> Result<MergeResult> {
    let _ = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git add failed in sub-worktree")?;

    let status_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git diff --cached failed")?;

    if !status_output.status.success() {
        let commit_msg = format!("{node_id} iter-{iter}: completed");
        let output = std::process::Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(sub_worktree_dir)
            .output()
            .context("git commit failed in sub-worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git commit in sub-worktree failed: {stderr}");
        }
    }

    let output = std::process::Command::new("git")
        .args(["merge", sub_branch, "--no-edit"])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("git merge failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if keep_conflict {
            return Ok(MergeResult::ConflictPendingResolution(stderr.to_string()));
        }
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(pipeline_worktree_dir)
            .output();
        return Ok(MergeResult::Conflict(stderr.to_string()));
    }

    // Sub-worktree and branch are intentionally kept alive (refs #32).
    // They survive until cleanup_run removes them, allowing prompt/artifact
    // inspection and tmux re-attach for completed iterations.

    info!("Merged sub-worktree {sub_branch} into pipeline branch");
    Ok(MergeResult::Success)
}

pub(crate) fn worktree_has_tracked_changes(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .context("git status failed")?;

    let status = String::from_utf8_lossy(&output.stdout);
    Ok(status.lines().any(|line| !line.starts_with("??")))
}

/// Check that no conflict markers remain in any tracked file.
pub(crate) fn has_conflict_markers(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["grep", "-rlE", "^<{7} |^={7}$|^>{7} "])
        .current_dir(worktree_dir)
        .output()
        .context("git grep failed")?;

    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Validate merge resolution: no conflict markers, clean working tree.
pub(crate) fn validate_merge_resolution(worktree_dir: &std::path::Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();

    if has_conflict_markers(worktree_dir)? {
        problems.push("conflict markers remain in tracked files".to_string());
    }

    if worktree_has_tracked_changes(worktree_dir)? {
        problems.push("working tree is not clean (uncommitted changes)".to_string());
    }

    Ok(problems)
}

pub(crate) fn create_worktree(
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

#[cfg(test)]
mod tests {
    use super::*;

    // Duplicated from lib.rs's test module (≈13 other call sites there still
    // need it) — a 14-line `git init/config/add/commit` fixture. Do not move.
    fn init_test_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "initial"]);
    }

    #[test]
    fn sub_worktree_path_follows_canonical_schema() {
        let path = sub_worktree_path(
            std::path::Path::new("/repo"),
            "20260101-120000-abc",
            "impl-1",
            1,
        );
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
    fn cm_sub_worktree_creates_and_merges() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-run";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        assert!(sub_wt_dir.exists());

        // Make a code change in the sub-worktree
        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Verify the file is present in the pipeline worktree
        assert!(wt_dir.join("foo.rs").exists());
    }

    #[test]
    fn cm_sub_worktree_survives_after_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-survive";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Sub-worktree directory must still exist after merge (refs #32)
        assert!(
            sub_wt_dir.exists(),
            "sub-worktree directory must survive merge for inspection"
        );

        // Sub-worktree branch must still exist after merge
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branch_check.stdout);
        assert!(
            branches.contains(&sub_branch),
            "sub-branch must survive merge; got: {branches}"
        );
    }

    #[test]
    fn cm_merge_conflict_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-conflict";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Create two sub-worktrees that will conflict
        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        // Both modify the same file with different content
        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        // Merge first succeeds
        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        // Merge second → conflict
        let r2 =
            commit_and_merge_sub_worktree(&sub_wt_2, &wt_dir, &sub_branch_2, "impl-2", 1).unwrap();
        assert!(matches!(r2, MergeResult::Conflict(_)));
    }

    #[test]
    fn doc_only_clean_worktree_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-clean";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_dirty_worktree_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-dirty";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Modify a tracked file
        std::fs::write(wt_dir.join("README.md"), "# modified\n").unwrap();

        assert!(worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_untracked_files_not_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-untracked";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Add an untracked file (like artifacts)
        let port_dir = wt_dir.join(".pdo/artifacts/planner/iter-1/plan");
        std::fs::create_dir_all(&port_dir).unwrap();
        std::fs::write(port_dir.join("output.md"), "# plan\n").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn validate_merge_resolution_clean_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.is_empty(),
            "clean repo should pass validation, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_conflict_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(
            repo.join("conflict.txt"),
            "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nafter\n",
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "conflict.txt"])
            .current_dir(repo)
            .output();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("conflict markers")),
            "should detect conflict markers, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(repo.join("README.md"), "# modified\n").unwrap();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("not clean")),
            "should detect dirty worktree, got: {problems:?}"
        );
    }

    #[test]
    fn conflict_pending_resolution_keeps_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-pending";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        let r2 = commit_and_merge_sub_worktree_inner(
            &sub_wt_2,
            &wt_dir,
            &sub_branch_2,
            "impl-2",
            1,
            true,
        )
        .unwrap();
        assert!(
            matches!(r2, MergeResult::ConflictPendingResolution(_)),
            "expected ConflictPendingResolution"
        );

        // Conflict markers should remain in worktree (merge NOT aborted)
        let content = std::fs::read_to_string(wt_dir.join("shared.txt")).unwrap();
        assert!(
            content.contains("<<<<<<<"),
            "conflict markers should remain in the file"
        );
    }

    #[test]
    fn create_worktree_with_source_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        // Create a feature branch with a file
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap()
        };
        run(&["checkout", "-b", "feature-branch"]);
        std::fs::write(repo.join("feature.txt"), "feature content\n").unwrap();
        run(&["add", "feature.txt"]);
        run(&["commit", "-m", "add feature"]);
        // Go back to default branch
        let default_out = std::process::Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(repo)
            .output()
            .unwrap();
        let branch_list = String::from_utf8_lossy(&default_out.stdout).to_string();
        let default_branch = branch_list
            .trim()
            .lines()
            .find(|b| *b != "feature-branch")
            .unwrap_or("master");
        run(&["checkout", default_branch]);

        // Create worktree from feature-branch
        let wt_dir = repo
            .join(".pdo")
            .join("runs")
            .join("test-run")
            .join("worktree");
        create_worktree(repo, &wt_dir, "pdo/run-test-run", "feature-branch").unwrap();

        // The worktree should contain feature.txt from the feature branch
        assert!(wt_dir.join("feature.txt").exists());
        assert_eq!(
            std::fs::read_to_string(wt_dir.join("feature.txt")).unwrap(),
            "feature content\n"
        );
    }

    #[test]
    fn worktree_dir_for_run_follows_canonical_schema() {
        let path =
            worktree_dir_for_run(std::path::Path::new("/target-repo"), "20260101-120000-abc");
        assert_eq!(
            path,
            PathBuf::from("/target-repo/.pdo/runs/20260101-120000-abc/worktree")
        );
    }

    // New per-module unit test (#276 AC "new per-module unit tests"):
    // reap_orphan_sub_worktree was previously covered only end-to-end by
    // crates/pdo-daemon/tests/spawn_abort_recovery.rs.
    #[test]
    fn reap_orphan_sub_worktree_removes_dir_and_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-reap-orphan";
        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, "HEAD").unwrap();

        // Precondition: worktree dir + branch exist.
        assert!(sub_wt_dir.exists());
        let before = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&before.stdout).contains(&sub_branch),
            "sub-branch should exist before reap"
        );

        reap_orphan_sub_worktree(repo, &sub_wt_dir, &sub_branch);

        // Postcondition: dir gone, branch deleted.
        assert!(!sub_wt_dir.exists(), "sub-worktree dir must be removed");
        let after = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&after.stdout).trim().is_empty(),
            "sub-branch must be deleted after reap"
        );
    }
}
