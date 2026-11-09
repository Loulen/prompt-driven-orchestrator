//! The worktree lifecycle of a Run, against real repositories: create,
//! classify, merge back, deliver, reap (ADR-0036, ADR-0037, ADR-0060).

use std::path::PathBuf;

use anyhow::Result;
use pdo_workspace::adapters::git::worktrees::*;

/// The merge-back on its **conservative** arm: no spawn base, so a conflict still reads `MergeResult::Conflict`. A test that wants the
/// #503 adoption path calls `commit_and_merge_sub_worktree_inner` and passes the
/// base it cut from.
fn commit_and_merge_sub_worktree(
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
        None,
        &CommitIdentity::default(),
    )
}

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

    std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

    let result =
        commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
    assert!(matches!(result, MergeResult::Success));

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

    let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

    let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
    let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
    create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

    std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
    std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

    let r1 = commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
    assert!(matches!(r1, MergeResult::Success));

    let r2 = commit_and_merge_sub_worktree(&sub_wt_2, &wt_dir, &sub_branch_2, "impl-2", 1).unwrap();
    assert!(matches!(r2, MergeResult::Conflict(_)));
}

// ── #503 / ADR-0036 ──────────────────────────────────────────────────────
//
// The topology the whole issue is about, built without PDO and without an
// agent: a run whose terminal node rebased its branch onto an integration
// branch that moved mid-run, then re-resolved the run's own work by hand.
// `git merge` in the pipeline worktree cannot fast-forward any more and
// conflicts on the files both sides rewrote.

/// Build a repo in the #503 shape and return `(repo, pipeline worktree, sub
/// worktree, pipeline branch, sub branch)`.
///
/// `node_supersedes` decides whether the node rewrites the pipeline's own
/// change (the real occurrence: a version bump on top of it) or leaves it
/// alone. Only the first form conflicts.
struct RebasedNode {
    repo: PathBuf,
    pipeline_wt: PathBuf,
    sub_wt: PathBuf,
    pipeline_branch: String,
    sub_branch: String,
    /// What `create_sub_worktree` reported — the #503 guard's only input, and
    /// what a real spawn writes to `NodeStarted.base_sha`.
    spawn_base: String,
}

fn rebased_terminal_node_repo(tmp: &tempfile::TempDir, node_supersedes: bool) -> RebasedNode {
    let repo = tmp.path().to_path_buf();
    init_test_repo(&repo);
    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed in {}: {}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    };
    std::fs::write(repo.join("VERSION"), "1.4.1\n").unwrap();
    git(&repo, &["add", "VERSION"]);
    git(&repo, &["commit", "-m", "base: 1.4.1"]);
    let base_branch = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    // The run's pipeline worktree, and an earlier node's work merged back onto it.
    let run_id = "test-503";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(&repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    std::fs::write(wt_dir.join("VERSION"), "1.5.0\n").unwrap();
    std::fs::write(wt_dir.join("fix.rs"), "pub fn fix() {}\n").unwrap();
    git(&wt_dir, &["add", "-A"]);
    git(&wt_dir, &["commit", "-m", "implementer: the fix — 1.5.0"]);

    // A concurrent run lands its own work on the integration branch, mid-run.
    std::fs::write(repo.join("VERSION"), "1.5.0\n").unwrap();
    std::fs::write(repo.join("other.rs"), "pub fn other() {}\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "concurrent run: other — 1.5.0"]);

    // The terminal node's sub-worktree, cut from the pipeline branch tip.
    let sub_wt = sub_worktree_path(&repo, run_id, "ship", 1);
    let sub_branch = sub_worktree_branch(run_id, "ship", 1);
    let spawn_base = create_sub_worktree(&repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
    assert_eq!(
        spawn_base,
        rev_parse(&repo, &pipeline_branch).unwrap(),
        "the reported base must be the pipeline tip it was cut from"
    );

    // Its prompt sends it to publish against the integration branch, so it
    // rebases and resolves the incidental collision on VERSION.
    let rebase = std::process::Command::new("git")
        .args(["rebase", &base_branch])
        .current_dir(&sub_wt)
        .output()
        .unwrap();
    if !rebase.status.success() {
        std::fs::write(sub_wt.join("VERSION"), "1.5.0\n").unwrap();
        git(&sub_wt, &["add", "-A"]);
        let cont = std::process::Command::new("git")
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .current_dir(&sub_wt)
            .output()
            .unwrap();
        assert!(cont.status.success(), "rebase --continue failed");
    }
    if node_supersedes {
        // …and then bumps the version per its prompt, rewriting the very line
        // the pipeline branch also owns. This is what makes the merge-back
        // unrecoverable rather than a clean add/add.
        std::fs::write(sub_wt.join("VERSION"), "1.6.0\n").unwrap();
        git(&sub_wt, &["add", "-A"]);
        git(&sub_wt, &["commit", "-m", "ship it: 1.6.0"]);
    }

    RebasedNode {
        repo,
        pipeline_wt: wt_dir,
        sub_wt,
        pipeline_branch,
        sub_branch,
        spawn_base,
    }
}

/// AC1 — the invariant `commit_and_merge_sub_worktree_inner` relies on is
/// broken by the node's rebase, and the merge-back must nevertheless land.
#[test]
fn merge_back_survives_node_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    // Precondition: the ancestry the merge-back assumed is gone.
    let ancestor = std::process::Command::new("git")
        .args([
            "merge-base",
            "--is-ancestor",
            &node.pipeline_branch,
            &node.sub_branch,
        ])
        .current_dir(&node.repo)
        .output()
        .unwrap();
    assert!(
        !ancestor.status.success(),
        "fixture must break the ancestry invariant, or it tests nothing"
    );

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        Some(&node.spawn_base),
        &CommitIdentity::default(),
    )
    .unwrap();

    let MergeResult::ResolvedInNodeFavour(adoption) = result else {
        panic!("expected the merge-back to be resolved in the node's favour");
    };

    // The pipeline branch now carries the node's tree, byte for byte.
    let node_tree = rev_parse(&node.repo, &format!("{}^{{tree}}", adoption.node_tip)).unwrap();
    let pipeline_tree =
        rev_parse(&node.repo, &format!("{}^{{tree}}", node.pipeline_branch)).unwrap();
    assert_eq!(
        pipeline_tree, node_tree,
        "the pipeline branch must end up on the node's tree"
    );
    assert_eq!(
        std::fs::read_to_string(node.pipeline_wt.join("VERSION")).unwrap(),
        "1.6.0\n",
        "the working tree must show the node's version, not the pipeline's"
    );
    // …including the concurrent run's file the node rebased onto.
    assert!(node.pipeline_wt.join("other.rs").exists());
    assert!(node.pipeline_wt.join("fix.rs").exists());
    assert!(!adoption.conflicting_files.is_empty());
}

