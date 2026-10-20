//! The references a pipeline makes to its own nodes and ports: dangling edges
//! and region members, and the `when:` fields of a switch.

use std::collections::{HashMap, HashSet};

use super::definition::PipelineDef;
use super::diagnostic::ParseError;
use super::edge::EdgeDef;
use super::node::{FrontmatterFieldDecl, NodeDef, NodeType, Port, LIVE_DIR};

/// Dangling references: an edge endpoint naming a missing node, a port not
/// declared on its node, or a loop-region member naming a missing node. Emergent
/// inputs are exempt — an edge target port on a *regular* node names the emergent
/// input and is valid by construction (ADR-0011).
///
/// Warnings at edit time (ADR-0001 sharp tool: the editor never blocks), refusals
/// at run launch — a run over a dangling reference is guaranteed to stall silently
/// mid-run, so rejecting it is a runtime-coherence invariant, not prescriptive
/// validation.
pub fn dangling_edge_references(pipeline: &PipelineDef) -> Vec<String> {
    let node_ids: HashSet<&str> = pipeline.nodes.iter().map(|n| n.id.as_str()).collect();

    // `ports` names the port(s) the endpoint claims on its node: one for a
    // target, one PER CARRIED PORT for a multi-port source (ADR-0073) — a typo in
    // the second port of a `ports:` list is the same dangling reference as a typo
    // in the first, and silently ignoring it would stall the run mid-flight.
    let check = |edge: &EdgeDef,
                 node_id: &str,
                 ports: &[String],
                 role: &str,
                 get_ports: fn(&NodeDef) -> &[Port]|
     -> Vec<String> {
        if !node_ids.contains(node_id) {
            return vec![format!(
                "{}: {role} references non-existent node '{node_id}'",
                edge_label(edge)
            )];
        }
        let node = pipeline.nodes.iter().find(|n| n.id == node_id).unwrap();
        if role == "target" && node.node_type.has_emergent_inputs() {
            return Vec::new();
        }
        ports
            .iter()
            .filter(|port| !get_ports(node).iter().any(|p| p.name == **port))
            .map(|port| {
                format!(
                    "{}: {role} port '{port}' not found on node '{node_id}'",
                    edge_label(edge)
                )
            })
            .collect()
    };

    let mut errors = Vec::new();
    for edge in &pipeline.edges {
        errors.extend(check(
            edge,
            &edge.source.node,
            &edge.source.ports,
            "source",
            |n| &n.outputs,
        ));
        errors.extend(check(
            edge,
            &edge.target.node,
            std::slice::from_ref(&edge.target.port),
            "target",
            |n| &n.inputs,
        ));
    }

    // A region member naming a missing node is the same class of dangling
    // reference: the region silently no-ops at runtime, its gates matching no node.
    for region in &pipeline.loops {
        for member in &region.members {
            if !node_ids.contains(member.as_str()) {
                errors.push(format!(
                    "loop region '{}': member references non-existent node '{}'",
                    region.id, member
                ));
            }
        }
    }
    errors
}

/// `edge 'design.out -> review.in'`: how a diagnostic names an edge.
fn edge_label(edge: &EdgeDef) -> String {
    format!(
        "edge '{} -> {}.{}'",
        edge.source.label(),
        edge.target.node,
        edge.target.port
    )
}

/// The first edge that carries a live artifact (ADR-0089), refused with the
/// artifact and the edge it names. A live artifact is for humans only: nothing
/// guarantees its content, so wiring it would hand the downstream node a file
/// that may never come. A runtime-coherence invariant, not a prescriptive rule —
/// which is why it refuses at parse instead of warning like a dangling
/// reference. An edge naming an *output* of the same name is an ordinary edge.
pub(crate) fn live_artifacts_on_edges(pipeline: &PipelineDef) -> Result<(), ParseError> {
    for edge in &pipeline.edges {
        let Some(source) = pipeline.nodes.iter().find(|n| n.id == edge.source.node) else {
            continue;
        };
        let wired = source.live_artifacts.iter().find(|live| {
            edge.source.carries(&live.name)
                && !source.outputs.iter().any(|port| port.name == live.name)
        });
        if let Some(live) = wired {
            return Err(ParseError::InvalidField(format!(
                "{}: carries live artifact '{}' of node '{}'; a live artifact cannot be \
                 wired (wire an output instead)",
                edge_label(edge),
                live.name,
                source.id
            )));
        }
    }
    Ok(())
}

