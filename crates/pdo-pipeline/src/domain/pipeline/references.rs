//! The references a pipeline makes to its own nodes and ports: dangling edges
//! and region members, and the `when:` fields of a switch.

use std::collections::{HashMap, HashSet};

use super::definition::PipelineDef;
use super::diagnostic::ParseError;
use super::edge::EdgeDef;
use super::node::{FrontmatterFieldDecl, NodeDef, NodeType, Port};

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

    let edge_label = |edge: &EdgeDef| {
        format!(
            "edge '{} -> {}.{}'",
            edge.source.label(),
            edge.target.node,
            edge.target.port
        )
    };

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