/// Nothing is destroyed: the superseded pipeline tip is the resolution
/// commit's FIRST parent, so `git log` on the pipeline branch still reaches it.
#[test]
fn resolving_in_the_node_favour_keeps_the_old_tip_reachable() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        Some(&node.spawn_base),
        &CommitIdentity::default(),
    )
    .unwrap();
    let MergeResult::ResolvedInNodeFavour(adoption) = result else {
        panic!("expected the merge-back to be resolved in the node's favour");
    };

    let parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", &adoption.merge_commit])
        .current_dir(&node.repo)
        .output()
        .unwrap();
    let parents = String::from_utf8_lossy(&parents.stdout).trim().to_string();
    let mut shas = parents.split_whitespace();
    assert_eq!(shas.next(), Some(adoption.merge_commit.as_str()));
    assert_eq!(
        shas.next(),
        Some(adoption.pipeline_tip.as_str()),
        "the superseded pipeline tip must be the FIRST parent; got: {parents}"
    );
    assert_eq!(shas.next(), Some(adoption.node_tip.as_str()));

    for tip in [&adoption.pipeline_tip, &adoption.node_tip] {
        let reachable = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", tip, &node.pipeline_branch])
            .current_dir(&node.repo)
            .output()
            .unwrap();
        assert!(
            reachable.status.success(),
            "{tip} must stay reachable from {}",
            node.pipeline_branch
        );
    }
}

/// AC2 — the guard must not turn every conflict into a silent adoption. Two
/// sibling nodes writing the same line incompatibly is a *genuine* conflict:
/// the first one's work is on the pipeline branch and absent from the second's
/// tree, so taking the second's tree would drop it.
///
/// And this is exactly what the structural guard sees: impl-1's merge moved the
/// pipeline tip off impl-2's spawn base.
#[test]
fn genuine_semantic_conflict_still_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-503-genuine";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
    let base_1 = create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();
    let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
    let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
    let base_2 = create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();
    assert_eq!(base_1, base_2, "both siblings were cut from the same tip");

    std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
    std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

    let r1 = commit_and_merge_sub_worktree_inner(
        &sub_wt_1,
        &wt_dir,
        &sub_branch_1,
        "impl-1",
        1,
        Some(&base_1),
        &CommitIdentity::default(),
    )
    .unwrap();
    assert!(matches!(r1, MergeResult::Success));

    let r2 = commit_and_merge_sub_worktree_inner(
        &sub_wt_2,
        &wt_dir,
        &sub_branch_2,
        "impl-2",
        1,
        Some(&base_2),
        &CommitIdentity::default(),
    )
    .unwrap();
    assert!(
        matches!(r2, MergeResult::Conflict(_)),
        "a genuine conflict must stay a failure"
    );
    assert_eq!(
        std::fs::read_to_string(wt_dir.join("shared.txt")).unwrap(),
        "from impl-1\n",
        "impl-1's merged work must survive the refused merge-back"
    );
}

/// The same refusal on the #503 topology itself: a terminal node that rebased is
/// *not* enough — if anything reached the pipeline branch after its spawn, its
/// tree is not authoritative for the run and the conflict stands.
#[test]
fn a_rebased_node_whose_base_went_stale_still_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    // Something else lands on the pipeline branch after the sub-worktree was cut
    // — a non-isolated node committing its docs, say.
    std::fs::write(node.pipeline_wt.join("PLAN.md"), "# plan\n").unwrap();
    for args in [vec!["add", "-A"], vec!["commit", "-m", "docs: a plan"]] {
        assert!(std::process::Command::new("git")
            .args(&args)
            .current_dir(&node.pipeline_wt)
            .output()
            .unwrap()
            .status
            .success());
    }
    assert_ne!(
        node.spawn_base,
        rev_parse(&node.repo, &node.pipeline_branch).unwrap()
    );

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        Some(&node.spawn_base),
        &CommitIdentity::default(),
    )
    .unwrap();
    assert!(
        matches!(result, MergeResult::Conflict(_)),
        "a stale spawn base must not be adopted — the docs commit would be dropped"
    );
    assert!(
        node.pipeline_wt.join("PLAN.md").exists(),
        "the foreign work must survive"
    );
}

/// A Run recorded by a pre-#503 daemon has no `base_sha`, and an unknown base is
/// not a licence to rewrite a branch.
#[test]
fn an_unknown_spawn_base_is_never_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    assert!(matches!(result, MergeResult::Conflict(_)));
}

/// AC3 — `detail` used to be `git merge`'s *stderr*, which is byte-empty on a
/// conflict: every `merge_conflict_detected` event PDO ever wrote said nothing.
#[test]
fn conflict_detail_is_not_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let MergeResult::Conflict(conflict) = result else {
        panic!("expected a conflict when adoption is not allowed");
    };
    assert!(
        conflict.detail.contains("CONFLICT"),
        "detail must carry git's conflict report; got: {:?}",
        conflict.detail
    );
}

/// AC4 — the event must be diagnosable without archaeology: the conflicting
/// files, and the two commits nothing else exposes.
#[test]
fn conflict_detail_names_the_files_and_both_tips() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let expected_pipeline_tip = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
    let expected_node_tip = rev_parse(&node.repo, &node.sub_branch).unwrap();

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let MergeResult::Conflict(conflict) = result else {
        panic!("expected a conflict when adoption is not allowed");
    };

    assert_eq!(conflict.pipeline_tip, expected_pipeline_tip);
    assert_eq!(conflict.node_tip, expected_node_tip);
    assert_eq!(
        conflict.conflicting_files,
        vec!["VERSION".to_string()],
        "the unmerged paths must be read before the abort wipes the index"
    );
    // And the refused merge left the pipeline worktree clean.
    assert!(!worktree_has_tracked_changes(&node.pipeline_wt).unwrap());
}

/// A node that rebased *cleanly* — carrying the pipeline's change forward
/// untouched — never reaches the adoption path at all: git resolves the
/// identical add/add itself. Pinned so the fix is not credited with a case it
/// does not handle, and so a future change that starts rewriting branches here
/// shows up as a failure.
#[test]
fn a_clean_node_rebase_needs_no_resolution_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, false);

    let before = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        Some(&node.spawn_base),
        &CommitIdentity::default(),
    )
    .unwrap();
    assert!(
        matches!(result, MergeResult::Success),
        "a clean rebase merges by itself"
    );
    assert_ne!(
        rev_parse(&node.repo, &node.pipeline_branch).unwrap(),
        before
    );
    assert!(node.pipeline_wt.join("other.rs").exists());
}

/// The mechanism's own precondition: adoption must not fire on a pipeline
/// worktree with uncommitted tracked work, because it ends in `reset --hard` —
/// which destroys in silence where `git merge` fails loudly.
#[test]
fn adoption_refuses_a_dirty_pipeline_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let pipeline_tip = rev_parse(&node.repo, &node.pipeline_branch).unwrap();
    let node_tip = rev_parse(&node.repo, &node.sub_branch).unwrap();
    std::fs::write(
        node.pipeline_wt.join("fix.rs"),
        "hand-edited, uncommitted\n",
    )
    .unwrap();

    assert!(!adoption_allowed(
        &node.pipeline_wt,
        &pipeline_tip,
        &node_tip,
        Some(&node.spawn_base),
    )
    .unwrap());
}

