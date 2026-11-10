//! The `unrouted` interruption (ADR-0011, « jamais de stall silencieux ») and
//! the prose it carries: which edges were candidates and why none fired.

use super::*;
use crate::domain::state::PARK_UNROUTED;

/// Explicit park on an unrouted convergence (ADR-0011). A convergence whose
/// branches are ALL dead is never spawned and becomes dead itself; the cascade
/// can render `End` unreachable, and the run would sit `Running` forever. Park
/// instead, so the state is diagnosable.
///
/// Only when this completion produced no forward progress (`actions` holds no
/// spawn and no end): if `End` is still reachable through a live path, a
/// convergence waiting on a running sibling is normal, not a stall.
#[allow(clippy::too_many_arguments)]
pub(super) fn unrouted_interrupt(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
    actions: &[SchedulerAction],
    fired_indices: &HashSet<usize>,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Option<SchedulerAction> {
    if actions.iter().any(|a| {
        matches!(
            a,
            SchedulerAction::Spawn { .. }
                | SchedulerAction::Complete
                | SchedulerAction::Halt { .. }
        )
    }) || !end_is_dead(pipeline, run_state, frontmatter_by_node, resolved_vars)
    {
        return None;
    }
    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);
    // Don't just say "unrouted": the operator needs each guard, whether it
    // fired, and the value actually read, to know where to `force_route`.
    let candidates = describe_candidate_edges(
        pipeline,
        completed_node_id,
        source_iter,
        fired_indices,
        frontmatter_fields,
    );
    Some(SchedulerAction::Interrupt {
        reason_code: PARK_UNROUTED.to_string(),
        region_id: None,
        message: format!(
            "unrouted: node '{completed_node_id}' (iter {source_iter}) completed \
             but conditional routing suppressed every path to End (no live branch \
             reaches End). Candidate edges:\n{candidates}\n\
             Route it explicitly with force_route (source '{completed_node_id}' \
             -> a target), or adjust the verdict the edges read."
        ),
    })
}

/// The `unrouted` park when pruning the branches not taken leaves `End` with no
/// live branch: a required input of a node died while its other inputs fired,
/// and that node was the way to `End` (ADR-0011, ADR-0082).
pub(super) fn unrouted_by_pruning(pruned: &[String]) -> SchedulerAction {
    SchedulerAction::Interrupt {
        reason_code: PARK_UNROUTED.to_string(),
        region_id: None,
        message: format!(
            "unrouted: {} skipped because its branch was not taken, and no live branch \
             reaches End any more. Route the Run explicitly with force_route (a completed \
             node -> a target).",
            pruned
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Is `End` dead for this Run — every branch into it died (ADR-0082)? `false`
/// for a pipeline without `End`.
pub(super) fn end_is_dead(
    pipeline: &PipelineDef,
    run_state: &RunState,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::End)
        .is_some_and(|end| {
            is_node_dead(
                pipeline,
                run_state,
                &end.id,
                frontmatter_by_node,
                vars,
                &mut HashSet::new(),
            )
        })
}

/// Renders a `serde_yaml` scalar as a short token for the `unrouted` diagnostic:
/// a compact YAML flow for a mapping/sequence, so multi-line YAML never lands in a
/// run's `awaiting_reason`.
pub(super) fn yaml_token(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .replace('\n', " "),
    }
}

/// The field names a `when:` clause reads (its mapping keys, `any:` flattened one
/// level). Used to name, in the `unrouted` diagnostic, exactly which fields the
/// operator should look at — and what value was actually read for each.
pub(super) fn when_fields(when: &serde_yaml::Value) -> Vec<String> {
    let mut fields = Vec::new();
    if let Some(map) = when.as_mapping() {
        for (k, v) in map {
            let Some(key) = k.as_str() else { continue };
            if key == "any" {
                if let Some(seq) = v.as_sequence() {
                    for sub in seq {
                        for f in when_fields(sub) {
                            if !fields.contains(&f) {
                                fields.push(f);
                            }
                        }
                    }
                }
            } else if !fields.contains(&key.to_string()) {
                fields.push(key.to_string());
            }
        }
    }
    fields
}

/// Builds the enriched `unrouted` diagnostic: one line per candidate edge with its
/// guard, whether it fired, and the **value actually read** for each field the
/// guard tests — so the operator sees why no branch is live from the run state
/// alone, without reading the daemon log.
pub(super) fn describe_candidate_edges(
    pipeline: &PipelineDef,
    source_node_id: &str,
    source_iter: i64,
    fired_indices: &HashSet<usize>,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (idx, edge) in pipeline.edges.iter().enumerate() {
        if edge.source.node != source_node_id {
            continue;
        }
        let guard = if edge.is_else {
            "else".to_string()
        } else if let Some(when) = &edge.when {
            format!("when {}", yaml_token(when))
        } else {
            "(unconditional)".to_string()
        };
        let fired = fired_indices.contains(&idx);
        // `iter` reads the source's lap, not a frontmatter field.
        let reads: Vec<String> = edge
            .when
            .as_ref()
            .map(when_fields)
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                if f == "iter" {
                    format!("iter={source_iter}")
                } else {
                    match frontmatter_fields.get(&f) {
                        Some(v) => format!("{f}={}", yaml_token(v)),
                        None => format!("{f}=<absent>"),
                    }
                }
            })
            .collect();
        let read_note = if reads.is_empty() {
            String::new()
        } else {
            format!(" (read {})", reads.join(", "))
        };
        lines.push(format!(
            "  - {} -> {}  {}  => {}{}",
            edge.source.label(),
            edge.target.node,
            guard,
            if fired { "FIRED" } else { "not fired" },
            read_note,
        ));
    }
    if lines.is_empty() {
        format!("  (node '{source_node_id}' has no outgoing edges)")
    } else {
        lines.join("\n")
    }
}

