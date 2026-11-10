//! The teardown of ONE Run: its sessions killed, its sandbox merged back and
//! purged, its outputs preserved to the durable store (ADR-0020), its
//! worktrees, branches, secondary snapshots and run directory removed, then
//! `RunArchived` appended.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use tracing::{error, info, warn};

use crate::application::create_run::repos::{secondary_repos_dir, snapshot_owning_secondary};
use crate::application::events::append_event;
use crate::application::paths::{dirs_next_home, durable_run_dir, effective_repo_root};
use crate::application::sandbox::{cleanup, sandbox_home_roots};
use crate::domain::event::{Event, EventKind};
use crate::domain::state::RunState;
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::remove_secondary_snapshot;

/// Tear ONE run down and mark it `Archived`: kill its tmux sessions, merge and
/// purge its sandbox, preserve its outputs to the durable store, remove its
/// worktrees, branches, secondary snapshots and run dir, then append
/// `RunArchived`. No cascade and no status precondition here — the caller
/// ([`super::cleanup_run`]) holds the page-mount lock and has already decided this
/// run must go.
pub async fn archive_single_run(state: &Runs, run_state: &RunState) -> Result<(), Box<Response>> {
    let run_id = run_state.run_id.as_str();

    if let Err(error) = state.pages.release(run_id).await {
        error!("cleanup_run: failed to remove page mounts for run {run_id}: {error}");
        return Err(Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "page mount cleanup failed" })),
            )
                .into_response(),
        ));
    }

    let tmux = state.tmux();
    for node in run_state.nodes.values() {
        let session_name =
            pdo_session::domain::names::node_session_name(run_id, &node.node_id, node.iter);
        tmux.kill(&session_name);
    }
    let mgr_session = pdo_session::domain::names::manager_session_name(run_id);
    tmux.kill(&mgr_session);
    // Kill any ad-hoc run shell before the worktree is torn down.
    let shell_session = pdo_session::domain::names::shell_session_name(run_id);
    tmux.kill(&shell_session);

    // Merge the transcripts back, then destroy the container and purge its staging,
    // all BEFORE any `git worktree remove` below: the container bind-mounts the repo,
    // so removing a live worktree under it hits a busy mount. The tmux sessions were
    // killed just above, so the staging is quiescent. `cleanup` merges FIRST —
    // harvest before purge — capturing post-terminal growth the detached terminal
    // merge missed; idempotent, so the double-merge is safe.
    if !run_state.sandbox.is_off() {
        match sandbox_home_roots(state) {
            Ok((home_root, sandbox_root)) => cleanup(
                state.docker_cmd_override.as_deref().unwrap_or("docker"),
                &home_root,
                &sandbox_root,
                run_id,
            ),
            Err(e) => {
                warn!("cleanup_run: cannot merge/purge sandbox staging for run {run_id}: {e:#}")
            }
        }
    }

    let repo_root = effective_repo_root(state, run_state);
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);

    // Copy the run's outputs to the durable store BEFORE any worktree is destroyed,
    // so an archived run stays inspectable (ADR-0020). Ordering is load-bearing:
    // `git worktree remove` wipes `worktree/.pdo/artifacts` well before the
    // `remove_dir_all(&run_dir)` at the end.
    preserve_run_outputs_on_archive(&repo_root, &run_dir, run_id);

    // Sub-worktrees (nodes/) must go before the main worktree.
    let nodes_dir = run_dir.join("nodes");
    if nodes_dir.exists() {
        for node_entry in std::fs::read_dir(&nodes_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            for iter_entry in std::fs::read_dir(node_entry.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let sub_wt = iter_entry.path();
                if sub_wt.is_dir() {
                    let _ = std::process::Command::new("git")
                        .args(["worktree", "remove", "--force"])
                        .arg(&sub_wt)
                        .current_dir(&repo_root)
                        .output();
                }
            }
        }
    }

    let worktree_dir = run_dir.join("worktree");
    if worktree_dir.exists() {
        let output = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_dir)
            .current_dir(&repo_root)
            .output();
        if let Ok(o) = output {
            if !o.status.success() {
                warn!(
                    "git worktree remove failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
    }

    // #672 / ADR-0062: take back the per-skill exclusions this Run wrote in the target
    // repo's `info/exclude` (its `# pdo <run-id>` marked lines only — every other
    // line, PDO's for another Run or the user's, stays intact).
    if let Err(e) = state.skills.withdraw(&repo_root, run_id) {
        warn!("cleanup_run: cannot remove skill exclusions for run {run_id}: {e:#}");
    }

    for pattern in [format!("pdo/run-{run_id}"), format!("pdo/sub-{run_id}*")] {
        let branch_output = std::process::Command::new("git")
            .args(["branch", "--list", &pattern])
            .current_dir(&repo_root)
            .output();
        if let Ok(o) = branch_output {
            let branches = String::from_utf8_lossy(&o.stdout);
            for branch in branches.lines() {
                let branch = branch.trim().trim_start_matches("* ");
                if !branch.is_empty() {
                    let _ = std::process::Command::new("git")
                        .args(["branch", "-D", branch])
                        .current_dir(&repo_root)
                        .output();
                }
            }
        }
    }

    // Tear down every secondary snapshot BEFORE `remove_dir_all`, and from inside the
    // SECONDARY repo: the `--detach` registration lives in the secondary's `.git`,
    // OUTSIDE `repo_root`, so `remove_dir_all(run_dir)` cannot clear it. Without the
    // per-secondary `prune`, every multi-repo Run leaks a dangling registration and a
    // future `worktree add` at the same path fails 255.
    //
    // DISK-DRIVEN, not projection-driven: the scan must cover snapshots the projection
    // no longer lists — a secondary removed mid-run keeps its snapshot until now, and
    // an orphan snapshot whose event never landed has no pin at all. Iterating
    // `target_repos` would skip and leak both.
    let repos_dir = secondary_repos_dir(&repo_root, run_id);
    for entry in std::fs::read_dir(&repos_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let snapshot_dir = entry.path();
        if !snapshot_dir.is_dir() {
            continue;
        }
        match snapshot_owning_secondary(&snapshot_dir) {
            Some(secondary) => remove_secondary_snapshot(&secondary, &snapshot_dir),
            None => {
                warn!(
                    "cleanup_run: cannot resolve the owning secondary repo of snapshot {} \
                     (its git pointer is unreadable); removing the directory but leaving any \
                     dangling registration for `git worktree prune` in the secondary",
                    snapshot_dir.display()
                );
                let _ = std::fs::remove_dir_all(&snapshot_dir);
            }
        }
    }

    if run_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&run_dir) {
            warn!("failed to remove run dir {}: {e}", run_dir.display());
        }
    }

    let archived_event = Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: EventKind::RunArchived,
        node_id: None,
        iter: None,
        payload: None,
    };
    if let Err(e) = append_event(state, &archived_event).await {
        error!("failed to append run_archived: {e}");
        return Err(Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response(),
        ));
    }

    info!("Run {run_id} archived (cleanup complete)");
    Ok(())
}