/// Unrelated histories are not a diverged pipeline branch — `git merge` fails
/// there too, and adopting would replace the run's whole tree.
#[test]
fn adoption_refuses_unrelated_histories() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let ours = rev_parse(repo, "HEAD").unwrap();

    // A parentless commit: a second root, i.e. a history that shares nothing
    // with ours. Built with `commit-tree` so the worktree stays untouched — the
    // dirty-tree precondition must not be what this test happens to trip on.
    let tree = rev_parse(repo, "HEAD^{tree}").unwrap();
    let alien = std::process::Command::new("git")
        .args(["commit-tree", &tree, "-m", "unrelated root"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(alien.status.success());
    let alien = String::from_utf8_lossy(&alien.stdout).trim().to_string();
    assert!(!share_an_ancestor(repo, &ours, &alien));

    // The structural rule holds — the base IS the tip — and it is still refused.
    assert!(!adoption_allowed(repo, &ours, &alien, Some(&ours)).unwrap());
}

// ── #654 / ADR-0060: the delivery primitive ─────────────────────────────

/// A tree with nothing to stage yields NO commit — the branch does not move.
#[test]
fn stage_and_commit_takes_no_empty_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let before = rev_parse(repo, "HEAD").unwrap();

    assert_eq!(
        stage_and_commit(repo, "n", 1, &CommitIdentity::default()).unwrap(),
        None
    );
    assert_eq!(rev_parse(repo, "HEAD").unwrap(), before);
}

/// `git add -A` semantics with the repo's `.gitignore` as the ONLY exclusion:
/// a new file, a modification and a deletion all go in, an ignored file does
/// not, and the message is the deterministic one.
#[test]
fn stage_and_commit_follows_add_dash_a_and_the_repo_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap()
    };
    std::fs::write(repo.join(".gitignore"), "ignored/\n").unwrap();
    std::fs::write(repo.join("doomed.txt"), "bye\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "base"]);

    std::fs::write(repo.join("README.md"), "# changed\n").unwrap();
    std::fs::remove_file(repo.join("doomed.txt")).unwrap();
    std::fs::write(repo.join("added.rs"), "pub fn added() {}\n").unwrap();
    std::fs::create_dir_all(repo.join("ignored")).unwrap();
    std::fs::write(repo.join("ignored/noise.txt"), "noise\n").unwrap();

    let sha = stage_and_commit(repo, "impl-1", 3, &CommitIdentity::default())
        .unwrap()
        .unwrap();
    assert_eq!(sha, rev_parse(repo, "HEAD").unwrap());

    let subject = String::from_utf8_lossy(&git(&["log", "--format=%s", "-1"]).stdout)
        .trim()
        .to_string();
    assert_eq!(subject, "impl-1 iter-3: completed");

    let files =
        String::from_utf8_lossy(&git(&["show", "--name-status", "--format="]).stdout).to_string();
    assert!(files.contains("M\tREADME.md"), "{files}");
    assert!(files.contains("D\tdoomed.txt"), "{files}");
    assert!(files.contains("A\tadded.rs"), "{files}");
    assert!(
        !files.contains("noise.txt"),
        "an ignored file stays out: {files}"
    );
}

/// #796: the run's artifact store never enters the Run branch, even when the
/// target repo's `.gitignore` does not cover `.pdo/` — a fresh artifact is left
/// untracked, an already-tracked one (inherited from a previous run's commit)
/// keeps its committed content, and the node's real work is still delivered.
#[test]
fn stage_and_commit_keeps_the_run_artifacts_out_of_the_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap()
    };
    // A previous run committed a report at the same node/iter path.
    let tracked = repo.join(".pdo/artifacts/tester/iter-1/out/output.md");
    std::fs::create_dir_all(tracked.parent().unwrap()).unwrap();
    std::fs::write(&tracked, "---\nVerdict: Pass\n---\nold report\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "previous run"]);

    std::fs::write(&tracked, "---\nVerdict: Fail\n---\nnew report\n").unwrap();
    let fresh = repo.join(".pdo/artifacts/tester/iter-1/shots/a.png");
    std::fs::create_dir_all(fresh.parent().unwrap()).unwrap();
    std::fs::write(&fresh, b"png").unwrap();
    std::fs::write(repo.join("src.rs"), "pub fn work() {}\n").unwrap();

    let sha = stage_and_commit(repo, "tester", 1, &CommitIdentity::default())
        .unwrap()
        .unwrap();
    assert_eq!(sha, rev_parse(repo, "HEAD").unwrap());

    let files =
        String::from_utf8_lossy(&git(&["show", "--name-status", "--format="]).stdout).to_string();
    assert!(files.contains("A\tsrc.rs"), "{files}");
    assert!(
        !files.contains(".pdo/artifacts"),
        "artifacts stay out: {files}"
    );

    let status = String::from_utf8_lossy(&git(&["status", "--porcelain"]).stdout).to_string();
    assert!(
        status.contains(" M .pdo/artifacts/tester/iter-1/out/output.md"),
        "{status}"
    );
    assert!(
        status.contains("?? .pdo/artifacts/tester/iter-1/shots/"),
        "{status}"
    );
}

/// #796: only artifacts are exempt from the completion commit — nothing else
/// under `.pdo/` (a versioned pipeline edited by the node) is.
#[test]
fn stage_and_commit_still_delivers_other_pdo_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap()
    };
    let yaml = repo.join(".pdo/pipelines/p.yaml");
    std::fs::create_dir_all(yaml.parent().unwrap()).unwrap();
    std::fs::write(&yaml, "name: p\n").unwrap();

    stage_and_commit(repo, "editor", 1, &CommitIdentity::default())
        .unwrap()
        .unwrap();
    let files =
        String::from_utf8_lossy(&git(&["show", "--name-status", "--format="]).stdout).to_string();
    assert!(files.contains("A\t.pdo/pipelines/p.yaml"), "{files}");
}

/// #489: a staging failure is LOUD. A leftover `index.lock` makes `git add -A`
/// exit non-zero; the delivery must bail instead of reporting a success that
/// silently drops the node's work.
#[test]
fn stage_and_commit_bails_on_a_staging_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    std::fs::write(repo.join("work.rs"), "pub fn work() {}\n").unwrap();
    std::fs::write(repo.join(".git/index.lock"), "").unwrap();

    let err = stage_and_commit(repo, "n", 1, &CommitIdentity::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("git add -A"),
        "the error names the step: {err}"
    );
    assert!(
        repo.join("work.rs").exists(),
        "a failed delivery destroys nothing"
    );
}

