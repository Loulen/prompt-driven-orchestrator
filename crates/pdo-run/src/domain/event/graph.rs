//! The pipeline graph a Run froze at start, as its `RunStarted` payload carries
//! it: the edges and the nodes with their ports.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EdgeInfo {
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub halt_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Record<string, unknown>")]
    pub when_clause: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortBrief {
    pub name: String,
    /// The port's side, or its default side when the pipeline sets none.
    #[ts(as = "pdo_pipeline::domain::pipeline::PortSide")]
    pub side: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NodeDefInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// A `NodeType` wire token (`NodeType::as_str`).
    #[ts(as = "pdo_pipeline::domain::pipeline::NodeType")]
    pub node_type: String,
    /// Where this node works (#653, ADR-0060), as the Run's pipeline snapshot
    /// froze it: `true` ⇒ its own sub-worktree, `false` ⇒ the Run worktree.
    /// `None` for a type that carries no isolation (`merge` is isolated by
    /// construction; structural nodes have no worktree of their own) and for a
    /// pre-#653 snapshot. `skip_serializing_if` keeps the wire byte-identical
    /// when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub isolated_worktree: Option<bool>,
    /// The « Orchestrator » toggle (#723/#724, ADR-0064) as the Run's snapshot
    /// froze it at start: `true` ⇒ the strong orchestrator↔children binding
    /// holds this node's NodeRun. `false` (default) keeps every pre-#723 run
    /// unbound; `skip_serializing_if` keeps the wire byte-identical when unset.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub orchestrator: bool,
    pub view_x: Option<f64>,
    pub view_y: Option<f64>,
    pub inputs: Vec<PortBrief>,
    pub outputs: Vec<PortBrief>,
}
