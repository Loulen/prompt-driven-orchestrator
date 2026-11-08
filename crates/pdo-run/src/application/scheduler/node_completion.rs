//! The scheduler after a node's completion, and the retry of the nodes the
//! session cap holds `waiting`.

use tracing::warn;

use crate::application::events::load_all_run_ids;
use crate::application::events::reload_run_state;
use crate::application::node::spawn::SpawnContext;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_pipeline_path;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::paths::run_scoped_pipeline_path;
use crate::application::scheduler::frontmatter::resolve_completed_frontmatter;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::scheduler::frontmatter::resolve_source_frontmatter;
use crate::application::scheduler::interpreter::{ActionOutcome, SpawnDedup};
use crate::application::scheduler::{advance as run_advance, interpreter as scheduler_interpreter};
use crate::domain::scheduler;
use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
use crate::domain::{event, state};
use crate::Runs;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

pub async fn handle_node_completion(
    state: &Runs,
    run_state: &state::RunState,
    run_id: &str,
    completed_node_id: &str,
    events: &[event::Event],
) {
    let repo_root = effective_repo_root(state, run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };

    let pipeline = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let resolved_vars = resolve_run_variables(&pipeline, events);

    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);
    let frontmatter_fields =
        resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

    // Per-node frontmatter for EVERY completed producer, not just the source: it is
    // what lets convergence suppression (ADR-0011) re-evaluate upstream conditional
    // edges and avoid a silent stall at a `Merge` fed by a suppressed branch.
    let frontmatter_by_node = resolve_completed_frontmatter(&pipeline, run_state, &artifacts_dir);

    // The Coût de boucle is the scheduler's input for a capped region.
    let scheduler_state =
        super::with_loop_costs_for_caps(state, run_state, events, &pipeline).await;
    let actions = scheduler::evaluate_outgoing_edges_full(
        &pipeline,
        &scheduler_state,
        completed_node_id,
        &resolved_vars,
        &frontmatter_fields,
        &frontmatter_by_node,
    );

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    // Pass 1 runs on the caller's `run_state` snapshot under `InternalOnly`: the
    // completion path applies NO scheduler-side dedup, because a same-iter-live
    // spawn is a legal restart inside `spawn_node`'s own guard.
    for action in &actions {
        match scheduler_interpreter::interpret(
            state,
            &spawn_ctx,
            run_state,
            SpawnDedup::InternalOnly,
            source_iter,
            action,
        )
        .await
        {
            // A terminal action unwinds the WHOLE driver, not just this loop.
            ActionOutcome::Completed
            | ActionOutcome::Halted { .. }
            | ActionOutcome::Interrupted { .. } => return,
            // `SpawnSkipped` is unreachable under `InternalOnly`.
            ActionOutcome::Spawned { .. }
            | ActionOutcome::Progressed
            | ActionOutcome::SpawnSkipped { .. } => {}
        }
    }

    // Pass 1 may have appended events (e.g. LoopBreakReceived). Re-project, or
    // `evaluate_loop_body_completion` races its own predecessors and advances the
    // loop one extra iteration after a break.
    let Some((fresh_events, fresh_run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    let fresh_resolved_vars = resolve_run_variables(&pipeline, &fresh_events);
    let fresh_spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &fresh_resolved_vars,
        repo_root: &repo_root,
    };

    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Loop)
    {
        let loop_actions = scheduler::evaluate_loop_body_completion(
            &pipeline,
            &fresh_run_state,
            &loop_node.id,
            &fresh_resolved_vars,
        );
        // Pass 2 runs on the reloaded `fresh_run_state`. `source_iter` is irrelevant
        // here (SwitchRouted is never produced) and is forwarded verbatim.
        for action in &loop_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::InternalOnly,
                source_iter,
                action,
            )
            .await
            {
                ActionOutcome::Completed
                | ActionOutcome::Halted { .. }
                | ActionOutcome::Interrupted { .. } => return,
                ActionOutcome::Spawned { .. }
                | ActionOutcome::Progressed
                | ActionOutcome::SpawnSkipped { .. } => {}
            }
        }
    }

    for region in pipeline
        .loops
        .iter()
        .filter(|r| r.kind == pipeline::LoopKind::Collection)
    {
        let collection_actions =
            scheduler::evaluate_collection_barrier(&pipeline, &fresh_run_state, region);
        for action in &collection_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::InternalOnly,
                source_iter,
                action,
            )
            .await
            {
                ActionOutcome::Completed
                | ActionOutcome::Halted { .. }
                | ActionOutcome::Interrupted { .. } => return,
                ActionOutcome::Spawned { .. }
                | ActionOutcome::Progressed
                | ActionOutcome::SpawnSkipped { .. } => {}
            }
        }
    }

    // ADR-0086: this completion may be the one a Run past `End` waits for —
    // the last started node of a Run that waits for them, or a node launched
    // after the Run went past `End`, which re-opened it.
    reevaluate_end(state, run_id).await;
}

