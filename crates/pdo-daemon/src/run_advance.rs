//! Run advancement — the single-pass "tick" that drives a live Run forward.
//!
//! Issue #235: the sequence *load events → [`event_log::project`] → compute what
//! is ready → [`spawn_node`] → maybe complete the Run* was re-implemented inline
//! at several call sites in `lib.rs`. This module owns that single-pass tick
//! behind one entry point — [`advance_run`] — so the sequence lives in exactly
//! one place.
//!
//! Scope (slice-1): only the **single-pass** family is consolidated here. The
//! two-pass *edge-firing* site `handle_node_completion`'s **body** still lives in
//! `lib.rs` with its load-bearing `reload_run_state` re-projection, deferred to a
//! tracked follow-up (no integration backstop yet — #235 plan, §5 follow-up A).
//!
//! Node-completion HEAD + TAIL (#275, #354): this module owns both halves of
//! node completion.
//!
//! - The **tail** [`complete_node`] (#275) is the shared post-`NodeCompleted`
//!   sequence the node-done sites used to re-implement inline: it composes the
//!   deferred two-pass HNC site (`handle_node_completion`, still bodied in
//!   `lib.rs`) with [`advance_run`], the cross-run `retry_waiting_nodes`, and the
//!   single completion gate.
//! - The **head** [`evaluate_completion_head`] (#354) is the shared *pure*
//!   decision the three guard-driven completion sites (`node_done`, the
//!   `mark_node_done` command arm, `node_skip`) used to copy: build the
//!   `NodeCompleted` guard probe → [`transition_guard::validate_transition`] →
//!   reject (409) / no-op (200) / allow. It touches no DB and no tmux, so every
//!   caller runs it identically; the async, side-effecting checks stay caller-side
//!   (`run_is_forgotten`, #328; the sub-worktree merge + `check_output_validation_with_retry`,
//!   #36). `handle_merge_resolver_done` has no transition-guard head and stays a
//!   tail-only caller.
//!
//! Both are Layer-3 convenience seams (ADR-0009): pure head, side-effecting tail.
//! Sharing the head does **not** collapse the per-caller tail divergence ratified
//! by ADR-0023 — `node_done` detaches its tail ([`CompletionOrder::CompletionFirst`]),
//! the `mark_node_done` arm runs its tail inline ([`CompletionOrder::SweepFirst`],
//! no self-reap), and `node_skip` ends the run as `Skipped` without a tail advance.
//!
//! Non-reentrancy (ADR-0009 / #122): [`advance_run`] calls [`spawn_node`], the
//! pure `scheduler*` evaluators, and `append_event`/`emit_*` in a **linear**
//! sequence. It never calls another advancement helper or itself, and never
//! wires scheduler-driving code onto `event_tx`. [`spawn_node`] stays a leaf.
//! [`complete_node`] is likewise linear and non-reentrant: it runs HNC + the sweep
//! + retry once, then the single completion gate — never from an all-runs sweep.

use tracing::{error, info};

use crate::event_log;
use crate::pipeline;
use crate::scheduler;
use crate::scheduler_dispatcher;
use crate::transition_guard;
use crate::worktree_ops::worktree_dir_for_run;
use crate::{
    append_event, effective_repo_root, emit_loop_action, handle_node_completion, load_events,
    resolve_run_pipeline_path, resolve_run_variables, retry_waiting_nodes, spawn_node, AppState,
    SpawnContext,
};

/// Advance one Run by a single tick: spawn whatever the scheduler says is ready
/// (plus any pending loop-iteration seeds), or — when there is nothing left to
/// spawn — complete the Run if every expected node is done.
///
/// A no-op unless the run is `Running` or `AwaitingUser`. This is the canonical
/// body the inline `spawn_ready_after_event` used to carry; every former call
/// site reaches it (directly or through the `spawn_ready_after_event` shim).
pub(crate) async fn advance_run(state: &AppState, run_id: &str) {
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
        // Pipeline was modified but no new nodes need spawning. If all current
        // pipeline nodes are completed, re-complete the run so it doesn't stay
        // dangling in Running state after a trivial YAML edit. The expected set
        // here is the *current* pipeline (post-modification), not the run's
        // frozen snapshot — that is why this site derives ids from
        // `pipeline.nodes` rather than [`expected_completion_node_ids`].
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
        match action {
            scheduler::SchedulerAction::LoopIterStarted { .. } => {
                emit_loop_action(state, run_id, action).await;
            }
            scheduler::SchedulerAction::Spawn { node_id, iter } => {
                if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                    spawn_node(state, &spawn_ctx, node, *iter).await;
                }
            }
            _ => {}
        }
    }

    info!(
        "advance_run: spawned {} node(s) and seeded {} loop action(s) for run {run_id}",
        ready.len(),
        loop_seed_actions.len()
    );
}

