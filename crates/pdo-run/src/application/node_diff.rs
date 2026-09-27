//! A node's own diff (a read).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::application::events::load_projected;
use crate::application::paths::effective_repo_root;
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::sub_worktree_branch;

/// The diff of a node's own branch against the Run's (a read: it asks no
/// authorization).
pub async fn node_diff(state: &Runs, run_id: String, node_id: String) -> Response {
    let (_, run_state) = match load_projected(state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    let node = match run_state.nodes.get(&node_id) {
        Some(n) => n,
        None => return (StatusCode::NOT_FOUND, "node not found").into_response(),
    };

    // #654 / ADR-0060: the delivery is what a NodeRun contributed, so the diff is
    // between the two Run-branch tips it moved between — the ONE answer that works
    // for a non-isolated NodeRun (which owns no branch to diff) as well as for an
    // isolated one. The type and the isolation never enter into it.
    //
    // The `pdo/sub-*` fallback is for a NodeRun that has not delivered yet: an
    // isolated node still working, or a pre-#654 Run whose log carries no
    // delivery. Its work is real and visible on its branch; it is just not on the
    // Run's branch yet.
    let (left, right) = match &node.delivery {
        Some(d) => (d.before.clone(), d.after.clone()),
        None => (
            format!("pdo/run-{run_id}"),
            sub_worktree_branch(&run_id, &node_id, node.iter),
        ),
    };

    // `:(exclude).pdo/` mirrors `run_diff`: the blackboard artefacts are the Run's
    // plumbing, not the node's contribution, and a delivery stages them like
    // anything else the repo does not ignore (#654 — `.gitignore` is the target
    // repo's policy, and PDO must not pretend otherwise at commit time; this is
    // the *reading* surface).
    // #748: same repository as `run_diff` — the Run's effective repo, not the cwd.
    let repo = effective_repo_root(state, &run_state);
    let output = match std::process::Command::new("git")
        .args(["diff", &left, &right, "--", ".", ":(exclude).pdo/"])
        .current_dir(&repo)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("git diff failed: {e}"),
            )
                .into_response();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown revision") || stderr.contains("not a git repository") {
            return (StatusCode::NOT_FOUND, "node branch not found").into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("git diff failed: {stderr}"),
        )
            .into_response();
    }

    let diff = String::from_utf8_lossy(&output.stdout);
    (StatusCode::OK, diff.into_owned()).into_response()
}