/// Recursively copy `src` into `dst`. Plain files and subdirectories ONLY — sound
/// because the artifacts tree the callers copy holds `output.md` files plus optional
/// images, with no symlinks or nested `.git`.
pub(crate) fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Preserve a run's durable outputs from the soon-to-be-destroyed repo-local
/// `run_dir` into `~/.pdo/runs/<id>/`, so an archived run stays inspectable
/// (ADR-0020).
///
/// Best-effort throughout: any failure is logged and swallowed, because archival MUST
/// still succeed and append `RunArchived`. Called from `cleanup_run` **before** the
/// worktree is destroyed — copy first, delete after.
pub(crate) fn preserve_run_outputs_on_archive(
    repo_root: &std::path::Path,
    run_dir: &std::path::Path,
    run_id: &str,
) {
    let Some(durable) = durable_run_dir(run_id) else {
        warn!("#315: $HOME unset — skipping durable output preservation for run {run_id}");
        return;
    };

    // If the target repo *is* $HOME then `run_dir == durable`, and the copy would sit
    // inside the tree `remove_dir_all(run_dir)` is about to delete. A repo merely
    // *under* $HOME is disjoint and fine.
    let repo_is_home = match (
        repo_root.canonicalize(),
        dirs_next_home().and_then(|h| h.canonicalize().ok()),
    ) {
        (Ok(repo), Some(home)) => repo == home,
        _ => false,
    };
    if repo_is_home {
        warn!(
            "#315: repo root == $HOME for run {run_id} — durable store overlaps run dir, skipping preservation"
        );
        return;
    }

    let src_artifacts = run_dir.join("worktree").join(".pdo").join("artifacts");
    let src_yaml = run_dir.join("pipeline.yaml");
    let src_prompts = run_dir.join("pipeline.prompts");

    // Nothing on disk to preserve. Return WITHOUT creating an empty durable dir, or a
    // no-op archive litters `~/.pdo/runs/` with hollow directories.
    if !src_artifacts.is_dir() && !src_yaml.is_file() && !src_prompts.is_dir() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&durable) {
        warn!(
            "#315: failed to create durable dir {}: {e}",
            durable.display()
        );
        return;
    }

    if src_artifacts.is_dir() {
        if let Err(e) = copy_dir_all(&src_artifacts, &durable.join("artifacts")) {
            warn!("#315: failed to preserve artifacts for run {run_id}: {e}");
        }
    }

    if src_yaml.is_file() {
        if let Err(e) = std::fs::copy(&src_yaml, durable.join("pipeline.yaml")) {
            warn!("#315: failed to preserve pipeline.yaml for run {run_id}: {e}");
        }
    }

    if src_prompts.is_dir() {
        if let Err(e) = copy_dir_all(&src_prompts, &durable.join("pipeline.prompts")) {
            warn!("#315: failed to preserve pipeline.prompts for run {run_id}: {e}");
        }
    }
}
