//! The scheduler (ADR-0009, layer 3): the one interpreter of the evaluators'
//! actions, and the drivers that feed it (a Run's advance, a node's
//! completion, the retry of waiting nodes, the run-level stall). A transition
//! of the Run: nothing here asks the Autorisation d'opération (D10).

pub mod advance;
mod children_binding;
mod completion_head;
pub mod frontmatter;
pub mod interpreter;
pub mod node_completion;
pub mod stall;

/// The scheduler's **cost input** (ADR draft plafond-de-cout-de-boucle). When
/// the pipeline caps at least one bounded region (`max_cost_usd`, or a live
/// `set_region_max_cost` override, which caps a region whose YAML declares
/// none), fold the
/// **Coût de boucle** of each region from the same attributed breakdown as the
/// Run total (ADR-0058) onto a copy of the Run state, so the pure scheduler
/// reads it at re-entry. Never persisted (ADR-0022/0029); a pipeline with no
/// cap pays nothing and borrows the snapshot as is.
pub async fn with_loop_costs_for_caps<'a>(
    state: &crate::Runs,
    run_state: &'a crate::domain::state::RunState,
    events: &[crate::domain::event::Event],
    pipeline: &pdo_pipeline::domain::pipeline::PipelineDef,
) -> std::borrow::Cow<'a, crate::domain::state::RunState> {
    let capped = pipeline
        .loops
        .iter()
        .any(|region| crate::domain::scheduler::region_has_cost_cap(run_state, region));
    if !capped {
        return std::borrow::Cow::Borrowed(run_state);
    }
    let costs = state
        .live_facts
        .loop_costs(run_state, events, &pipeline.loops)
        .await;
    let mut with_costs = run_state.clone();
    with_costs.loop_costs = costs;
    std::borrow::Cow::Owned(with_costs)
}
