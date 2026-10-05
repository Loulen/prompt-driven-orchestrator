//! `parse_pipeline`: the one door every reader of a pipeline YAML goes through.

use super::definition::PipelineDef;
use super::diagnostic::{Diagnostic, ParseError, ParseResult, Severity};
use super::node::{NodeDef, NodeType, Port, PortSide, PortType};
use super::normalize::normalize_node_value;
use super::references::{dangling_edge_references, validate_switch_when_clauses};

/// Top-level YAML keys accepted by the unknown-field lint.
/// MUST list every serializable field of PipelineDef — see `known_keys_cover_serialized_pipeline`.
pub(crate) const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "name",
    "version",
    "variables",
    "nodes",
    "edges",
    "loops",
    "notes",
    "prompt_required",
    "grid_size",
];

pub fn parse_pipeline(yaml: &str) -> Result<ParseResult, ParseError> {
    let mut raw: serde_yaml::Value = serde_yaml::from_str(yaml)?;

    if raw
        .as_mapping()
        .ok_or_else(|| ParseError::MissingField("root must be a mapping".into()))?
        .get(serde_yaml::Value::String("name".into()))
        .is_none()
    {
        return Err(ParseError::MissingField("name".into()));
    }

    let mut diagnostics = Vec::new();

    if let Some(nodes) = raw
        .as_mapping_mut()
        .and_then(|m| m.get_mut(serde_yaml::Value::String("nodes".into())))
        .and_then(|v| v.as_sequence_mut())
    {
        for node_val in nodes.iter_mut() {
            diagnostics.extend(normalize_node_value(node_val)?);
        }
    }

    let mut pipeline: PipelineDef = serde_yaml::from_value(raw.clone())?;

    for node in &mut pipeline.nodes {
        for port in &mut node.inputs {
            if port.side.is_none() {
                port.side = Some(PortSide::Left);
            }
        }
        for port in &mut node.outputs {
            if port.side.is_none() {
                port.side = Some(PortSide::Right);
            }
        }
    }

    for node in &mut pipeline.nodes {
        match node.node_type {
            NodeType::Switch => {
                if node.inputs.len() != 1 || node.inputs[0].name != "in" {
                    return Err(ParseError::MissingField(format!(
                        "switch node '{}' must have exactly one input named 'in'",
                        node.id
                    )));
                }
                let has_default = node.outputs.iter().any(|p| p.name == "default");
                if !has_default {
                    node.outputs.push(Port {
                        name: "default".into(),
                        repeated: false,
                        side: Some(PortSide::Right),
                        port_type: PortType::Markdown,
                        frontmatter: None,
                        when: None,
                        description: None,
                        instructions: None,
                        required: false,
                    });
                }
            }
            NodeType::Loop => {
                if node.max_iter.is_none() {
                    return Err(ParseError::MissingField(format!(
                        "loop node '{}' must declare 'max_iter'",
                        node.id
                    )));
                }
                let expected_inputs = ["in", "break"];
                let expected_outputs = ["body", "done"];
                let input_names: Vec<&str> = node.inputs.iter().map(|p| p.name.as_str()).collect();
                let output_names: Vec<&str> =
                    node.outputs.iter().map(|p| p.name.as_str()).collect();
                for name in &expected_inputs {
                    if !input_names.contains(name) {
                        return Err(ParseError::MissingField(format!(
                            "loop node '{}' must have input '{}'",
                            node.id, name
                        )));
                    }
                }
                for name in &expected_outputs {
                    if !output_names.contains(name) {
                        return Err(ParseError::MissingField(format!(
                            "loop node '{}' must have output '{}'",
                            node.id, name
                        )));
                    }
                }
            }
            _ => {}
        }
    }

    for node in &pipeline.nodes {
        if node.node_type != NodeType::Agent {
            for port in node
                .outputs
                .iter()
                .filter(|port| port.instructions.is_some())
            {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    message: format!(
                        "deterministic node '{}' preserves instructions on output '{}' but does not inject them because no agent produces that output",
                        node.id, port.name
                    ),
                });
            }
        }
    }

    if let Some(mapping) = raw.as_mapping() {
        for key in mapping.keys() {
            if let Some(k) = key.as_str() {
                if !KNOWN_TOP_LEVEL_KEYS.contains(&k) {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!("unknown field '{k}' (ignored)"),
                    });
                }
            }
        }
    }

    // Warnings here, refusals at run launch: see `dangling_edge_references`.
    for message in dangling_edge_references(&pipeline) {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message,
        });
    }

    let start_nodes: Vec<&NodeDef> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Start)
        .collect();
    let end_nodes: Vec<&NodeDef> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::End)
        .collect();

    if start_nodes.len() != 1 {
        return Err(ParseError::MissingField(format!(
            "pipeline must have exactly one start node, found {}",
            start_nodes.len()
        )));
    }
    if end_nodes.len() != 1 {
        return Err(ParseError::MissingField(format!(
            "pipeline must have exactly one end node, found {}",
            end_nodes.len()
        )));
    }

    let start = start_nodes[0];
    if !start.inputs.is_empty() {
        return Err(ParseError::MissingField(
            "start node must have zero inputs".into(),
        ));
    }
    if start.outputs.len() != 1 || start.outputs[0].name != "user_prompt" {
        return Err(ParseError::MissingField(
            "start node must have exactly one output port named 'user_prompt'".into(),
        ));
    }

    let end = end_nodes[0];
    if !end.outputs.is_empty() {
        return Err(ParseError::MissingField(
            "end node must have zero outputs".into(),
        ));
    }
    if end.inputs.len() != 1 || end.inputs[0].name != "result" {
        return Err(ParseError::MissingField(
            "end node must have exactly one input port named 'result'".into(),
        ));
    }

    validate_switch_when_clauses(&pipeline)?;

    // Auto-materialize bounded regions here, at the model boundary rather than in
    // the editor's `addEdge` (ADR-0011 clause (b)): every reader funnels through
    // this function, so a cycle that arrived any other way (hand-written file,
    // imported workflow, a mere reopen) still gets a lap counter and an `Exhausted`
    // halt instead of running unbounded through the generic forward-spawn path.
    //
    // Derived, never written: the file on disk stays untouched, and the id is a
    // pure function of the member set, so repeated parses agree and the region's
    // lap counter keeps its identity.
    pipeline
        .loops
        .extend(crate::domain::loops::materialize_missing_regions(&pipeline));

    // Legacy control nodes are the carve-out above (a `type: loop` node governs its
    // own laps, so no region is materialized over it). Warn rather than leave the
    // canvas silently unable to render the loop.
    let legacy: Vec<String> = pipeline
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, NodeType::Loop | NodeType::Switch))
        .map(|n| {
            let kind = if n.node_type == NodeType::Loop {
                "loop"
            } else {
                "switch"
            };
            format!("{kind} '{}'", n.id)
        })
        .collect();
    if !legacy.is_empty() {
        // Only mention `max_iter` when a loop node exists: a `switch`-only pipeline
        // has no bound to edit, and naming a control the user cannot find misleads.
        let has_loop = pipeline.nodes.iter().any(|n| n.node_type == NodeType::Loop);
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message: format!(
                "legacy control node(s) {} were removed from the model (ADR-0011) and \
                 the canvas cannot represent them — run `pdo migrate` to convert them \
                 into conditional edges and a `loops:` region{}",
                legacy.join(", "),
                if has_loop {
                    "; until then this loop has no region on the canvas and its `max_iter` is not editable"
                } else {
                    ""
                }
            ),
        });
    }

    Ok(ParseResult {
        pipeline,
        diagnostics,
    })
}