/// Builds the exit-edge listing for an exhausted/ended bounded region: one line
/// per member→non-member edge with its guard and whether it fires at the exhausted
/// lap. Evaluated against the just-completed member's frontmatter, as
/// [`crate::domain::loop_region::exhaustion_outcome`] does — keep the two in step or the
/// diagnostic will contradict the routing decision.
pub(super) fn describe_region_exit_edges(
    pipeline: &PipelineDef,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    current_iter: i64,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> String {
    let member_set: HashSet<&str> = region.members.iter().map(String::as_str).collect();
    let mut lines: Vec<String> = Vec::new();
    for edge in &pipeline.edges {
        if !member_set.contains(edge.source.node.as_str())
            || member_set.contains(edge.target.node.as_str())
        {
            continue;
        }
        let guard = if edge.is_else {
            "else".to_string()
        } else if let Some(when) = &edge.when {
            format!("when {}", yaml_token(when))
        } else {
            "(unconditional)".to_string()
        };
        let single = [edge];
        let fired =
            !edge_router::fired_edges(&single, frontmatter_fields, resolved_vars, current_iter)
                .is_empty();
        let reads: Vec<String> = edge
            .when
            .as_ref()
            .map(when_fields)
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                if f == "iter" {
                    format!("iter={current_iter}")
                } else {
                    match frontmatter_fields.get(&f) {
                        Some(v) => format!("{f}={}", yaml_token(v)),
                        None => format!("{f}=<absent>"),
                    }
                }
            })
            .collect();
        let read_note = if reads.is_empty() {
            String::new()
        } else {
            format!(" (read {})", reads.join(", "))
        };
        lines.push(format!(
            "  - {} -> {}  {}  => {}{}",
            edge.source.label(),
            edge.target.node,
            guard,
            if fired { "FIRED" } else { "not fired" },
            read_note,
        ));
    }
    if lines.is_empty() {
        "  (region has no member->non-member exit edge)".to_string()
    } else {
        lines.join("\n")
    }
}
