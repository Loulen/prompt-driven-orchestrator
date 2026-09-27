//! Fast-forward (#803, ADR-0070 §2), against a real repository with a real
//! origin: "did the branch move, and did the files move with it?" has no
//! meaningful answer over a mocked git.

use pdo_workspace::adapters::git::branches::list_branches;
use pdo_workspace::adapters::git::fast_forward::fast_forward_branch;
use pdo_workspace::domain::fast_forward::FastForwardRefusal;

use crate::common::*;

#[test]
fn fast_forward_moves_a_behind_branch_and_its_checkout() {
    let (origin, work) = clone_behind_origin(3);
    let before = head_sha(work.path());

    let done = fast_forward_branch(work.path(), "main").expect("a lossless fast-forward");

    assert_eq!(done.commits, 3, "the count is the écart that was shown");
    assert_eq!(done.upstream, "origin/main");
    assert_eq!(
        head_sha(work.path()),
        head_sha(origin.path()),
        "the branch now sits on the upstream tip"
    );
    assert_ne!(head_sha(work.path()), before, "and it actually moved");
    // `main` IS checked out here, so the working tree walked forward with the
    // ref — the file the origin added must exist on disk.
    assert_eq!(
        done.moved_checkout.as_deref(),
        Some(work.path().display().to_string().as_str()),
        "the popover must be able to name the checkout that moved"
    );
    assert!(
        work.path().join("upstream-move-2.txt").exists(),
        "an `update-ref` alone would have left the fetched files missing"
    );
    // The gap is gone from the list the same response carries.
    let branches = list_branches(work.path()).unwrap();
    assert_eq!(branch_named(&branches, "main").behind, Some(0));
}

#[test]
fn fast_forward_of_an_unchecked_out_branch_is_a_pure_ref_move() {
    let (origin, work) = init_repo_with_remote();
    // `feature-remote-only` exists on origin only; give the clone a local
    // branch tracking it, WITHOUT checking it out (main stays HEAD).
    run_git(
        work.path(),
        &[
            "branch",
            "--track",
            "feature-remote-only",
            "origin/feature-remote-only",
        ],
    );
    run_git(origin.path(), &["checkout", "feature-remote-only"]);
    commit_n(origin.path(), 2, "remote-side");
    run_git(origin.path(), &["checkout", "main"]);
    run_git(work.path(), &["fetch", "--quiet", "origin"]);

    let head_before = head_sha(work.path());
    let done = fast_forward_branch(work.path(), "feature-remote-only").expect("a pure ref move");

    assert_eq!(done.commits, 2);
    assert!(
        done.moved_checkout.is_none(),
        "no checkout holds the branch: none of the user's files change"
    );
    assert_eq!(
        head_sha(work.path()),
        head_before,
        "the CHECKED-OUT branch must not budge when another branch is advanced"
    );
    assert!(
        !work.path().join("remote-side-0.txt").exists(),
        "a pure ref move touches no file in the working tree"
    );
    let branches = list_branches(work.path()).unwrap();
    assert_eq!(
        branch_named(&branches, "feature-remote-only").behind,
        Some(0),
        "the ref really moved, not just the report"
    );
}

#[test]
fn fast_forward_refuses_a_dirty_tree_and_names_the_files() {
    let (_origin, work) = clone_behind_origin(2);
    std::fs::write(work.path().join("README.md"), "# dirtied\n").unwrap();
    let before = head_sha(work.path());

    let refusal = fast_forward_branch(work.path(), "main").expect_err("a named refusal");

    assert_eq!(refusal.reason, FastForwardRefusal::DirtyTree);
    assert_eq!(refusal.dirty_count, Some(1));
    assert!(
        refusal.dirty_files.iter().any(|l| l.contains("README.md")),
        "the popover lists what is in the way: {:?}",
        refusal.dirty_files
    );
    assert!(refusal.checkout.is_some(), "and where to look");
    assert_eq!(
        head_sha(work.path()),
        before,
        "a refusal changes nothing at all"
    );
}

#[test]
fn fast_forward_ignores_untracked_files() {
    let (origin, work) = clone_behind_origin(1);
    // Untracked files survive a fast-forward untouched: refusing over them
    // would refuse the commonest working tree there is.
    std::fs::write(work.path().join("scratch.txt"), "notes\n").unwrap();

    fast_forward_branch(work.path(), "main").expect("untracked files never block");

    assert_eq!(head_sha(work.path()), head_sha(origin.path()));
    assert!(
        work.path().join("scratch.txt").exists(),
        "and they are still there afterwards"
    );
}

#[test]
fn fast_forward_refuses_a_diverged_branch() {
    let (_origin, work) = clone_behind_origin(3);
    commit_n(work.path(), 1, "local-work");
    let before = head_sha(work.path());

    let refusal = fast_forward_branch(work.path(), "main").expect_err("a named refusal");

    assert_eq!(refusal.reason, FastForwardRefusal::Diverged);
    assert_eq!(
        head_sha(work.path()),
        before,
        "the local commit is exactly what must not be dropped"
    );
}

#[test]
fn fast_forward_refuses_a_branch_checked_out_in_another_worktree() {
    let (_origin, work) = clone_behind_origin(2);
    // Park `main` in a sibling worktree, the shape a PDO Run itself creates.
    let elsewhere = tempfile::tempdir().unwrap();
    let park = elsewhere.path().join("parked");
    run_git(work.path(), &["checkout", "-q", "-b", "side"]);
    run_git(
        work.path(),
        &["worktree", "add", park.to_str().unwrap(), "main"],
    );
    let before =
        pdo_workspace::adapters::git::repository::resolve_commit(work.path(), "refs/heads/main")
            .unwrap();

    let refusal = fast_forward_branch(work.path(), "main").expect_err("a named refusal");

    assert_eq!(refusal.reason, FastForwardRefusal::CheckedOutElsewhere);
    assert!(
        refusal
            .checkout
            .as_deref()
            .is_some_and(|c| c.contains("parked")),
        "the worktree is named so the person knows where to look: {refusal:?}"
    );
    assert_eq!(
        pdo_workspace::adapters::git::repository::resolve_commit(work.path(), "refs/heads/main")
            .unwrap(),
        before
    );
}

#[test]
fn fast_forward_refuses_a_branch_without_an_upstream() {
    let tmp = tempfile::tempdir().unwrap();
    init_test_repo(tmp.path());
    run_git(tmp.path(), &["checkout", "-q", "-b", "solo"]);

    let refusal = fast_forward_branch(tmp.path(), "solo").expect_err("a named refusal");

    assert_eq!(refusal.reason, FastForwardRefusal::NoUpstream);
}

#[test]
fn fast_forward_refuses_a_branch_already_up_to_date() {
    let (_origin, work) = init_repo_with_remote();

    let refusal = fast_forward_branch(work.path(), "main").expect_err("a named refusal");

    assert_eq!(
        refusal.reason,
        FastForwardRefusal::UpToDate,
        "the benign race: nothing to take, and nothing red about it"
    );
}
