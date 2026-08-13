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
    ensure_sub_worktree, reap_orphan_sub_worktree, sub_worktree_branch, sub_worktree_path,
};
use crate::{
    admission, append_event_with, count_global_live_sessions_excluding, event_log,
    input_resolution, merge_action, panic_payload_message, pipeline, prompt_augmenter,
    reload_run_state_with, stored_autocomplete_turn_end, stored_default_model, stored_session_cap,
    tmux_session_manager, transition_guard, AppState,
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
    /// Per-daemon `docker` binary override for the sandbox wiring (#407), `Copy`
    /// like the rest of the bundle. `None` in production (real `docker`).
    pub(crate) docker_cmd_override: Option<&'a str>,
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
            docker_cmd_override: state.docker_cmd_override.as_deref(),
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
    ///
    /// Carries what the caller has to be able to say out loud (#489-A): a
    /// `restart_node` that reused an existing sub-worktree handed the fresh agent
    /// the dead session's uncommitted work, and that changes what the operator (or
    /// the manager) should tell it to do.
    Spawned {
        /// The sub-worktree already existed on the right branch and was reused **in
        /// place** — nothing was re-cut, nothing was destroyed. Always `false` for
        /// a node that owns no sub-worktree.
        reused_sub_worktree: bool,
        /// The commit the sub-worktree is cut from (#503 / ADR-0036), carried over
        /// unchanged on a reuse. `None` for a node with no sub-worktree.
        base_sha: Option<String>,
        /// Every interrupted git operation found in the reused worktree's private
        /// gitdir (`index.lock`, `MERGE_HEAD`, `rebase-*`), in scan order (#516).
        /// Reported, not removed — see `worktree_ops::ensure_sub_worktree`. Empty
        /// for a fresh cut or a node with no sub-worktree.
        interrupted_git_ops: Vec<String>,
    },
    /// Admission cap reached: the node entered `waiting` (`NodeWaiting`
    /// appended); `retry_waiting_nodes` re-drives it later.
    Throttled,
    /// The transition guard refused the spawn before any side effect
    /// (already live / already completed iteration).
    Refused { reason: String },
    /// The Run is sandboxed and its container is not up yet (#445): the spawn was
    /// declined before any side effect and **no event was appended**, so the node
    /// keeps no state and the scheduler still reports it ready. It is replayed when
    /// `SandboxPrepReady` drives the next `advance_run`.
    ///
    /// Distinct from [`SpawnOutcome::Refused`] (nothing to retry — the work is
    /// already live or done) and from [`SpawnOutcome::Throttled`] (a `NodeWaiting`
    /// reservation *was* appended and the cross-run admission sweep owns the retry).
    Deferred { reason: String },
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
    // Events kept alongside the projection: #503's `base_sha` lives on the previous
    // `NodeStarted` of this same iteration, and a reuse has to carry it forward
    // (#489-B / ADR-0037 §6).
    let loaded = reload_run_state_with(deps.db, run_id).await;
    let projected = loaded.as_ref().map(|(_, s)| s);
    match transition_guard::validate_transition(projected, &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason } => {
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return SpawnOutcome::Refused { reason };
        }
        transition_guard::Verdict::Reject { reason } => {
            // #515: the cause is typed now; forward its historical prose (the
            // `Refused` outcome carries a `String`, unchanged).
            let reason = reason.to_string();
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return SpawnOutcome::Refused { reason };
        }
    }

    // #407: whether the Run is sandboxed (immutable, projected from RunStarted). When
    // it is, the tail below is wrapped to run inside `pdo-sbx-<run_id>`. Read from the
    // guard projection — `sandbox` never changes over a Run's life. A `bool` and not
    // the mode (#432): the profile name is owned, and only the off-ness matters here.
    let run_sandboxed = projected.is_some_and(|s| !s.sandbox.is_off());

    // #445: SANDBOX PRECONDITION — "a sandboxed Run whose prep is not `ready` is not
    // schedulable". Carried by the spawn itself, deliberately NOT by its callers: the
    // create path gated correctly (`lib.rs`, the detached prep task) while the pipeline
    // watcher and `retry_waiting_nodes` reached the spawn with no idea a container was
    // still being built, so the tail below `docker exec`ed into a name that did not
    // exist yet → exit 1 in ~30 ms → the tmux window's command ended → `session_died`.
    // Enforced HERE (after the transition guard, before admission and before any
    // sub-worktree is created) so the invariant holds for every present and future
    // caller, exactly as the transition guard does for illegal starts.
    //
    // Appends NOTHING and reserves nothing. That is load-bearing for the replay: a
    // node with no state stays in `compute_ready_to_spawn`, so the `advance_run` that
    // follows `SandboxPrepReady` starts it. A `NodeWaiting` reservation would flip it
    // to `Waiting`, which `compute_ready_to_spawn` skips — the deferred spawn would
    // then depend on the *cross-run* admission sweep and a Run whose only trigger was
    // the watcher could wedge for ever. `run_stall_reason` knows about this window and
    // will not read it as a silent spawn-abort (it fails loud only past its own
    // sandbox-prep grace).
    if let Some(reason) = projected.and_then(|s| s.sandbox_spawn_block()) {
        info!(
            "spawn_node deferred for {} iter {iter}: {reason} (replayed on sandbox_prep_ready)",
            node.id
        );
        return SpawnOutcome::Deferred { reason };
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
    //
    // #489-C: the count EXCLUDES the slot this very spawn is taking back. A
    // `restart_node` kills the node's session and then re-spawns the SAME
    // iteration, but appends no lifecycle event in between — so the node still
    // projects `Running` and, at `live == cap`, the restart is throttled against
    // *itself*, deterministically. Nothing then rescues it: `retry_waiting_nodes`
    // has no timer, `resume_run` treats a throttled node as owned by the sweep,
    // boot recovery only looks at `Running`/`AwaitingUser`, and the Stop button
    // 409s because `node_stop` requires `Running`. The Run froze for good.
    //
    // The exclusion key is `(run_id, node_id, iter)` — never `(node_id, iter)`:
    // the count is global across Runs while node ids are local to a pipeline, so
    // two concurrent Runs of the same pipeline both have an `implementer` at
    // `iter 1`, and a Run-blind key would discount the *other* Run's live session
    // and overshoot the cap. It is unconditional because it can only ever subtract
    // when that exact triple currently holds a session, and `validate_start` allows
    // re-spawning a live iteration for exactly one reason: this spawn replaces it
    // (`transition_guard.rs`, "Same iter: legal restart/promotion").
    //
    // Computed INSIDE the lock, from the same all-Runs projection as the count —
    // reusing the pre-lock projection above would reopen the check-and-reserve race
    // the lock exists to close.
    let admission_guard = deps.admission_lock.lock().await;
    let cap = admission::configured_cap_with(stored_session_cap(deps.db).await);
    let live = count_global_live_sessions_excluding(
        deps.db,
        Some(admission::SlotExclusion {
            run_id,
            node_id: &node.id,
            iter,
        }),
    )
    .await;
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
    // #503 / ADR-0036: the commit the sub-worktree is cut from, recorded on
    // `NodeStarted`. It is the sole basis on which a later merge-back conflict may
    // be resolved in the node's favour — no base recorded, no resolution.
    let mut spawn_base_sha: Option<String> = None;
    // #489: what the wire has to be able to say about the sub-worktree.
    let mut reused_sub_worktree = false;
    // #516: every interrupted git op left in a reused sub-worktree, in scan order.
    // Routed to both the re-spawned node's preamble and the wire response.
    let mut interrupted_git_ops: Vec<String> = Vec::new();
    let working_dir = if has_sub_worktree {
        let sub_wt_dir = sub_worktree_path(spawn_ctx.repo_root, run_id, &node.id, iter);
        let sub_branch = sub_worktree_branch(run_id, &node.id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");

        // #503 / ADR-0036 under reuse (ADR-0037 §6): a reuse does not cut anything,
        // so it carries the ORIGINAL base forward rather than deriving a new one.
        let previous_base_sha = loaded
            .as_ref()
            .and_then(|(events, _)| merge_action::spawn_base_sha(events, &node.id, iter));

        // #489-B: `ensure_sub_worktree`, not `create_sub_worktree`. The bare create
        // replayed `git worktree add -b <branch>` on a branch that already existed
        // and failed with exit 255 on EVERY re-spawn of the same iteration — i.e. on
        // every `restart_node` of a `code-mutating` / `merge` node, 100% of the time.
        match ensure_sub_worktree(
            spawn_ctx.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
            previous_base_sha.as_deref(),
        ) {
            Ok(ensured) => {
                spawn_base_sha = ensured.base_sha;
                reused_sub_worktree = !ensured.created;
                interrupted_git_ops = ensured.entry_state.interrupted_git_ops().to_vec();
                // #489-B: `Some(...)` ONLY when this spawn created the worktree.
                // On a reuse, any later abort in the panic-isolated span would send
                // `fail_spawn_before_start` into `reap_orphan_sub_worktree`, and
                // `worktree remove --force` succeeds on a dirty tree — it would
                // destroy exactly the work the restart exists to save. Gated, the
                // abort path appends `RunFailed` and destroys nothing: the Run goes
                // terminal with the work intact, `resume_run` reopens it and the next
                // classification answers `Reusable`. #279's invariant ("an aborted
                // spawn leaves no orphan") is untouched — it only ever covered what
                // the spawn itself created.
                if ensured.created {
                    orphan_to_reap = Some((sub_wt_dir.clone(), sub_branch));
                }
            }
            Err(e) => {
                error!("failed to ensure sub-worktree for {}: {e:#}", node.id);
                return SpawnOutcome::Failed {
                    reason: format!("failed to ensure sub-worktree for {}: {e:#}", node.id),
                };
            }
        }
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

    // #347/#424: resolve what the session will actually launch with, BEFORE the
    // span below. The `NodeStarted` payload appended *inside* the span records the
    // **resolved** values (what the flags really carried, which is what the resume
    // path reads back), and `stored_default_model` is async — it cannot be awaited
    // from the spawn seam further down and still be visible to the append. This is
    // a move, not an extra read: the same single DB read, earlier.
    // `__manager__` / `__merge_resolver__` are infra sessions with no NodeDef and
    // stay at the account default — they don't route through `spawn_node` (#296).
    let default_effective = stored_default_model(deps.db).await;
    let resolved_model = tmux_session_manager::resolve_node_model(
        node.model.as_deref(),
        default_effective.as_deref(),
    );
    let resolved_effort = tmux_session_manager::resolve_node_effort(node.effort.as_deref());

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
        // #465: the secondary repos come from the SAME fresh projection, resolved to
        // absolute snapshot paths for injection (the sub-worktree does not inherit
        // the snapshot files).
        let (source_iters, repeated_iters, secondary_repos) =
            match reload_run_state_with(deps.db, run_id).await {
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
                ),
                None => (HashMap::new(), HashMap::new(), Vec::new()),
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
            // #447: same single resolver as the manager preamble and
            // `PDO_DAEMON_URL` — `run_sandboxed` is already projected above for the
            // spawn precondition. Defensive here: no node preamble consumes
            // `daemon_url` yet (see the note in `node_primitives::start_node`).
            daemon_url: &crate::sandbox_container::daemon_url(deps.port, run_sandboxed),
            foreach_context,
            source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
            input_images,
            start_prompt_present,
            source_iters,
            repeated_iters,
            secondary_repos,
            // #516: both computed above (outside this span), borrowed read-only
            // here so `build_preamble` can route the interrupted-git-op notice. The
            // `Vec` is moved into `SpawnOutcome::Spawned` only after this span is
            // awaited, so the borrow is already released — see G7 in the plan.
            reused_sub_worktree,
            interrupted_git_ops: &interrupted_git_ops,
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
                "model": resolved_model,
                "effort": resolved_effort,
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
    let tail = if is_script {
        tmux_session_manager::SessionTail::Script {
            timeout_secs: tmux_session_manager::SCRIPT_TIMEOUT_SECS,
            env: &script_env,
        }
    } else {
        // Resolved above the panic span so the `NodeStarted` payload could record
        // the same values the flags carry (#347/#424).
        tmux_session_manager::SessionTail::Agent {
            model: resolved_model,
            effort: resolved_effort,
        }
    };
    // A script node executes the RAW bash body (`role_prompt`), never the
    // augmented prompt — the preamble is prose an agent reads, not runnable bash.
    let spawn_prompt: &str = if is_script {
        &role_prompt
    } else {
        &full_prompt
    };
    // #407: wrap the tail in `docker exec … pdo-sbx-<run>` when sandboxed. The
    // marker MUST equal the session name (the kill path scans `/proc` for it), and
    // the workdir is the node's own working dir.
    let sandbox_wrap = run_sandboxed.then(|| tmux_session_manager::SandboxWrap {
        docker_bin: deps.docker_cmd_override.unwrap_or("docker"),
        uid: crate::sandbox_container::host_uid(),
        gid: crate::sandbox_container::host_gid(),
        marker: &session_name,
        workdir: &working_dir,
    });
    // #433 / ADR-0043: arm the turn-end `Stop` hook only when the operator has
    // opted into turn-end auto-completion (the SAME setting as the daemon sweep,
    // read FRESH — parity with model/effort) and never for a `script` node (bash
    // tail, no `claude`). Resolved here, at the spawn edge, so a `PUT /settings`
    // takes effect on the next node with no daemon restart.
    let inject_hook = !is_script && stored_autocomplete_turn_end(deps.db).await;
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
        sandbox_wrap.as_ref(),
        inject_hook,
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

    SpawnOutcome::Spawned {
        reused_sub_worktree,
        base_sha: spawn_base_sha,
        interrupted_git_ops,
    }
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
