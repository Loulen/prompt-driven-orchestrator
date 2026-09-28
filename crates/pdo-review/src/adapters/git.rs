//! git behind [`ReviewGit`], through the workspace's git adapter: every call
//! runs the real binary in the Run's repository.

use std::path::Path;

use pdo_workspace::adapters::git::revisions::{
    file_at_ref, merge_base_with_head, rev_parse, rev_parse_object, run_git, snapshot_worktree,
    GitError,
};

use crate::application::ports::{GitFailure, RawDiff, ReviewGit};
use crate::domain::diff::{parse_patch, FileDiff, StructuredDiff};

/// The `git` binary.
pub struct CliGit;

fn failure(e: GitError) -> GitFailure {
    GitFailure {
        unknown_revision: e.is_unknown_revision(),
        message: e.to_string(),
    }
}

/// `git diff` with the options every structured read shares.
const DIFF: [&str; 6] = [
    "-c",
    "core.quotepath=false",
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--find-renames",
];

impl ReviewGit for CliGit {
    fn raw_diff(&self, repo: &Path, range: &str) -> Result<RawDiff, String> {
        let output = std::process::Command::new("git")
            .args(["diff", range, "--", ".", ":(exclude).pdo/"])
            .current_dir(repo)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(RawDiff {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    /// `three_dot` selects `from...to` (merge-base as the left side — the
    /// Run's fork → tip default, mirroring `compute_run_loc`) versus a plain
    /// `from to` (an arbitrary pair of Run refs, ADR-0067 §1). `.pdo/` is
    /// excluded in both forms, like the raw diff and the LOC stat, so counted
    /// = shown.
    fn structured(
        &self,
        repo: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure> {
        let range = format!("{from}...{to}");
        let mut args: Vec<&str> = DIFF.to_vec();
        if three_dot {
            args.push(&range);
        } else {
            args.push(from);
            args.push(to);
        }
        args.extend(["--", ".", ":(exclude).pdo/"]);
        let stdout = run_git(repo, &args).map_err(failure)?;
        let files = parse_patch(&String::from_utf8_lossy(&stdout));
        Ok(StructuredDiff::of(
            from,
            to,
            rev_parse_object(repo, from),
            rev_parse_object(repo, to),
            three_dot,
            files,
        ))
    }

    /// The worktree is snapshotted as a tree and compared two-dot against
    /// `from` — or, when `three_dot`, against the merge-base of `from` and the
    /// worktree's `HEAD`, mirroring the fork → tip default. `to_sha` is the
    /// snapshot tree id.
    fn structured_worktree(
        &self,
        worktree: &Path,
        from: &str,
        to: &str,
        three_dot: bool,
    ) -> Result<StructuredDiff, GitFailure> {
        let snapshot = snapshot_worktree(worktree).map_err(failure)?;
        let base = if three_dot {
            merge_base_with_head(worktree, from).map_err(failure)?
        } else {
            from.to_string()
        };
        let mut d = self.structured(worktree, &base, &snapshot, false)?;
        d.from_ref = from.to_string();
        d.from_sha = rev_parse(worktree, from);
        d.to_ref = to.to_string();
        d.to_sha = Some(snapshot);
        d.three_dot = three_dot;
        Ok(d)
    }

    /// `git diff <from> <to> -- <path>`, two-dot, `-U0`, renames followed so a
    /// moved file still maps. `Ok(vec![])` = identical content at both refs.
    fn path_diff(
        &self,
        repo: &Path,
        from: &str,
        to: &str,
        path: &str,
    ) -> Result<Vec<FileDiff>, GitFailure> {
        let mut args: Vec<&str> = DIFF.to_vec();
        args.extend(["-U0", from, to, "--", path]);
        let stdout = run_git(repo, &args).map_err(failure)?;
        Ok(parse_patch(&String::from_utf8_lossy(&stdout)))
    }

    fn rev_parse(&self, repo: &Path, git_ref: &str) -> Option<String> {
        rev_parse(repo, git_ref)
    }

    fn rev_parse_object(&self, repo: &Path, git_ref: &str) -> Option<String> {
        rev_parse_object(repo, git_ref)
    }

    fn snapshot_worktree(&self, worktree: &Path) -> Result<String, GitFailure> {
        snapshot_worktree(worktree).map_err(failure)
    }

    fn file_at_ref(&self, repo: &Path, git_ref: &str, path: &str) -> Result<Vec<u8>, GitFailure> {
        file_at_ref(repo, git_ref, path).map_err(failure)
    }
}

/// Stage 3 (ADR-0080): the adapter against a real repository.
#[cfg(test)]
mod tests {
    use super::*;
    use pdo_workspace::adapters::git::revisions::{is_safe_path, is_safe_ref};

    #[test]
    fn ref_and_path_safety() {
        assert!(is_safe_ref("pdo/run-abc"));
        assert!(is_safe_ref("a1b2c3^{commit}"));
        assert!(!is_safe_ref(""));
        assert!(!is_safe_ref("--output=/tmp/x"));
        assert!(!is_safe_ref("a b"));
        assert!(is_safe_path("src/lib.rs"));
        assert!(is_safe_path("my file.txt"));
        assert!(!is_safe_path("../etc/passwd"));
        assert!(!is_safe_path("/etc/passwd"));
        assert!(!is_safe_path("-x"));
        assert!(!is_safe_path(""));
    }

    #[test]
    fn compute_over_a_real_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        let c0 = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "-b", "work"]);
        std::fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(repo.join("b.txt"), "new\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo")).unwrap();
        std::fs::write(repo.join(".pdo/art.txt"), "blackboard\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c1"]);
        let c1 = git(&["rev-parse", "HEAD"]);
        // Advance main after the fork: three-dot must not show it as a deletion.
        git(&["checkout", "-q", "main"]);
        std::fs::write(repo.join("main_only.txt"), "m\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c2"]);
        let c2 = git(&["rev-parse", "HEAD"]);

        let d = CliGit.structured(repo, "main", "work", true).unwrap();
        assert!(d.three_dot);
        // `from_sha` is the ref AS RESOLVED (main's tip), while the diff itself is
        // taken from the merge-base (c0) — the three-dot contract.
        assert_eq!(d.from_sha.as_deref(), Some(c2.as_str()));
        assert_eq!(d.to_sha.as_deref(), Some(c1.as_str()));
        assert_eq!(d.files_changed, 2, "{d:?}");
        let paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
        assert_eq!((d.additions, d.deletions), (3, 1));
        assert!(!paths.iter().any(|p| p.contains("main_only")));
        assert!(!paths.iter().any(|p| p.contains(".pdo")));

        // Two-dot on an explicit pair.
        let d2 = CliGit.structured(repo, &c0, &c1, false).unwrap();
        assert!(!d2.three_dot);
        assert_eq!(d2.files_changed, 2);

        // Unknown ref → a 404-able error.
        let err = CliGit.structured(repo, "main", "nope", true).unwrap_err();
        assert!(err.unknown_revision, "{err:?}");

        // File at ref.
        let content = CliGit.file_at_ref(repo, "work", "b.txt").unwrap();
        assert_eq!(content, b"new\n");
        let missing = CliGit.file_at_ref(repo, "main", "b.txt").unwrap_err();
        assert!(missing.unknown_revision || !missing.message.is_empty());
    }

    #[test]
    fn compute_worktree_sees_uncommitted_and_untracked_without_touching_the_index() {
        // #835: on a dirty tree, the worktree diff shows the modified tracked
        // file and the untracked one (not `.pdo/`, not the ignored file), the
        // node's own index stays as it was, and `git status` is unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        let c0 = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "-b", "work"]);
        // One commit on the run branch.
        std::fs::write(repo.join("committed.txt"), "c\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c1"]);
        // Advance main after the fork: three-dot must not show it.
        git(&["checkout", "-q", "main"]);
        std::fs::write(repo.join("main_only.txt"), "m\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c2"]);
        git(&["checkout", "-q", "work"]);
        // Then a dirty tree on top of the run branch.
        std::fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();
        std::fs::write(repo.join("ignored.txt"), "nope\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo")).unwrap();
        std::fs::write(repo.join(".pdo/art.txt"), "blackboard\n").unwrap();
        let status_before = git(&["status", "--porcelain"]);

        let d = CliGit
            .structured_worktree(repo, "main", "worktree", true)
            .unwrap();
        assert!(d.three_dot);
        assert_eq!(d.from_ref, "main");
        assert_eq!(d.to_ref, "worktree");
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["a.txt", "committed.txt", "untracked.txt"],
            "{d:?}"
        );
        assert_eq!((d.additions, d.deletions), (4, 1));

        // The snapshot is a tree the object store knows: file-at-ref and a
        // two-dot diff against it work like for any ref (comments re-map, #752).
        let snapshot = d.to_sha.clone().unwrap();
        assert_eq!(
            CliGit.rev_parse_object(repo, &snapshot).as_deref(),
            Some(snapshot.as_str())
        );
        assert!(
            CliGit.rev_parse(repo, &snapshot).is_none(),
            "a tree, not a commit"
        );
        assert_eq!(
            CliGit
                .file_at_ref(repo, &snapshot, "untracked.txt")
                .unwrap(),
            b"new\n"
        );
        let only_dirty = CliGit.path_diff(repo, "HEAD", &snapshot, "a.txt").unwrap();
        assert_eq!(only_dirty.len(), 1);

        // Nothing moved for the node: same status, same index, same HEAD.
        assert_eq!(git(&["status", "--porcelain"]), status_before);
        assert_eq!(git(&["diff", "--cached", "--name-only"]), "");
        assert_ne!(git(&["rev-parse", "HEAD"]), c0);

        // Two-dot from an explicit ref: main's advance shows up as a deletion.
        let d2 = CliGit
            .structured_worktree(repo, "main", "worktree", false)
            .unwrap();
        assert!(!d2.three_dot);
        assert!(d2
            .files
            .iter()
            .any(|f| f.old_path.as_deref() == Some("main_only.txt")));

        // A clean tree snapshots to HEAD's own tree: fork → worktree == fork → tip.
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "c3"]);
        let clean = CliGit
            .structured_worktree(repo, "main", "worktree", true)
            .unwrap();
        assert_eq!(clean.to_sha, CliGit.rev_parse_object(repo, "HEAD^{tree}"));
        assert_eq!(
            clean.files_changed,
            CliGit
                .structured(repo, "main", "work", true)
                .unwrap()
                .files_changed
        );
    }

    #[test]
    fn snapshot_worktree_works_when_the_target_repo_gitignores_the_pdo_directory() {
        // #835 FP finding: `git add -A -- . ':(exclude).pdo/'` aborts when
        // `.pdo/` is gitignored as a directory — the documented target-repo
        // setup (ADR-0060). The snapshot must succeed, leave `.pdo/` out, and
        // keep a `.pdo` path that *is* tracked at HEAD exactly as HEAD has it.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        std::fs::write(repo.join(".gitignore"), ".pdo/\ntarget/\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "c0"]);
        std::fs::write(repo.join("a.txt"), "one\nTWO\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo/runs")).unwrap();
        std::fs::write(repo.join(".pdo/runs/art.txt"), "blackboard\n").unwrap();
        let status_before = git(&["status", "--porcelain"]);

        let d = CliGit
            .structured_worktree(repo, "main", "worktree", true)
            .unwrap();
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["a.txt", "untracked.txt"], "{d:?}");
        assert_eq!(git(&["status", "--porcelain"]), status_before);

        // Same rule when `.pdo` is *not* ignored: it stays out of the snapshot.
        std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        let d = CliGit
            .structured_worktree(repo, "main", "worktree", true)
            .unwrap();
        let mut paths: Vec<_> = d
            .files
            .iter()
            .map(|f| f.new_path.clone().unwrap())
            .collect();
        paths.sort();
        assert_eq!(paths, vec![".gitignore", "a.txt", "untracked.txt"], "{d:?}");

        // A `.pdo` path tracked at HEAD is snapshotted as HEAD has it — neither
        // dropped nor updated from the disk.
        std::fs::write(repo.join(".pdo/pipeline.yaml"), "v1\n").unwrap();
        git(&["add", "-f", ".pdo/pipeline.yaml"]);
        git(&["commit", "-q", "-m", "c1"]);
        std::fs::write(repo.join(".pdo/pipeline.yaml"), "v2\n").unwrap();
        let snapshot = CliGit.snapshot_worktree(repo).unwrap();
        assert_eq!(
            CliGit
                .file_at_ref(repo, &snapshot, ".pdo/pipeline.yaml")
                .unwrap(),
            b"v1\n"
        );
        assert!(CliGit
            .file_at_ref(repo, &snapshot, ".pdo/runs/art.txt")
            .is_err());
    }
}
