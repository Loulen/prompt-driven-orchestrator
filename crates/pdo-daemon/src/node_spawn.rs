//! Node spawn primitive: the single, injectable sequence that turns a ready
//! NodeRun into a live tmux session under the global admission cap.
//!
//! Carved out of the lib.rs god-file (#356) next to its callers, mirroring
//! worktree_ops (#276) and run_advance. `spawn_node` takes a narrow
//! `SpawnDeps` (db, event sink, admission lock, panic flag, port, tmux
//! override) instead of the full `AppState`, so its ordering invariants —
//! transition guard (#212), atomic cap check-and-reserve (#213), panic
//! isolation + orphan reaping, and "fail loud as RunFailed" (#279) — are
//! unit-testable without a live daemon. It is a leaf primitive (ADR-0009,
//! Couche 2): it appends events and touches tmux/worktree, and never
//! re-enters the scheduler.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use tracing::{error, info, warn};

use crate::worktree_ops::{
    create_sub_worktree, reap_orphan_sub_worktree, sub_worktree_branch, sub_worktree_path,
};
use crate::{
    admission, append_event_with, count_global_live_sessions, event_log, input_resolution,
    panic_payload_message, pipeline, prompt_augmenter, reload_run_state_with, stored_default_model,
    stored_session_cap, tmux_session_manager, transition_guard, AppState,
};

pub(crate) struct SpawnContext<'a> {
    pub(crate) pipeline: &'a pipeline::PipelineDef,
    pub(crate) run_id: &'a str,
    pub(crate) pipeline_path: &'a std::path::Path,
    pub(crate) worktree_dir: &'a std::path::Path,
    pub(crate) artifacts_dir: &'a std::path::Path,
    pub(crate) resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    pub(crate) repo_root: &'a std::path::Path,
}

/// Narrow, hand-buildable bundle of the exactly-six side-effects `spawn_node`
/// touches. A struct-of-borrows (NOT a trait): `admission_lock` is a bare
/// `tokio::sync::Mutex`, so the guard it hands out borrows `AppState`'s
/// lifetime — a trait object would fight that borrow. Every field is a `Copy`
/// reference / `u16` / `Option<&str>`, so the whole thing is `Copy` and can be
/// threaded on to `fail_spawn_before_start` without reborrow gymnastics.
///
/// `from_state` is the only production constructor; a unit test builds the same
/// struct out of a `test_state_with_dir` `AppState`, which is the whole point of
/// the seam (#356) — spawn is drivable in-process with fakes.
#[derive(Clone, Copy)]
pub(crate) struct SpawnDeps<'a> {
    pub(crate) db: &'a sqlx::SqlitePool,
    pub(crate) event_tx: &'a tokio::sync::broadcast::Sender<event_log::Event>,
    pub(crate) admission_lock: &'a tokio::sync::Mutex<()>,
    pub(crate) panic_on_spawn: &'a std::sync::atomic::AtomicBool,
    pub(crate) port: u16,
    pub(crate) tmux_cmd_override: Option<&'a str>,
}

impl<'a> SpawnDeps<'a> {
    /// Project the six spawn side-effects out of the full daemon state.
    pub(crate) fn from_state(state: &'a AppState) -> Self {
        Self {
            db: &state.db,
            event_tx: &state.event_tx,
            admission_lock: &state.admission_lock,
            panic_on_spawn: &state.panic_on_spawn,
            port: state.port,
            tmux_cmd_override: state.tmux_cmd_override.as_deref(),
        }
    }
}

/// A collection-region member (ADR-0011 / #269) reads its OWN deposited item:
/// the fan-out deposits `_item.md` under the entry's artifact dir, one per
/// lap — there is no separate driver node like the retired ForEach.
fn find_collection_context(
    spawn_ctx: &SpawnContext<'_>,
    node_id: &str,
    iter: i64,
) -> Option<prompt_augmenter::ForEachContext> {
    crate::loop_region::collection_region_for_member(spawn_ctx.pipeline, node_id)?;
    let item_path = spawn_ctx
        .artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join("_item.md");
    let item_content = std::fs::read_to_string(&item_path).ok()?;
    let total = std::fs::read_dir(spawn_ctx.artifacts_dir.join(node_id))
        .map(|entries| {
            entries
                .filter(|e| e.as_ref().is_ok_and(|e| e.path().is_dir()))
                .count()
        })
        .unwrap_or(0) as i64;
    let current_item = item_content
        .split("---")
        .nth(2)
        .unwrap_or("")
        .trim()
        .to_string();
    Some(prompt_augmenter::ForEachContext {
        current_item,
        current_iter: iter,
        total,
    })
}