/// The non-isolated arm: the node worked in the Run's worktree, so its commit
/// IS the delivery — no merge, and the two reported tips bracket it.
#[test]
fn deliver_in_place_commits_on_the_run_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-inplace";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    std::fs::write(wt_dir.join("shared.rs"), "pub fn shared() {}\n").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &wt_dir,
        false,
        "unused",
        "shared-1",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(d) = outcome else {
        panic!("a non-isolated delivery never conflicts");
    };
    assert!(d.changed());
    assert_eq!(d.after, rev_parse(&wt_dir, "HEAD").unwrap());
    assert_eq!(d.after, rev_parse(repo, &pipeline_branch).unwrap());
}

#[test]
fn deliver_in_place_excludes_resources_materialized_by_provisioning() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    std::fs::write(repo.join("local-secret.txt"), "SECRET TOKEN abc123\n").unwrap();

    let run_id = "test-deliver-provisioned";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let plan = pdo_workspace::adapters::fs::provisioner::resolve(
        repo,
        &[pdo_workspace::domain::provisioning::ScopedRules {
            scope: pdo_workspace::domain::provisioning::ProvisioningScope::Run,
            rules: pdo_workspace::domain::provisioning::ProvisioningRules {
                copy: vec!["local-secret.txt".into()],
                ..Default::default()
            },
        }],
    )
    .unwrap();
    pdo_workspace::adapters::fs::provisioner::provision_missing(repo, &wt_dir, &plan).unwrap();
    std::fs::write(wt_dir.join("node-work.rs"), "pub fn node_work() {}\n").unwrap();
    let staged = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&wt_dir)
        .output()
        .unwrap();
    assert!(staged.status.success());

    let outcome = deliver_node_work(
        &wt_dir,
        &wt_dir,
        false,
        "unused",
        "shared-1",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(delivered) = outcome else {
        panic!("expected a delivery");
    };
    assert!(delivered.changed());

    let committed = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(&wt_dir)
        .output()
        .unwrap();
    assert!(committed.status.success());
    let committed = String::from_utf8_lossy(&committed.stdout);
    assert!(committed.lines().any(|path| path == "node-work.rs"));
    assert!(
        !committed.lines().any(|path| path == "local-secret.txt"),
        "provisioned resources must not enter delivery commits: {committed}"
    );
}

/// A non-isolated node that commits its own work: the delivery takes no commit,
/// but its `before` is the Run worktree's HEAD when the node started, so the
/// pair still brackets what the node did (CORE-231).
#[test]
fn deliver_in_place_brackets_the_commits_the_node_made_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-self-commit";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let started_at = rev_parse(&wt_dir, "HEAD").unwrap();

    std::fs::write(wt_dir.join("own.rs"), "pub fn own() {}\n").unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&wt_dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
    };
    git(&["add", "own.rs"]);
    git(&["commit", "-m", "the node's own commit"]);
    let tip = rev_parse(&wt_dir, "HEAD").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &wt_dir,
        false,
        "unused",
        "shared-1",
        1,
        Some(&started_at),
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(d) = outcome else {
        panic!("a non-isolated delivery never conflicts");
    };
    assert!(d.changed(), "the node's own commit is its delivery");
    assert_eq!(d.before, started_at);
    assert_eq!(d.after, tip, "nothing left behind ⇒ no delivery commit");
}

/// The no-op reports itself as one: same tip on both sides, no commit taken.
#[test]
fn deliver_in_place_on_a_clean_tree_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-noop";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    create_worktree(repo, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &wt_dir,
        false,
        "unused",
        "shared-1",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(d) = outcome else {
        panic!("expected a delivery");
    };
    assert!(!d.changed(), "before == after ⇒ nothing was committed");
}

/// The isolated arm: commit in the sub-worktree, then merge it into the Run's
/// branch. The reported tips bracket the merge.
#[test]
fn deliver_isolated_commits_then_merges_back() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-isolated";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let sub_wt = sub_worktree_path(repo, run_id, "forked-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "forked-1", 1);
    create_sub_worktree(repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
    std::fs::write(sub_wt.join("forked.rs"), "pub fn forked() {}\n").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &sub_wt,
        true,
        &sub_branch,
        "forked-1",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(d) = outcome else {
        panic!("a clean merge-back delivers");
    };
    assert!(d.changed());
    assert!(wt_dir.join("forked.rs").exists());
    assert_eq!(d.after, rev_parse(repo, &pipeline_branch).unwrap());
}

#[test]
fn deliver_isolated_excludes_resources_materialized_in_the_sub_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    std::fs::write(repo.join("local-secret.txt"), "SECRET TOKEN abc123\n").unwrap();

    let run_id = "test-deliver-isolated-provisioned";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let sub_wt = sub_worktree_path(repo, run_id, "forked-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "forked-1", 1);
    create_sub_worktree(repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
    let plan = pdo_workspace::adapters::fs::provisioner::resolve(
        repo,
        &[pdo_workspace::domain::provisioning::ScopedRules {
            scope: pdo_workspace::domain::provisioning::ProvisioningScope::IsolatedNode,
            rules: pdo_workspace::domain::provisioning::ProvisioningRules {
                symlink: vec!["local-secret.txt".into()],
                ..Default::default()
            },
        }],
    )
    .unwrap();
    pdo_workspace::adapters::fs::provisioner::provision_missing(repo, &sub_wt, &plan).unwrap();
    std::fs::write(sub_wt.join("isolated-work.rs"), "pub fn isolated() {}\n").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &sub_wt,
        true,
        &sub_branch,
        "forked-1",
        1,
        None,
        &CommitIdentity::default(),
    )
    .unwrap();
    let DeliveryOutcome::Delivered(delivered) = outcome else {
        panic!("expected a delivery");
    };
    assert!(delivered.changed());

    let committed = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(&wt_dir)
        .output()
        .unwrap();
    assert!(committed.status.success());
    let committed = String::from_utf8_lossy(&committed.stdout);
    assert!(committed.lines().any(|path| path == "isolated-work.rs"));
    assert!(
        !committed.lines().any(|path| path == "local-secret.txt"),
        "provisioned resources must not merge into the Run branch: {committed}"
    );
}

/// A directory git knows nothing about is not a worktree — the precondition
/// the runtime reads before deciding there is anything to deliver.
#[test]
fn is_git_worktree_answers_for_a_real_worktree_and_a_bare_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    assert!(is_git_worktree(repo));

    let orphan = tempfile::tempdir().unwrap();
    assert!(!is_git_worktree(orphan.path()));
    assert!(!is_git_worktree(&orphan.path().join("nope")));
}

