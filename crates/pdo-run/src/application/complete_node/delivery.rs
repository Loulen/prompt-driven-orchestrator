//! Delivering a node iteration's work into the Run's branch: the sub-worktree
//! merged back, a shared worktree left clean, the read-only secondaries
//! untouched.

use std::path::Path;
use std::sync::Arc;

use tracing::{error, info, warn};

use pdo_workspace::adapters::git::worktrees;
use pdo_workspace::adapters::git::worktrees::{
    secondary_snapshot_path, sub_worktree_branch, sub_worktree_path, worktree_has_tracked_changes,
};

use super::*;
use crate::application::node::reap::short;
use crate::domain::{completion_refusal, event, merge_action, state};
use crate::Runs;

/// The shared node-completion body: everything `POST …/done` does, for either
/// caller (#469 §3).
///
/// The read-only guard for secondary repos, as a pure probe (ADR-0042).
///
/// Returns the FIRST secondary whose snapshot has uncommitted **tracked** changes.
/// `worktree_has_tracked_changes` ignores `??`, so untracked scratch is tolerated. A
/// transient git error fails OPEN (warn + skip) rather than blocking a legitimate
/// completion. Called at the completion edge, never from `transition_guard`, which is
/// pure and IO-free.
pub fn secondary_repos_dirtied_refusal(
    repo_root: &Path,
    run_id: &str,
    run_state: &state::RunState,
) -> Option<completion_refusal::CompletionRefusal> {
    for pin in &run_state.target_repos {
        // A writable secondary (the default) is *meant* to be modified, so its dirty
        // tracked tree is not a refusal. This `continue` is the single switch that
        // turns "writing is refused" into "writing is offered" (ADR-0047).
        if !pin.read_only {
            continue;
        }
        let snapshot = secondary_snapshot_path(repo_root, run_id, &pin.alias);
        if !snapshot.exists() {
            continue;
        }
        match worktree_has_tracked_changes(&snapshot) {
            Ok(true) => {
                return Some(
                    completion_refusal::CompletionRefusal::SecondaryRepoDirtied {
                        alias: pin.alias.clone(),
                        message: format!(
                            "the read-only secondary repo '{}' ({}) has uncommitted changes to \
                         tracked files; revert them (git checkout) before completing — a \
                         secondary is read-only (#465)",
                            pin.alias,
                            snapshot.display()
                        ),
                    },
                );
            }
            Ok(false) => {}
            Err(e) => {
                warn!(
                    "could not check secondary snapshot cleanliness for '{}' in run {run_id}: \
                     {e}; allowing the completion",
                    pin.alias
                );
            }
        }
    }
    None
}

