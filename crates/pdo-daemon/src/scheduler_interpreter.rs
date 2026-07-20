//! The single interpreter of `SchedulerAction` effects (#357).
//!
//! ADR-0009 splits scheduling into **decision** and **effect**. The pure
//! producers in `scheduler.rs` (`evaluate_outgoing_edges_full`,
//! `evaluate_loop_body_completion`, `evaluate_collection_barrier`,
//! `seed_pending_loops`) compute a `Vec<SchedulerAction>` without touching the
//! world. *Executing* that list — spawning a node, emitting a
//! completion/halt/switch/loop/collection event, depositing collection items —
//! used to be copy-pasted across three drivers: `handle_node_completion`
//! (3 loops), `re_evaluate_after_command` (3 loops), and the loop-seed match in
//! `run_advance::advance_run`. Adding an effect to one variant meant editing up
//! to seven `match` blocks, and the one real divergence between the copies (a
//! spawn dedup guard) was a silent drift rather than a visible parameter.
//!
//! This module owns that effect exactly once:
//!
//! - [`interpret`] runs ONE [`SchedulerAction`] as a linear sequence of Layer-2
//!   primitives (`spawn_node`, the `emit_*` event sinks,
//!   `passthrough_switch_artifact`, `deposit_collection_items`). It NEVER
//!   re-enters the scheduler (no call to `advance_run` / `re_evaluate_after_command`
//!   / itself, no reload/re-projection — that stays in the drivers), so it is a
//!   Layer-3 convenience seam, not a fourth scheduling entry point.
//! - [`admit_spawn`] is the *pure* core that carries the sole historical
//!   divergence between the drivers as a typed argument ([`SpawnDedup`]) instead
//!   of a silently-desynchronised code copy. It is unit-tested here without a
//!   daemon.
//!
//! Behaviour preservation is strict: this is a carve, not a fix. The latent
//! same-iter double-spawn on the completion path (see the
//! `nonloop_node_respawned_per_loop_lap` incident) is *frozen* behind
//! [`SpawnDedup::InternalOnly`], not corrected — unifying the two paths onto
//! `GuardSuperfluous` is a behaviour change tracked as a separate follow-up.

use crate::event_log::{self, RunState};
use crate::node_spawn::{spawn_node, SpawnContext, SpawnDeps, SpawnOutcome};
use crate::scheduler::SchedulerAction;
use crate::transition_guard;
use crate::{
    deposit_collection_items, emit_collection_action, emit_loop_action, emit_run_event,
    passthrough_switch_artifact, AppState,
};

/// Whether to re-apply the scheduler-side spawn dedup
/// ([`transition_guard::spawn_superfluous`]) before executing a `Spawn`.
///
/// This is the ONLY behavioural divergence between the three drivers, now a
/// visible typed argument instead of a code copy (#357).
///
/// NB: deliberately NOT named `AdmissionPolicy` (as the issue text proposed).
/// "admission" is already a loaded term in this crate — it is the *concurrent
/// session cap* (`admission.rs`, `SpawnDeps::admission_lock`, CONTEXT.md §749).
/// The guard controlled here is `spawn_superfluous` (a redundant spawn
/// proposal), a distinct concept from "is a session slot free"; naming it
/// `AdmissionPolicy` would conflate the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpawnDedup {
    /// Re-evaluation paths (`resume_run`, `extend_cycle`, region routes): reject
    /// a spawn whose node already has a live iteration (at any iter) or whose
    /// proposed iteration has already completed.
    GuardSuperfluous,
    /// Completion / advance / loop-seed paths: no scheduler-side dedup; the
    /// transition guard inside `spawn_node` is the only net (a same-iter-live
    /// spawn is a legal restart there).
    InternalOnly,
}

/// The pure decision of [`admit_spawn`]: let the `Spawn` proceed, or skip it
/// with a human-readable reason (fed into `ReEvalSummary.skipped`, ADR-0025).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpawnAdmission {
    Admit,
    Skip { reason: String },
}

