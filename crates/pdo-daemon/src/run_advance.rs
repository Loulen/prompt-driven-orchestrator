//! Run advancement — the single-pass "tick" that drives a live Run forward, plus
//! the two halves of node completion: the pure head [`evaluate_completion_head`]
//! and the side-effecting tail [`complete_node`].
//!
//! Don't make [`advance_run`] or [`complete_node`] reentrant (ADR-0009 / #122):
//! keep them a linear sequence over [`spawn_node`], the pure `scheduler*`
//! evaluators and `append_event`; never call an advancement helper from another
//! one, and never wire scheduler-driving code onto `event_tx` — the cycle would
//! not be caught by the compiler or the tests. [`spawn_node`] stays a leaf.
//!
//! Don't add side effects to the head: its purity (no DB, no tmux, no clock) is
//! what lets all three completion callers share it. The async checks stay
//! caller-side — `run_is_forgotten` before the head, the sub-worktree merge and
//! `check_output_validation_with_retry` after an `Allow`.
//!
//! Don't collapse the per-caller tail divergence ratified by ADR-0023.

use tracing::{error, info};

use crate::event_log;
use crate::node_spawn::{spawn_node, SpawnContext, SpawnDeps};
use crate::pipeline;
use crate::scheduler;
use crate::scheduler_dispatcher;
use crate::scheduler_interpreter::{self, SpawnDedup};
use crate::transition_guard;
use crate::worktree_ops::worktree_dir_for_run;
use crate::{
    append_event, effective_repo_root, handle_node_completion, load_events,
    resolve_completed_frontmatter, resolve_run_pipeline_path, resolve_run_variables,
    retry_waiting_nodes, AppState,
};

/// Advance one Run by a single tick: spawn whatever the scheduler says is ready
/// (plus any pending loop-iteration seeds), or — when there is nothing left to
/// spawn — complete the Run if every expected node is done.
///
/// A no-op unless the run is `Running` or `AwaitingUser`.
pub(crate) async fn advance_run(state: &AppState, run_id: &str) {
    // Must run before the readiness sweep: a structurally-unreachable node would
    // otherwise sit forever and the run would never reach "all expected nodes done".
    sweep_auto_skips(state, run_id).await;

    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("advance_run: failed to load events for {run_id}: {e}");
            return;
        }
    };
    let Some(run_state) = event_log::project(&events) else {
        return;
    };

    if run_state.status != event_log::RunStatus::Running
        && run_state.status != event_log::RunStatus::AwaitingUser
    {
        return;
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipeline = parse_result.pipeline;

    let resolved_vars = resolve_run_variables(&pipeline, &events);
    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    let loop_seed_actions = scheduler::seed_pending_loops(&pipeline, &run_state, &resolved_vars);

    if ready.is_empty() && loop_seed_actions.is_empty() {
        // Don't use [`expected_completion_node_ids`] here: this site must judge
        // against the *current* (post-YAML-edit) pipeline, not the run's frozen
        // snapshot, or an edited run stays dangling in Running forever.
        let pipeline_node_ids: Vec<String> = pipeline.nodes.iter().map(|n| n.id.clone()).collect();
        maybe_complete_run(state, run_id, &pipeline_node_ids, &run_state, false).await;
        return;
    }

    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    spawn_each(state, &spawn_ctx, &ready).await;

    for action in &loop_seed_actions {
        // InternalOnly, not scheduler dedup: a loop seed is always a fresh iter-1.
        let _ = scheduler_interpreter::interpret(
            state,
            &spawn_ctx,
            &run_state,
            SpawnDedup::InternalOnly,
            1,
            action,
        )
        .await;
    }

    info!(
        "advance_run: spawned {} node(s) and seeded {} loop action(s) for run {run_id}",
        ready.len(),
        loop_seed_actions.len()
    );
}

