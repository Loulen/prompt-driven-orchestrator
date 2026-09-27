//! The branch list a Run can be cut from, read off real clones (#571, #802).

use pdo_workspace::adapters::git::branches::list_branches;
use pdo_workspace::domain::branches::BranchKind;

use crate::common::*;

#[test]
fn list_branches_returns_branches() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    // Exactly one local branch (the default checked-out one, whatever the
    // operator's `init.defaultBranch` names it) and no remotes — the shape,
    // not just "non-empty".
    let branches = list_branches(tmp.path()).unwrap();
    assert_eq!(branches.len(), 1, "one local branch expected: {branches:?}");
    assert_eq!(branches[0].kind, BranchKind::Local);
}

#[test]
fn list_branches_includes_remote_tracking_branches() {
    let (_origin, work) = init_repo_with_remote();
    let branches = list_branches(work.path()).unwrap();
    assert!(
        branches
            .iter()
            .any(|b| b.name == "origin/feature-remote-only" && b.kind == BranchKind::Remote),
        "remote-only tracking ref must be listed: {branches:?}"
    );
}

#[test]
fn list_branches_excludes_origin_head_symref() {
    let (_origin, work) = init_repo_with_remote();
    let branches = list_branches(work.path()).unwrap();
    // Neither the symref itself nor its bare-`origin` shadow may appear.
    assert!(
        !branches.iter().any(|b| b.name == "origin/HEAD"),
        "origin/HEAD symref must never be listed: {branches:?}"
    );
    assert!(
        !branches.iter().any(|b| b.name == "origin"),
        "bare 'origin' (the symref's short form) must never be listed: {branches:?}"
    );
}

#[test]
fn list_branches_dedups_remote_twin_of_local_branch() {
    let (_origin, work) = init_repo_with_remote();
    let branches = list_branches(work.path()).unwrap();
    // The clone created a local `main` tracking `origin/main`; the remote twin
    // is dropped, the local kept.
    assert!(
        branches
            .iter()
            .any(|b| b.name == "main" && b.kind == BranchKind::Local),
        "local main must be listed: {branches:?}"
    );
    assert!(
        !branches.iter().any(|b| b.name == "origin/main"),
        "origin/main (twin of local main) must be deduped away: {branches:?}"
    );
}

#[test]
fn list_branches_keeps_both_remotes_for_same_name() {
    let (_origin, work) = init_repo_with_remote();
    let _upstream = add_upstream_with_same_branch(work.path());
    let branches = list_branches(work.path()).unwrap();
    // No local `feature-remote-only`, so BOTH tracking refs survive — two
    // remotes may point at different SHAs; merging them would lie.
    assert!(
        branches
            .iter()
            .any(|b| b.name == "origin/feature-remote-only" && b.kind == BranchKind::Remote),
        "origin twin must be listed: {branches:?}"
    );
    assert!(
        branches
            .iter()
            .any(|b| b.name == "upstream/feature-remote-only" && b.kind == BranchKind::Remote),
        "upstream twin must be listed: {branches:?}"
    );
}

#[test]
fn list_branches_orders_locals_before_remotes() {
    let (_origin, work) = init_repo_with_remote();
    let branches = list_branches(work.path()).unwrap();
    let first_remote = branches.iter().position(|b| b.kind == BranchKind::Remote);
    let last_local = branches.iter().rposition(|b| b.kind == BranchKind::Local);
    // Both partitions are non-empty here (local main + remote feature), so a
    // strict last-local < first-remote proves the ordering, not luck.
    let (Some(fr), Some(ll)) = (first_remote, last_local) else {
        panic!("expected at least one local and one remote: {branches:?}");
    };
    assert!(ll < fr, "all locals must precede all remotes: {branches:?}");
}

#[test]
fn list_branches_reports_the_gap_after_origin_moves_ahead() {
    let (origin, work) = init_repo_with_remote();
    commit_n(origin.path(), 3, "upstream-move");
    run_git(work.path(), &["fetch", "--quiet", "origin"]);

    let main = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!(main.behind, Some(3), "three commits arrived on origin");
    assert_eq!(main.ahead, Some(0));
}

#[test]
fn list_branches_reports_a_diverged_gap_on_both_axes() {
    let (origin, work) = init_repo_with_remote();
    commit_n(origin.path(), 2, "theirs");
    commit_n(work.path(), 1, "mine");
    run_git(work.path(), &["fetch", "--quiet", "origin"]);

    let main = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!((main.ahead, main.behind), (Some(1), Some(2)));
}

#[test]
fn list_branches_reports_a_zero_gap_when_level_with_upstream() {
    let (_origin, work) = init_repo_with_remote();
    // A tracking branch that is exactly level: `%(upstream:track)` is EMPTY here,
    // which must read as `0↑ 0↓` ("up to date"), never as "no upstream".
    let main = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!((main.ahead, main.behind), (Some(0), Some(0)));
}

#[test]
fn list_branches_leaves_a_branch_without_upstream_without_a_gap() {
    let (_origin, work) = init_repo_with_remote();
    run_git(work.path(), &["checkout", "-q", "-b", "local-only"]);

    let solo = branch_named(&list_branches(work.path()).unwrap(), "local-only").clone();
    assert_eq!(solo.upstream, None);
    assert_eq!(
        (solo.ahead, solo.behind),
        (None, None),
        "a branch tracking nothing has no écart — not a zero one"
    );
}

#[test]
fn list_branches_carries_the_tip_commit_date_and_subject() {
    let (_origin, work) = init_repo_with_remote();
    std::fs::write(work.path().join("z.txt"), "z\n").unwrap();
    run_git(work.path(), &["add", "."]);
    run_git(work.path(), &["commit", "-m", "a readable subject"]);

    let main = branch_named(&list_branches(work.path()).unwrap(), "main").clone();
    assert_eq!(
        main.last_commit_subject.as_deref(),
        Some("a readable subject")
    );
    let date = main.last_commit_at.expect("a tip date");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&date).is_ok(),
        "the date must parse as RFC-3339 for the client: {date}"
    );
}
