//! The fetch verb and the date it leaves, against a real origin (#802).

use pdo_workspace::adapters::git::branches::list_branches;
use pdo_workspace::adapters::git::fetch::{fetch_all_remotes, fetch_in_flight};
use pdo_workspace::adapters::git::repository::last_fetch_at;
use pdo_workspace::domain::branches::FetchErrorKind;

use crate::common::*;

#[test]
fn last_fetch_at_is_absent_before_any_fetch_and_set_after_one() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    assert_eq!(
        last_fetch_at(tmp.path()),
        None,
        "a repo that never fetched has no date to show"
    );

    let (_origin, work) = init_repo_with_remote();
    run_git(work.path(), &["fetch", "--quiet", "origin"]);
    let date = last_fetch_at(work.path()).expect("FETCH_HEAD dates the fetch");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&date).is_ok(),
        "{date}"
    );
}

#[test]
fn last_fetch_at_reads_an_empty_fetch_head_as_never() {
    // What a fetch that reached no remote leaves behind — freshly dated, but
    // zero bytes. Reporting its mtime would print "fetched just now" over
    // numbers nobody refreshed. Reproduced here as a plain truncation, which is
    // exactly what a terminal `git fetch` does when it fails.
    let (_origin, work) = init_repo_with_remote();
    run_git(work.path(), &["fetch", "--quiet", "origin"]);
    std::fs::write(work.path().join(".git/FETCH_HEAD"), b"").unwrap();
    assert_eq!(last_fetch_at(work.path()), None);
}

#[tokio::test]
async fn fetch_all_remotes_makes_a_new_origin_commit_visible() {
    let (origin, work) = init_repo_with_remote();
    // Origin moves ahead and the clone has NOT fetched: the gap is invisible.
    commit_n(origin.path(), 3, "after-clone");
    let before = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!(before.behind, Some(0), "stale refs still look level");

    assert_eq!(
        fetch_all_remotes(work.path()).await,
        None,
        "fetch succeeded"
    );

    let after = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!(
        after.behind,
        Some(3),
        "the fetch made the three commits visible"
    );
    assert!(last_fetch_at(work.path()).is_some());
}

#[tokio::test]
async fn fetch_all_remotes_prunes_a_branch_that_vanished_upstream() {
    let (origin, work) = init_repo_with_remote();
    run_git(origin.path(), &["branch", "-D", "feature-remote-only"]);

    assert_eq!(fetch_all_remotes(work.path()).await, None);

    let branches = list_branches(work.path()).unwrap();
    assert!(
        !branches
            .iter()
            .any(|b| b.name == "origin/feature-remote-only"),
        "a ref deleted upstream must be pruned away: {branches:?}"
    );
}

#[tokio::test]
async fn fetch_all_remotes_names_a_repo_with_no_remote() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    let error = fetch_all_remotes(tmp.path()).await.expect("a named reason");
    assert_eq!(
        error.kind,
        FetchErrorKind::NoRemote,
        "a purely local repo is a legitimate case with its own name"
    );
}

#[tokio::test]
async fn a_failed_fetch_leaves_the_list_and_the_previous_date_intact() {
    let (_origin, work) = init_repo_with_remote();
    run_git(work.path(), &["fetch", "--quiet", "origin"]);
    let dated_at = last_fetch_at(work.path()).expect("a first successful fetch");

    // Break the remote the way an offline machine or a moved repo would.
    run_git(
        work.path(),
        &["remote", "set-url", "origin", "/nonexistent/repo.git"],
    );
    let error = fetch_all_remotes(work.path()).await.expect("a failure");
    assert!(
        !error.message.is_empty(),
        "git's own words reach the popover"
    );

    // ADR-0070 §1: the écart degrades to "unknown, as of the last successful
    // fetch" — the list itself is untouched and launching stays possible.
    let branches = list_branches(work.path()).unwrap();
    assert!(branches.iter().any(|b| b.name == "main"));
    // git truncates and re-dates FETCH_HEAD on its way into a doomed fetch, so
    // without the restore this reads as "fetched just now" over refs nobody
    // refreshed — and the popover's "last successful fetch 2 d ago" would be a
    // fabrication. The date must be the one the LAST SUCCESS left.
    assert_eq!(
        last_fetch_at(work.path()).as_deref(),
        Some(dated_at.as_str()),
        "a failed fetch must leave the last successful fetch's date standing"
    );
}

#[tokio::test]
async fn concurrent_fetches_of_one_repo_share_a_single_flight() {
    let (_origin, work) = init_repo_with_remote();
    let path = work.path().to_path_buf();
    let outcomes = futures_util::future::join_all((0..4).map(|_| {
        let path = path.clone();
        async move { fetch_all_remotes(&path).await }
    }))
    .await;
    assert!(
        outcomes.iter().all(|o| o.is_none()),
        "every joined caller reads the leader's outcome: {outcomes:?}"
    );
    assert!(
        !fetch_in_flight()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&path),
        "the flight is released, so the next ask fetches for real"
    );
}