/// Spawn each node in `ready_set` (in the order given) through [`spawn_node`].
///
/// Shared by [`advance_run`] (ready set = [`scheduler_dispatcher::compute_ready_to_spawn`])
/// and `retry_waiting_nodes` (ready set = [`scheduler_dispatcher::waiting_nodes`]).
/// The caller-supplied order is honoured verbatim — under the session cap it
/// decides who grabs the last free slot, so it must not be re-sorted here.
/// `spawn_node` re-checks admission per node, so a node that still can't get a
/// slot simply stays `Waiting`.
pub(crate) async fn spawn_each(
    state: &AppState,
    spawn_ctx: &SpawnContext<'_>,
    ready_set: &[scheduler_dispatcher::ReadySpawn],
) {
    for rs in ready_set {
        if let Some(node) = spawn_ctx.pipeline.nodes.iter().find(|n| n.id == rs.node_id) {
            spawn_node(state, spawn_ctx, node, rs.iter).await;
        }
    }
}

/// The set of node ids that must all be `Completed` for the Run to be done, as
/// seen from a *node-done* site.
///
/// Prefer the run's own `node_defs` snapshot (frozen at run start, so a mid-run
/// YAML edit can't change what "all done" means for an in-flight run); fall back
/// to whatever nodes have appeared in the log when no snapshot exists (legacy
/// runs). This reproduces the inline derivation the node-done / mark-node-done /
/// merge-resolver-done sites used before consolidation.
///
/// NB: [`advance_run`]'s own completion branch deliberately uses a *different*
/// set (`pipeline.nodes`, the post-modification pipeline) — see its comment.
pub(crate) fn expected_completion_node_ids(run_state: &event_log::RunState) -> Vec<String> {
    if !run_state.node_defs.is_empty() {
        run_state.node_defs.iter().map(|nd| nd.id.clone()).collect()
    } else {
        run_state.nodes.keys().cloned().collect()
    }
}

/// Pure decision: should a `RunCompleted` be emitted for this projected state?
///
/// True iff the run is in a completion-permitting status **and** every expected
/// node is `Completed`. The permitted status is `Running`; setting
/// `complete_when_awaiting_user` widens it to also include `AwaitingUser` — the
/// `mark_node_done` path, where the just-finished node was interactive so the
/// run can still project as `AwaitingUser` at the completion check. Every other
/// caller permits only `Running`.
///
/// Pure over the projected `RunState` (AC#4: state in → decision out, no HTTP),
/// so the gate is unit-tested directly below.
pub(crate) fn should_complete_run(
    run_state: &event_log::RunState,
    expected_node_ids: &[String],
    complete_when_awaiting_user: bool,
) -> bool {
    let status_permits = run_state.status == event_log::RunStatus::Running
        || (complete_when_awaiting_user && run_state.status == event_log::RunStatus::AwaitingUser);
    // A collection region mid-fan-out (ADR-0011 / #269) is unfinished work even
    // when every node projects `Completed`: node-level status reflects the
    // LATEST event, so a member whose lap 1 finished while laps 2..N are still
    // running can transiently project Completed. The barrier (CollectionDone)
    // is the only truthful "region finished" signal.
    let collections_done = run_state.collection_states.values().all(|cs| cs.done);
    status_permits && collections_done && run_state.all_nodes_completed(expected_node_ids)
}

/// Emit exactly one `RunCompleted` if [`should_complete_run`] says so; returns
/// whether it emitted.
///
/// The single home for run-completion emission on the single-pass paths: the
/// `advance_run` "nothing ready" branch and the shared node-done tail
/// [`complete_node`] (reached by `node_done`, the `mark_node_done` command arm,
/// and `handle_merge_resolver_done`) all route here. `append_event` does **not**
/// de-dup `RunCompleted`, so this must stay the only completion emitter on these
/// paths — never call it from an all-runs/waiting sweep. The returned `bool`
/// makes the single-`RunCompleted` invariant directly observable to
/// [`complete_node`] without a re-projection; `advance_run` ignores it.
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
/// Behavior-equivalent on the final state today — HNC and `advance_run` cover
/// disjoint spawn sets (HNC fires the just-completed producer's conditional /
/// loop / foreach edges; `advance_run` spawns only unconditionally-ready nodes,
/// its `ready_nodes` set explicitly skipping Switch/Loop/ForEach and any node
/// already present), `spawn_node` re-validates each transition before any side
/// effect (a duplicate `NodeStarted` is a NoOp), and `all_nodes_completed`
/// requires the *full* expected set — so neither order can re-fire the other's
/// spawns nor complete the run early. The two orders are nonetheless preserved
/// per-caller, so the #275 extraction is a strictly behavior-preserving carve
/// auditable by diff. Collapsing to one variant is #235 follow-up A (needs the
/// order-equivalence integration test first).
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
    /// The run advanced but not all expected nodes are done yet (or it completed
    /// earlier in the same tail via an HNC `Complete`/`Halt` action — either way
    /// the completion gate emitted nothing).
    StillRunning,
    /// The run projects as `Halted`; no completion emitted (`node_done`'s
    /// short-circuit, now uniform across callers — see [`complete_node`]).
    Halted,
}

