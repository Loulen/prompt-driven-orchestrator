use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Warning,
    #[allow(dead_code)]
    Error,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    #[allow(dead_code)]
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NodeType {
    DocOnly,
    CodeMutating,
    Start,
    End,
    Switch,
    Loop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontmatterFieldDecl {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl std::fmt::Display for PortSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortSide::Left => write!(f, "left"),
            PortSide::Right => write!(f, "right"),
            PortSide::Top => write!(f, "top"),
            PortSide::Bottom => write!(f, "bottom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    #[serde(default)]
    pub repeated: bool,
    #[serde(default)]
    pub side: Option<PortSide>,
    #[serde(default)]
    pub frontmatter: Option<HashMap<String, FrontmatterFieldDecl>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(default)]
    pub inputs: Vec<Port>,
    #[serde(default)]
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub interactive: bool,
    pub view: Option<ViewPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iter: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeEndpoint {
    pub node: String,
    pub port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef {
    pub source: EdgeEndpoint,
    pub target: EdgeEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum VariableType {
    Int,
    Float,
    String,
    Bool,
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDef {
    #[serde(rename = "type")]
    pub var_type: VariableType,
    pub default: serde_yaml::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDef {
    pub name: String,
    pub version: Option<String>,
    #[serde(default, deserialize_with = "deserialize_variables")]
    pub variables: HashMap<String, VariableDef>,
    #[serde(default)]
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
    #[serde(default = "default_true")]
    pub auto_merge_resolver: bool,
}

fn default_true() -> bool {
    true
}

fn infer_variable_type(val: &serde_yaml::Value) -> VariableType {
    match val {
        serde_yaml::Value::Bool(_) => VariableType::Bool,
        serde_yaml::Value::Number(n) => {
            if n.is_f64() && !n.is_i64() && !n.is_u64() {
                VariableType::Float
            } else {
                VariableType::Int
            }
        }
        serde_yaml::Value::Sequence(_) => VariableType::List,
        _ => VariableType::String,
    }
}

fn deserialize_variables<'de, D>(deserializer: D) -> Result<HashMap<String, VariableDef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: HashMap<String, serde_yaml::Value> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();

    for (name, value) in raw {
        let is_explicit = value.as_mapping().is_some_and(|m| {
            m.contains_key(serde_yaml::Value::String("type".into()))
                && m.contains_key(serde_yaml::Value::String("default".into()))
        });
        let var_def = if is_explicit {
            serde_yaml::from_value::<VariableDef>(value).map_err(serde::de::Error::custom)?
        } else {
            VariableDef {
                var_type: infer_variable_type(&value),
                default: value,
            }
        };
        result.insert(name, var_def);
    }

    Ok(result)
}

impl PipelineDef {
    pub fn variable_defaults(&self) -> HashMap<String, serde_yaml::Value> {
        self.variables
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect()
    }
}

#[derive(Debug)]
pub struct ParseResult {
    pub pipeline: PipelineDef,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("missing required field: {0}")]
    MissingField(String),
}

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
    let valid_types = [
        "doc-only",
        "code-mutating",
        "start",
        "end",
        "switch",
        "loop",
    ];

    if let Some(nodes) = raw
        .as_mapping_mut()
        .and_then(|m| m.get_mut(serde_yaml::Value::String("nodes".into())))
        .and_then(|v| v.as_sequence_mut())
    {
        for node_val in nodes.iter_mut() {
            if let Some(node_map) = node_val.as_mapping_mut() {
                let type_key = serde_yaml::Value::String("type".into());
                let node_id = node_map
                    .get(serde_yaml::Value::String("id".into()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
                    .to_string();

                match node_map.get(&type_key).and_then(|v| v.as_str()) {
                    None => {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!(
                                "node '{node_id}': missing 'type', defaulting to 'doc-only'"
                            ),
                        });
                        node_map.insert(type_key, serde_yaml::Value::String("doc-only".into()));
                    }
                    Some(t) if !valid_types.contains(&t) => {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            message: format!(
                                "node '{node_id}': unknown node type '{t}', defaulting to 'doc-only'"
                            ),
                        });
                        node_map.insert(type_key, serde_yaml::Value::String("doc-only".into()));
                    }
                    _ => {}
                }
            }
        }
    }

    // Reject when: on edges (moved to Switch nodes since #45)
    if let Some(edges) = raw
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("edges".into())))
        .and_then(|v| v.as_sequence())
    {
        for edge_val in edges {
            if let Some(edge_map) = edge_val.as_mapping() {
                if edge_map.contains_key(serde_yaml::Value::String("when".into())) {
                    return Err(ParseError::MissingField(
                        "edges no longer accept 'when:' (since #45). Move the condition into a Switch node.".into(),
                    ));
                }
            }
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
                        frontmatter: None,
                        when: None,
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

    let known_keys: &[&str] = &[
        "name",
        "version",
        "variables",
        "nodes",
        "edges",
        "auto_merge_resolver",
    ];
    if let Some(mapping) = raw.as_mapping() {
        for key in mapping.keys() {
            if let Some(k) = key.as_str() {
                if !known_keys.contains(&k) {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        message: format!("unknown field '{k}' (ignored)"),
                    });
                }
            }
        }
    }

    let node_ids: std::collections::HashSet<&str> =
        pipeline.nodes.iter().map(|n| n.id.as_str()).collect();

    let check_endpoint = |endpoint: &EdgeEndpoint,
                          role: &str,
                          get_ports: fn(&NodeDef) -> &[Port]|
     -> Option<Diagnostic> {
        if !node_ids.contains(endpoint.node.as_str()) {
            return Some(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "edge {role} references non-existent node '{}'",
                    endpoint.node
                ),
            });
        }
        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == endpoint.node)
            .unwrap();
        if !get_ports(node).iter().any(|p| p.name == endpoint.port) {
            return Some(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "edge {role} port '{}' not found on node '{}'",
                    endpoint.port, endpoint.node
                ),
            });
        }
        None
    };

    for edge in &pipeline.edges {
        if let Some(d) = check_endpoint(&edge.source, "source", |n| &n.outputs) {
            diagnostics.push(d);
        }
        if let Some(d) = check_endpoint(&edge.target, "target", |n| &n.inputs) {
            diagnostics.push(d);
        }
    }

    // Validate start/end node constraints
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

    Ok(ParseResult {
        pipeline,
        diagnostics,
    })
}

