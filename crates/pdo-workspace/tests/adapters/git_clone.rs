//! A shallow clone for the contexts that read a repository without working in
//! it, and the index queries of a worktree.

use std::collections::BTreeMap;
use std::time::Duration;

use pdo_workspace::adapters::git::clone::{clone_full, clone_shallow, CloneError};
use pdo_workspace::adapters::git::repository::{info_exclude_path, is_tracked};
use pdo_workspace::domain::clone::CloneFailure;

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
    clone_shallow(
        &url,
        None,
        &target,
        &BTreeMap::new(),
        rx,
        Duration::from_secs(60),
    )
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
        &BTreeMap::new(),
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
        &BTreeMap::new(),
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

#[tokio::test]
async fn a_full_clone_takes_every_commit_into_a_new_parent_and_reports_progress() {
    let origin = tempfile::tempdir().unwrap();
    init_test_repo(origin.path());
    std::fs::write(origin.path().join("second.txt"), "2\n").unwrap();
    run_git(origin.path(), &["add", "."]);
    run_git(origin.path(), &["commit", "-m", "second"]);
    let dest = tempfile::tempdir().unwrap();
    // The clones folder does not exist yet: it is created.
    let target = dest.path().join("pdo/repos/app");
    let url = format!("file://{}", origin.path().display());
    let lines = std::sync::Mutex::new(Vec::<String>::new());
    let (_tx, rx) = tokio::sync::oneshot::channel();
    clone_full(
        &url,
        &target,
        &BTreeMap::new(),
        &|line: &str| lines.lock().unwrap().push(line.to_string()),
        rx,
    )
    .await
    .unwrap();
    let log = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&target)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "2");
    let remote = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&target)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&remote.stdout).trim(), url);
    assert!(!lines.lock().unwrap().is_empty());

    // An empty folder takes the clone.
    let empty = dest.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let (_tx, rx) = tokio::sync::oneshot::channel();
    clone_full(&url, &empty, &BTreeMap::new(), &|_: &str| {}, rx)
        .await
        .unwrap();
    assert!(empty.join("second.txt").is_file());
}

#[tokio::test]
async fn a_failed_or_cancelled_full_clone_leaves_nothing() {
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join("missing");
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let err = clone_full(
        "file:///definitely/not/a/repo.git",
        &target,
        &BTreeMap::new(),
        &|_: &str| {},
        rx,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, CloneFailure::Failed(ref stderr) if stderr.contains("not/a/repo")),
        "{err:?}"
    );
    assert!(!target.exists());

    let origin = tempfile::tempdir().unwrap();
    init_test_repo(origin.path());
    let target = dest.path().join("cancelled");
    let (tx, rx) = tokio::sync::oneshot::channel();
    tx.send(()).unwrap();
    let url = format!("file://{}", origin.path().display());
    let err = clone_full(&url, &target, &BTreeMap::new(), &|_: &str| {}, rx)
        .await
        .unwrap_err();
    assert_eq!(err, CloneFailure::Cancelled);
    assert!(!target.exists());
    // No hidden half clone either.
    let left: Vec<_> = std::fs::read_dir(dest.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
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
