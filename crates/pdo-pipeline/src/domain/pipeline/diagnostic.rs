//! What a parse reports: its warnings, its refusals, and the parsed pipeline.

use super::definition::PipelineDef;

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
    /// A field that is present but meaningless where it sits (#669: `skills` on
    /// a `script` node). Refused at parse, naming the node and the fix.
    #[error("{0}")]
    InvalidField(String),
    #[error("switch node '{node_id}' output '{port}': when-clause field '{field}' not found in upstream schema")]
    UndeclaredWhenField {
        node_id: String,
        port: String,
        field: String,
    },
}