/// PURE decision: should this `Spawn { node_id, iter }` proceed under `policy`?
///
/// Delegates the real logic to the existing pure
/// [`transition_guard::spawn_superfluous`]. No `AppState`, no IO, no async —
/// this is the exact point where the two policies diverge, so it carries the
/// net test (`skip vs restart`).
pub(crate) fn admit_spawn(
    policy: SpawnDedup,
    run_state: &RunState,
    node_id: &str,
    iter: i64,
) -> SpawnAdmission {
    match policy {
        SpawnDedup::GuardSuperfluous => {
            match transition_guard::spawn_superfluous(run_state, node_id, iter) {
                Some(reason) => SpawnAdmission::Skip { reason },
                None => SpawnAdmission::Admit,
            }
        }
        SpawnDedup::InternalOnly => SpawnAdmission::Admit,
    }
}

/// What one [`SchedulerAction`] actually produced, so `re_evaluate_after_command`
/// can rebuild its `ReEvalSummary` (ADR-0025) and the fire-and-forget drivers
/// (`handle_node_completion`, `advance_run`) know when to stop dispatching.
///
/// Not `#[must_use]`: the fire-and-forget drivers intentionally drop
/// [`ActionOutcome::Spawned`].
pub(crate) enum ActionOutcome {
    /// An effect was emitted / routed / deposited (Switch, Loop*, Collection*,
    /// or a `Spawn` whose target node is absent from the pipeline): nothing for
    /// the caller to fold.
    Progressed,
    /// `spawn_node` ran; carries the raw [`SpawnOutcome`] for `record_spawn`.
    Spawned {
        node_id: String,
        iter: i64,
        outcome: SpawnOutcome,
    },
    /// `GuardSuperfluous` skipped the spawn before any effect. Never produced
    /// under [`SpawnDedup::InternalOnly`].
    SpawnSkipped { reason: String },
    /// `RunCompleted` was emitted — the driver must stop dispatching and return.
    Completed,
    /// `RunHalted` was emitted — the driver must stop dispatching and return.
    Halted { message: String },
}