/// Auto-skip every node that has become **structurally unreachable** — its
/// producing branch was not taken, so nothing will ever spawn it (ADR-0011).
/// Each skipped node is marked satisfied with an *empty output* so a downstream
/// resolver finds a concrete artifact rather than a missing file.
///
/// Must iterate: skipping one node can render a downstream either/or dead in
/// turn. The pass count is capped at the node count so a pathological graph
/// cannot spin.
async fn sweep_auto_skips(state: &AppState, run_id: &str) {
    let mut passes = 0usize;
    loop {
        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(_) => return,
        };
        let Some(run_state) = event_log::project(&events) else {
            return;
        };
        if run_state.status != event_log::RunStatus::Running
            && run_state.status != event_log::RunStatus::AwaitingUser
        {
            return;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
        let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
            return;
        };
        let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
            return;
        };
        let pipeline = parse_result.pipeline;

        // Bound: at most one skip per node over the whole sweep.
        if passes > pipeline.nodes.len() {
            return;
        }
        passes += 1;

        let resolved_vars = resolve_run_variables(&pipeline, &events);
        let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let frontmatter_by_node =
            resolve_completed_frontmatter(&pipeline, &run_state, &artifacts_dir);

        let skips = scheduler::unreachable_nodes(
            &pipeline,
            &run_state,
            &frontmatter_by_node,
            &resolved_vars,
        );
        if skips.is_empty() {
            return;
        }

        let no_overrides = std::collections::HashMap::new();
        let mut skipped_ids = Vec::new();
        for (node_id, reason) in &skips {
            // A structurally-unreachable node never started, so it skips at iter 1.
            let iter = 1;
            if let Err(e) = crate::node_primitives::write_skip_outputs(
                &pipeline,
                node_id,
                iter,
                &no_overrides,
                &artifacts_dir,
            ) {
                error!("auto-skip: failed to write outputs for {node_id} in {run_id}: {e}");
                continue;
            }
            let event = event_log::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "source": "auto_skip_unreachable",
                    "skipped": true,
                    "reason": reason,
                })),
            };
            match append_event(state, &event).await {
                Ok(()) => {
                    info!("auto-skip: node {node_id} in run {run_id} unreachable — {reason}");
                    skipped_ids.push(node_id.clone());
                }
                Err(e) => error!("auto-skip: failed to append skip for {node_id}: {e}"),
            }
        }

        if skipped_ids.is_empty() {
            return;
        }
        // `fire_edges` re-projects internally, so the next pass sees the updated
        // state and can skip a newly-dead node.
        for node_id in &skipped_ids {
            fire_edges(state, run_id, node_id).await;
        }
    }
}

/// Spawn each node in `ready_set` through [`spawn_node`].
///
/// Don't re-sort or de-duplicate `ready_set`: under the session cap its order
/// decides who grabs the last free slot.
pub(crate) async fn spawn_each(
    state: &AppState,
    spawn_ctx: &SpawnContext<'_>,
    ready_set: &[scheduler_dispatcher::ReadySpawn],
) {
    for rs in ready_set {
        if let Some(node) = spawn_ctx.pipeline.nodes.iter().find(|n| n.id == rs.node_id) {
            spawn_node(SpawnDeps::from_state(state), spawn_ctx, node, rs.iter).await;
        }
    }
}

/// The set of node ids that must all be `Completed` for the Run to be done, as
/// seen from a *node-done* site.
///
/// Prefers the run's `node_defs` snapshot, frozen at run start, so a mid-run YAML
/// edit can't change what "all done" means for an in-flight run; the `nodes`-keys
/// fallback exists only for legacy runs with no snapshot.
///
/// [`advance_run`]'s own completion branch deliberately uses a *different* set.
pub(crate) fn expected_completion_node_ids(run_state: &event_log::RunState) -> Vec<String> {
    if !run_state.node_defs.is_empty() {
        run_state.node_defs.iter().map(|nd| nd.id.clone()).collect()
    } else {
        run_state.nodes.keys().cloned().collect()
    }
}

/// Pure decision: should a `RunCompleted` be emitted for this projected state?
///
/// `complete_when_awaiting_user` exists for `mark_node_done`, where the
/// just-finished node was interactive so the run still projects `AwaitingUser` at
/// the completion check. Every other caller permits only `Running`.
pub(crate) fn should_complete_run(
    run_state: &event_log::RunState,
    expected_node_ids: &[String],
    complete_when_awaiting_user: bool,
) -> bool {
    let status_permits = run_state.status == event_log::RunStatus::Running
        || (complete_when_awaiting_user && run_state.status == event_log::RunStatus::AwaitingUser);
    // Don't drop this in favour of node statuses: node status reflects the LATEST
    // event, so a collection member whose lap 1 finished while laps 2..N run can
    // transiently project Completed. The barrier is the only truthful signal.
    let collections_done = run_state.collection_states.values().all(|cs| cs.done);
    status_permits && collections_done && run_state.all_nodes_completed(expected_node_ids)
}