#[test]
fn shared_worktree_clean_passes() {
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
fn shared_worktree_dirty_detected() {
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
fn shared_worktree_untracked_files_not_flagged() {
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
    let path = worktree_dir_for_run(std::path::Path::new("/target-repo"), "20260101-120000-abc");
    assert_eq!(
        path,
        PathBuf::from("/target-repo/.pdo/runs/20260101-120000-abc/worktree")
    );
}

// New per-module unit test (#276 AC "new per-module unit tests"):
// reap_orphan_sub_worktree was previously covered only end-to-end by
// crates/pdo/tests/spawn_abort_recovery.rs.
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

// ── #489-B / ADR-0037 : classify + ensure ────────────────────────────────
//
// The six locks a re-spawn can hit were MEASURED, and the #497 corollary ("it
// is the branch ref that blocks, not the directory") is true for its own
// incident and false in general:
//
// | starting state                                   | `add -b B dir base` |
// |--------------------------------------------------|---------------------|
// | nothing                                          | ✅ 0                |
// | branch + dir + registered ← the `restart_node` case | ❌ 255 branch exists |
// | branch, dir deleted, still registered            | ❌ 255              |
// | branch, dir absent and unregistered ← the #498 case | ❌ 255           |
// | branch deleted, dir non-empty                    | ❌ 128 dir exists   |
// | branch checked out in another live worktree      | ❌ 255              |

/// A repo with a Run's pipeline worktree, ready to cut sub-worktrees from.
fn repo_with_pipeline_branch(tmp: &tempfile::TempDir, run_id: &str) -> (PathBuf, String) {
    let repo = tmp.path().to_path_buf();
    init_test_repo(&repo);
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(&repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    (repo, pipeline_branch)
}

#[test]
fn classify_reports_absent_when_nothing_is_there() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-absent");
    let dir = sub_worktree_path(&repo, "cls-absent", "impl-1", 1);
    let branch = sub_worktree_branch("cls-absent", "impl-1", 1);
    assert_eq!(
        classify_sub_worktree(&repo, &dir, &branch, &base),
        SubWorktreeState::Absent
    );
}

/// **THE #489 case.** A sub-worktree registered on its own branch is reusable,
/// and its uncommitted work is exactly why.
#[test]
fn classify_reports_reusable_and_sees_the_work_in_flight() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-reuse");
    let dir = sub_worktree_path(&repo, "cls-reuse", "impl-1", 1);
    let branch = sub_worktree_branch("cls-reuse", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();

    let clean = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert_eq!(
        clean,
        SubWorktreeState::Reusable {
            has_work: false,
            base_moved: false,
            interrupted_git_ops: vec![],
        }
    );

    // Untracked counts (the opposite polarity to `worktree_has_tracked_changes`):
    // an agent's scratch files are work a reap would destroy.
    std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();
    let dirty = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert_eq!(
        dirty,
        SubWorktreeState::Reusable {
            has_work: true,
            base_moved: false,
            interrupted_git_ops: vec![],
        }
    );
}

/// Interrupted git ops are REPORTED, and the worktree stays reusable. Refusing
/// here would remove the last recovery lever on a state the restart can improve;
/// and deleting the markers ourselves is what git warns against — PDO cannot
/// prove the writer is dead (#485 is the precedent that cost).
///
/// **THE #516 case.** Both an `index.lock` AND a `MERGE_HEAD` are planted, and
/// **both** must surface, in scan order. The pre-#516 scanner returned at the
/// first marker, hiding the `MERGE_HEAD` behind the `index.lock` — the agent
/// cleared the lock it was told about, ran `pdo complete`, and the merge-back
/// took a silent two-parent commit.
#[test]
fn classify_reports_every_interrupted_git_op_without_refusing_the_reuse() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-lock");
    let dir = sub_worktree_path(&repo, "cls-lock", "impl-1", 1);
    let branch = sub_worktree_branch("cls-lock", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
    std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();

    // The gitdir is read from the `gitdir:` pointer, never derived from the
    // basename: `.git/worktrees/` is named by basename, so every node collides
    // on `iter-1` and git disambiguates to `iter-11`, `iter-12`…
    let gitdir = private_gitdir(&dir).expect("a linked worktree has a gitdir pointer");

    // A single marker still surfaces as a one-element Vec.
    std::fs::write(gitdir.join("index.lock"), "").unwrap();
    let one = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert_eq!(one.interrupted_git_ops(), ["index.lock"]);

    // #516: a coexisting `MERGE_HEAD` must NOT be masked by the `index.lock`.
    std::fs::write(gitdir.join("MERGE_HEAD"), "").unwrap();
    let both = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert_eq!(
        both,
        SubWorktreeState::Reusable {
            has_work: true,
            base_moved: false,
            interrupted_git_ops: vec!["index.lock".into(), "MERGE_HEAD".into()],
        }
    );
    // Scan order is load-bearing: `index.lock` first (the notice says "remove it
    // before anything else").
    assert_eq!(both.interrupted_git_ops(), ["index.lock", "MERGE_HEAD"]);
}

/// The #498 shape: the branch ref outlives its worktree, so `worktree add -b`
/// refuses it (exit 255) for ever. Nothing of value is on disk → recycle.
#[test]
fn classify_recycles_an_orphaned_branch_with_no_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-orphan");
    let dir = sub_worktree_path(&repo, "cls-orphan", "impl-1", 1);
    let branch = sub_worktree_branch("cls-orphan", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
    // A worktree removed cleanly, leaving only the branch behind.
    std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&dir)
        .current_dir(&repo)
        .output()
        .unwrap();

    assert!(matches!(
        classify_sub_worktree(&repo, &dir, &branch, &base),
        SubWorktreeState::Recyclable { .. }
    ));
}

/// A registration whose directory was `rm -rf`ed is prunable → recycle.
#[test]
fn classify_recycles_a_prunable_registration() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-prunable");
    let dir = sub_worktree_path(&repo, "cls-prunable", "impl-1", 1);
    let branch = sub_worktree_branch("cls-prunable", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let state = classify_sub_worktree(&repo, &dir, &branch, &base);
    let SubWorktreeState::Recyclable { detail } = state else {
        panic!("a registration pointing at a deleted dir is recyclable, got {state:?}");
    };
    assert!(detail.contains("prunable"), "{detail}");
}

/// The branch checked out in ANOTHER live worktree. Reaping it would delete a
/// directory that is not ours — refuse, and name what holds it.
#[test]
fn classify_refuses_a_branch_held_by_another_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-held");
    let dir = sub_worktree_path(&repo, "cls-held", "impl-1", 1);
    let branch = sub_worktree_branch("cls-held", "impl-1", 1);
    // Somebody checked our branch out somewhere else entirely.
    let elsewhere = repo.join("borrowed");
    create_worktree(&repo, &elsewhere, &branch, &base).unwrap();

    let state = classify_sub_worktree(&repo, &dir, &branch, &base);
    let SubWorktreeState::Occupied { detail } = state else {
        panic!("expected Occupied, got {state:?}");
    };
    assert!(detail.contains("borrowed"), "{detail}");
}