/// Interpret ONE [`SchedulerAction`]. A linear sequence of Layer-2 primitives
/// that NEVER re-enters the scheduler (no `advance_run` / `re_evaluate` / self
/// call, no reload/re-projection — those stay in the drivers). ADR-0009.
///
/// `run_state` is the driver's per-pass snapshot (INV-2) and is *never* reloaded
/// or re-projected here. `source_iter` is consulted only by `SwitchRouted`
/// (INV-3). `emit_collection_action` runs *before* `deposit_collection_items`
/// (INV-7).
pub(crate) async fn interpret(
    state: &AppState,
    ctx: &SpawnContext<'_>,
    run_state: &RunState,
    policy: SpawnDedup,
    source_iter: i64,
    action: &SchedulerAction,
) -> ActionOutcome {
    let run_id = ctx.run_id;
    match action {
        SchedulerAction::Spawn { node_id, iter } => {
            match admit_spawn(policy, run_state, node_id, *iter) {
                SpawnAdmission::Skip { reason } => ActionOutcome::SpawnSkipped { reason },
                SpawnAdmission::Admit => {
                    // Same silent no-op as the drivers: a node absent from the
                    // pipeline is skipped without effect (nothing to record).
                    let Some(node) = ctx.pipeline.nodes.iter().find(|n| n.id == *node_id) else {
                        return ActionOutcome::Progressed;
                    };
                    let outcome = spawn_node(SpawnDeps::from_state(state), ctx, node, *iter).await;
                    ActionOutcome::Spawned {
                        node_id: node_id.clone(),
                        iter: *iter,
                        outcome,
                    }
                }
            }
        }
        SchedulerAction::Halt { message } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::RunHalted,
                Some(serde_json::json!({ "message": message })),
            )
            .await;
            ActionOutcome::Halted {
                message: message.clone(),
            }
        }
        SchedulerAction::Complete => {
            emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
            ActionOutcome::Completed
        }
        SchedulerAction::SwitchRouted {
            node_id,
            chosen_branch,
        } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::SwitchRouted,
                Some(serde_json::json!({
                    "node_id": node_id,
                    "chosen_branch": chosen_branch,
                })),
            )
            .await;
            passthrough_switch_artifact(ctx, node_id, chosen_branch, source_iter);
            ActionOutcome::Progressed
        }
        SchedulerAction::LoopIterStarted { .. }
        | SchedulerAction::LoopBreakReceived { .. }
        | SchedulerAction::LoopMaxReached { .. }
        | SchedulerAction::LoopDone { .. } => {
            emit_loop_action(state, run_id, action).await;
            ActionOutcome::Progressed
        }
        SchedulerAction::CollectionStarted { entry, items, .. } => {
            // INV-7: emit BEFORE depositing.
            emit_collection_action(state, run_id, action).await;
            deposit_collection_items(ctx.artifacts_dir, entry, items);
            ActionOutcome::Progressed
        }
        SchedulerAction::CollectionEmpty { .. } | SchedulerAction::CollectionDone { .. } => {
            emit_collection_action(state, run_id, action).await;
            ActionOutcome::Progressed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{now_iso, project, Event, EventKind};

    // Fixtures mirror transition_guard.rs's test helpers: build a lifecycle
    // event, then project a list of them into a RunState.
    fn ev(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
        let payload = if kind == EventKind::RunStarted {
            Some(serde_json::json!({ "pipeline_name": "test" }))
        } else {
            None
        };
        Event {
            id: None,
            run_id: "run-1".into(),
            ts: now_iso(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload,
        }
    }

    fn state_from(events: &[Event]) -> RunState {
        project(events).expect("projected state")
    }

    #[test]
    fn same_iter_live_respawn_skipped_under_guard_admitted_under_internal() {
        // x is live at iter 1. GuardSuperfluous (re-eval paths) skips a re-spawn;
        // InternalOnly (completion/advance) admits it (spawn_node's own guard
        // then treats same-iter-live as a legal restart). This IS the frozen
        // divergence (INV-1).
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("x"), Some(1)),
        ]);
        assert!(matches!(
            admit_spawn(SpawnDedup::GuardSuperfluous, &state, "x", 1),
            SpawnAdmission::Skip { .. }
        ));
        assert_eq!(
            admit_spawn(SpawnDedup::InternalOnly, &state, "x", 1),
            SpawnAdmission::Admit
        );
    }

    #[test]
    fn completed_iter_skipped_but_fresh_lap_admitted_under_guard() {
        // x completed iter 1: re-spawning iter 1 is superfluous, but a fresh lap
        // at iter 2 is legitimate work (extend_cycle / region laps).
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("x"), Some(1)),
            ev(EventKind::NodeCompleted, Some("x"), Some(1)),
        ]);
        assert!(matches!(
            admit_spawn(SpawnDedup::GuardSuperfluous, &state, "x", 1),
            SpawnAdmission::Skip { .. }
        ));
        assert_eq!(
            admit_spawn(SpawnDedup::GuardSuperfluous, &state, "x", 2),
            SpawnAdmission::Admit
        );
    }

    #[test]
    fn internal_only_admits_regardless_of_liveness_or_completion() {
        // InternalOnly re-applies NO scheduler dedup — every proposal is admitted
        // at this layer, and spawn_node's transition guard is the sole net. This
        // locks the "no guard on the completion/advance paths" half of INV-1.
        let state = state_from(&[
            ev(EventKind::RunStarted, None, None),
            ev(EventKind::NodeStarted, Some("x"), Some(2)),
            ev(EventKind::NodeCompleted, Some("done"), Some(1)),
        ]);
        // Another iter live (GuardSuperfluous would skip): admitted here.
        assert_eq!(
            admit_spawn(SpawnDedup::InternalOnly, &state, "x", 3),
            SpawnAdmission::Admit
        );
        // Same iter live (legal restart downstream): admitted here.
        assert_eq!(
            admit_spawn(SpawnDedup::InternalOnly, &state, "x", 2),
            SpawnAdmission::Admit
        );
        // Already-completed iter (GuardSuperfluous would skip): admitted here.
        assert_eq!(
            admit_spawn(SpawnDedup::InternalOnly, &state, "done", 1),
            SpawnAdmission::Admit
        );
    }

    #[test]
    fn guard_admits_fresh_node() {
        // A never-seen node under GuardSuperfluous is genuine missing work.
        let state = state_from(&[ev(EventKind::RunStarted, None, None)]);
        assert_eq!(
            admit_spawn(SpawnDedup::GuardSuperfluous, &state, "fresh", 1),
            SpawnAdmission::Admit
        );
    }
}
