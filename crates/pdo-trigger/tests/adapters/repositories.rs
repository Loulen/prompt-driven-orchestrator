//! The pre-cut fetch of a fire (#804, ADR-0070 §1), against real git
//! repositories: moved from the legacy crate's in-crate tests.

use pdo_trigger::adapters::contexts::GitRepositories;
use pdo_trigger::application::fire::targets::fetch_targets;
use pdo_trigger::application::ports::Repositories;
use pdo_workspace::adapters::git::branches::list_branches;
use pdo_workspace::adapters::git::repository::validate_source_branch;
use pdo_workspace::domain::branches::{BranchEntry, FetchError};

fn run_git(dir: &std::path::Path, args: &[&str]) {
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

/// A `git clone` of a freshly-seeded origin, so `work` carries real
/// remote-tracking refs. Returns `(origin, work)`; keep both alive.
fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let origin = tempfile::tempdir().unwrap();
    run_git(origin.path(), &["init", "-b", "main"]);
    run_git(origin.path(), &["config", "user.email", "test@test.com"]);
    run_git(origin.path(), &["config", "user.name", "Test"]);
    std::fs::write(origin.path().join("README.md"), "# test\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "init"]);

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

/// Add `count` commits on `dir`'s current branch, moving an origin ahead of its
/// clone without a push.
fn commit_n(dir: &std::path::Path, count: usize, prefix: &str) {
    for i in 0..count {
        std::fs::write(
            dir.join(format!("{prefix}-{i}.txt")),
            format!("{prefix} {i}\n"),
        )
        .unwrap();
        run_git(dir, &["add", "."]);
        run_git(dir, &["commit", "-m", &format!("{prefix} {i}")]);
    }
}

fn behind(dir: &std::path::Path) -> Option<u32> {
    let branches = list_branches(dir).unwrap();
    let main: &BranchEntry = branches.iter().find(|b| b.name == "main").unwrap();
    main.behind
}

async fn refresh(targets: &[String]) -> Option<FetchError> {
    GitRepositories.refresh(targets).await
}

#[tokio::test]
async fn the_pre_cut_fetch_makes_the_source_fresh_at_the_fire() {
    let (origin, work) = init_repo_with_remote();
    // Someone pushed since the last fetch — the very case a scheduled fire used
    // to miss, cutting an "up to date" worktree from a stale ref.
    commit_n(origin.path(), 2, "pushed-since");
    assert_eq!(behind(work.path()), Some(0), "stale refs still look level");

    let targets = fetch_targets(Some(work.path().to_str().unwrap()), None);
    assert_eq!(refresh(&targets).await, None);
    assert_eq!(
        behind(work.path()),
        Some(2),
        "the cut that follows sees the two commits pushed since"
    );
}

#[tokio::test]
async fn a_failed_pre_cut_fetch_names_the_reason_and_still_lets_the_fire_through() {
    let (_origin, work) = init_repo_with_remote();
    run_git(
        work.path(),
        &["remote", "set-url", "origin", "/nonexistent/repo.git"],
    );

    let targets = fetch_targets(Some(work.path().to_str().unwrap()), None);
    let error = refresh(&targets)
        .await
        .expect("the reason the Run will carry");
    assert!(!error.message.is_empty(), "git's own words");
    // ADR-0070 §1 / story 21: nothing here refuses the fire. The source ref is
    // still resolvable, so the cut that follows happens on local state.
    assert!(validate_source_branch(work.path(), "main").is_ok());
}

#[tokio::test]
async fn the_pre_cut_fetch_refreshes_secondaries_but_only_the_primary_is_reported() {
    let (_primary_origin, primary) = init_repo_with_remote();
    let (secondary_origin, secondary) = init_repo_with_remote();
    commit_n(secondary_origin.path(), 1, "secondary-move");

    let targets = fetch_targets(
        Some(primary.path().to_str().unwrap()),
        Some(&format!(
            r#"[{{"repo":"{}"}}]"#,
            secondary.path().to_str().unwrap()
        )),
    );
    assert_eq!(refresh(&targets).await, None);
    assert_eq!(
        behind(secondary.path()),
        Some(1),
        "a secondary is refreshed by the same gesture"
    );
}

#[tokio::test]
async fn the_pre_cut_fetch_stays_silent_about_a_path_that_is_not_a_repository() {
    // The create chokepoint is about to refuse this fire naming the path; a `git
    // remote` error here would bury that message under a fetch one.
    let tmp = tempfile::tempdir().unwrap();
    let targets = fetch_targets(Some(tmp.path().to_str().unwrap()), None);
    assert_eq!(refresh(&targets).await, None);
}

#[test]
fn a_required_repo_is_a_git_repository() {
    let (_origin, work) = init_repo_with_remote();
    let path = work.path().to_str().unwrap();
    assert!(GitRepositories.required(Some(path)).is_ok());
    assert!(GitRepositories
        .required(None)
        .unwrap_err()
        .contains("target_repo"));
    assert!(GitRepositories.required(Some("   ")).is_err());
    assert!(GitRepositories
        .required(Some("/no/such/path/pdo-guard-dry-run-xyz"))
        .is_err());
}
