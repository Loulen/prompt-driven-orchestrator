//! The checks a spawn passes before any side effect: the transition guard, the
//! benign loser of a duplicate, the sandbox precondition, an empty script, and
//! the admission cap (an atomic check-and-reserve).

use pdo_session::domain::names::node_session_name;
use tracing::{error, info, warn};

use super::interrupts::*;
use super::*;
use crate::application::admission_count::count_global_live_sessions_excluding;
use crate::application::events::append_event_with;
use crate::application::events::reload_run_state_with;
use crate::application::settings::stored_session_cap;
use crate::domain::admission;
use crate::domain::event;
use crate::domain::transition_guard;
use pdo_pipeline::domain::pipeline;

/// The Run's log and projection, loaded once at the head of the spawn.
pub(super) type Loaded = Option<(Vec<event::Event>, crate::domain::state::RunState)>;

/// Refuse, defer or fail the spawn before any side effect — or hand back the
/// Run as the guard read it.
pub(super) async fn guard(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) -> Result<Loaded, SpawnOutcome> {
    let run_id = spawn_ctx.run_id;
    // Transition guard (#212): refuse an illegal NodeStarted BEFORE any side
    // effect (sub-worktree creation, tmux session spawn) — never after. This
    // covers every caller: scheduler dispatch, resume re-evaluation,
    // restart_node, waiting-node retries.
    let started_probe = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeStarted,
        node_id: Some(node.id.clone()),
        iter: Some(iter),
        payload: None,
    };
    // Events kept alongside the projection: #503's `base_sha` lives on the previous
    // `NodeStarted` of this same iteration, and a reuse has to carry it forward
    // (#489-B / ADR-0037 §6).
    let loaded = reload_run_state_with(deps.runs, run_id).await;
    let projected = loaded.as_ref().map(|(_, s)| s);
    match transition_guard::validate_transition(projected, &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason } => {
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return Err(SpawnOutcome::Refused { reason });
        }
        transition_guard::Verdict::Reject { reason } => {
            // #515: the cause is typed now; forward its historical prose (the
            // `Refused` outcome carries a `String`, unchanged).
            let reason = reason.to_string();
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return Err(SpawnOutcome::Refused { reason });
        }
    }

    // #785 / ADR-0050 §2 — the BENIGN LOSER, caught before any side effect. The
    // guard above admits a `NodeStarted` on an iteration that is already live
    // (a "legal restart": `restart_node` / `retry_node` kill the session FIRST, so
    // for them the session is gone by the time they get here). A second spawn of
    // the same `(node, iter)` whose tmux session is STILL ALIVE is not a restart:
    // it is a racing duplicate (two edges to one target, a `resume_run` ↔ sweep
    // race, …) and the winner owns the iteration. Refuse it here — no redundant
    // `NodeStarted`, no sub-worktree work, and above all no `spawn_aborted`
    // parking of a run whose winner is healthy. The post-`NodeStarted` branch on
    // the tmux error below covers the residual window where both spawns pass this
    // probe before either creates its session.
    if let Some(state) = projected {
        if transition_guard::live_iteration(state, &node.id) == Some(iter) {
            let session_name = node_session_name(run_id, &node.id, iter);
            if deps.tmux.session_exists(&session_name) {
                let reason = format!(
                    "node {} iter {iter} already has a live session {session_name}: \
                     duplicate spawn is a benign no-op (ADR-0050 §2)",
                    node.id
                );
                warn!("Run {run_id}: {reason}");
                return Err(SpawnOutcome::Refused { reason });
            }
        }
    }

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
        return Err(SpawnOutcome::Deferred { reason });
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
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                None,
                &reason,
            )
            .await;
            return Err(SpawnOutcome::Failed { reason });
        }
    }

    Ok(loaded)
}

/// Take a session slot, or throttle the node into `waiting`. The guard handed
/// back holds the admission lock until the reservation event is appended.
pub(super) async fn admit<'a>(
    deps: SpawnDeps<'a>,
    run_id: &str,
    node: &pipeline::NodeDef,
    iter: i64,
) -> Result<tokio::sync::MutexGuard<'a, ()>, SpawnOutcome> {
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
    let cap = crate::adapters::env::configured_cap_with(stored_session_cap(deps.runs).await);
    let live = count_global_live_sessions_excluding(
        deps.runs,
        Some(admission::SlotExclusion {
            run_id,
            node_id: &node.id,
            iter,
        }),
    )
    .await;
    if !admission::can_admit(live, cap) {
        let waiting = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeWaiting,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "live_sessions": live, "cap": cap })),
        };
        if let Err(e) = append_event_with(deps.runs, &waiting).await {
            error!("failed to append node_waiting for {}: {e}", node.id);
        }
        info!(
            "node {} throttled into waiting ({live}/{cap} sessions live)",
            node.id
        );
        return Err(SpawnOutcome::Throttled);
    }

    Ok(admission_guard)
}