/// **#599 / ADR-0050 §3.** The agent ran `git checkout -b feature/…` in its
/// own sub-worktree, following a project git-flow. The worktree is at OUR
/// path, so it is the node's work — `Reusable`, never `Occupied`. Classifying
/// it `Occupied` would strand a restart on `sub_worktree_occupied`, refusing
/// the agent's own git (the opposite of the Sharp-tool posture, ADR-0001).
#[test]
fn classify_reuses_a_worktree_the_agent_switched_to_a_feature_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-feature");
    let dir = sub_worktree_path(&repo, "cls-feature", "impl-1", 1);
    let branch = sub_worktree_branch("cls-feature", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();

    // The agent branches its own feature off the cut.
    let out = std::process::Command::new("git")
        .args(["checkout", "-b", "feature/x"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let state = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert!(
        matches!(state, SubWorktreeState::Reusable { .. }),
        "a worktree at our path on a feature branch is Reusable, not Occupied; got {state:?}"
    );

    // Work in flight is still seen (untracked scratch counts).
    std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();
    let dirty = classify_sub_worktree(&repo, &dir, &branch, &base);
    assert!(matches!(
        dirty,
        SubWorktreeState::Reusable { has_work: true, .. }
    ));

    // …and `ensure_sub_worktree` reuses it in place, creating nothing and
    // destroying nothing — the whole point of not calling it `Occupied`.
    let ensured = ensure_sub_worktree(&repo, &dir, &branch, &base, Some("deadbeef")).unwrap();
    assert!(
        !ensured.created,
        "a feature-branch worktree is reused, not re-cut"
    );
    assert!(
        dir.join("scratch.txt").exists(),
        "the agent's work must survive"
    );
}

/// **#599 / ADR-0036 (amended).** The merge-back follows the sub-worktree's
/// real HEAD, so work the agent committed on a `feature/…` branch lands in the
/// pipeline branch — merging by the `pdo/sub-*` name instead would bring back
/// the stale tip ("Already up to date") and drop the work in silence.
#[test]
fn merge_back_follows_head_when_the_agent_switched_to_a_feature_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-feature-merge";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

    let sub_wt = sub_worktree_path(repo, run_id, "impl-1", 1);
    let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
    create_sub_worktree(repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();

    // The agent branches off and works on feature/x; pdo/sub-* stays behind.
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&sub_wt)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["checkout", "-b", "feature/x"]);
    std::fs::write(sub_wt.join("delivered.rs"), "pub fn delivered() {}\n").unwrap();

    let result = commit_and_merge_sub_worktree(&sub_wt, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
    assert!(matches!(result, MergeResult::Success), "got {result:?}");
    assert!(
        wt_dir.join("delivered.rs").exists(),
        "work committed on feature/x must reach the pipeline worktree — no silent \
         'Already up to date'"
    );

    // The fixture really did leave the pdo/sub-* ref behind, or the test would
    // pass for the wrong reason (merging the name would have worked too).
    let stale_name_tip = rev_parse(repo, &sub_branch).unwrap();
    let real_head = rev_parse(&sub_wt, "HEAD").unwrap();
    assert_ne!(
        stale_name_tip, real_head,
        "pdo/sub-* must stay behind feature/x, or this tests nothing"
    );
}

/// A non-empty directory git never registered. `worktree add` fails 128 there,
/// and we have no idea what is in it → refuse.
#[test]
fn classify_refuses_a_foreign_non_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-foreign");
    let dir = sub_worktree_path(&repo, "cls-foreign", "impl-1", 1);
    let branch = sub_worktree_branch("cls-foreign", "impl-1", 1);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("junk"), "?\n").unwrap();

    assert!(matches!(
        classify_sub_worktree(&repo, &dir, &branch, &base),
        SubWorktreeState::Occupied { .. }
    ));
}

/// Measured nuance: an EMPTY unregistered directory is accepted by
/// `git worktree add`, so `Absent` is not "nothing on disk".
#[test]
fn classify_treats_an_empty_foreign_directory_as_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "cls-empty");
    let dir = sub_worktree_path(&repo, "cls-empty", "impl-1", 1);
    let branch = sub_worktree_branch("cls-empty", "impl-1", 1);
    std::fs::create_dir_all(&dir).unwrap();

    assert_eq!(
        classify_sub_worktree(&repo, &dir, &branch, &base),
        SubWorktreeState::Absent
    );
    // …and the create really does succeed from there.
    assert!(ensure_sub_worktree(&repo, &dir, &branch, &base, None).is_ok());
}

/// **THE REGRESSION.** `create_sub_worktree` on an existing sub-worktree fails
/// 255; `ensure_sub_worktree` reuses it and touches nothing.
#[test]
fn ensure_reuses_an_existing_sub_worktree_and_destroys_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-reuse");
    let dir = sub_worktree_path(&repo, "ens-reuse", "impl-1", 1);
    let branch = sub_worktree_branch("ens-reuse", "impl-1", 1);

    let first = ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
    assert!(first.created);
    let original_base = first
        .base_sha
        .clone()
        .expect("a fresh cut records its base");

    // The bare create — what every re-spawn used to call — refuses outright.
    let err = create_sub_worktree(&repo, &dir, &branch, &base)
        .expect_err("git worktree add -b on an existing branch must fail");
    assert!(format!("{err:#}").contains("already exists"), "{err:#}");

    // Work in flight: an untracked file AND a tracked file modified. The tracked
    // leg is the load-bearing one — it is the only thing that distinguishes
    // "reused" from "committed first, then re-cut".
    std::fs::write(dir.join("scratch.txt"), "in flight\n").unwrap();
    std::fs::write(dir.join("README.md"), "# edited\n").unwrap();
    let head_before = rev_parse(&dir, "HEAD").unwrap();

    let second = ensure_sub_worktree(&repo, &dir, &branch, &base, Some(&original_base)).unwrap();
    assert!(!second.created, "a reuse creates nothing");
    assert!(matches!(
        second.entry_state,
        SubWorktreeState::Reusable { has_work: true, .. }
    ));
    // #503 / ADR-0036: the ORIGINAL base, carried over. Re-deriving `HEAD` here
    // would yield the node's own commit and silently kill the adoption rule for
    // every restarted node.
    assert_eq!(second.base_sha.as_deref(), Some(original_base.as_str()));

    assert_eq!(
        std::fs::read_to_string(dir.join("scratch.txt")).unwrap(),
        "in flight\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("README.md")).unwrap(),
        "# edited\n"
    );
    assert_eq!(rev_parse(&dir, "HEAD").unwrap(), head_before);
}

/// The #498 lever: an orphaned branch is reaped and re-cut, and the reap now
/// actually works (`worktree prune` before `branch -D`).
#[test]
fn ensure_recycles_an_orphaned_branch_and_re_cuts() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-recycle");
    let dir = sub_worktree_path(&repo, "ens-recycle", "impl-1", 1);
    let branch = sub_worktree_branch("ens-recycle", "impl-1", 1);
    ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
    // The failure mode #498 documents: the directory vanishes, the registration
    // and the branch ref stay.
    std::fs::remove_dir_all(&dir).unwrap();

    let again = ensure_sub_worktree(&repo, &dir, &branch, &base, None).unwrap();
    assert!(again.created);
    assert!(matches!(
        again.entry_state,
        SubWorktreeState::Recyclable { .. }
    ));
    assert!(dir.exists());
    assert!(again.base_sha.is_some());
}