/// A live artifact's name becomes a path under the iteration's `live/` folder
/// (ADR-0089): it must be one plain path segment, and unique on its node. That
/// folder sits beside the output folders, so a node that publishes live
/// artifacts cannot also have an output named `live`.
pub(crate) fn live_artifact_names(pipeline: &PipelineDef) -> Result<(), ParseError> {
    for node in &pipeline.nodes {
        let publishes = node.free_live_artifacts || !node.live_artifacts.is_empty();
        if publishes && node.outputs.iter().any(|port| port.name == LIVE_DIR) {
            return Err(ParseError::InvalidField(format!(
                "node '{}': an output named '{LIVE_DIR}' would share the folder of the \
                 node's live artifacts; rename the output",
                node.id
            )));
        }
        let mut seen = HashSet::new();
        for live in &node.live_artifacts {
            let name = live.name.as_str();
            let plain = !name.trim().is_empty()
                && name != "."
                && name != ".."
                && !name.contains(['/', '\\']);
            if !plain {
                return Err(ParseError::InvalidField(format!(
                    "node '{}': live artifact name '{name}' must be a plain file name \
                     (no '/', not '.' or '..')",
                    node.id
                )));
            }
            if !seen.insert(name) {
                return Err(ParseError::InvalidField(format!(
                    "node '{}': live artifact '{name}' is declared twice",
                    node.id
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_switch_when_clauses(pipeline: &PipelineDef) -> Result<(), ParseError> {
    let variable_names: HashSet<&str> = pipeline.variables.keys().map(|k| k.as_str()).collect();

    for node in &pipeline.nodes {
        if node.node_type != NodeType::Switch {
            continue;
        }
        let upstream_schema = resolve_switch_upstream_schema(pipeline, &node.id);
        for port in &node.outputs {
            if port.name == "default" {
                continue;
            }
            let when = match &port.when {
                Some(w) => w,
                None => continue,
            };
            let mapping = match when.as_mapping() {
                Some(m) => m,
                None => continue,
            };
            for (key, _) in mapping {
                let field_name = match key.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                if let Some(var_name) = field_name.strip_prefix('$') {
                    if !variable_names.contains(var_name) {
                        return Err(ParseError::UndeclaredWhenField {
                            node_id: node.id.clone(),
                            port: port.name.clone(),
                            field: field_name.to_string(),
                        });
                    }
                    continue;
                }
                match &upstream_schema {
                    Some(schema) if schema.contains_key(field_name) => {}
                    _ => {
                        return Err(ParseError::UndeclaredWhenField {
                            node_id: node.id.clone(),
                            port: port.name.clone(),
                            field: field_name.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The frontmatter schema declared on the upstream output port feeding a switch's
/// `in` port. `None` when the node isn't a switch, nothing connects to `in`, or the
/// upstream port declares no schema.
pub fn resolve_switch_upstream_schema(
    pipeline: &PipelineDef,
    switch_node_id: &str,
) -> Option<HashMap<String, FrontmatterFieldDecl>> {
    let node = pipeline.nodes.iter().find(|n| n.id == switch_node_id)?;
    if node.node_type != NodeType::Switch {
        return None;
    }
    let edge = pipeline
        .edges
        .iter()
        .find(|e| e.target.node == switch_node_id && e.target.port == "in")?;
    let source_node = pipeline.nodes.iter().find(|n| n.id == edge.source.node)?;
    let source_port = source_node
        .outputs
        .iter()
        .find(|p| p.name == edge.source.port())?;
    source_port.frontmatter.clone()
}
