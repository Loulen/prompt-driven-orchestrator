//! Real temporary repositories for the adapter tests (stage 3, ADR-0080).

#![allow(dead_code)]

use std::path::Path;

pub fn run_git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn init_test_repo(dir: &std::path::Path) {
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

/// A `git clone` of a freshly-seeded origin, so `work` carries real
/// remote-tracking refs (`origin/main` — the twin of the local `main` — and
/// `origin/feature-remote-only`) PLUS the `origin/HEAD` symref every clone
/// writes, covering the symref-filter case for free. Returns `(origin,
/// work)`; keep both alive so the refs keep resolving.
pub fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let origin = tempfile::tempdir().unwrap();
    run_git(origin.path(), &["init", "-b", "main"]);
    run_git(origin.path(), &["config", "user.email", "test@test.com"]);
    run_git(origin.path(), &["config", "user.name", "Test"]);
    std::fs::write(origin.path().join("README.md"), "# test\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "init"]);
    run_git(origin.path(), &["checkout", "-b", "feature-remote-only"]);
    std::fs::write(origin.path().join("x.txt"), "x\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "feat"]);
    run_git(origin.path(), &["checkout", "main"]);

    let work = tempfile::tempdir().unwrap();
    run_git(
        origin.path(),
        &[
            "clone",
            "--quiet",
            origin.path().to_str().unwrap(),
            work.path().to_str().unwrap(),
        ],
    );
    run_git(work.path(), &["config", "user.email", "test@test.com"]);
    run_git(work.path(), &["config", "user.name", "Test"]);
    (origin, work)
}

// ── #802 / ADR-0070: écart amont, dates, and the fetch verb ──────────────
//
// Every test below drives a REAL pair of repos (a clone of a filesystem
// "origin"), because the whole point of the feature is what git reports about
// an actual divergence. The prior art is `validate_source_branch`'s suite.

/// Add `count` commits on `dir`'s current branch. Used to move an origin ahead
/// of its clone without a push (the origin here has a worktree, so a push to
/// its checked-out branch would be refused).
pub fn commit_n(dir: &std::path::Path, count: usize, prefix: &str) {
    for i in 0..count {
        let file = dir.join(format!("{prefix}-{i}.txt"));
        std::fs::write(&file, format!("{prefix} {i}\n")).unwrap();
        run_git(dir, &["add", "."]);
        run_git(dir, &["commit", "-m", &format!("{prefix} {i}")]);
    }
}

/// Add a second remote `upstream` (from a fresh local origin) that also has a
/// `feature-remote-only` branch, then fetch it — no network, a filesystem
/// path. Leaves `work` with both `origin/feature-remote-only` and
/// `upstream/feature-remote-only`. Returns the upstream tempdir to keep alive.
pub fn add_upstream_with_same_branch(work: &std::path::Path) -> tempfile::TempDir {
    let up = tempfile::tempdir().unwrap();
    run_git(up.path(), &["init", "-b", "main"]);
    run_git(up.path(), &["config", "user.email", "test@test.com"]);
    run_git(up.path(), &["config", "user.name", "Test"]);
    std::fs::write(up.path().join("README.md"), "# up\n").unwrap();
    run_git(up.path(), &["add", "."]);
    run_git(up.path(), &["commit", "-m", "init"]);
    run_git(up.path(), &["checkout", "-b", "feature-remote-only"]);
    std::fs::write(up.path().join("y.txt"), "y\n").unwrap();
    run_git(up.path(), &["add", "."]);
    run_git(up.path(), &["commit", "-m", "feat"]);
    run_git(up.path(), &["checkout", "main"]);

    run_git(
        work,
        &["remote", "add", "upstream", up.path().to_str().unwrap()],
    );
    run_git(work, &["fetch", "--quiet", "upstream"]);
    up
}

// ── Fast-forward (#803, ADR-0070 §2) ────────────────────────────────────
//
// Every case is driven against a REAL repository with a real origin: the
// question "did the branch move, and did the files move with it?" has no
// meaningful answer over a mocked git.

/// A clone whose `main` is `n` commits behind `origin/main`, already fetched
/// so the gap is visible without any network.
pub fn clone_behind_origin(n: usize) -> (tempfile::TempDir, tempfile::TempDir) {
    let (origin, work) = init_repo_with_remote();
    commit_n(origin.path(), n, "upstream-move");
    run_git(work.path(), &["fetch", "--quiet", "origin"]);
    (origin, work)
}

pub fn head_sha(repo: &Path) -> String {
    pdo_workspace::adapters::git::repository::resolve_commit(repo, "HEAD").expect("HEAD resolves")
}

pub fn branch_named<'a>(
    branches: &'a [pdo_workspace::domain::branches::BranchEntry],
    name: &str,
) -> &'a pdo_workspace::domain::branches::BranchEntry {
    branches
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no branch {name} in {branches:?}"))
}