/// Evaluate the end of a Run again (ADR-0086), on its state as it is now:
/// once no started node is alive, a Run whose routing went past `End` ends
/// (`Complete`, or `Halt` for an edge into `End` with a `reason:`). Called
/// after a node completes, and after a node is stopped — the gestures that
/// can leave the last started node of a Run past `End` settled.
pub async fn reevaluate_end(state: &Runs, run_id: &str) {
    let Some((events, run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    if run_state.status != state::RunStatus::Running {
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
    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let frontmatter = resolve_completed_frontmatter(&pipeline, &run_state, &artifacts_dir);
    let vars = resolve_run_variables(&pipeline, &events);
    let Some(action) = scheduler::end_reached_action(&pipeline, &run_state, &frontmatter, &vars)
    else {
        return;
    };
    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &vars,
        repo_root: &repo_root,
    };
    let _ = scheduler_interpreter::interpret(
        state,
        &spawn_ctx,
        &run_state,
        SpawnDedup::InternalOnly,
        1,
        &action,
    )
    .await;
}

/// Thin shim over the single-pass advancement tick owned by
/// [`run_advance::advance_run`].
pub async fn spawn_ready_after_event(state: &Runs, run_id: &str) {
    run_advance::advance_run(state, run_id).await;
}

/// Re-drive nodes throttled into `waiting` by the session cap, across *all* runs.
///
/// Called whenever a slot may have freed. The cap is daemon-wide, so a slot freed
/// in one Run can let a `waiting` node in a *different* Run start — hence the scan
/// over every run. `spawn_node` re-checks admission per node, so a node that still
/// can't get a slot simply stays `waiting`.
pub async fn retry_waiting_nodes(state: &Runs) {
    let run_ids = match load_all_run_ids(state).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("retry_waiting_nodes: failed to load run ids: {e}");
            return;
        }
    };
    for run_id in run_ids {
        let Some((events, run_state)) = reload_run_state(state, &run_id).await else {
            continue;
        };
        if run_state.status != state::RunStatus::Running
            && run_state.status != state::RunStatus::AwaitingUser
        {
            continue;
        }
        let waiting = scheduler_dispatcher::waiting_nodes(&run_state);
        if waiting.is_empty() {
            continue;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let pipeline_path =
            resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
        let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
            continue;
        };
        let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
            continue;
        };
        let pipeline = parse_result.pipeline;
        let resolved_vars = resolve_run_variables(&pipeline, &events);
        let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let spawn_ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: &run_id,
            pipeline_path: &pipeline_path,
            worktree_dir: &worktree_dir,
            artifacts_dir: &artifacts_dir,
            resolved_vars: &resolved_vars,
            repo_root: &repo_root,
        };
        // Shares the spawn loop with `advance_run` but must stay out of
        // `maybe_complete_run`: `RunCompleted` is not de-duped, and only the per-run
        // single-pass path may emit it.
        run_advance::spawn_each(state, &spawn_ctx, &waiting).await;
    }
}