/// Emit exactly one `RunCompleted` if [`should_complete_run`] says so; returns
/// whether it emitted.
///
/// `append_event` does **not** de-dup `RunCompleted`, so this must stay the only
/// completion emitter on the single-pass paths — never call it from an
/// all-runs/waiting sweep, or a run emits several.
pub(crate) async fn maybe_complete_run(
    state: &AppState,
    run_id: &str,
    expected_node_ids: &[String],
    run_state: &event_log::RunState,
    complete_when_awaiting_user: bool,
) -> bool {
    if !should_complete_run(run_state, expected_node_ids, complete_when_awaiting_user) {
        return false;
    }
    let run_completed = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunCompleted,
        node_id: None,
        iter: None,
        payload: None,
    };
    if let Err(e) = append_event(state, &run_completed).await {
        error!("failed to append run_completed: {e}");
        return false;
    }
    true
}

/// Which order the completion tail runs the producer's edge-firing pass
/// ([`handle_node_completion`]) relative to the readiness sweep ([`advance_run`]).
///
/// The two orders are believed behavior-equivalent (the two passes cover disjoint
/// spawn sets and `spawn_node` re-validates every transition), but don't collapse
/// them to one variant without an order-equivalence integration test first —
/// nothing here would catch a divergence.
pub(crate) enum CompletionOrder {
    /// `node_done` & `handle_merge_resolver_done`: edges, then sweep.
    CompletionFirst,
    /// `mark_node_done` arm: sweep, then edges (the interactive node is already gone).
    SweepFirst,
}

/// What the completion tail did — lets each caller keep its own log line / HTTP
/// response while sharing the tail.
pub(crate) enum CompletionOutcome {
    /// `RunCompleted` was emitted on this call.
    RunCompleted,
    /// The run advanced but not all expected nodes are done yet — or it completed
    /// earlier in the same tail via an HNC `Complete`/`Halt` action. Either way
    /// the completion gate emitted nothing.
    StillRunning,
    Halted,
}

/// The three-way outcome of the pure completion **head** decision.
///
/// The caller's contract per variant:
/// - [`CompletionHead::Reject`] → `409 CONFLICT { error }`.
/// - [`CompletionHead::NoOp`]   → `200 { ok, noop, reason }`.
/// - [`CompletionHead::Allow`]  → run your own side effects, append your own
///   `NodeCompleted`, drive your own tail.
pub(crate) enum CompletionHead {
    Reject { reason: String },
    NoOp { reason: String },
    Allow,
}

/// The shared, **pure** *head* of node completion, used by `node_done`, the
/// `mark_node_done` command arm and `node_skip`.
///
/// Decides only *whether* the completion is legal: no DB, no tmux, no append, no
/// clock (the guard is ts-blind, so the probe carries an empty `ts`). Keep it
/// that way — see the module header.
///
/// `None` `run_state` yields `Allow`; each caller decides whether to admit it
/// (`node_done` and `node_skip` 404 *before* calling; `mark` forwards it).
pub(crate) fn evaluate_completion_head(
    run_state: Option<&event_log::RunState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> CompletionHead {
    let probe = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: String::new(),
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: None,
    };
    match transition_guard::validate_transition(run_state, &probe) {
        transition_guard::Verdict::Reject { reason } => CompletionHead::Reject {
            reason: reason.to_string(),
        },
        transition_guard::Verdict::NoOp { reason } => CompletionHead::NoOp { reason },
        transition_guard::Verdict::Allow => CompletionHead::Allow,
    }
}

/// The completion head for a **local skip**: identical to
/// [`evaluate_completion_head`] but the probe carries `skipped: true`, so a node
/// that never started is `Allow`ed — a skip legitimately satisfies a node stuck
/// waiting on an unreachable input.
pub(crate) fn evaluate_skip_completion_head(
    run_state: Option<&event_log::RunState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> CompletionHead {
    let probe = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: String::new(),
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "skipped": true })),
    };
    match transition_guard::validate_transition(run_state, &probe) {
        transition_guard::Verdict::Reject { reason } => CompletionHead::Reject {
            reason: reason.to_string(),
        },
        transition_guard::Verdict::NoOp { reason } => CompletionHead::NoOp { reason },
        transition_guard::Verdict::Allow => CompletionHead::Allow,
    }
}

/// Reload + re-project, then fire the just-completed producer's outgoing edges.
///
/// Don't pass the caller's projection through: `handle_node_completion` does not
/// re-project on its first pass, so it needs a fresh `events` slice.
async fn fire_edges(state: &AppState, run_id: &str, completed_node_id: &str) {
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("complete_node: failed to load events for {run_id}: {e}");
            return;
        }
    };
    let Some(run_state) = event_log::project(&events) else {
        return;
    };
    handle_node_completion(state, &run_state, run_id, completed_node_id, &events).await;
}

