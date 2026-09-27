//! The Operations the gestures on a Run ask the Autorisation d'opération
//! about (ADR-0081): a node's is targeted `<run>/<node>`, a Run's by its id.
//! The scheduler's own transitions never build one (D10).

use pdo_kernel::{Operation, ResourceKind, Verb};

/// `action` on the node `node_id` of `run_id`.
pub fn on_node(action: &str, run_id: &str, node_id: &str) -> Operation {
    Operation::new(ResourceKind::Node, Verb::Action(action.to_string()))
        .on(format!("{run_id}/{node_id}"))
}

/// `action` on the Run `run_id`.
pub fn on_run(action: &str, run_id: &str) -> Operation {
    Operation::new(ResourceKind::Run, Verb::Action(action.to_string())).on(run_id)
}
