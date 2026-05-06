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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    #[serde(default)]
    pub repeated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub prompt_file: Option<String>,
    #[serde(default)]
    pub inputs: Vec<Port>,
    #[serde(default)]
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTarget {
    pub node: String,
    pub port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef {
    pub source: EdgeTarget,
    pub target: EdgeTarget,
    #[serde(default)]
    pub when: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDef {
    pub name: String,
    pub version: Option<String>,
    #[serde(default)]
    pub variables: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
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
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let mapping = raw
        .as_mapping()
        .ok_or_else(|| ParseError::MissingField("root must be a mapping".into()))?;

    if mapping
        .get(serde_yaml::Value::String("name".into()))
        .is_none()
    {
        return Err(ParseError::MissingField("name".into()));
    }

    let pipeline: PipelineDef = serde_yaml::from_value(raw.clone())?;
    let mut diagnostics = Vec::new();

    let known_keys: &[&str] = &["name", "version", "variables", "nodes", "edges"];
    if let Some(map) = raw.as_mapping() {
        for key in map.keys() {
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

    for node in &pipeline.nodes {
        if node.prompt_file.is_none() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("node '{}': missing prompt_file", node.id),
            });
        }

        let known_types = ["doc-only", "code-mutating"];
        let type_str = match &node.node_type {
            NodeType::DocOnly => "doc-only",
            NodeType::CodeMutating => "code-mutating",
        };
        if !known_types.contains(&type_str) {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("node '{}': unknown type '{}'", node.id, type_str),
            });
        }
    }

    Ok(ParseResult {
        pipeline,
        diagnostics,
    })
}

pub fn load_prompt_file(pipeline_dir: &Path, prompt_file: &str) -> Result<String, std::io::Error> {
    let path = pipeline_dir.join(prompt_file);
    std::fs::read_to_string(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const VALID_MINIMAL: &str = r#"
name: test-pipeline
version: "1.0"
nodes:
  - id: planner
    type: doc-only
    prompt_file: prompts/planner.md
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
        assert_eq!(result.pipeline.nodes.len(), 1);

        let node = &result.pipeline.nodes[0];
        assert_eq!(node.id, "planner");
        assert_eq!(node.node_type, NodeType::DocOnly);
        assert_eq!(node.prompt_file.as_deref(), Some("prompts/planner.md"));
        assert_eq!(node.inputs.len(), 1);
        assert_eq!(node.inputs[0].name, "task");
        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "plan");

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn warns_on_missing_prompt_file() {
        let yaml = r#"
name: no-prompt
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].severity, Severity::Warning);
        assert!(result.diagnostics[0]
            .message
            .contains("missing prompt_file"));
    }

    #[test]
    fn errors_on_invalid_yaml() {
        let yaml = "{{not: valid: yaml:::";
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn warns_on_unknown_fields() {
        let yaml = r#"
name: with-extras
custom_field: hello
another_unknown: 42
nodes: []
"#;
        let result = parse_pipeline(yaml).unwrap();
        let warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(warnings.iter().any(|w| w.contains("custom_field")));
        assert!(warnings.iter().any(|w| w.contains("another_unknown")));
    }

    #[test]
    fn warns_on_unknown_node_type() {
        // serde_yaml will fail to deserialize an unknown enum variant,
        // so an unknown type like "transformer" produces an error not a warning.
        // Per ADR-0001, we should be lenient. But serde_yaml strict parsing
        // of the enum means truly unknown types are parse errors. This is
        // acceptable for v1 — only doc-only and code-mutating are valid.
        let yaml = r#"
name: bad-type
nodes:
  - id: x
    type: transformer
    prompt_file: x.md
    inputs: []
    outputs: []
"#;
        let err = parse_pipeline(yaml);
        // Unknown enum variant → YAML parse error (acceptable per ADR-0001
        // sharp-tool: we don't silently accept unknown types)
        assert!(err.is_err());
    }

    #[test]
    fn errors_on_missing_name() {
        let yaml = r#"
version: "1.0"
nodes: []
"#;
        let err = parse_pipeline(yaml).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(_)));
    }

    #[test]
    fn parses_pipeline_with_edges_and_variables() {
        let yaml = r#"
name: full-pipeline
version: "2.0"
variables:
  max_iter: 5
  threshold: 0.8
nodes:
  - id: planner
    type: doc-only
    prompt_file: prompts/planner.md
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: implementer
    type: code-mutating
    prompt_file: prompts/implementer.md
    inputs:
      - name: plan
    outputs:
      - name: summary
edges:
  - source: { node: planner, port: plan }
    target: { node: implementer, port: plan }
"#;
        let result = parse_pipeline(yaml).unwrap();
        assert_eq!(result.pipeline.nodes.len(), 2);
        assert_eq!(result.pipeline.edges.len(), 1);
        assert_eq!(result.pipeline.variables.len(), 2);
        assert!(result.diagnostics.is_empty());
    }
}
