//! A shallow clone for the contexts that read a repository without working in
//! it, and the index queries of a worktree.

use std::time::Duration;

use pdo_workspace::adapters::git::clone::{clone_shallow, CloneError};
use pdo_workspace::adapters::git::repository::{info_exclude_path, is_tracked};

use crate::common::{init_test_repo, run_git};

#[tokio::test]
async fn a_repository_is_cloned_one_commit_deep() {
    let origin = tempfile::tempdir().unwrap();
    init_test_repo(origin.path());
    std::fs::write(origin.path().join("second.txt"), "2\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "second"]);
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join("clone");
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let url = format!("file://{}", origin.path().display());
    clone_shallow(&url, None, &target, rx, Duration::from_secs(60))
        .await
        .unwrap();
    assert!(target.join("second.txt").is_file());
    let log = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&target)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "1");
}

#[tokio::test]
async fn a_missing_repository_fails_with_gits_message_and_a_cancel_stops_the_clone() {
    let dest = tempfile::tempdir().unwrap();
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let err = clone_shallow(
        "file:///definitely/not/a/repo.git",
        None,
        &dest.path().join("x"),
        rx,
        Duration::from_secs(60),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, CloneError::Failed(ref stderr) if !stderr.is_empty()),
        "{err:?}"
    );

    let (tx, rx) = tokio::sync::oneshot::channel();
    tx.send(()).unwrap();
    let err = clone_shallow(
        "https://example.invalid/o/r",
        None,
        &dest.path().join("y"),
        rx,
        Duration::from_secs(60),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, CloneError::Cancelled | CloneError::Failed(_)),
        "{err:?}"
    );
}

#[test]
fn the_exclude_file_is_the_common_one_and_tracked_paths_are_seen() {
    let repo = tempfile::tempdir().unwrap();
    init_test_repo(repo.path());
    let exclude = info_exclude_path(repo.path()).unwrap().unwrap();
    assert!(exclude.ends_with("info/exclude"), "{}", exclude.display());
    assert!(is_tracked(repo.path(), "README.md").unwrap());
    assert!(!is_tracked(repo.path(), "absent").unwrap());
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(info_exclude_path(outside.path()).unwrap(), None);
}