/// The shared post-`NodeCompleted` tail that drives a Run forward after one of
/// its nodes completes (ADR-0009).
///
/// PRECONDITION: the caller has already appended its `NodeCompleted` (plus any
/// companion events) and done any session reap. `completed_node_id` is the node
/// whose edges to fire — on the merge-resolver path that is the *original
/// conflicting node*, not the route's `__merge_resolver__` param, so it cannot be
/// re-derived from the request.
///
/// `retry_waiting_nodes` is cross-run on purpose: a freed session slot can start a
/// `waiting` node in another run. Never call this tail from an all-runs/waiting
/// sweep — see [`maybe_complete_run`]'s single-emitter rule.
pub(crate) async fn complete_node(
    state: &AppState,
    run_id: &str,
    completed_node_id: &str,
    order: CompletionOrder,
    complete_when_awaiting_user: bool,
) -> CompletionOutcome {
    match order {
        CompletionOrder::CompletionFirst => {
            fire_edges(state, run_id, completed_node_id).await;
            advance_run(state, run_id).await;
            retry_waiting_nodes(state).await;
        }
        CompletionOrder::SweepFirst => {
            advance_run(state, run_id).await;
            retry_waiting_nodes(state).await;
            fire_edges(state, run_id, completed_node_id).await;
        }
    }

    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("complete_node: failed to reload events for {run_id}: {e}");
            return CompletionOutcome::StillRunning;
        }
    };
    let Some(run_state) = event_log::project(&events) else {
        return CompletionOutcome::StillRunning;
    };
    if run_state.status == event_log::RunStatus::Halted {
        return CompletionOutcome::Halted;
    }
    let expected = expected_completion_node_ids(&run_state);
    if maybe_complete_run(
        state,
        run_id,
        &expected,
        &run_state,
        complete_when_awaiting_user,
    )
    .await
    {
        CompletionOutcome::RunCompleted
    } else {
        CompletionOutcome::StillRunning
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{
        IterationInfo, NodeDefInfo, NodeState, NodeStatus, RunState, RunStatus,
    };
    use crate::pipeline::{NodeDef, NodeType, PipelineDef, Port, PortType};
    use crate::scheduler_dispatcher::compute_ready_to_spawn;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn doc_node(id: &str) -> NodeDef {
        NodeDef {
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: "task".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        }
    }

    /// A pipeline of root `agent` nodes (no edges) — every node is immediately
    /// ready, so `compute_ready_to_spawn` reflects pure declaration order.
    fn roots_pipeline(ids: &[&str]) -> PipelineDef {
        PipelineDef {
            name: "roots".into(),
            version: None,
            variables: HashMap::new(),
            nodes: ids.iter().map(|id| doc_node(id)).collect(),
            edges: Vec::new(),
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        }
    }

    fn node_def_info(id: &str) -> NodeDefInfo {
        NodeDefInfo {
            isolated_worktree: None,
            id: id.into(),
            name: None,
            node_type: "agent".into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    fn completed_node(id: &str) -> NodeState {
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            skip_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    fn running_node(id: &str) -> NodeState {
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Running,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    #[test]
    fn ready_set_preserves_yaml_declaration_order() {
        // Declared NOT alphabetically: a HashSet or a re-sort would still pass a
        // laxer assertion, and would break who grabs the last slot under the cap.
        let pipeline = roots_pipeline(&["gamma", "alpha", "beta"]);
        let state = RunState::new("run-1".into(), "roots".into());

        let ready: Vec<String> = compute_ready_to_spawn(&pipeline, &state)
            .into_iter()
            .map(|r| r.node_id)
            .collect();

        assert_eq!(ready, vec!["gamma", "alpha", "beta"]);
    }

    #[test]
    fn expected_ids_prefer_node_defs_snapshot() {
        let mut state = RunState::new("run-1".into(), "p".into());
        state.node_defs = vec![node_def_info("a"), node_def_info("b")];
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("ghost".into(), running_node("ghost"));

        let mut ids = expected_completion_node_ids(&state);
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn expected_ids_fall_back_to_node_keys_when_no_snapshot() {
        let mut state = RunState::new("run-1".into(), "p".into());
        state.nodes.insert("x".into(), completed_node("x"));
        state.nodes.insert("y".into(), running_node("y"));

        let mut ids = expected_completion_node_ids(&state);
        ids.sort();
        assert_eq!(ids, vec!["x".to_string(), "y".to_string()]);
    }

    fn state_with(status: RunStatus, nodes: &[(&str, NodeState)]) -> RunState {
        let mut s = RunState::new("run-1".into(), "p".into());
        s.status = status;
        for (id, n) in nodes {
            s.nodes.insert((*id).into(), n.clone());
        }
        s
    }

    #[test]
    fn completes_when_running_and_all_expected_done() {
        let s = state_with(
            RunStatus::Running,
            &[("a", completed_node("a")), ("b", completed_node("b"))],
        );
        let expected = vec!["a".to_string(), "b".to_string()];
        assert!(should_complete_run(&s, &expected, false));
    }

    #[test]
    fn stays_running_when_work_remains() {
        let s = state_with(
            RunStatus::Running,
            &[("a", completed_node("a")), ("b", running_node("b"))],
        );
        let expected = vec!["a".to_string(), "b".to_string()];
        assert!(!should_complete_run(&s, &expected, false));
    }

    #[test]
    fn empty_expected_set_never_completes() {
        // Not vacuous-true: a run with no expected nodes is not "all done".
        let s = state_with(RunStatus::Running, &[]);
        assert!(!should_complete_run(&s, &[], false));
    }

    #[test]
    fn awaiting_user_does_not_complete_by_default() {
        let s = state_with(RunStatus::AwaitingUser, &[("a", completed_node("a"))]);
        let expected = vec!["a".to_string()];
        assert!(!should_complete_run(&s, &expected, false));
    }

    #[test]
    fn awaiting_user_completes_only_when_flag_set() {
        let s = state_with(RunStatus::AwaitingUser, &[("a", completed_node("a"))]);
        let expected = vec!["a".to_string()];
        assert!(should_complete_run(&s, &expected, true));
    }

    #[test]
    fn undone_collection_region_blocks_completion() {
        // A collection member can transiently project Completed mid-fan-out;
        // only the barrier unblocks run completion.
        let mut s = state_with(RunStatus::Running, &[("a", completed_node("a"))]);
        s.collection_states.insert(
            "fan".into(),
            event_log::CollectionState {
                region_id: "fan".into(),
                total_items: 3,
                entry: String::new(),
                members: Vec::new(),
                done: false,
            },
        );
        let expected = vec!["a".to_string()];
        assert!(!should_complete_run(&s, &expected, false));

        s.collection_states.get_mut("fan").unwrap().done = true;
        assert!(should_complete_run(&s, &expected, false));
    }

    #[test]
    fn terminal_status_never_completes_even_when_all_done() {
        for status in [RunStatus::Completed, RunStatus::Failed, RunStatus::Halted] {
            let s = state_with(status.clone(), &[("a", completed_node("a"))]);
            let expected = vec!["a".to_string()];
            assert!(
                !should_complete_run(&s, &expected, true),
                "status {status:?} must not re-complete"
            );
        }
    }

    /// `validate_completion` reads `iterations[]`, never node-level `status`.
    fn node_iter(id: &str, iter: i64, status: NodeStatus) -> NodeState {
        let completed_at = (status == NodeStatus::Completed).then(|| "t1".to_string());
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: status.clone(),
            iter,
            started_at: Some("t0".into()),
            completed_at: completed_at.clone(),
            failure_reason: None,
            skip_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status,
                started_at: Some("t0".into()),
                completed_at,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    #[test]
    fn completion_head_allows_running_iteration() {
        let s = state_with(
            RunStatus::Running,
            &[("n", node_iter("n", 1, NodeStatus::Running))],
        );
        assert!(matches!(
            evaluate_completion_head(Some(&s), "run-1", "n", 1),
            CompletionHead::Allow
        ));
    }

    #[test]
    fn completion_head_noops_duplicate_completion() {
        let s = state_with(
            RunStatus::Running,
            &[("n", node_iter("n", 1, NodeStatus::Completed))],
        );
        assert!(matches!(
            evaluate_completion_head(Some(&s), "run-1", "n", 1),
            CompletionHead::NoOp { .. }
        ));
    }

    #[test]
    fn completion_head_rejects_never_started() {
        let s = state_with(RunStatus::Running, &[]);
        assert!(matches!(
            evaluate_completion_head(Some(&s), "run-1", "n", 1),
            CompletionHead::Reject { .. }
        ));
    }

    #[test]
    fn completion_head_rejects_on_non_running_run() {
        let s = state_with(
            RunStatus::Halted,
            &[("n", node_iter("n", 1, NodeStatus::Running))],
        );
        assert!(matches!(
            evaluate_completion_head(Some(&s), "run-1", "n", 1),
            CompletionHead::Reject { .. }
        ));
    }

    #[test]
    fn completion_head_allows_when_no_projected_state() {
        assert!(matches!(
            evaluate_completion_head(None, "run-1", "n", 1),
            CompletionHead::Allow
        ));
    }
}
