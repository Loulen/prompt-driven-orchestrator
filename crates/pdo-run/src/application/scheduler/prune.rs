//! Pruning: the nodes whose branch was not taken are settled `Skipped`, dead
//! for routing (ADR-0082), and what only they held back is released.
//!
//! Like [`super::advance`], never re-entrant (ADR-0009): a linear sequence over
//! the pure `scheduler` evaluators, `append_event` and the one interpreter.

use crate::domain::{event, projection, state};
use tracing::{error, info};

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::node::spawn::SpawnContext;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::scheduler::frontmatter::resolve_completed_frontmatter;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::scheduler::interpreter::{
    self as scheduler_interpreter, ActionOutcome, SpawnDedup,
};
use crate::application::scheduler::node_completion::settle_loops_collections_and_end;
use crate::domain::scheduler;
use crate::Runs;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// Prune every node that has become **structurally unreachable** — its
/// producing branch was not taken, so nothing will ever spawn it (ADR-0011).
/// Each pruned node is settled `Skipped` with an *empty output* on disk, but it
/// is dead for routing (ADR-0082): it fires no edge, so an `End` reachable only
/// through it stays unreached and a Run parked `unrouted` stays parked. What it
/// fed is released by the routing of its live siblings
/// ([`scheduler::released_by_pruning`]).
///
/// Must iterate: pruning one node can render a downstream either/or dead in
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
        let spawn_ctx = SpawnContext {
            pipeline: &pipeline,
            run_id,
            pipeline_path: &pipeline_path,
            worktree_dir: &worktree_dir,
            artifacts_dir: &artifacts_dir,
            resolved_vars: &resolved_vars,
            repo_root: &repo_root,
        };
        if release_pruned(state, &spawn_ctx, &skipped_ids).await {
            return;
        }
    }
}

/// Drive on what the pruning of `pruned` released: the convergences — `End`
/// included — that waited only on them, reached by their live producers'
/// routing ([`scheduler::released_by_pruning`]); then the loops, collection
/// barriers and end a settled node may close, as after a completion. Returns
/// whether the Run left the sweep's hands (completed, halted, parked).
async fn release_pruned(state: &Runs, ctx: &SpawnContext<'_>, pruned: &[String]) -> bool {
    let Ok(events) = load_events(state, ctx.run_id).await else {
        return true;
    };
    let Some(run_state) = projection::project(&events) else {
        return true;
    };
    let frontmatter_by_node =
        resolve_completed_frontmatter(ctx.pipeline, &run_state, ctx.artifacts_dir);
    let actions = scheduler::released_by_pruning(
        ctx.pipeline,
        &run_state,
        pruned,
        &frontmatter_by_node,
        ctx.resolved_vars,
    );
    for action in &actions {
        // `GuardSuperfluous`: release only MISSING work, never a node already
        // launched by another path.
        match scheduler_interpreter::interpret(
            state,
            ctx,
            &run_state,
            SpawnDedup::GuardSuperfluous,
            1,
            action,
        )
        .await
        {
            ActionOutcome::Completed
            | ActionOutcome::Halted { .. }
            | ActionOutcome::Interrupted { .. } => return true,
            ActionOutcome::Spawned { .. }
            | ActionOutcome::Progressed
            | ActionOutcome::SpawnSkipped { .. } => {}
        }
    }
    settle_loops_collections_and_end(state, ctx, 1).await;
    false
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
                "source": state::AUTO_SKIP_UNREACHABLE_SOURCE,
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
