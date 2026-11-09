//! A region re-entered by an edge: the next lap, or what its exhaustion routes to.

use super::*;

/// The bounded region's **effective** iteration cap: the live
/// `set_region_max_iter` override if the operator raised it in flight, else the
/// declared `max_iter` resolved from the pipeline. Read the override off
/// `run_state` (folded from the append-only log), never the YAML: that is what
/// makes the raise uniform across a literal and a `$var` cap, and durable across a
/// reopen re-projection.
pub fn effective_region_max_iter(
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    run_state
        .region_max_iter_overrides
        .get(region.id.as_str())
        .copied()
        .unwrap_or_else(|| {
            crate::domain::loop_region::resolve_region_max_iter(region, resolved_vars)
        })
}

/// The bounded region's **effective** cost cap (ADR draft
/// plafond-de-cout-de-boucle): the live `set_region_max_cost` override first,
/// read off `run_state` (folded from the append-only log), so it beats a
/// literal and a `$var` cap alike and survives a reopen; an override of `null`
/// means the cap was removed. Else the declared `max_cost_usd`, resolved
/// against the Run's variables. `None` = no cap, also for a cap that does not
/// resolve: the launch refuses those, so this only guards a hand-edited
/// snapshot, and parking a Run on a value it cannot read would be worse. A
/// `collection` is never capped, whatever the log says.
pub fn effective_region_max_cost(
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Option<f64> {
    if region.kind != pdo_pipeline::domain::pipeline::LoopKind::Bounded {
        return None;
    }
    if let Some(over) = run_state.region_max_cost_overrides.get(region.id.as_str()) {
        return *over;
    }
    pdo_pipeline::domain::loops::resolve_region_max_cost(region, resolved_vars)
        .ok()
        .flatten()
}

/// Whether a bounded region has a cost cap at all, before its `$var` is
/// resolved: the live override when there is one (`null`: removed), else its
/// declared `max_cost_usd`. What decides if the scheduler needs the Coût de
/// boucle as an input.
pub fn region_has_cost_cap(
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
) -> bool {
    region.kind == pdo_pipeline::domain::pipeline::LoopKind::Bounded
        && match run_state.region_max_cost_overrides.get(region.id.as_str()) {
            Some(over) => over.is_some(),
            None => region.max_cost_usd.is_some(),
        }
}

/// `$1.20`, `$0.0084`: a spend in the budget message, with the front's
/// adaptive precision (4 decimals under $1, `formatCostAmount`).
fn usd_text(usd: f64) -> String {
    if usd.abs() < 1.0 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

/// `$5`, `$2.50`, `$0.0050`: a cap as the region's corner writes it
/// (`formatCostCap`), so the reason and the canvas name the same amount.
fn cap_text(usd: f64) -> String {
    if usd.fract() == 0.0 {
        format!("${usd:.0}")
    } else if usd < 0.01 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

/// The `region_budget_exhausted` park, or `None` when the region has no cap,
/// no known cost yet, or is still under its cap.
///
/// PURE: the **Coût de boucle** is an INPUT, `run_state.loop_costs`, which the
/// interpreter fills from the attributed cost fold (ADR-0058) right before it
/// calls the scheduler. The scheduler never reads a transcript. An unknown cost
/// (`usd: None`) never counts as `$0` and never blocks; a partial cost counts
/// its known part, and the message says so.
fn budget_exhausted(
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    current_iter: i64,
    max_iter: i64,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Option<SchedulerAction> {
    let cap = effective_region_max_cost(run_state, region, resolved_vars)?;
    let cost = run_state.loop_costs.get(region.id.as_str())?;
    let spent = cost.usd?;
    if spent < cap {
        return None;
    }
    let approx = if cost.estimated { "~" } else { "" };
    let partial = if cost.partial {
        " (partial cost: a member execution has no readable cost, so the known part \
         is a lower bound)"
    } else {
        ""
    };
    Some(SchedulerAction::Interrupt {
        reason_code: crate::domain::state::PARK_REGION_BUDGET_EXHAUSTED.to_string(),
        message: format!(
            "budget reached: bounded region '{id}' reached its cost cap: \
             {approx}{spent} spent of {cap} after lap {current_iter}/{max_iter}{partial}. \
             Its exit was not followed. Raise the cap with set_region_max_cost \
             (max_cost_usd), route the exit with force_route (source '{id}' -> a \
             target), or end the region with end_region.",
            id = region.id,
            spent = usd_text(spent),
            cap = cap_text(cap),
        ),
        region_id: Some(region.id.clone()),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_region_reentry(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    entry_id: &str,
    completed_member: &str,
    source_iter: i64,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let max_iter = effective_region_max_iter(run_state, region, resolved_vars);
    let region_loop_state = run_state.loop_states.get(region.id.as_str());
    let current_iter = region_loop_state.map(|ls| ls.current_iter).unwrap_or(1);
    // An ended region (`end_region` projected as `done`) never starts another lap —
    // it routes its exit at the current iter, like exhaustion. A `force_route` on
    // the region is likewise an exit-now request: stop looping rather than run out
    // the (possibly just-raised) cap.
    let forced_region_route = run_state.forced_routes.get(region.id.as_str()).cloned();
    let ended = region_loop_state.is_some_and(|ls| ls.done) || forced_region_route.is_some();

    // Drop a back-edge from a member that lags the region counter
    // (`current_iter > source_iter`): it represents a lap the region ALREADY took.
    // `reopen_run` re-fires the outgoing edges of *every* settled-complete member
    // in one pass, so a lagging member would spawn the entry a lap ahead of the
    // frontier, forking the loop into two concurrent branches over the same
    // worktree. The frontier member's forward edge drives the legitimate resume.
    // An `ended`/force-routed region still routes its exit — terminal routing, not
    // a lap advance — so the guard scopes to the plain NextLap path only.
    if !ended && forced_region_route.is_none() && current_iter > source_iter {
        return actions;
    }

    let runtime = crate::domain::loop_region::RegionRuntime {
        current_iter,
        max_iter,
        exhausted: false,
    };

    // A fired back-edge means at least one re-entry was requested this lap.
    let decision = if ended {
        crate::domain::loop_region::LapDecision::Exhausted
    } else {
        crate::domain::loop_region::resolve_lap(pipeline, region, &runtime, 1)
    };
    match decision {
        crate::domain::loop_region::LapDecision::NextLap { iter, entry } => {
            // The Plafond de coût is checked HERE, at the same decision point as
            // `max_iter` (which wins when both are reached: the `Exhausted` arm
            // below never consults the cap). Reached => park the Run, never
            // follow the exhaustion exit: that would spend more.
            if let Some(park) =
                budget_exhausted(run_state, region, current_iter, max_iter, resolved_vars)
            {
                actions.push(park);
                return actions;
            }
            actions.push(SchedulerAction::LoopIterStarted {
                loop_node_id: region.id.clone(),
                iter,
                max_iter,
            });
            let entry = if entry.is_empty() {
                entry_id.to_string()
            } else {
                entry
            };
            actions.push(SchedulerAction::Spawn {
                node_id: entry,
                iter,
            });
        }
        crate::domain::loop_region::LapDecision::Exhausted => {
            // #600: a `force_route` on the region overrides the `when:`-based
            // exhaustion routing — spawn the forced target (or complete, if it is
            // `End`), never the "exhausted — unrouted" park. This is the region
            // twin of the node-scoped force route in `evaluate_outgoing_edges_full`.
            if let Some(target) = forced_region_route {
                let end_node_id = pipeline
                    .nodes
                    .iter()
                    .find(|n| n.node_type == NodeType::End)
                    .map(|n| n.id.clone());
                if end_node_id.as_deref() == Some(target.as_str()) {
                    actions.push(SchedulerAction::Complete);
                } else {
                    actions.push(SchedulerAction::Spawn {
                        node_id: target,
                        iter: 1,
                    });
                }
                return actions;
            }
            match crate::domain::loop_region::exhaustion_outcome(
                pipeline,
                region,
                &runtime,
                frontmatter_fields,
                resolved_vars,
            ) {
                crate::domain::loop_region::ExhaustionOutcome::Routed(targets) => {
                    let end_node_id = pipeline
                        .nodes
                        .iter()
                        .find(|n| n.node_type == NodeType::End)
                        .map(|n| n.id.as_str());
                    // An exit into a convergence (ADR-0082) starts it only once
                    // its other branches are resolved too: a collection still
                    // fanning out, a sibling still running. Their routing spawns
                    // it when they resolve; the exhausted loop reads resolved then.
                    for target in targets.into_iter().filter(|t| {
                        check_all_upstream_completed(
                            pipeline,
                            run_state,
                            t,
                            completed_member,
                            frontmatter_by_node,
                            resolved_vars,
                        )
                    }) {
                        if end_node_id == Some(target.as_str()) {
                            actions.push(SchedulerAction::Complete);
                        } else {
                            actions.push(SchedulerAction::Spawn {
                                node_id: target,
                                iter: 1,
                            });
                        }
                    }
                }
                crate::domain::loop_region::ExhaustionOutcome::Unrouted => {
                    // Only the just-completed member's frontmatter is in scope, so
                    // read values only appear where the guard reads one of its fields.
                    let exit_edges = describe_region_exit_edges(
                        pipeline,
                        region,
                        current_iter,
                        frontmatter_fields,
                        resolved_vars,
                    );
                    let (reason_code, head) = if ended {
                        (
                            crate::domain::state::PARK_REGION_ENDED_UNROUTED,
                            format!(
                                "ended — unrouted: bounded region '{}' was closed by end_region \
                                 at iter {current_iter} but no exit edge matched",
                                region.id
                            ),
                        )
                    } else {
                        (
                            crate::domain::state::PARK_REGION_EXHAUSTED,
                            format!(
                                "exhausted — unrouted: bounded region '{}' reached max_iter \
                                 {max_iter} with the continuation condition still true and no \
                                 matching exit edge",
                                region.id
                            ),
                        )
                    };
                    let message = format!(
                        "{head}. Exit edges:\n{exit_edges}\n\
                         Raise the cap with set_region_max_iter, or route the exit with \
                         force_route (source '{}' -> a target).",
                        region.id
                    );
                    actions.push(SchedulerAction::Interrupt {
                        reason_code: reason_code.to_string(),
                        message,
                        region_id: Some(region.id.clone()),
                    });
                }
            }
        }
        crate::domain::loop_region::LapDecision::NoReentry => {}
    }

    actions
}
