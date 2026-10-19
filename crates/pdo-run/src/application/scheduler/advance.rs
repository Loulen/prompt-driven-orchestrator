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

use crate::domain::{event, projection, state};
use tracing::{error, info};

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::node::spawn::{spawn_node, SpawnContext, SpawnDeps};
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::scheduler::frontmatter::resolve_completed_frontmatter;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::scheduler::interpreter::{self as scheduler_interpreter, SpawnDedup};
use crate::domain::scheduler;
use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
use crate::Runs;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

pub use super::children_binding::*;
pub use super::completion_head::*;

/// Advance one Run by a single tick: spawn whatever the scheduler says is ready
/// (plus any pending loop-iteration seeds), or — when there is nothing left to
/// spawn — complete the Run if every expected node is done.
///
/// A no-op unless the run is `Running` or `AwaitingUser`.
pub async fn advance_run(state: &Runs, run_id: &str) {
    // Must run before the readiness sweep: a structurally-unreachable node would
    // otherwise sit forever and the run would never reach "all expected nodes done".
    sweep_auto_skips(state, run_id).await;

    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("advance_run: failed to load events for {run_id}: {e}");
            return;
        }
    };
    let Some(run_state) = projection::project(&events) else {
        return;
    };

    if run_state.status != state::RunStatus::Running
        && run_state.status != state::RunStatus::AwaitingUser
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
        //
        // ADR-0086: an on-demand node never launched is not expected — the Run
        // does not wait for a node only the user may launch.
        let pipeline_node_ids: Vec<String> = pipeline
            .nodes
            .iter()
            .filter(|n| !crate::domain::on_demand::is_unlaunched(&pipeline, &run_state, &n.id))
            .map(|n| n.id.clone())
            .collect();
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
pub(super) async fn sweep_auto_skips(state: &Runs, run_id: &str) {
    let mut passes = 0usize;
    loop {
        let events = match load_events(state, run_id).await {
            Ok(e) => e,
            Err(_) => return,
        };
        let Some(run_state) = projection::project(&events) else {
            return;
        };
        if run_state.status != state::RunStatus::Running
            && run_state.status != state::RunStatus::AwaitingUser
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

        let skipped_ids = append_auto_skips(state, run_id, &pipeline, &artifacts_dir, &skips).await;

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

/// Skip each of `skips` — `(node_id, reason)` as [`scheduler::unreachable_nodes`]
/// returns them — with an empty output, so a downstream resolver finds a
/// concrete artifact rather than a missing file. Returns the nodes skipped.
/// Fires no edge: that is the caller's business.
pub(crate) async fn append_auto_skips(
    state: &Runs,
    run_id: &str,
    pipeline: &pipeline::PipelineDef,
    artifacts_dir: &std::path::Path,
    skips: &[(String, String)],
) -> Vec<String> {
    let no_overrides = std::collections::HashMap::new();
    let mut skipped_ids = Vec::new();
    for (node_id, reason) in skips {
        // A structurally-unreachable node never started, so it skips at iter 1.
        let iter = 1;
        if let Err(e) = crate::application::node::primitives::write_skip_outputs(
            pipeline,
            node_id,
            iter,
            &no_overrides,
            artifacts_dir,
        ) {
            error!("auto-skip: failed to write outputs for {node_id} in {run_id}: {e}");
            continue;
        }
        let event = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeCompleted,
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
    skipped_ids
}

/// Spawn each node in `ready_set` through [`spawn_node`].
///
/// Don't re-sort or de-duplicate `ready_set`: under the session cap its order
/// decides who grabs the last free slot.
pub async fn spawn_each(
    state: &Runs,
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
///
/// An on-demand node never launched is not expected (ADR-0086).
pub fn expected_completion_node_ids(run_state: &state::RunState) -> Vec<String> {
    if !run_state.node_defs.is_empty() {
        run_state
            .node_defs
            .iter()
            .filter(|nd| !nd.on_demand || run_state.nodes.contains_key(&nd.id))
            .map(|nd| nd.id.clone())
            .collect()
    } else {
        run_state.nodes.keys().cloned().collect()
    }
}

/// Pure decision: should a `RunCompleted` be emitted for this projected state?
///
/// `complete_when_awaiting_user` exists for `mark_node_done`, where the
/// just-finished node was interactive so the run still projects `AwaitingUser` at
/// the completion check. Every other caller permits only `Running`.
pub fn should_complete_run(
    run_state: &state::RunState,
    expected_node_ids: &[String],
    complete_when_awaiting_user: bool,
) -> bool {
    let status_permits = run_state.status == state::RunStatus::Running
        || (complete_when_awaiting_user && run_state.status == state::RunStatus::AwaitingUser);
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
pub async fn maybe_complete_run(
    state: &Runs,
    run_id: &str,
    expected_node_ids: &[String],
    run_state: &state::RunState,
    complete_when_awaiting_user: bool,
) -> bool {
    if !should_complete_run(run_state, expected_node_ids, complete_when_awaiting_user) {
        return false;
    }
    let run_completed = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::RunCompleted,
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

// ─ Liaison forte orchestrateur ↔ enfants (#724, ADR-0064) ────────────────────
//
// Le NodeRun d’un nœud « orchestrator » (toggle gelé au démarrage du Run,
// `NodeDefInfo::orchestrator`) ne se termine pas pendant que ses runs enfants
// (mêmes `parent_run_id` + `parent_node_id`, ADR-0064) sont non terminaux :
//
// - la **porte** ([`orchestrator_binding_refusal`]) refuse `pdo complete` tant
//   que des enfants tournent, avec un message qui compte ; au moins un enfant
//   `failed` parque le nœud `AwaitingUser` — l’agent a rendu la main, c’est
//   l’utilisateur qui tranche (retry de l’enfant ou forçage `mark_node_done`,
//   qui n’est PAS gated : c’est le forçage) — et pose le **pilote**
//   ([`spawn_children_settled_watcher`]) qui complétera le nœud de lui-même
//   quand tous les enfants seront terminaux — « le nœud se complète ».
//
// Aucune propagation de mort vers le bas : la liaison ne fait que RETENIR le
// parent ; stop du nœud, archive et forget du parent ne touchent aucun enfant,
// et un restart ré-adopte les enfants vivants (ils sont relus à chaque verdict,
// jamais mis en cache).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::NodeDefInfo;
    use crate::domain::scheduler::dispatcher::compute_ready_to_spawn;
    use crate::domain::state::IterationInfo;
    use crate::domain::state::NodeState;
    use crate::domain::state::NodeStatus;
    use crate::domain::state::RunState;
    use crate::domain::state::RunStatus;
    use pdo_pipeline::domain::pipeline::{NodeDef, NodeType, PipelineDef, Port, PortType};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn doc_node(id: &str) -> NodeDef {
        NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: "task".into(),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
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
            orchestrator: false,
            on_demand: false,
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
            grid_size: None,
            wait_for_started_nodes: false,
        }
    }

    fn node_def_info(id: &str) -> NodeDefInfo {
        NodeDefInfo {
            isolated_worktree: None,
            orchestrator: false,
            on_demand: false,
            id: id.into(),
            name: None,
            node_type: "agent".into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    // ─ Liaison forte (#724) : le verdict est pur, il se teste sans daemon ──

    fn snap(total: usize, failed: &[&str], active: &[&str]) -> ChildrenSnapshot {
        ChildrenSnapshot {
            total,
            failed: failed.iter().map(|s| s.to_string()).collect(),
            active: active.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn binding_verdicts_follow_the_contract() {
        // Pas d'enfant : la liaison ne dit rien — un toggle sans spawn, ou un
        // agent qui n'a pas encore créé ses enfants, complète normalement.
        assert_eq!(snap(0, &[], &[]).verdict(), ChildrenVerdict::NoChildren);
        // Tous terminaux (completed/skipped/…), aucun failed → complétion.
        assert_eq!(snap(2, &[], &[]).verdict(), ChildrenVerdict::AllSettled);
        // Un enfant actif (running / awaiting_user / paused) → le nœud attend.
        assert_eq!(
            snap(2, &[], &["c1"]).verdict(),
            ChildrenVerdict::Pending {
                active: 1,
                failed: 0
            }
        );
        // Un enfant failed (terminal) → arbitrage utilisateur, mais les autres
        // enfants actifs comptent toujours dans l'attente.
        assert_eq!(
            snap(3, &["c1"], &["c2"]).verdict(),
            ChildrenVerdict::Pending {
                active: 1,
                failed: 1
            }
        );
        // Tous échoués : plus rien d'actif, mais le verdict reste un refus.
        assert_eq!(
            snap(2, &["c1", "c2"], &[]).verdict(),
            ChildrenVerdict::Pending {
                active: 0,
                failed: 2
            }
        );
    }

    #[test]
    fn binding_marker_is_detected_on_the_right_iteration_only() {
        let marker = event::Event {
            id: None,
            run_id: "r".into(),
            ts: "t".into(),
            kind: event::EventKind::NodeAwaitingUser,
            node_id: Some("orch".into()),
            iter: Some(2),
            payload: Some(serde_json::json!({ "reason": BINDING_MARKER_REASON })),
        };
        // Le NodeAwaitingUser d'un nœud interactif (payload vide) n'est pas un
        // marqueur : la liaison ne doit pas le confondre avec son attente.
        let interactive_spawn = event::Event {
            kind: event::EventKind::NodeAwaitingUser,
            payload: None,
            ..marker.clone()
        };
        assert!(binding_marker_present(
            std::slice::from_ref(&marker),
            "orch",
            2
        ));
        assert!(!binding_marker_present(
            std::slice::from_ref(&marker),
            "orch",
            1
        ));
        assert!(!binding_marker_present(
            std::slice::from_ref(&marker),
            "other",
            2
        ));
        assert!(!binding_marker_present(&[interactive_spawn], "orch", 2));
    }

    fn completed_node(id: &str) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
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
            awaiting: None,
        }
    }

    fn running_node(id: &str) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
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
            awaiting: None,
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

    /// ADR-0086: the Run does not wait for an on-demand node nobody launched;
    /// once launched, it is expected like any node.
    #[test]
    fn expected_ids_leave_out_an_on_demand_node_never_launched() {
        let mut state = RunState::new("run-1".into(), "p".into());
        let mut review = node_def_info("review");
        review.on_demand = true;
        state.node_defs = vec![node_def_info("a"), review];
        state.nodes.insert("a".into(), completed_node("a"));

        assert_eq!(expected_completion_node_ids(&state), vec!["a".to_string()]);
        assert!(should_complete_run(
            &state,
            &expected_completion_node_ids(&state),
            false
        ));

        state.nodes.insert("review".into(), running_node("review"));
        let mut ids = expected_completion_node_ids(&state);
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "review".to_string()]);
        assert!(!should_complete_run(&state, &ids, false));
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
            state::CollectionState {
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
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
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
                interactive: false,
                completion_released: false,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
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