/// The three-way outcome of the pure completion **head** decision (#354).
///
/// Mirrors [`transition_guard::Verdict`] at the run-advance layer so callers
/// match on a completion-specific type rather than the guard's internal enum:
/// - [`CompletionHead::Reject`]  → the caller returns `409 CONFLICT { error }`.
/// - [`CompletionHead::NoOp`]    → the caller returns `200 { ok, noop, reason }`.
/// - [`CompletionHead::Allow`]   → the caller runs its own side effects, appends
///   its own `NodeCompleted`, and drives its own tail.
pub(crate) enum CompletionHead {
    Reject { reason: String },
    NoOp { reason: String },
    Allow,
}

/// The shared, **pure** *head* of node completion (#354): the guard-verdict
/// decision that `node_done`, the `mark_node_done` command arm, and `node_skip`
/// used to copy verbatim.
///
/// Builds the `NodeCompleted` guard probe for `(run_id, node_id, iter)`, runs it
/// through [`transition_guard::validate_transition`] against the projected
/// `RunState`, and maps the verdict to [`CompletionHead`].
///
/// Pure: no DB, no tmux, no append, no clock dependence — the guard ignores
/// `Event::ts`, so the probe carries an empty `ts`. That purity is what lets all
/// three callers share it and what makes it unit-testable directly. It decides
/// only *whether* the completion is legal; it builds no completion event and runs
/// no tail. The async / side-effecting neighbours stay caller-side by design:
/// `run_is_forgotten` (async DB, #328) runs *before* this head; the sub-worktree
/// merge / doc-cleanliness check and `check_output_validation_with_retry` (async +
/// in-session nudge, #36) run *after* an `Allow`, before the caller's append.
///
/// `run_state` is forwarded verbatim: `None` (no projected state) yields the
/// guard's `Allow` — the caller decides whether to admit `None` (node_done and
/// node_skip 404 on it *before* calling this; mark forwards it).
pub(crate) fn evaluate_completion_head(
    run_state: Option<&event_log::RunState>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> CompletionHead {
    let probe = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: String::new(), // guard is ts-blind; keep the seam clock-free
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: None,
    };
    match transition_guard::validate_transition(run_state, &probe) {
        transition_guard::Verdict::Reject { reason } => CompletionHead::Reject { reason },
        transition_guard::Verdict::NoOp { reason } => CompletionHead::NoOp { reason },
        transition_guard::Verdict::Allow => CompletionHead::Allow,
    }
}

/// Reload + re-project, then fire the just-completed producer's outgoing edges
/// via [`handle_node_completion`].
///
/// HNC needs a *fresh* `events` slice + `RunState` (its first pass re-projects
/// nothing itself), so this reloads rather than trusting a stale caller
/// projection — matching what each node-done site did inline before #275. A load
/// failure is logged and skipped (same as the inline sites' `else { return }`).
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

