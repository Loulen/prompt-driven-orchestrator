//! The pipeline graph a Run froze at start, as its `RunStarted` payload carries
//! it: the edges and the nodes with their ports.

use pdo_pipeline::domain::pipeline;
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

/// A port as the Run's snapshot summarises it. A port has no side: an event log
/// written before carries a `side` key, which reads back ignored.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortBrief {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    /// An *Output intermédiaire* (CONTEXT.md): shown to humans while the node
    /// works, never checked, never wired. Absent from the wire when `false`,
    /// so a pre-option snapshot reads unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional = nullable)]
    pub intermediate: bool,
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
    /// `None` for a type that carries no isolation (structural nodes have no
    /// worktree of their own), for a pre-#653 snapshot, and for a `merge` in an
    /// archived snapshot (the type is retired, ADR-0082 — its NodeRun's frozen
    /// isolation lives on `NodeStarted`). `skip_serializing_if` keeps the wire byte-identical
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
    /// The « on demand » option (ADR-0086) as the Run's snapshot froze it:
    /// `true` ⇒ only the user's ▶ launches this node, and until then it holds
    /// nothing. `skip_serializing_if` keeps the wire byte-identical when unset.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_demand: bool,
    pub view_x: Option<f64>,
    pub view_y: Option<f64>,
    pub inputs: Vec<PortBrief>,
    pub outputs: Vec<PortBrief>,
}

fn port_brief(p: &pipeline::Port) -> PortBrief {
    PortBrief {
        name: p.name.clone(),
        description: p.description.clone(),
        intermediate: p.intermediate,
    }
}

/// A pipeline node, as a Run freezes its definition.
pub fn node_def_from_pipeline(n: &pipeline::NodeDef) -> NodeDefInfo {
    NodeDefInfo {
        id: n.id.clone(),
        name: Some(n.name.clone()),
        node_type: n.node_type.as_str().into(),
        // #653/ADR-0060: the isolation the Run's snapshot froze for this node —
        // what the completion path reads back to know whether a sub-worktree has
        // to be merged, without re-deriving it from a type that no longer says.
        isolated_worktree: n.isolated_worktree,
        // #723/#724/ADR-0064: the « Orchestrator » toggle, frozen at run start —
        // what the strong binding gate reads to know whether this node's NodeRun
        // is held while its child runs are non-terminal.
        orchestrator: n.orchestrator,
        // ADR-0086: frozen like the other node options.
        on_demand: n.on_demand,
        view_x: n.view.as_ref().map(|v| v.x),
        view_y: n.view.as_ref().map(|v| v.y),
        inputs: n.inputs.iter().map(port_brief).collect(),
        outputs: n.outputs.iter().map(port_brief).collect(),
    }
}

/// A pipeline edge, as a Run freezes it.
pub fn edge_info_from_pipeline(e: &pipeline::EdgeDef) -> EdgeInfo {
    EdgeInfo {
        source_node: e.source.node.clone(),
        source_port: e.source.port().to_string(),
        target_node: e.target.node.clone(),
        target_port: e.target.port.clone(),
        halt_message: e.reason.clone(),
        when_clause: None,
    }
}
