//! Whether a `Running` Run is silently stalled — no live node, nothing the
//! scheduler can spawn — and why. Pure over the scheduler's own outputs.

use crate::domain::scheduler;
use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
use crate::domain::state;
use pdo_pipeline::domain::pipeline;

/// Idle window after which a `Running` run with a ready-to-spawn node but no live
/// node is treated as a silent spawn-abort rather than a node about to be driven.
///
/// It bounds **an event-driven spawn** — worktree creation plus a `tmux
/// new-session`, milliseconds of work — so two minutes is three orders of magnitude
/// of headroom. Do NOT "re-align" it on any agent-silence threshold: it is never
/// compared against an agent's silence, and measuring an agent by a duration is
/// exactly what this codebase abandoned.
pub const SPAWN_STALL_GRACE_SECS: i64 = 120;

/// Idle window after which a sandboxed Run stuck at `sandbox_prep = pending` is
/// treated as a lost preparation rather than one still working.
///
/// Deliberately an order of magnitude above [`SPAWN_STALL_GRACE_SECS`]: the two
/// measure different things. This one bounds staging up to a couple of gigabytes of
/// `~/.claude` plus a cold `docker build` — 83-87 s for a 2 GB profile with a warm
/// image, and legitimately minutes on a machine's first build.
pub const SANDBOX_PREP_STALL_GRACE_SECS: i64 = 900;