/// Layer-3 convenience command (ADR-0009): the shared post-`NodeCompleted` tail
/// that drives a Run forward after one of its nodes completes.
///
/// PRECONDITION (the **post-append seam**): the caller has already appended its
/// `NodeCompleted` (and any bespoke companion events — e.g. `mark_node_done`'s
/// `source` payload + `CommandIssued`) and done any session reap. `completed_node_id`
/// is the node whose edges to fire; for the merge-resolver path this is the
/// *original conflicting node*, not the route's `__merge_resolver__` param, which
/// is why it cannot be re-derived from the request.
///
/// Linear, non-reentrant: fire the producer's edges + the readiness sweep
/// (`advance_run`) + the cross-run `retry_waiting_nodes` (a freed session slot can
/// start a `waiting` node in another run, #159), in the requested `order`, then a
/// single reload → Halted short-circuit → the single completion gate
/// ([`maybe_complete_run`], the only `RunCompleted` emitter here). Never call it
/// from an all-runs/waiting sweep (single-emitter rule).
///
/// The Halted short-circuit is uniform across all three callers: only `node_done`
/// short-circuited before, but `maybe_complete_run` already no-ops on a terminal
/// status, so returning [`CompletionOutcome::Halted`] for the merge / mark paths
/// emits no `RunCompleted` either way — behavior-preserving.
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

    // Single reload → project → Halted short-circuit → single completion gate.
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

    // --- fixtures -----------------------------------------------------------

    fn doc_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: vec![Port {
                name: "task".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            model: None,
        }
    }

    /// A pipeline of root DocOnly nodes (no edges) — every node is immediately
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
            id: id.into(),
            name: None,
            node_type: "doc-only".into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    fn completed_node(id: &str) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn running_node(id: &str) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Running,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    // --- ready-set order (AC#4: state in -> spawn order out) ---------------

    #[test]
    fn ready_set_preserves_yaml_declaration_order() {
        // Declared gamma, alpha, beta (NOT alphabetical). The spawn order must
        // follow YAML declaration order — it decides who grabs the last free
        // slot under the session cap, so a HashSet/re-sort would be a regression.
        let pipeline = roots_pipeline(&["gamma", "alpha", "beta"]);
        let state = RunState::new("run-1".into(), "roots".into());

        let ready: Vec<String> = compute_ready_to_spawn(&pipeline, &state)
            .into_iter()
            .map(|r| r.node_id)
            .collect();

        assert_eq!(ready, vec!["gamma", "alpha", "beta"]);
    }

    // --- expected-id derivation (protects the STEP-4 dup collapse) ---------

    #[test]
    fn expected_ids_prefer_node_defs_snapshot() {
        // node_defs present -> authoritative, even when extra nodes leaked into
        // the live `nodes` map. This is the frozen run snapshot.
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

    // --- completion gate (AC#4: state in -> complete? out, no HTTP) --------

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
        // all_nodes_completed is false on an empty set (not vacuous-true): a run
        // with no expected nodes is not "all done".
        let s = state_with(RunStatus::Running, &[]);
        assert!(!should_complete_run(&s, &[], false));
    }

    #[test]
    fn awaiting_user_does_not_complete_by_default() {
        // The single-pass / merge-resolver / node_done sites permit ONLY Running.
        let s = state_with(RunStatus::AwaitingUser, &[("a", completed_node("a"))]);
        let expected = vec!["a".to_string()];
        assert!(!should_complete_run(&s, &expected, false));
    }

    #[test]
    fn awaiting_user_completes_only_when_flag_set() {
        // The mark_node_done command arm (interactive node just finished) opts in.
        let s = state_with(RunStatus::AwaitingUser, &[("a", completed_node("a"))]);
        let expected = vec!["a".to_string()];
        assert!(should_complete_run(&s, &expected, true));
    }

    #[test]
    fn undone_collection_region_blocks_completion() {
        // ADR-0011 / #269: node status is the LATEST event, so a collection
        // member can transiently project Completed mid-fan-out. Only the
        // barrier (`done`) unblocks run completion.
        let mut s = state_with(RunStatus::Running, &[("a", completed_node("a"))]);
        s.collection_states.insert(
            "fan".into(),
            event_log::CollectionState {
                region_id: "fan".into(),
                total_items: 3,
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

    // --- completion head (#354): state in -> guard verdict out, no HTTP -----

    /// A node carrying a single iteration row — the shape `validate_completion`
    /// actually reads (it consults `iterations[]`, never node-level `status`).
    fn node_iter(id: &str, iter: i64, status: NodeStatus) -> NodeState {
        let completed_at = (status == NodeStatus::Completed).then(|| "t1".to_string());
        NodeState {
            node_id: id.into(),
            status: status.clone(),
            iter,
            started_at: Some("t0".into()),
            completed_at: completed_at.clone(),
            failure_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status,
                started_at: Some("t0".into()),
                completed_at,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
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
        // Node present but no iteration row for iter 1 -> "never started".
        let s = state_with(RunStatus::Running, &[]);
        assert!(matches!(
            evaluate_completion_head(Some(&s), "run-1", "n", 1),
            CompletionHead::Reject { .. }
        ));
    }

    #[test]
    fn completion_head_rejects_on_non_running_run() {
        // Run-status gate fires before the iteration is inspected.
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
        // mark_node_done's None path: guard maps None -> Allow, forwarded verbatim.
        assert!(matches!(
            evaluate_completion_head(None, "run-1", "n", 1),
            CompletionHead::Allow
        ));
    }
}