/// **The** delivery of a NodeRun's work onto the Run's branch (#654 / ADR-0060).
///
/// Every completion path reaches this one function — `pdo complete`, the injected
/// `Stop` hook, the liveness sweep's turn-end auto-completion, a human's *Mark
/// complete* (`mark_node_done`) and a `script` node's own tail — and it runs
/// **before** the terminal event is appended, therefore before any downstream node
/// is dispatched: an isolated successor forked after this point sees a
/// non-isolated predecessor's work.
///
/// PDO gives the Node no git instruction: it edits files, and the runtime delivers
/// what it left. Whatever the node committed itself is kept as it is; whatever it
/// left behind becomes one commit under `<node-id> iter-<N>: completed`, staged
/// with `git add -A` semantics and the repo's own `.gitignore` as the only
/// exclusion policy. An isolated NodeRun then has its sub-worktree merged back; a
/// non-isolated one has already committed in place. Nothing left behind ⇒ no
/// commit, no event, no empty commit.
///
/// `None` ⇒ the completion may carry on. `Some(refusal)` ⇒ it must stop, and every
/// refusal built here has already appended its own parking events — the caller
/// only projects it.
#[allow(clippy::too_many_arguments)]
pub async fn deliver_node_run(
    state: &Arc<Runs>,
    events: &[event::Event],
    run_state: &state::RunState,
    repo_root: &std::path::Path,
    worktree_dir: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Option<completion_refusal::CompletionRefusal> {
    // #653/ADR-0060: where the NodeRun WORKED, read from its frozen isolation —
    // not from a type that no longer implies a working directory.
    let isolated = node_run_isolation(events, run_state, node_id, iter);
    let node_worktree_dir = if isolated {
        sub_worktree_path(repo_root, run_id, node_id, iter)
    } else {
        worktree_dir.to_path_buf()
    };
    let sub_branch = sub_worktree_branch(run_id, node_id, iter);

    // An absent workspace is not a git failure. A Run whose worktree is gone —
    // archived or cleaned up (ADR-0020) — has nothing to deliver in place, and
    // parking the Run over it would punish a completion that arrives late through
    // no fault of the node. Warn and let it through, exactly as the pre-#654
    // cleanliness probe did. An ISOLATED NodeRun deliberately does not take this
    // exit: its merge-back must still fail loudly if either worktree is missing,
    // because that is #489's silent-loss guard.
    if !isolated && !worktrees::is_git_worktree(worktree_dir) {
        warn!(
            "Run {run_id}: {} is not a git worktree — nothing to deliver for {node_id} iter-{iter}",
            worktree_dir.display()
        );
        return None;
    }

    // #503 / ADR-0036: the commit an isolated sub-worktree was cut from, read off
    // the log we already loaded. On a conflict, `worktree_ops` compares it to the
    // pipeline branch's actual tip: still equal ⇒ the divergence is the run's own
    // history rewritten by this node, and resolving in its favour drops nothing.
    // No base recorded ⇒ no resolution.
    let spawn_base = merge_action::spawn_base_sha(events, node_id, iter);
    // CORE-43: the identity the extensions gave the node signs every commit of
    // its delivery, as it signs the commits the node made itself.
    let identity = merge_action::frozen_git_identity(events, node_id, iter);

    // The lock guards the *git operation*, not the nodes' work: two non-isolated
    // NodeRuns still run side by side in the shared worktree with no runtime
    // serialisation (ADR-0060). It only stops two deliveries from racing on the
    // same index — which git would refuse anyway — and it is what makes "the first
    // one to complete commits everything then visible" a statement rather than a
    // coin toss.
    let delivery = {
        let _lock = state.merge_lock.lock().await;
        worktrees::deliver_node_work(
            worktree_dir,
            &node_worktree_dir,
            isolated,
            &sub_branch,
            node_id,
            iter,
            spawn_base.as_deref(),
            &identity,
        )
    };

    let outcome = match delivery {
        Ok(o) => o,
        Err(e) => {
            // A staging, commit or merge failure is an infra give-up, never a
            // business failure (ADR-0049): the node is `Interrupted`, the Run
            // parks `AwaitingUser` with the reason, and the work stays on disk
            // exactly where the node left it — nothing is reverted, nothing is
            // cleaned.
            let reason = format!("delivery_failed: could not deliver {node_id} iter-{iter}: {e:#}");
            error!("{reason}");
            let interrupt_event = event::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::NodeInterrupted,
                node_id: Some(node_id.to_string()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "reason": reason,
                    "reason_code": "delivery_failed",
                })),
            };
            let _ = append_event(state, &interrupt_event).await;
            reap_dead_node_after_run_failure(
                state,
                repo_root,
                run_id,
                node_id,
                iter,
                !run_state.sandbox.is_off(),
            );
            return Some(completion_refusal::CompletionRefusal::DeliveryFailed {
                node_id: node_id.to_string(),
                error: format!("{e:#}"),
            });
        }
    };

    match outcome {
        // The Run's branch carries the node's work — or the node left nothing and
        // the branch did not move. Either way the completion continues; only the
        // first case is worth an event, and that event is what makes the NodeRun's
        // diff answerable whatever its type or isolation.
        worktrees::DeliveryOutcome::Delivered(delivered) => {
            if delivered.changed() {
                let event = event::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event::EventKind::NodeDelivered,
                    node_id: Some(node_id.to_string()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "before": delivered.before,
                        "after": delivered.after,
                        "isolated": isolated,
                    })),
                };
                let _ = append_event(state, &event).await;
            } else {
                info!("Node {node_id} iter-{iter} delivered nothing — no commit taken");
            }
            None
        }
        // #503: not a failure — but PDO rewrote a branch, so it is logged with both
        // tips, the resolution commit and the blast radius. The completion
        // continues exactly as a clean merge would.
        worktrees::DeliveryOutcome::ResolvedInNodeFavour(adoption) => {
            let resolved = event::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::MergeResolvedInNodeFavour,
                node_id: Some(node_id.to_string()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "reason": format!(
                        "merge-back of {node_id} conflicted on {} file(s) and was resolved in \
                         the node's favour",
                        adoption.conflicting_files.len(),
                    ),
                    "rule": worktrees::ADOPTION_RULE,
                    "pipeline_tip": adoption.pipeline_tip,
                    "node_tip": adoption.node_tip,
                    "merge_commit": adoption.merge_commit,
                    "conflicting_files": adoption.conflicting_files,
                })),
            };
            let _ = append_event(state, &resolved).await;
            warn!(
                "Run {run_id}: merge-back of {node_id} resolved in the node's favour \
                 ({} conflicting file(s)); pipeline branch now at {} (#503)",
                adoption.conflicting_files.len(),
                adoption.merge_commit,
            );
            // The adoption moved the Run's branch too, so it delivered — and the
            // node's diff must show what landed.
            let delivered = event::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::NodeDelivered,
                node_id: Some(node_id.to_string()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "before": adoption.pipeline_tip,
                    "after": adoption.merge_commit,
                    "isolated": isolated,
                })),
            };
            let _ = append_event(state, &delivered).await;
            None
        }
        worktrees::DeliveryOutcome::Conflict(conflict) => {
            let reason = format!(
                "merge conflict on {node_id}: {} conflicting file(s) merging {} into the \
                 pipeline branch at {}",
                conflict.conflicting_files.len(),
                short(&conflict.node_tip),
                short(&conflict.pipeline_tip),
            );
            let conflict_event = event::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::MergeConflictDetected,
                node_id: Some(node_id.to_string()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "reason": format!("conflict merging {node_id} into pipeline branch"),
                    // #503 AC3/AC4: git's own report (stdout included — the half
                    // that pre-#503 was dropped, and the only half a conflict
                    // writes), the two tips, and the file list.
                    "detail": conflict.detail,
                    "pipeline_tip": conflict.pipeline_tip,
                    "node_tip": conflict.node_tip,
                    "conflicting_files": conflict.conflicting_files,
                })),
            };
            let _ = append_event(state, &conflict_event).await;

            // #503 AC5 / ADR-0049: the delivery just failed on a merge conflict — a
            // runtime give-up, NOT a business failure. Mark the node `Interrupted`
            // (not `Failed`): the run parks `AwaitingUser` with the reason (derived
            // in `finalize`), never `RunFailed`. This still closes the window where
            // the node stayed projected `running` for ever with a live session and
            // a UI offering Stop/Retry; the worktree with the conflicting work is
            // preserved for the human to resolve and reopen.
            let node_interrupted = event::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::NodeInterrupted,
                node_id: Some(node_id.to_string()),
                iter: Some(iter),
                // #601: prefix the refusal slug so `finalize` derives
                // `awaiting_reason_code = merge_conflict` from the node.
                payload: Some(serde_json::json!({
                    "reason": format!("merge_conflict: {reason}"),
                    "reason_code": "merge_conflict",
                })),
            };
            let _ = append_event(state, &node_interrupted).await;

            warn!("Merge conflict for node {node_id} in run {run_id}: {reason}");
            reap_dead_node_after_run_failure(
                state,
                repo_root,
                run_id,
                node_id,
                iter,
                !run_state.sandbox.is_off(),
            );
            Some(completion_refusal::CompletionRefusal::MergeConflict {
                node_id: node_id.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    /// ADR-0047: `secondary_repos_dirtied_refusal` now skips writable secondaries
    /// (`read_only == false`, the default) and only trips on a read-only opt-in
    /// with a dirty tracked tree. This is the switch that turns "writing refused"
    /// into "writing offered" — the A/B control of the whole feature.
    #[test]
    fn secondary_dirtied_guard_only_trips_on_read_only_pins() {
        use pdo_workspace::adapters::git::worktrees::{
            create_secondary_snapshot, secondary_snapshot_path,
        };

        fn git(dir: &std::path::Path, args: &[&str]) {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        }
        fn init_repo_with_tracked_file(dir: &std::path::Path) -> String {
            std::fs::create_dir_all(dir).unwrap();
            git(dir, &["init", "-b", "main"]);
            git(dir, &["config", "user.email", "t@t"]);
            git(dir, &["config", "user.name", "t"]);
            std::fs::write(dir.join("F.txt"), "v1\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "-m", "init"]);
            let out = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("primary"); // acts as repo_root
        std::fs::create_dir_all(&primary).unwrap();
        let run_id = "guard-run";

        // Two secondaries: snapshot each, then dirty a TRACKED file in both.
        let ro = tmp.path().join("ro");
        let rw = tmp.path().join("rw");
        let ro_sha = init_repo_with_tracked_file(&ro);
        let rw_sha = init_repo_with_tracked_file(&rw);
        let ro_snap = secondary_snapshot_path(&primary, run_id, "ro");
        let rw_snap = secondary_snapshot_path(&primary, run_id, "rw");
        create_secondary_snapshot(&ro, &ro_snap, &ro_sha).unwrap();
        create_secondary_snapshot(&rw, &rw_snap, &rw_sha).unwrap();
        std::fs::write(ro_snap.join("F.txt"), "tampered\n").unwrap();
        std::fs::write(rw_snap.join("F.txt"), "tampered\n").unwrap();

        let pin =
            |repo: &std::path::Path, alias: &str, sha: &str, read_only: bool| event_log::RepoPin {
                repo: repo.to_string_lossy().to_string(),
                alias: alias.to_string(),
                sha: sha.to_string(),
                base_branch: None,
                read_only,
            };
        let mut state = event_log::RunState::new(run_id.to_string(), "p".to_string());

        // A writable secondary, dirtied, is tolerated → no refusal.
        state.target_repos = vec![pin(&rw, "rw", &rw_sha, false)];
        assert!(
            secondary_repos_dirtied_refusal(&primary, run_id, &state).is_none(),
            "a writable secondary with a dirty tracked tree must NOT be refused"
        );

        // A read-only secondary, dirtied, still trips the guard.
        state.target_repos = vec![pin(&ro, "ro", &ro_sha, true)];
        match secondary_repos_dirtied_refusal(&primary, run_id, &state) {
            Some(completion_refusal::CompletionRefusal::SecondaryRepoDirtied { alias, .. }) => {
                assert_eq!(alias, "ro");
            }
            other => panic!("expected SecondaryRepoDirtied for a read-only pin, got {other:?}"),
        }

        // Mixed list: the writable one is skipped, the read-only one is caught.
        state.target_repos = vec![
            pin(&rw, "rw", &rw_sha, false),
            pin(&ro, "ro", &ro_sha, true),
        ];
        match secondary_repos_dirtied_refusal(&primary, run_id, &state) {
            Some(completion_refusal::CompletionRefusal::SecondaryRepoDirtied { alias, .. }) => {
                assert_eq!(
                    alias, "ro",
                    "must skip the writable pin and catch the read-only one"
                );
            }
            other => panic!("expected the read-only pin to be refused, got {other:?}"),
        }
    }
}