#[test]
fn ensure_refuses_an_occupied_sub_worktree_without_touching_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "ens-occupied");
    let dir = sub_worktree_path(&repo, "ens-occupied", "impl-1", 1);
    let branch = sub_worktree_branch("ens-occupied", "impl-1", 1);
    let elsewhere = repo.join("borrowed");
    create_worktree(&repo, &elsewhere, &branch, &base).unwrap();

    let err = ensure_sub_worktree(&repo, &dir, &branch, &base, None)
        .expect_err("an occupied sub-worktree must be refused, never reaped");
    assert!(format!("{err:#}").contains("occupied"), "{err:#}");
    // Untouched: the other worktree is still there, on the branch.
    assert!(elsewhere.exists());
    assert!(elsewhere.join("README.md").exists());
}

/// #489-B(a): the pre-#489 reap left BOTH locks in place when the directory had
/// already gone — `branch -D` fails on a branch a registration still pins.
#[test]
fn reap_clears_a_registration_whose_directory_already_vanished() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "reap-missing");
    let dir = sub_worktree_path(&repo, "reap-missing", "impl-1", 1);
    let branch = sub_worktree_branch("reap-missing", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    reap_orphan_sub_worktree(&repo, &dir, &branch);

    let branches = std::process::Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
        "the branch must be gone, not pinned by a stale registration"
    );
    // And the whole point: a fresh cut is possible again.
    assert!(create_sub_worktree(&repo, &dir, &branch, &base).is_ok());
}

/// #489-B(b) — **the silent total loss**. With a leftover `index.lock`,
/// `git add -A` exits 128, `diff --cached --quiet` exits 0, no commit is taken,
/// `git merge` says "Already up to date" and the pre-#489 code returned
/// `MergeResult::Success` on 100% of the work lost — no conflict, no event, no
/// trace.
#[test]
fn a_failing_git_add_fails_loudly_instead_of_reporting_a_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, base) = repo_with_pipeline_branch(&tmp, "add-fails");
    let wt_dir = repo.join(".pdo/runs/add-fails/worktree");
    let dir = sub_worktree_path(&repo, "add-fails", "impl-1", 1);
    let branch = sub_worktree_branch("add-fails", "impl-1", 1);
    create_sub_worktree(&repo, &dir, &branch, &base).unwrap();

    std::fs::write(dir.join("the_whole_point.rs"), "fn main() {}\n").unwrap();
    let gitdir = private_gitdir(&dir).unwrap();
    std::fs::write(gitdir.join("index.lock"), "").unwrap();

    let err = commit_and_merge_sub_worktree(&dir, &wt_dir, &branch, "impl-1", 1)
        .expect_err("a failed `git add -A` must not project to MergeResult::Success");
    assert!(
        format!("{err:#}").contains("git add -A"),
        "the failure must name what broke; got {err:#}"
    );
    // And the loss is what it protects: the file never reached the pipeline tree.
    assert!(!wt_dir.join("the_whole_point.rs").exists());
}

#[test]
fn create_sub_worktree_spawn_failure_preserves_os_cause() {
    // Regression guard (#298): on the spawn-failure branch the OS-level
    // io::Error must survive as a *queryable* `source()`, i.e. a 2-link anyhow
    // chain [context, io::Error]. The pre-#298 duplicate used
    // `map_err(anyhow!("…: {e}"))`, which flattened the cause into the message
    // → a 1-link chain. Asserting the chain length (not the OS-specific string)
    // makes this test portable and gives it teeth against a regression to the
    // flattening form.
    let tmp = tempfile::tempdir().unwrap();

    // repo_root does NOT exist → git's chdir(current_dir) fails with ENOENT
    // *before* exec, so `.output()` returns Err(io::Error) (the spawn-failure
    // branch). This holds even if `git` is not installed on the runner.
    let nonexistent_repo = tmp.path().join("no-such-repo");

    // sub_worktree_dir's parent IS a valid writable temp path, so the earlier
    // `create_dir_all(parent)?` succeeds and does not shield the .output() error.
    let sub_wt_dir = tmp.path().join("sub").join("iter-1");

    let err = create_sub_worktree(&nonexistent_repo, &sub_wt_dir, "pdo/sub-x", "base")
        .expect_err("spawn must fail when repo_root does not exist");

    assert_eq!(
        err.chain().count(),
        2,
        "OS cause must be preserved as a distinct source(); got chain: {err:#}"
    );
}

// ---- #465 (ADR-0042): secondary read-only snapshots ----

/// Seed a repo with a named tracked file and return its HEAD sha.
fn init_repo_with_file(dir: &std::path::Path, name: &str, contents: &str) -> String {
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
    std::fs::write(dir.join(name), contents).unwrap();
    run(&["add", "-A"]);
    run(&["commit", "-m", "init"]);
    rev_parse(dir, "HEAD").unwrap()
}

#[test]
fn secondary_snapshot_path_is_third_sibling() {
    let path = secondary_snapshot_path(
        std::path::Path::new("/primary"),
        "20260101-120000-abc",
        "repoB",
    );
    assert_eq!(
        path,
        PathBuf::from("/primary/.pdo/runs/20260101-120000-abc/repos/repoB"),
    );
}

#[test]
fn rev_parse_verified_resolves_head_and_rejects_bogus() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let sha = init_repo_with_file(repo, "B.txt", "SECONDARY-MARKER-v1\n");
    assert_eq!(rev_parse_verified(repo, "HEAD").unwrap(), sha);
    // A ref that resolves to nothing / is ambiguous must fail loud.
    assert!(rev_parse_verified(repo, "no-such-branch").is_err());
}

#[test]
fn create_secondary_snapshot_pins_and_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("repoA");
    let secondary = tmp.path().join("repoB");
    std::fs::create_dir_all(&primary).unwrap();
    std::fs::create_dir_all(&secondary).unwrap();
    init_test_repo(&primary);
    let sha_b = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

    let run_id = "test-sec-run";
    let dest = secondary_snapshot_path(&primary, run_id, "repoB");
    create_secondary_snapshot(&secondary, &dest, &sha_b).unwrap();

    // The snapshot exists, is a THIRD sibling of worktree/ and nodes/, is pinned
    // to the frozen SHA, and its file is readable.
    assert!(dest.join("B.txt").exists());
    assert_eq!(rev_parse(&dest, "HEAD").unwrap(), sha_b);
    assert_eq!(
        std::fs::read_to_string(dest.join("B.txt")).unwrap(),
        "SECONDARY-MARKER-v1\n",
    );
    // Registered in the SECONDARY's .git, not the primary's.
    let listed = std::process::Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&secondary)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&listed.stdout).contains("repos/repoB"));
}