pub fn canonical_prompt_path(pipeline_path: &Path, node_id: &str) -> std::path::PathBuf {
    let dir = pipeline_path.parent().unwrap_or(std::path::Path::new("."));
    let stem = pipeline_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pipeline");
    dir.join(format!("{stem}.prompts"))
        .join(format!("{node_id}.md"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const START_END_NODES: &str = r#"
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result"#;

    fn with_start_end(yaml: &str) -> String {
        if yaml.contains("type: start") && yaml.contains("type: end") {
            return yaml.to_string();
        }
        let replacement = format!("nodes:{START_END_NODES}");
        if yaml.contains("nodes: []") {
            yaml.replacen("nodes: []", &replacement, 1)
        } else {
            yaml.replacen("nodes:", &replacement, 1)
        }
    }

    const VALID_MINIMAL: &str = r#"
name: test-pipeline
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
"#;

    #[test]
    fn parses_valid_minimal_pipeline() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        assert_eq!(result.pipeline.name, "test-pipeline");
        assert_eq!(result.pipeline.version.as_deref(), Some("1.0"));
        assert_eq!(result.pipeline.nodes.len(), 3);

        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.name, "planner");
        assert_eq!(node.node_type, NodeType::DocOnly);
        assert_eq!(node.inputs.len(), 1);
        assert_eq!(node.inputs[0].name, "task");
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "plan");

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn fails_on_missing_node_name() {
        let yaml = with_start_end(
            r#"
name: no-name
nodes:
  - id: ab12cd34
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn ignores_deprecated_prompt_file_field() {
        let yaml = with_start_end(
            r#"
name: old-style
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    prompt_file: prompts/worker.md
    inputs:
      - name: in
    outputs:
      - name: out
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn errors_on_invalid_yaml() {
        let yaml = "{{not: valid: yaml:::";
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn errors_on_missing_pipeline_name() {
        let yaml = r#"
version: "1.0"
nodes: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(_)));
    }

    #[test]
    fn rejects_pipeline_without_start_node() {
        let yaml = r#"
name: no-start
nodes:
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("start"), "error should mention start: {msg}");
    }

    #[test]
    fn rejects_pipeline_without_end_node() {
        let yaml = r#"
name: no-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("end"), "error should mention end: {msg}");
    }

    #[test]
    fn rejects_start_node_with_inputs() {
        let yaml = r#"
name: bad-start
nodes:
  - id: start
    name: Start
    type: start
    inputs:
      - name: oops
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero inputs"),
            "error should mention zero inputs: {msg}"
        );
    }

    #[test]
    fn rejects_start_node_with_wrong_output_port() {
        let yaml = r#"
name: bad-start-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: wrong_name
  - id: end
    name: End
    type: end
    inputs:
      - name: result
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("user_prompt"),
            "error should mention user_prompt: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_outputs() {
        let yaml = r#"
name: bad-end
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs:
      - name: oops
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("zero outputs"),
            "error should mention zero outputs: {msg}"
        );
    }

    #[test]
    fn rejects_end_node_with_wrong_input_port() {
        let yaml = r#"
name: bad-end-port
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: wrong_name
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("result"), "error should mention result: {msg}");
    }

    #[test]
    fn rejects_legacy_halt_target_syntax() {
        let yaml = with_start_end(
            r#"
name: with-halt
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { halt: { message: "Blocked" } }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn parses_edge_with_reason() {
        let yaml = with_start_end(
            r#"
name: with-reason
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    outputs:
      - name: review
edges:
  - source: { node: ab12cd34, port: review }
    target: { node: end, port: result }
    reason: "Blocked after {iter} iterations"
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let edge = &result.pipeline.edges[0];
        assert_eq!(
            edge.reason.as_deref(),
            Some("Blocked after {iter} iterations")
        );
        assert_eq!(edge.target.node, "end");
        assert_eq!(edge.target.port, "result");
    }

    #[test]
    fn parses_interactive_node() {
        let yaml = with_start_end(
            r#"
name: interactive-pipe
nodes:
  - id: ab000001
    name: griller
    type: doc-only
    interactive: true
    inputs:
      - name: task
    outputs:
      - name: brief
  - id: ab000002
    name: worker
    type: code-mutating
    inputs:
      - name: brief
    outputs:
      - name: summary
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let griller = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert!(griller.interactive);
        let worker = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000002")
            .unwrap();
        assert!(!worker.interactive);
    }

    #[test]
    fn interactive_defaults_to_false() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert!(!planner.interactive);
    }

    #[test]
    fn parses_typed_variables_explicit_form() {
        let yaml = with_start_end(
            r#"
name: typed-vars
variables:
  max_iter:
    type: int
    default: 5
  mode:
    type: string
    default: strict
  verbose:
    type: bool
    default: true
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars.len(), 3);
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
    }

    #[test]
    fn parses_variables_inferred_type_from_value() {
        let yaml = with_start_end(
            r#"
name: inferred-vars
variables:
  max_iter: 5
  threshold: 0.8
  mode: strict
  verbose: true
  tags: [a, b, c]
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let vars = &result.pipeline.variables;
        assert_eq!(vars["max_iter"].var_type, VariableType::Int);
        assert_eq!(vars["threshold"].var_type, VariableType::Float);
        assert_eq!(vars["mode"].var_type, VariableType::String);
        assert_eq!(vars["verbose"].var_type, VariableType::Bool);
        assert_eq!(vars["tags"].var_type, VariableType::List);
    }

    #[test]
    fn variable_defaults_extracts_values() {
        let yaml = with_start_end(
            r#"
name: defaults-test
variables:
  max_iter: 5
  threshold: 0.8
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let defaults = result.pipeline.variable_defaults();
        assert_eq!(
            defaults["max_iter"],
            serde_yaml::Value::Number(serde_yaml::Number::from(5))
        );
    }

    #[test]
    fn parses_pipeline_with_edges_and_variables() {
        let yaml = with_start_end(
            r#"
name: full-pipeline
version: "2.0"
variables:
  max_iter: 5
  threshold: 0.8
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: plan
    outputs:
      - name: summary
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 1);
        assert_eq!(result.pipeline.variables.len(), 2);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn warns_on_edge_to_nonexistent_node() {
        let yaml = with_start_end(
            r#"
name: bad-edge
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ghost, port: plan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("non-existent node 'ghost'")));
    }

    #[test]
    fn warns_on_port_name_typo() {
        let yaml = with_start_end(
            r#"
name: bad-port
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    outputs:
      - name: plan
  - id: ab000002
    name: implementer
    type: doc-only
    inputs:
      - name: plan
edges:
  - source: { node: ab000001, port: plaan }
    target: { node: ab000002, port: plaan }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings
            .iter()
            .any(|w| w.contains("source port 'plaan' not found")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("target port 'plaan' not found")));
    }

    #[test]
    fn no_warning_on_cycle_in_topology() {
        let yaml = with_start_end(
            r#"
name: cycle
nodes:
  - id: ab000001
    name: implementer
    type: doc-only
    inputs:
      - name: review
    outputs:
      - name: code
  - id: ab000002
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: code }
    target: { node: ab000002, port: code }
  - source: { node: ab000002, port: review }
    target: { node: ab000001, port: review }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "cycle should not produce warnings, got: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parses_nodes_with_view_positions() {
        let yaml = with_start_end(
            r#"
name: with-view
nodes:
  - id: ab12cd34
    name: planner
    type: doc-only
    view: { x: 100, y: 200 }
    outputs:
      - name: plan
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let view = node.view.as_ref().unwrap();
        assert_eq!(view.x, 100.0);
        assert_eq!(view.y, 200.0);
    }

    #[test]
    fn rejects_edge_with_when_clause() {
        let yaml = with_start_end(
            r#"
name: conditional
nodes:
  - id: ab000001
    name: reviewer
    type: doc-only
    outputs:
      - name: review
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: review
    outputs:
      - name: code
edges:
  - source: { node: ab000001, port: review }
    target: { node: ab000002, port: review }
    when:
      iter: { lt: 5 }
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("edges no longer accept 'when:'"),
            "error should mention when removal: {msg}"
        );
    }

    #[test]
    fn parses_multiple_nodes_with_multiple_ports() {
        let yaml = with_start_end(
            r#"
name: multi-port
nodes:
  - id: ab000001
    name: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
      - name: task_list
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: plan
      - name: task_list
    outputs:
      - name: summary
  - id: ab000003
    name: reviewer
    type: doc-only
    inputs:
      - name: summary
    outputs:
      - name: review
edges:
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: plan }
  - source: { node: ab000001, port: task_list }
    target: { node: ab000002, port: task_list }
  - source: { node: ab000002, port: summary }
    target: { node: ab000003, port: summary }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert_eq!(result.pipeline.edges.len(), 3);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn parses_output_port_with_frontmatter_schema() {
        let yaml = with_start_end(
            r#"
name: with-schema
nodes:
  - id: ab12cd34
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &node.outputs[0];
        assert_eq!(port.name, "review");
        let schema = port.frontmatter.as_ref().unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema["verdict"].field_type, "enum");
        assert_eq!(
            schema["verdict"].allowed,
            Some(vec!["PASS".into(), "FAIL".into()])
        );
        assert_eq!(schema["score"].field_type, "int");
        assert!(schema["score"].allowed.is_none());
    }

    #[test]
    fn port_side_defaults_left_for_inputs_right_for_outputs() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
    }

    #[test]
    fn parses_explicit_port_side_all_four_values() {
        let yaml = with_start_end(
            r#"
name: sides-test
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs:
      - name: left-in
        side: left
      - name: right-in
        side: right
      - name: top-in
        side: top
      - name: bottom-in
        side: bottom
    outputs:
      - name: left-out
        side: left
      - name: right-out
        side: right
      - name: top-out
        side: top
      - name: bottom-out
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        assert_eq!(node.inputs[1].side, Some(PortSide::Right));
        assert_eq!(node.inputs[2].side, Some(PortSide::Top));
        assert_eq!(node.inputs[3].side, Some(PortSide::Bottom));

        assert_eq!(node.outputs[0].side, Some(PortSide::Left));
        assert_eq!(node.outputs[1].side, Some(PortSide::Right));
        assert_eq!(node.outputs[2].side, Some(PortSide::Top));
        assert_eq!(node.outputs[3].side, Some(PortSide::Bottom));
    }

    #[test]
    fn port_side_omitted_gets_contextual_default() {
        let yaml = with_start_end(
            r#"
name: defaults-test
nodes:
  - id: ab12cd34
    name: worker
    type: doc-only
    inputs:
      - name: a
      - name: b
        side: top
    outputs:
      - name: x
      - name: y
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();

        // Omitted input defaults to left
        assert_eq!(node.inputs[0].side, Some(PortSide::Left));
        // Explicit side preserved
        assert_eq!(node.inputs[1].side, Some(PortSide::Top));
        // Omitted output defaults to right
        assert_eq!(node.outputs[0].side, Some(PortSide::Right));
        // Explicit side preserved
        assert_eq!(node.outputs[1].side, Some(PortSide::Bottom));
    }

    #[test]
    fn output_port_without_frontmatter_has_none() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let planner = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab12cd34")
            .unwrap();
        let port = &planner.outputs[0];
        assert!(port.frontmatter.is_none());
    }

    #[test]
    fn canonical_prompt_path_for_template() {
        let pp = std::path::Path::new("/pipelines/review-loop.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/pipelines/review-loop.prompts/ab12cd34.md"
        );
    }

    #[test]
    fn canonical_prompt_path_for_run() {
        let pp = std::path::Path::new("/runs/run-1/pipeline.yaml");
        let path = canonical_prompt_path(pp, "ab12cd34");
        assert_eq!(
            path.to_str().unwrap(),
            "/runs/run-1/pipeline.prompts/ab12cd34.md"
        );
    }

    // --- auto_merge_resolver tests (issue #8) ---

    #[test]
    fn auto_merge_resolver_defaults_to_true() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        assert!(result.pipeline.auto_merge_resolver);
    }

    #[test]
    fn auto_merge_resolver_explicit_false() {
        let yaml = with_start_end(
            r#"
name: no-resolver
auto_merge_resolver: false
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(!result.pipeline.auto_merge_resolver);
    }

    #[test]
    fn auto_merge_resolver_explicit_true() {
        let yaml = with_start_end(
            r#"
name: with-resolver
auto_merge_resolver: true
nodes: []
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        assert!(result.pipeline.auto_merge_resolver);
    }

    // --- Switch node tests (issue #46) ---

    #[test]
    fn parses_switch_node_with_when_on_outputs() {
        let yaml = with_start_end(
            r#"
name: switch-test
nodes:
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.node_type, NodeType::Switch);
        assert_eq!(sw.inputs.len(), 1);
        assert_eq!(sw.inputs[0].name, "in");
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[0].name, "pass");
        assert!(sw.outputs[0].when.is_some());
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
    }

    #[test]
    fn switch_node_auto_appends_default_if_missing() {
        let yaml = with_start_end(
            r#"
name: switch-no-default
nodes:
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        assert_eq!(sw.outputs[1].name, "default");
        assert!(sw.outputs[1].when.is_none());
        assert_eq!(sw.outputs[1].side, Some(PortSide::Right));
    }

    #[test]
    fn switch_node_explicit_default_not_duplicated() {
        let yaml = with_start_end(
            r#"
name: switch-explicit-default
nodes:
  - id: ab000001
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS] }
      - name: default
        side: bottom
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let sw = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(sw.outputs.len(), 2);
        let default_port = sw.outputs.iter().find(|p| p.name == "default").unwrap();
        assert_eq!(default_port.side, Some(PortSide::Bottom));
    }

    #[test]
    fn switch_node_rejects_wrong_input_name() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: data
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input 'in': {msg}"
        );
    }

    #[test]
    fn switch_node_rejects_multiple_inputs() {
        let yaml = with_start_end(
            r#"
name: bad-switch
nodes:
  - id: ab000001
    name: bad-switch
    type: switch
    inputs:
      - name: in
      - name: extra
    outputs:
      - name: default
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exactly one input named 'in'"),
            "error should mention input constraint: {msg}"
        );
    }

    // --- Loop node tests (issue #46) ---

    #[test]
    fn parses_loop_node_with_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-test
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: 5
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        assert_eq!(lp.node_type, NodeType::Loop);
        assert_eq!(lp.inputs.len(), 2);
        assert_eq!(lp.outputs.len(), 2);
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_u64(), Some(5));
    }

    #[test]
    fn parses_loop_node_with_variable_max_iter() {
        let yaml = with_start_end(
            r#"
name: loop-var-test
variables:
  max_iter_review: 3
nodes:
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let result = parse_pipeline(&yaml).unwrap();
        let lp = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "ab000001")
            .unwrap();
        let max_iter = lp.max_iter.as_ref().unwrap();
        assert_eq!(max_iter.as_str(), Some("$max_iter_review"));
    }

    #[test]
    fn loop_node_rejects_missing_max_iter() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must declare 'max_iter'"),
            "error should mention max_iter: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_break_input() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
    outputs:
      - name: body
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have input 'break'"),
            "error should mention break: {msg}"
        );
    }

    #[test]
    fn loop_node_rejects_missing_body_output() {
        let yaml = with_start_end(
            r#"
name: bad-loop
nodes:
  - id: ab000001
    name: bad-loop
    type: loop
    max_iter: 3
    inputs:
      - name: in
      - name: break
    outputs:
      - name: done
"#,
        );
        let err = parse_pipeline(&yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must have output 'body'"),
            "error should mention body: {msg}"
        );
    }

    // --- Round-trip test (issue #46) ---

    #[test]
    fn round_trip_loop_switch_pipeline() {
        let yaml = r#"
name: review-loop
version: "1.0"
variables:
  max_iter_review: 3
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: review-loop
    type: loop
    max_iter: "$max_iter_review"
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
  - id: ab000002
    name: implementer
    type: code-mutating
    inputs:
      - name: task
    outputs:
      - name: code
  - id: ab000003
    name: reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL, APPROVED]
  - id: ab000004
    name: verdict-switch
    type: switch
    inputs:
      - name: in
    outputs:
      - name: pass
        when:
          verdict: { in: [PASS, APPROVED] }
      - name: default
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: in }
  - source: { node: ab000001, port: body }
    target: { node: ab000002, port: task }
  - source: { node: ab000002, port: code }
    target: { node: ab000003, port: code }
  - source: { node: ab000003, port: review }
    target: { node: ab000004, port: in }
  - source: { node: ab000004, port: pass }
    target: { node: ab000001, port: break }
  - source: { node: ab000001, port: done }
    target: { node: end, port: result }
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert_eq!(result.pipeline.nodes.len(), 6);
        assert_eq!(result.pipeline.edges.len(), 6);

        let loop_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Loop)
            .unwrap();
        assert_eq!(
            loop_node.max_iter.as_ref().unwrap().as_str(),
            Some("$max_iter_review")
        );

        let switch_node = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Switch)
            .unwrap();
        assert_eq!(switch_node.outputs.len(), 2);

        // Re-serialize and re-parse — no drift
        let serialized = serde_yaml::to_string(&result.pipeline).unwrap();
        let result2 = parse_pipeline(&serialized).unwrap();
        assert_eq!(result2.pipeline.name, result.pipeline.name);
        assert_eq!(result2.pipeline.nodes.len(), result.pipeline.nodes.len());
        assert_eq!(result2.pipeline.edges.len(), result.pipeline.edges.len());
    }

    // --- Existing variants unchanged (issue #46) ---

    #[test]
    fn parse_fixture_review_loop_yaml() {
        let yaml = include_str!("../../../.maestro/pipelines/review-loop.yaml");
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.name, "review-loop");
        assert!(result
            .pipeline
            .nodes
            .iter()
            .any(|n| n.node_type == NodeType::Loop));
        assert!(result
            .pipeline
            .nodes
            .iter()
            .any(|n| n.node_type == NodeType::Switch));
    }

    #[test]
    fn existing_node_types_still_parse() {
        let result = parse_pipeline(VALID_MINIMAL).unwrap();
        let types: Vec<&NodeType> = result.pipeline.nodes.iter().map(|n| &n.node_type).collect();
        assert!(types.contains(&&NodeType::Start));
        assert!(types.contains(&&NodeType::End));
        assert!(types.contains(&&NodeType::DocOnly));
    }
}
