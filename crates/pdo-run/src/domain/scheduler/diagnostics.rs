//! The prose an `unrouted` interruption carries: which edges were candidates and
//! why none fired.

use super::*;

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
