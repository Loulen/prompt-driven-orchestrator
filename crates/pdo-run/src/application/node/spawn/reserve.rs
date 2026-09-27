//! The reservation of a spawn: the panic- and cancellation-isolated window
//! that builds the node's prompt and appends its `NodeStarted` (#279), then
//! releases the admission lock.

use std::collections::HashMap;

use anyhow::Context;
use tracing::warn;

use super::interrupts::*;
use super::*;
use crate::application::events::append_event_with;
use crate::application::events::reload_run_state_with;
use crate::application::node::prompt as prompt_augmenter;
use crate::application::supervision::panic_payload_message;
use crate::domain::event;
use crate::domain::input_resolution;
use pdo_pipeline::domain::pipeline;

use super::Plan;

/// The node's prompt and its script env once `NodeStarted` is durable; the
/// spawn interrupted otherwise.
pub(super) async fn reserve(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
    plan: &Plan,
    admission_guard: tokio::sync::MutexGuard<'_, ()>,
) -> Result<(String, Vec<(String, String)>), SpawnOutcome> {
    let run_id = spawn_ctx.run_id;
    let Plan {
        role_prompt,
        foreach_context,
        run_sandboxed,
        wt,
        resolved_harness,
        resolved_skills,
        skipped_skills,
        resolved_model,
        resolved_effort,
        session_id,
    } = plan;
    let run_sandboxed = *run_sandboxed;
    let has_sub_worktree = wt.has_sub_worktree;
    let working_dir = &wt.working_dir;
    let reused_sub_worktree = wt.reused_sub_worktree;
    let interrupted_git_ops = &wt.interrupted_git_ops;
    let node_provisioning = &wt.node_provisioning;
    let node_provisioning_plan = &wt.node_provisioning_plan;
    let spawn_base_sha = &wt.spawn_base_sha;
    let orphan_to_reap = &wt.orphan_to_reap;
    // Panic/cancellation-isolated spawn window (#279). Everything from here to
    // the `NodeStarted` append can panic (`build_full_prompt`, image discovery,
    // input resolution) or — when this runs in-request inside `node_done` — be
    // dropped if the completing client disconnects (hyper drops the in-flight
    // future at an `.await`). Before #279 either left the freshly-created
    // sub-worktree orphaned with NO `NodeStarted`, wedging the run `running`
    // forever: no live node, no error, nothing logged. It slips past every
    // recovery path — `advance_run` is event-triggered, the stale detector only
    // inspects live tmux sessions, and `reconcile_run_level_stall` saw the node
    // as "ready, about to be driven". Run the window under `catch_unwind` so a
    // panic becomes a LOUD failure (reap the orphan, fail the run) instead of a
    // silent stall (ADR-0004 « jamais de stall silencieux »). A dropped
    // (cancelled) future can't be caught here; the periodic detector in
    // `run_stall_reason` (#279 Layer 2) is the backstop for that path — and
    // since #304 (ADR-0023) the `node_done` tail runs DETACHED from the request
    // future, so the completing client's disconnect can no longer cancel this
    // window in the first place.
    // `tokio::sync::Mutex` doesn't poison, so the DB / admission state stay
    // usable after a caught panic (the property `run_isolated` relies on too).
    let span = std::panic::AssertUnwindSafe(async {
        // Debug-only one-shot fault injection (#279): exercises the catch + reap
        // + RunFailed path. Armed via `PDO_DEBUG_PANIC_SPAWN` or
        // `DaemonHandle::arm_spawn_panic`. Checked at the span head so the
        // orphaned worktree already exists and the reap has something to remove.
        if deps
            .panic_on_spawn
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            panic!("PDO_DEBUG_PANIC_SPAWN fault injection (#279)");
        }

        let is_entry_node = spawn_ctx.pipeline.edges.iter().any(|e| {
            e.target.node == node.id
                && spawn_ctx
                    .pipeline
                    .nodes
                    .iter()
                    .any(|n| n.id == e.source.node && n.node_type == pipeline::NodeType::Start)
        });
        let (input_images, input_files) = if is_entry_node {
            (
                prompt_augmenter::discover_input_images(spawn_ctx.artifacts_dir),
                prompt_augmenter::discover_input_files(spawn_ctx.artifacts_dir),
            )
        } else {
            (Vec::new(), Vec::new())
        };

        // Canonical input resolution (#194 / #210): re-project the run state at
        // spawn time so each input path follows its source's latest COMPLETED
        // iteration — a failed iteration's artifacts are never consumed, and an
        // external feeder keeps serving its completed iter at any lap.
        // #353: alongside the single-input source iters, resolve the `repeated`
        // pools from the SAME fresh projection — one artifact per COMPLETED
        // source iteration, so a failed iter's artifact is never pooled and no
        // raw `iter-*` glob reaches the agent/script.
        // #465: the secondary repos come from the SAME fresh projection, resolved to
        // absolute snapshot paths for injection (the sub-worktree does not inherit
        // the snapshot files).
        let (source_iters, repeated_iters, secondary_repos, run_started_at) =
            match reload_run_state_with(deps.runs, run_id).await {
                Some((_, fresh_state)) => (
                    input_resolution::resolved_source_iters(
                        spawn_ctx.pipeline,
                        &fresh_state,
                        &node.id,
                        iter,
                    ),
                    input_resolution::resolved_repeated_iters(
                        spawn_ctx.pipeline,
                        &fresh_state,
                        &node.id,
                    ),
                    prompt_augmenter::secondary_repo_contexts(
                        spawn_ctx.repo_root,
                        run_id,
                        &fresh_state.target_repos,
                    ),
                    // #796: the run's own start, to tell an interrupted attempt's
                    // output from one inherited through git.
                    fresh_state.started_at.as_deref().and_then(|ts| {
                        chrono::DateTime::parse_from_rfc3339(ts)
                            .ok()
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                    }),
                ),
                None => (HashMap::new(), HashMap::new(), Vec::new(), None),
            };

        // Precompute whether the Start prompt carries content so `build_preamble`
        // stays pure (#274). Gate on `!prompt_required` (the only branch that
        // consults it), NOT on the edge-based `is_entry_node` — that would regress
        // the `task`-port fallback (a node with no incoming edge still reads from
        // `_input`). On a genuine I/O error, fail toward "prompt present" and log:
        // a false negative would silently discard the run's actual brief.
        let start_prompt_present = if spawn_ctx.pipeline.prompt_required {
            false // value is never consulted for prompt-required pipelines — skip the read
        } else {
            match prompt_augmenter::read_start_prompt_present(spawn_ctx.artifacts_dir) {
                Ok(present) => present,
                Err(e) => {
                    warn!(
                        "entry-node input read failed (run {run_id} node {} iter {iter}): {e}; \
                         assuming a prompt is present",
                        node.id
                    );
                    true // fail toward "prompt present" — never tell the agent "no prompt" on an I/O error
                }
            }
        };

        // #599 AC1 (ADR-0049): the partial output an interrupted attempt at THIS
        // node left on disk for this iteration. On a same-iter re-spawn
        // (restart-with-artifacts) it is never wiped, so it is surfaced to the
        // fresh agent as input to build on. Empty on a first spawn — and (#796) a
        // file older than the run itself is not this run's attempt.
        let partial_outputs = prompt_augmenter::surviving_partial_outputs(
            node,
            spawn_ctx.artifacts_dir,
            iter,
            run_started_at,
        );

        let aug_ctx = prompt_augmenter::AugmentContext {
            pipeline: spawn_ctx.pipeline,
            node,
            run_id,
            iter,
            artifacts_dir: spawn_ctx.artifacts_dir,
            variables: spawn_ctx.resolved_vars,
            // #447: same single resolver as the manager preamble and
            // `PDO_DAEMON_URL` — `run_sandboxed` is already projected above for the
            // spawn precondition. Defensive here: no node preamble consumes
            // `daemon_url` yet (see the note in `node_primitives::start_node`).
            daemon_url: &pdo_sandbox::domain::container::daemon_url(deps.port, run_sandboxed),
            foreach_context: foreach_context.clone(),
            source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
            // #654: the same section, from the other isolation. This site only
            // ever builds a preamble for a node that spawns a session, so the
            // two are exhaustive and mutually exclusive.
            shared_worktree_dir: (!has_sub_worktree).then_some(working_dir.as_path()),
            input_images,
            input_files,
            start_prompt_present,
            source_iters,
            repeated_iters,
            secondary_repos,
            // #516: both computed above (outside this span), borrowed read-only
            // here so `build_preamble` can route the interrupted-git-op notice. The
            // `Vec` is moved into `SpawnOutcome::Spawned` only after this span is
            // awaited, so the borrow is already released — see G7 in the plan.
            reused_sub_worktree,
            interrupted_git_ops,
            partial_outputs: &partial_outputs,
        };

        let full_prompt = prompt_augmenter::build_full_prompt(&aug_ctx, role_prompt);

        // A `script` node (#248 / ADR-0017) runs the author's bash instead of
        // Claude. Compute its I/O env catalogue and pre-create its output dirs
        // here (inside the panic-isolated span, next to `aug_ctx`) and hand the
        // env back to the spawn below — a script can't read the prose preamble.
        let script_env = if node.node_type == pipeline::NodeType::Script {
            prompt_augmenter::precreate_output_dirs(&aug_ctx);
            prompt_augmenter::build_script_env(&aug_ctx)
        } else {
            Vec::new()
        };

        let node_started = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeStarted,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({
                "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
                "node_type": node.node_type.as_str(),
                "interactive": node.interactive,
                // #810/ADR-0007: FREEZE the « Orchestrator » toggle beside
                // `interactive`. The two together are the node's **genre**
                // (kind) — the axis Stats › Performance filters on. Frozen for
                // the same reason as the isolation below: reading the document
                // at stat time would reclassify history after an edit. A
                // pre-#810 `NodeStarted` has no key; its reader falls back to
                // the Run's pipeline snapshot, which has carried `orchestrator`
                // since #723.
                "orchestrator": node.orchestrator,
                // #653/ADR-0060: FREEZE where this NodeRun works. Every later
                // reader — the re-spawn above, the restart probe, the completion
                // path's merge-back decision — asks this event, never the
                // document, so an isolation edit lands on the next launch and
                // never under a live node's feet.
                "isolated_worktree": has_sub_worktree,
                "provisioning": node_provisioning,
                "provisioning_plan": node_provisioning_plan,
                // #424: the launch-time model and effort, **resolved** (post
                // node → instance precedence, post empty-string collapse) — not
                // the raw `NodeDef` values. This is what the resume path reads
                // back to re-pose `--effort`, so it has to be what the flag
                // actually carried. `None` serializes as JSON `null`; a script
                // node records both as `null` since it launches no agent.
                // Recording `model` alongside is deliberate even though nothing
                // reads it yet: the meaning of an effort level depends on the
                // model (supported levels and the default both vary per model),
                // so storing the effort alone would store half a fact.
                "model": resolved_model.as_deref(),
                "effort": resolved_effort.as_deref(),
                // #550/ADR-0046: the harness resolved at spawn, FROZEN here so the
                // resume path re-poses what was launched, never what the YAML or a
                // tier says now (ADR-0007). `null` for a `script` node (no agent).
                "harness": resolved_harness.as_ref().map(|r| r.harness.as_str()),
                // #669/ADR-0062: the skills actifs this session receives, FROZEN
                // with their origin tier (ids + current bank names), and the ids the
                // bank no longer had — the node runs without those, and the Run view
                // says so. `null` / empty for a `script` node (no agent).
                "skills": resolved_skills.as_ref().map(|r| &r.skills),
                "missing_skills": resolved_skills
                    .as_ref()
                    .map(|r| r.missing.clone())
                    .unwrap_or_default(),
                // #672: what the delivery could not write into the worktree (a
                // versioned homonym, an occupied path, content gone) — the node
                // runs without those and the node view says so.
                "skipped_skills": skipped_skills,
                // #473: the pinned Claude Code session id the agent launches with
                // (`claude --session-id <uuid>`). The sweep resolves this node's
                // transcript by it (`<uuid>.jsonl`), and the resume path re-enters
                // it (`--resume <uuid>`). `null` for a `script` node (no claude) and
                // for every pre-#473 row (legacy newest-mtime resolution / bare
                // `--continue`), so no migration is needed.
                "session_id": session_id.as_deref(),
                // #503 / ADR-0036: the sub-worktree's base commit — what the
                // merge-back compares the pipeline tip against to decide whether a
                // conflict is the run's own history rewritten by this node. `null`
                // for a node with no sub-worktree, which never merges back.
                "base_sha": spawn_base_sha,
            })),
        };
        // A failed `NodeStarted` append means the reservation was NOT recorded:
        // treat it as a spawn abort (reap + RunFailed) rather than launching a
        // tmux session the run's event log has no record of.
        //
        // Since #485 (ADR-0038) this ordering is also another subsystem's
        // correctness precondition, not just local hygiene: the orphan sweep's
        // "absent from the log ⇒ orphan ⇒ kill" verdict is only sound because no
        // session can exist before its reservation is durably appended. This was
        // the one spawn path that already got it right, and it is why the sweep
        // never killed a scheduler-spawned node once its snapshot was correctly
        // ordered. Do not reorder this for readability.
        append_event_with(deps.runs, &node_started)
            .await
            .context("failed to append node_started")?;
        Ok::<(String, Vec<(String, String)>), anyhow::Error>((full_prompt, script_env))
    });

    let span_outcome = futures_util::future::FutureExt::catch_unwind(span).await;

    // The reservation (`NodeStarted`) is recorded iff the span returned
    // `Ok(Ok(_))`; either way the admission lock can be released now — on failure
    // nothing was reserved, on success the projected state already counts the
    // session.
    drop(admission_guard);

    let (full_prompt, script_env) = match span_outcome {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            let reason = format!("spawn of node {} aborted before start: {e}", node.id);
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return Err(SpawnOutcome::Failed { reason });
        }
        Err(panic) => {
            let reason = format!(
                "spawn of node {} panicked before start: {}",
                node.id,
                panic_payload_message(panic.as_ref())
            );
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return Err(SpawnOutcome::Failed { reason });
        }
    };
    Ok((full_prompt, script_env))
}