#[test]
fn secondary_snapshot_isolation_survives_local_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("repoA");
    let secondary = tmp.path().join("repoB");
    std::fs::create_dir_all(&primary).unwrap();
    std::fs::create_dir_all(&secondary).unwrap();
    init_test_repo(&primary);
    let sha_v1 = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

    let dest = secondary_snapshot_path(&primary, "iso-run", "repoB");
    create_secondary_snapshot(&secondary, &dest, &sha_v1).unwrap();

    // Advance the operator's LOCAL checkout of the secondary AFTER the snapshot.
    std::fs::write(secondary.join("B.txt"), "SECONDARY-MARKER-v2\n").unwrap();
    let commit = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&secondary)
            .output()
            .unwrap();
    };
    commit(&["commit", "-aqm", "B: v2"]);

    // The snapshot must still be pinned at v1 (it is a snapshot, not a live mount).
    assert_eq!(rev_parse(&dest, "HEAD").unwrap(), sha_v1);
    assert_eq!(
        std::fs::read_to_string(dest.join("B.txt")).unwrap(),
        "SECONDARY-MARKER-v1\n",
    );
}

#[test]
fn dirty_tracked_trips_guard_untracked_is_tolerated() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("repoA");
    let secondary = tmp.path().join("repoB");
    std::fs::create_dir_all(&primary).unwrap();
    std::fs::create_dir_all(&secondary).unwrap();
    init_test_repo(&primary);
    let sha = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

    let dest = secondary_snapshot_path(&primary, "guard-run", "repoB");
    create_secondary_snapshot(&secondary, &dest, &sha).unwrap();

    // Clean snapshot → not dirty.
    assert!(!worktree_has_tracked_changes(&dest).unwrap());
    // Untracked scratch → tolerated (the probe ignores `??`).
    std::fs::write(dest.join("scratch_untracked.tmp"), "x").unwrap();
    assert!(!worktree_has_tracked_changes(&dest).unwrap());
    // A TRACKED modification → dirty (the guard trips).
    std::fs::write(dest.join("B.txt"), "tampered\n").unwrap();
    assert!(worktree_has_tracked_changes(&dest).unwrap());
}

#[test]
fn remove_secondary_snapshot_prunes_registration_anti_498() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("repoA");
    let secondary = tmp.path().join("repoB");
    std::fs::create_dir_all(&primary).unwrap();
    std::fs::create_dir_all(&secondary).unwrap();
    init_test_repo(&primary);
    let sha = init_repo_with_file(&secondary, "B.txt", "SECONDARY-MARKER-v1\n");

    let dest = secondary_snapshot_path(&primary, "cleanup-run", "repoB");
    create_secondary_snapshot(&secondary, &dest, &sha).unwrap();

    remove_secondary_snapshot(&secondary, &dest);

    // The registration is gone from the secondary (prune ran), and a fresh
    // `worktree add` at the SAME path succeeds — the #498 anti-dangling control.
    let listed = std::process::Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&secondary)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("repos/repoB"));

    let re_add = std::process::Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&dest)
        .arg(&sha)
        .current_dir(&secondary)
        .output()
        .unwrap();
    assert!(
        re_add.status.success(),
        "re-add at the same path must succeed (no dangling registration): {}",
        String::from_utf8_lossy(&re_add.stderr)
    );
}

// ── the identity of a delivery (CORE-43) ─────────────────────────────────────

/// Alice as a Node's environment gives her: author and committer.
fn alice() -> CommitIdentity {
    let env: std::collections::BTreeMap<String, String> = [
        ("GIT_AUTHOR_NAME", "Alice"),
        ("GIT_AUTHOR_EMAIL", "alice@example.com"),
        ("GIT_COMMITTER_NAME", "Alice"),
        ("GIT_COMMITTER_EMAIL", "alice@example.com"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    CommitIdentity::from_env(&env)
}

/// `author / committer` of `rev`, as `git log` prints them.
fn signed_by(dir: &std::path::Path, rev: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["log", "-1", "--format=%an %ae / %cn %ce", rev])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

const ALICE: &str = "Alice alice@example.com / Alice alice@example.com";

#[test]
fn deliver_in_place_signs_the_delivery_commit_with_the_given_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-identity";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    create_worktree(repo, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();
    std::fs::write(wt_dir.join("shared.rs"), "pub fn shared() {}\n").unwrap();

    deliver_node_work(
        &wt_dir,
        &wt_dir,
        false,
        "unused",
        "worker",
        1,
        None,
        &alice(),
    )
    .unwrap();
    assert_eq!(signed_by(&wt_dir, "HEAD"), ALICE);
}

/// The sub-worktree's commit and the merge commit that brings it back are both
/// Alice's: the Run's branch moved while the node worked, so the merge-back is a
/// true merge, not a fast-forward.
#[test]
fn deliver_isolated_signs_the_commit_and_the_merge_with_the_given_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);

    let run_id = "test-deliver-isolated-identity";
    let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
    let pipeline_branch = format!("pdo/run-{run_id}");
    create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();
    let sub_wt = sub_worktree_path(repo, run_id, "forked", 1);
    let sub_branch = sub_worktree_branch(run_id, "forked", 1);
    create_sub_worktree(repo, &sub_wt, &sub_branch, &pipeline_branch).unwrap();
    std::fs::write(wt_dir.join("sibling.rs"), "pub fn sibling() {}\n").unwrap();
    stage_and_commit(&wt_dir, "sibling", 1, &CommitIdentity::default()).unwrap();
    std::fs::write(sub_wt.join("forked.rs"), "pub fn forked() {}\n").unwrap();

    let outcome = deliver_node_work(
        &wt_dir,
        &sub_wt,
        true,
        &sub_branch,
        "forked",
        1,
        None,
        &alice(),
    )
    .unwrap();
    assert!(matches!(outcome, DeliveryOutcome::Delivered(_)));
    assert_eq!(signed_by(&sub_wt, "HEAD"), ALICE, "the sub-worktree commit");
    assert_eq!(
        rev_parse(&wt_dir, "HEAD^2").unwrap(),
        rev_parse(&sub_wt, "HEAD").unwrap(),
        "the merge-back is a merge commit"
    );
    assert_eq!(signed_by(&wt_dir, "HEAD"), ALICE, "the merge commit");
}

#[test]
fn the_resolution_in_the_node_favour_is_signed_with_the_given_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let node = rebased_terminal_node_repo(&tmp, true);

    let result = commit_and_merge_sub_worktree_inner(
        &node.sub_wt,
        &node.pipeline_wt,
        &node.sub_branch,
        "ship",
        1,
        Some(&node.spawn_base),
        &alice(),
    )
    .unwrap();
    let MergeResult::ResolvedInNodeFavour(adoption) = result else {
        panic!("expected the merge-back to be resolved in the node's favour");
    };
    assert_eq!(signed_by(&node.pipeline_wt, &adoption.merge_commit), ALICE);
}

/// Without a frozen identity, git signs with what the repository says.
#[test]
fn an_empty_identity_keeps_the_repository_one() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_test_repo(repo);
    std::fs::write(repo.join("work.rs"), "pub fn work() {}\n").unwrap();

    let sha = stage_and_commit(repo, "n", 1, &CommitIdentity::default())
        .unwrap()
        .unwrap();
    assert_ne!(signed_by(repo, &sha), ALICE);
}