/// What actually happened in a `spawn_node` call (ADR-0025 / #327). Every exit
/// path is distinguishable so callers that must tell the truth about a
/// re-scheduling (`re_evaluate_after_command`) can report the real effect
/// instead of assuming success. Callers on fire-and-forget paths simply drop it
/// (intentionally not `#[must_use]`).
#[derive(Debug, Clone)]
pub(crate) enum SpawnOutcome {
    /// A tmux session was launched and `NodeStarted` recorded.
    Spawned,
    /// Admission cap reached: the node entered `waiting` (`NodeWaiting`
    /// appended); `retry_waiting_nodes` re-drives it later.
    Throttled,
    /// The transition guard refused the spawn before any side effect
    /// (already live / already completed iteration).
    Refused { reason: String },
    /// The spawn aborted (empty script body, worktree creation failure,
    /// panic/error in the isolated span) — a failure was recorded.
    Failed { reason: String },
}

pub(crate) async fn spawn_node(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) -> SpawnOutcome {
    let run_id = spawn_ctx.run_id;

    // Transition guard (#212): refuse an illegal NodeStarted BEFORE any side
    // effect (sub-worktree creation, tmux session spawn) — never after. This
    // covers every caller: scheduler dispatch, resume re-evaluation,
    // restart_node, waiting-node retries.
    let started_probe = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some(node.id.clone()),
        iter: Some(iter),
        payload: None,
    };
    let projected = reload_run_state_with(deps.db, run_id).await.map(|(_, s)| s);
    match transition_guard::validate_transition(projected.as_ref(), &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason }
        | transition_guard::Verdict::Reject { reason } => {
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return SpawnOutcome::Refused { reason };
        }
    }

    // #248 / ADR-0017: refuse to spawn a `script` node with an empty body — it
    // would `bash <empty>` → exit 0 → a silent no-op masquerading as success.
    // `create_run` guards this at launch, but the scheduler and `restart_node`
    // reach `spawn_node` directly, and a mid-run edit could have emptied a
    // pending script's body since launch. Fail loud (before admission / any side
    // effect) rather than silently no-op.
    if node.node_type == pipeline::NodeType::Script {
        let body_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
        let body_empty = std::fs::read_to_string(&body_path)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if body_empty {
            let reason = format!("script node {} has an empty body", node.id);
            fail_spawn_before_start(deps, spawn_ctx.repo_root, run_id, &node.id, None, &reason)
                .await;
            return SpawnOutcome::Failed { reason };
        }
    }

    // Admission control (#159 / #213): bound the number of live NodeRun
    // sessions daemon-wide. The check is an ATOMIC check-and-reserve — the
    // `admission_lock` is held from the count until the reservation event
    // (`NodeStarted` / `NodeWaiting`) is appended, so concurrent spawns can
    // never all observe the same free slot and overshoot the cap. If admitting
    // one more would exceed the cap, the node enters `waiting` and holds no
    // session; `retry_waiting_nodes` re-drives it once a slot frees. Checked
    // first so a throttled node creates no worktree.
    let admission_guard = deps.admission_lock.lock().await;
    let cap = admission::configured_cap_with(stored_session_cap(deps.db).await);
    let live = count_global_live_sessions(deps.db).await;
    if !admission::can_admit(live, cap) {
        let waiting = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeWaiting,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "live_sessions": live, "cap": cap })),
        };
        if let Err(e) = append_event_with(deps.db, deps.event_tx, &waiting).await {
            error!("failed to append node_waiting for {}: {e}", node.id);
        }
        info!(
            "node {} throttled into waiting ({live}/{cap} sessions live)",
            node.id
        );
        return SpawnOutcome::Throttled;
    }

    let canonical_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let foreach_context = find_collection_context(spawn_ctx, &node.id, iter);

    let has_sub_worktree = node.node_type == pipeline::NodeType::CodeMutating
        || node.node_type == pipeline::NodeType::Merge;

    // Track the sub-worktree + branch this spawn creates so an abort in the
    // panic-isolated span below can reap them (#279). `None` for nodes that own
    // no worktree (doc-only / control nodes).
    let mut orphan_to_reap: Option<(PathBuf, String)> = None;
    let working_dir = if has_sub_worktree {
        let sub_wt_dir = sub_worktree_path(spawn_ctx.repo_root, run_id, &node.id, iter);
        let sub_branch = sub_worktree_branch(run_id, &node.id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");

        if let Err(e) = create_sub_worktree(
            spawn_ctx.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
        ) {
            error!("failed to create sub-worktree for {}: {e:#}", node.id);
            return SpawnOutcome::Failed {
                reason: format!("failed to create sub-worktree for {}: {e:#}", node.id),
            };
        }
        orphan_to_reap = Some((sub_wt_dir.clone(), sub_branch));
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

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
        let input_images = if is_entry_node {
            prompt_augmenter::discover_input_images(spawn_ctx.artifacts_dir)
        } else {
            Vec::new()
        };

        // Canonical input resolution (#194 / #210): re-project the run state at
        // spawn time so each input path follows its source's latest COMPLETED
        // iteration — a failed iteration's artifacts are never consumed, and an
        // external feeder keeps serving its completed iter at any lap.
        // #353: alongside the single-input source iters, resolve the `repeated`
        // pools from the SAME fresh projection — one artifact per COMPLETED
        // source iteration, so a failed iter's artifact is never pooled and no
        // raw `iter-*` glob reaches the agent/script.
        let (source_iters, repeated_iters) = match reload_run_state_with(deps.db, run_id).await {
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
            ),
            None => (HashMap::new(), HashMap::new()),
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

        let aug_ctx = prompt_augmenter::AugmentContext {
            pipeline: spawn_ctx.pipeline,
            node,
            run_id,
            iter,
            artifacts_dir: spawn_ctx.artifacts_dir,
            variables: spawn_ctx.resolved_vars,
            daemon_url: &format!("http://localhost:{}", deps.port),
            foreach_context,
            source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
            input_images,
            start_prompt_present,
            source_iters,
            repeated_iters,
        };

        let full_prompt = prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

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

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({
                "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
                "node_type": match node.node_type {
                    pipeline::NodeType::DocOnly => "doc-only",
                    pipeline::NodeType::CodeMutating => "code-mutating",
                    pipeline::NodeType::Start => "start",
                    pipeline::NodeType::End => "end",
                    pipeline::NodeType::Switch => "switch",
                    pipeline::NodeType::Loop => "loop",
                    pipeline::NodeType::Merge => "merge",
                    pipeline::NodeType::Script => "script",
                },
            })),
        };
        // A failed `NodeStarted` append means the reservation was NOT recorded:
        // treat it as a spawn abort (reap + RunFailed) rather than launching a
        // tmux session the run's event log has no record of.
        append_event_with(deps.db, deps.event_tx, &node_started)
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
            fail_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return SpawnOutcome::Failed { reason };
        }
        Err(panic) => {
            let reason = format!(
                "spawn of node {} panicked before start: {}",
                node.id,
                panic_payload_message(panic.as_ref())
            );
            fail_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return SpawnOutcome::Failed { reason };
        }
    };

    let session_name = tmux_session_manager::node_session_name(run_id, &node.id, iter);
    let is_script = node.node_type == pipeline::NodeType::Script;
    // #347: resolve the instance default fresh (stored → env → None), then let
    // the node's own `model:` override win over it. `__manager__` /
    // `__merge_resolver__` are infra sessions with no NodeDef and stay at the
    // account default — they don't route through `spawn_node` (#296).
    let default_effective = stored_default_model(deps.db).await;
    let tail = if is_script {
        tmux_session_manager::SessionTail::Script {
            timeout_secs: tmux_session_manager::SCRIPT_TIMEOUT_SECS,
            env: &script_env,
        }
    } else {
        tmux_session_manager::SessionTail::Agent {
            model: tmux_session_manager::resolve_node_model(
                node.model.as_deref(),
                default_effective.as_deref(),
            ),
        }
    };
    // A script node executes the RAW bash body (`role_prompt`), never the
    // augmented prompt — the preamble is prose an agent reads, not runnable bash.
    let spawn_prompt: &str = if is_script {
        &role_prompt
    } else {
        &full_prompt
    };
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        spawn_prompt,
        &working_dir,
        run_id,
        &node.id,
        iter,
        deps.port,
        deps.tmux_cmd_override,
        tail,
    ) {
        error!("failed to spawn tmux session: {e}");
    }

    if node.interactive {
        let awaiting = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: None,
        };
        if let Err(e) = append_event_with(deps.db, deps.event_tx, &awaiting).await {
            error!("failed to append node_awaiting_user: {e}");
        }
    }

    SpawnOutcome::Spawned
}

/// Fail a run loud when a node spawn aborts *before* `NodeStarted` is appended
/// (#279, Layer 1). Reaps any orphaned sub-worktree + branch the spawn created,
/// then appends a visible cause.
///
/// The cause is `RunFailed`, **not** `NodeFailed`: the node has no
/// `NodeStarted`, so `transition_guard::validate_fail` treats a `NodeFailed`
/// for it as a guard no-op (a failure for an iteration "that was never started")
/// — the run would stay `Running` and the fix would be defeated. `RunFailed` is
/// un-guarded and reliably moves the run terminal.
async fn fail_spawn_before_start(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    orphan: Option<&(PathBuf, String)>,
    reason: &str,
) {
    error!("Run {run_id}: node {node_id} spawn aborted before NodeStarted — {reason}");
    if let Some((sub_worktree_dir, sub_branch)) = orphan {
        reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
    }
    let run_failed = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunFailed,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "reason": reason })),
    };
    if let Err(e) = append_event_with(deps.db, deps.event_tx, &run_failed).await {
        error!("Run {run_id}: failed to append RunFailed after spawn abort: {e}");
    }
}