/// Decide whether a `Running` run is **stuck in a terminal-but-unreconciled
/// state**: no node that can drive it forward, no new work the scheduler can
/// produce, yet not `all completed` (that case is `maybe_complete_run`'s job).
///
/// `None` when the run still has a path forward, or is legitimately awaiting a
/// human. `idle_secs` distinguishes a node *about to be driven* (idle ≈ 0) from one
/// silently wedged.
///
/// Pure over the already-computed scheduler outputs, so the schedulability oracle
/// IS the real scheduler and this can never drift from it.
pub fn run_stall_reason(
    pipeline: &pipeline::PipelineDef,
    run_state: &state::RunState,
    ready: &[scheduler_dispatcher::ReadySpawn],
    loop_seed: &[scheduler::SchedulerAction],
    idle_secs: Option<i64>,
) -> Option<String> {
    // Only a Running run can silently stall. AwaitingUser is a legitimate live
    // state; Paused/Halted/terminal need no reconciliation.
    if run_state.status != state::RunStatus::Running {
        return None;
    }

    // A live node means the run can still advance organically — never reconcile
    // under it.
    let has_live_node = run_state.nodes.values().any(|n| n.status.can_progress());
    if has_live_node {
        return None;
    }

    // A sandboxed Run between `SandboxPrepStarted` and `SandboxPrepReady` looks
    // EXACTLY like a silent spawn-abort — ready node, no live node, and an idle clock
    // ticking up because the prep emits no event while it works. It is not one:
    // `spawn_node` is deliberately deferring and `SandboxPrepReady` will replay it,
    // and a cold `docker build` overruns the 120 s window comfortably.
    //
    // A LONGER grace, NOT an exemption: deferring for ever would trade a false failure
    // for a silent stall. Must come before the `ready` / `loop_seed` arms, because it
    // explains the whole class — while the prep is pending NOTHING in this Run can
    // start, whatever the scheduler proposes.
    if let Some(block) = run_state.sandbox_spawn_block() {
        if idle_secs.is_some_and(|s| s >= SANDBOX_PREP_STALL_GRACE_SECS) {
            return Some(format!(
                "run_stalled: {block}; idle {}s past the {SANDBOX_PREP_STALL_GRACE_SECS}s \
                 sandbox-prep grace — the preparation task is gone and nothing will \
                 re-drive this run (#445)",
                idle_secs.unwrap_or_default()
            ));
        }
        return None;
    }

    // A node the scheduler can spawn but no live node to run it.
    // `compute_ready_to_spawn` only returns nodes with NO state (or a Completed
    // loop-back), so a node an `advance_run` reached would already be `Running` or
    // `Waiting` — never still "ready". Its presence means `advance_run` never
    // completed for it. Past the grace window the spawn silently aborted (a panic
    // `spawn_node`'s catch missed, or a dropped completion future); below it, defer,
    // because `advance_run` is about to drive it.
    if !ready.is_empty() {
        if idle_secs.is_some_and(|s| s >= SPAWN_STALL_GRACE_SECS) {
            let mut names: Vec<&str> = ready.iter().map(|r| r.node_id.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            return Some(format!(
                "run_stalled: ready node(s) never driven — no live node, idle {}s \
                 (silent spawn-abort, #279): {}",
                idle_secs.unwrap_or_default(),
                names.join(", ")
            ));
        }
        return None;
    }

    // The scheduler can still seed a loop — not a stall.
    if !loop_seed.is_empty() {
        return None;
    }

    // Laps of an open collection region that were never started: the barrier can
    // never fire, so `should_complete_run` can never complete the run, and nothing
    // will ever mark a node `Stale` (the liveness sweep only inspects live tmux
    // sessions, and there are none).
    //
    // Deliberately NOT folded into the `open_region` arm below: an open loop/foreach
    // is deferred because the Pipeline Manager can unstick it by id, whereas nothing
    // re-proposes a missing lap — deferring here would be a silent stall. Same idle
    // grace as the arm above, so a fan-out burst caught mid-flight is never mistaken
    // for a wedge.
    let missing_laps: Vec<String> = run_state
        .collection_states
        .values()
        .filter(|cs| !cs.done)
        .flat_map(|cs| {
            let node = run_state.nodes.get(cs.entry.as_str());
            (1..=cs.total_items)
                .filter(move |lap| {
                    !node.is_some_and(|n| n.iterations.iter().any(|it| it.iter == *lap))
                })
                .map(|lap| format!("{}#{lap}/{}", cs.region_id, cs.total_items))
        })
        .collect();
    if !missing_laps.is_empty() {
        if idle_secs.is_some_and(|s| s >= SPAWN_STALL_GRACE_SECS) {
            return Some(format!(
                "run_stalled: collection region lap(s) never started — no live node, \
                 idle {}s; the barrier can never fire (#453): {}",
                idle_secs.unwrap_or_default(),
                missing_laps.join(", ")
            ));
        }
        return None;
    }

    // An open (not-`done`) region of ANY kind is the Pipeline Manager's domain: an
    // exhausted-unrouted region is surfaced as a Halt to be routed by id. Never
    // auto-fail under one — that would steal the manager's recovery path, and the
    // event-driven Halt is not re-derivable from this cold projection.
    //
    // **Exhaustive by construction**: `has_open_region` iterates every
    // `RegionStateKind`, so a new region-state map on `RunState` becomes a variant
    // `RegionStateKind::open_ids` must classify or fail to compile. A future region
    // kind can therefore never silently fall through to the blocker scan below.
    if run_state.has_open_region() {
        return None;
    }

    // All nodes Completed is `maybe_complete_run`'s success case — don't steal it.
    let pipeline_node_ids: Vec<String> = pipeline.nodes.iter().map(|n| n.id.clone()).collect();
    if run_state.all_nodes_completed(&pipeline_node_ids) {
        return None;
    }

    // Fail-fast only on a CONCRETE blocker: a node in a terminal but non-Completed
    // state whose downstream can never be scheduled. Without one there is no cause
    // to attribute, so defer rather than guess.
    let mut blockers: Vec<&str> = run_state
        .nodes
        .values()
        .filter(|n| {
            matches!(
                n.status,
                state::NodeStatus::Failed | state::NodeStatus::Stale | state::NodeStatus::Stopped
            )
        })
        .map(|n| n.node_id.as_str())
        .collect();
    if blockers.is_empty() {
        return None;
    }
    blockers.sort_unstable();

    Some(format!(
        "run_stalled: no live node and nothing schedulable; blocked behind: {}",
        blockers.join(", ")
    ))
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

    fn doc_node_def(id: &str) -> pipeline::NodeDef {
        pipeline::NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: pipeline::NodeType::Agent,
            inputs: Vec::new(),
            outputs: Vec::new(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn edge(src: &str, tgt: &str) -> pipeline::EdgeDef {
        pipeline::EdgeDef {
            source: pipeline::EdgeSource::single(src, "out"),
            target: pipeline::EdgeEndpoint {
                node: tgt.into(),
                port: "in".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    /// Two non-isolated nodes wired `a -> b`. `a` is the entry node (no incoming
    /// edges), `b` only spawns once `a` completes.
    fn linear_two_node_pipeline() -> pipeline::PipelineDef {
        pipeline::PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![doc_node_def("a"), doc_node_def("b")],
            edges: vec![edge("a", "b")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: false,
            grid_size: None,
            wait_for_started_nodes: false,
        }
    }

    /// The #453 projection: a `collection` region open at `total_items`, whose
    /// entry ran `started_laps` of them and completed every one it ran. Every
    /// node projects Completed, no session is alive, nothing is `Stale` — the
    /// exact shape `run_stall_reason` used to walk through without biting.
    fn run_state_collection_wedged(
        run_id: &str,
        total_items: i64,
        started_laps: i64,
    ) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "linear".into());
        rs.status = event_log::RunStatus::Running;
        rs.collection_states.insert(
            "topics_fanout".into(),
            event_log::CollectionState {
                region_id: "topics_fanout".into(),
                total_items,
                done: false,
                entry: "b".into(),
                members: vec!["b".into()],
            },
        );
        for (node_id, iters) in [("a", 1), ("b", started_laps)] {
            rs.nodes.insert(
                node_id.into(),
                event_log::NodeState {
                    missing_skills: Vec::new(),
                    skipped_skills: Vec::new(),
                    skills: None,
                    isolated_worktree: None,
                    harness: None,
                    cost: None,
                    node_id: node_id.into(),
                    status: event_log::NodeStatus::Completed,
                    iter: iters,
                    started_at: None,
                    completed_at: None,
                    failure_reason: None,
                    skip_reason: None,
                    iterations: (1..=iters)
                        .map(|iter| event_log::IterationInfo {
                            iter,
                            status: event_log::NodeStatus::Completed,
                            started_at: None,
                            completed_at: None,
                            interactive: false,
                            completion_released: false,
                            waited_ms: 0,
                            open_wait_since: None,
                        })
                        .collect(),
                    frontmatter_retries: 0,
                    frontmatter_violations: Vec::new(),
                    missing_outputs: Vec::new(),
                    delivery: None,
                    awaiting: None,
                },
            );
        }
        rs
    }

    fn run_state_failed_node(node_id: &str) -> event_log::RunState {
        run_state_with_node(
            "20260613-012555-stall",
            node_id,
            "agent",
            event_log::NodeStatus::Failed,
            1,
        )
    }

    /// A sandboxed Run mid-prep with a `full` staging profile — the projection the
    /// reconciler sees while the spawn is legitimately deferred.
    fn run_state_sandbox_mid_prep(run_id: &str) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "linear".into());
        rs.sandbox = event_log::SandboxMode::Profile("full".into());
        rs.sandbox_prep = Some(event_log::SandboxPrepState::Pending);
        rs
    }

    fn run_state_with_node(
        run_id: &str,
        node_id: &str,
        node_type: &str,
        status: event_log::NodeStatus,
        iter: i64,
    ) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "test".into());
        rs.node_defs.push(event_log::NodeDefInfo {
            isolated_worktree: None,
            orchestrator: false,
            on_demand: false,
            id: node_id.into(),
            name: None,
            node_type: node_type.into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });
        rs.nodes.insert(
            node_id.into(),
            event_log::NodeState {
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                skills: None,
                isolated_worktree: None,
                harness: None,
                cost: None,
                node_id: node_id.into(),
                status,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            },
        );
        rs
    }

    #[test]
    fn run_stall_reason_flags_a_running_run_with_a_failed_entry_and_nothing_schedulable() {
        // #214 added scope: a Running run whose only progress hinge (entry node
        // `a`) is Failed has no live node and nothing the scheduler can spawn —
        // `b` depends on `a` completing, which will never happen. The run would
        // otherwise sit Running forever (silent stall). Reconcile it terminal
        // with a cause that explains why nothing can advance.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_failed_node("a");

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(ready.is_empty(), "precondition: nothing schedulable");
        assert!(loop_seed.is_empty(), "precondition: no loop to seed");

        let reason = run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None)
            .expect("a Running run with no live node and nothing schedulable is stuck");
        assert!(
            reason.contains("a"),
            "stall cause {reason:?} must name the failed node blocking progress"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_fresh_run_with_a_schedulable_entry() {
        // A Running run where no node has started yet is the normal initial
        // state: the entry node is schedulable. Reconciling it would kill every
        // run the instant it is created. Must return None.
        let pipeline = linear_two_node_pipeline();
        let run_state = event_log::RunState::new("20260613-fresh".into(), "linear".into());

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(!ready.is_empty(), "precondition: entry node is schedulable");

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "a run with a schedulable node has a path forward — never reconcile it"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_with_a_live_node() {
        // A node actively Running keeps the run alive even if the scheduler has
        // nothing else queued. Reconciling here would race the live work to a
        // false RunFailed. Must return None regardless of empty scheduler output.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_with_node(
            "20260613-live",
            "a",
            "agent",
            event_log::NodeStatus::Running,
            1,
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "a live Running node means the run can still finish — never reconcile it"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_awaiting_a_human() {
        // AwaitingUser is a legitimate live state (interactive node waiting on a
        // person). It is not a silent stall and must never be auto-failed.
        let pipeline = linear_two_node_pipeline();
        let mut run_state = run_state_with_node(
            "20260613-await",
            "a",
            "agent",
            event_log::NodeStatus::AwaitingUser,
            1,
        );
        run_state.status = event_log::RunStatus::AwaitingUser;

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "an AwaitingUser run is waiting on a human, not stalled"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_with_an_open_loop_region_awaiting_a_route() {
        // A run observed still Running with an open loop region and no live node —
        // before its Halt lands, or awaiting a route with no failed node — must NOT
        // be auto-failed: that steals the Pipeline Manager's recovery path.
        //
        // The edge `a -> b` is CONDITIONAL, so `b` is never an entry/ready node. With
        // `a` Completed and `b` not started, NOTHING is schedulable and NOT all nodes
        // are completed, so neither the `ready` nor the `all_completed` guard can mask
        // the loop-awareness gap this test pins.
        let mut conditional_edge = edge("a", "b");
        conditional_edge.when = Some(serde_yaml::from_str("iter: { gte: 99 }").unwrap());
        let pipeline = pipeline::PipelineDef {
            name: "cond".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![doc_node_def("a"), doc_node_def("b")],
            edges: vec![conditional_edge],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: false,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let mut run_state = event_log::RunState::new("20260613-loop".into(), "cond".into());
        // Entry node `a` completed; nothing else live or schedulable.
        run_state.nodes.insert(
            "a".into(),
            event_log::NodeState {
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                skills: None,
                isolated_worktree: None,
                harness: None,
                cost: None,
                node_id: "a".into(),
                status: event_log::NodeStatus::Completed,
                iter: 2,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            },
        );
        // An open loop region (exhausted at max_iter, not done) awaiting a route.
        run_state.loop_states.insert(
            "review_loop".into(),
            event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 2,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "a run with an open loop region awaiting a manager route must not be \
             auto-failed — that is the manager's recovery path, not a fail-fast stall"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_with_a_waiting_node_behind_a_blocker() {
        // A run with a throttled `Waiting` node behind a `Stale`/`Failed` blocker is
        // NOT stalled: the `Waiting` node spawns the instant a slot frees, and
        // `can_progress()` INCLUDES `Waiting` precisely so this run is left alone.
        //
        // Collapsing onto the admission `holds_session()` set, which EXCLUDES
        // `Waiting`, would see "no live node", find the `Stale` blocker, and reconcile
        // a healthy throttled run to Failed.
        let pipeline = linear_two_node_pipeline();
        let mut run_state =
            event_log::RunState::new("20260629-waiting-blocker".into(), "linear".into());
        // Entry node `a` is Stale (a concrete blocker for the fail-fast path).
        run_state.nodes.insert(
            "a".into(),
            event_log::NodeState {
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                skills: None,
                isolated_worktree: None,
                harness: None,
                cost: None,
                node_id: "a".into(),
                status: event_log::NodeStatus::Stale,
                iter: 1,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            },
        );
        // Node `b` is throttled `Waiting`: ready but no admission slot yet. No
        // node is Running/AwaitingUser, so only `can_progress`'s inclusion of
        // `Waiting` keeps this run from being declared stalled.
        run_state.nodes.insert(
            "b".into(),
            event_log::NodeState {
                missing_skills: Vec::new(),
                skipped_skills: Vec::new(),
                skills: None,
                isolated_worktree: None,
                harness: None,
                cost: None,
                node_id: "b".into(),
                status: event_log::NodeStatus::Waiting,
                iter: 1,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                skip_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
                missing_outputs: Vec::new(),
                delivery: None,
                awaiting: None,
            },
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(
            ready.is_empty(),
            "precondition: nothing schedulable (the Stale node never completes, \
             so `b`'s input edge is unsatisfied)"
        );
        assert!(loop_seed.is_empty(), "precondition: no loop to seed");

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "a Waiting node can still progress (it spawns when a slot frees) — \
             a run behind a blocker but holding a Waiting node is NOT stalled"
        );
    }

    #[test]
    fn run_stall_reason_flags_a_ready_node_never_driven_past_grace() {
        // #279 Layer 2. After a silent spawn-abort the next node has NO event at
        // all, so the scheduler still computes it as `ready` — the pre-#279 guard
        // returned None ("about to be driven"). But with no live node and the run
        // gone idle past the grace window, `advance_run` never completed for it
        // and nothing will re-drive it (advance_run is event-triggered, the stale
        // detector only inspects live sessions). Fail loud rather than wedge.
        let pipeline = linear_two_node_pipeline();
        let run_state = event_log::RunState::new("20260630-spawn-abort".into(), "linear".into());

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(
            !ready.is_empty(),
            "precondition: entry node `a` is ready with no event (the orphan signature)"
        );
        assert!(
            run_state.nodes.values().all(|n| !n.status.can_progress()),
            "precondition: no live node"
        );

        let reason = run_stall_reason(
            &pipeline,
            &run_state,
            &ready,
            &loop_seed,
            Some(SPAWN_STALL_GRACE_SECS),
        )
        .expect("a ready node idle past the grace window with no live node is a #279 silent abort");
        assert!(
            reason.contains("#279): a"),
            "cause {reason:?} must name the undriven node and reference #279"
        );
    }

    #[test]
    fn run_stall_reason_defers_a_ready_node_within_the_grace_window() {
        // The normal initial state of every run: the entry node is ready and
        // `advance_run` is about to drive it. Failing here would kill a healthy
        // run mid-spawn. Within the grace window the idle dimension defers.
        let pipeline = linear_two_node_pipeline();
        let run_state = event_log::RunState::new("20260630-mid-spawn".into(), "linear".into());

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(!ready.is_empty(), "precondition: entry node is ready");

        assert!(
            run_stall_reason(
                &pipeline,
                &run_state,
                &ready,
                &loop_seed,
                Some(SPAWN_STALL_GRACE_SECS - 1),
            )
            .is_none(),
            "a ready node within the grace window is about to be driven — never fail it"
        );
    }

    #[test]
    fn run_stall_reason_defers_a_sandboxed_run_still_preparing_past_the_spawn_grace() {
        // #445, the false-positive half. A sandboxed Run between SandboxPrepStarted
        // and SandboxPrepReady has an entry node the scheduler reports ready, no live
        // node, and — because the prep emits nothing while it works — an idle clock
        // that keeps ticking. That is the #279 orphan signature exactly, so without a
        // sandbox arm the reconciler would fail every prep slower than 120 s: measured
        // 83-87 s for a 2 GB profile, and minutes on a cold `docker build`. Failing
        // those Runs would defeat the precondition that just saved them.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_sandbox_mid_prep("20260725-sbx-preparing");

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(
            !ready.is_empty(),
            "precondition: the deferred entry node is still in the ready set"
        );

        assert!(
            run_stall_reason(
                &pipeline,
                &run_state,
                &ready,
                &loop_seed,
                Some(SPAWN_STALL_GRACE_SECS * 2),
            )
            .is_none(),
            "a Run whose container is still being built is preparing, not stalled — \
             it must survive well past the 120s spawn-stall window"
        );
    }

    #[test]
    fn run_stall_reason_flags_a_sandbox_prep_lost_past_its_own_grace() {
        // The other half: deferring for ever would trade a false failure for a silent
        // stall, which ADR-0004 forbids just as strongly. Past the sandbox-prep grace
        // the preparation task is genuinely gone (killed with a previous daemon,
        // panicked outside its catch) and nothing will ever emit SandboxPrepReady, so
        // the Run is failed with a cause that names the sandbox instead of blaming
        // tmux — the diagnosability complaint in the report.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_sandbox_mid_prep("20260725-sbx-lost");

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        let reason = run_stall_reason(
            &pipeline,
            &run_state,
            &ready,
            &loop_seed,
            Some(SANDBOX_PREP_STALL_GRACE_SECS),
        )
        .expect("a prep pending past its grace window is a lost preparation, not work in flight");
        assert!(
            reason.contains("sandbox prep") && reason.contains("#445"),
            "cause {reason:?} must name the sandbox preparation rather than tmux"
        );
    }

    #[test]
    fn run_stall_reason_defers_a_ready_node_when_idle_is_unknown() {
        // Unknown idle (no parseable event timestamp) must never trigger the
        // fail-fast: only a *measured* idle past the grace window does. Fail-open
        // here would regress the pre-#279 "ready set ⇒ defer" guarantee.
        let pipeline = linear_two_node_pipeline();
        let run_state = event_log::RunState::new("20260630-unknown-idle".into(), "linear".into());

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, None).is_none(),
            "unknown idle must never fail a run — the ready node may be about to spawn"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_live_node_even_past_grace() {
        // The live-node guard precedes the idle check: a Running node idle past
        // the grace window is healthy long-running work (a slow agent), never a
        // #279 stall. (`idle` measures the event log, not the node's own work.)
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_with_node(
            "20260630-live-old",
            "a",
            "agent",
            event_log::NodeStatus::Running,
            1,
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, Some(100_000)).is_none(),
            "a live Running node is never a stall, regardless of how long the log has been idle"
        );
    }

    #[test]
    fn run_stall_reason_flags_a_collection_region_whose_laps_never_started() {
        // #453, the silent half. The engine fix means this state should no longer
        // occur, but nothing else in the daemon can see it if it does: `is_stalled`
        // needs a `Stale` node and only live tmux sessions ever become `Stale`, and
        // there are none. So the run reported `"stalled": false` for ever while the
        // barrier could never fire. Characterised now, and failed loud.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_collection_wedged("20260727-wedged", 2, 1);

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(
            run_state.nodes.values().all(|n| !n.status.can_progress()),
            "precondition: no live node — the wedge signature"
        );

        let reason = run_stall_reason(
            &pipeline,
            &run_state,
            &ready,
            &loop_seed,
            Some(SPAWN_STALL_GRACE_SECS),
        )
        .expect("an open collection region missing a lap, with no live node, is a stall");
        assert!(
            reason.contains("#453") && reason.contains("topics_fanout#2/2"),
            "cause {reason:?} must name the region and the missing lap"
        );
    }

    #[test]
    fn run_stall_reason_defers_a_collection_fan_out_within_the_grace_window() {
        // The healthy transient: `CollectionStarted` is appended and the laps are
        // spawning right now. Below the grace window nothing is failed.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_collection_wedged("20260727-fanning-out", 2, 1);

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(
                &pipeline,
                &run_state,
                &ready,
                &loop_seed,
                Some(SPAWN_STALL_GRACE_SECS - 1),
            )
            .is_none(),
            "a fan-out caught mid-burst is not a wedge"
        );
    }

    #[test]
    fn run_stall_reason_ignores_a_collection_region_whose_laps_all_ran() {
        // The control: every lap started (the post-fix world). The region is still
        // open — the barrier fires on the next sweep — so this must NOT be read as
        // a stall, whatever the idle clock says.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_collection_wedged("20260727-all-laps", 2, 2);

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, Some(100_000)).is_none(),
            "every lap accounted for is not a missing-lap stall"
        );
    }
}
