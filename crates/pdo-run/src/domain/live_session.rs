//! What a Run's projection says about one node for the gestures that speak to
//! its agent: the declared wait (the session context's), the text sent to its
//! input and the files imported for it.

use pdo_session::domain::declared_wait::{
    AwaitingMarker, IterationFacts, NodeWaitFacts, WaitFacts,
};

use crate::domain::state::{NodeStatus, RunState};

/// The facts of `node_id` in `run`, as the session context reads them.
pub fn wait_facts(run: &RunState, node_id: &str) -> WaitFacts {
    WaitFacts {
        run_live: run.status.is_live(),
        defined: run.node_defs.iter().any(|d| d.id == node_id),
        is_script: run
            .node_defs
            .iter()
            .any(|d| d.id == node_id && d.node_type == "script"),
        node: run.nodes.get(node_id).map(|node| NodeWaitFacts {
            iter: node.iter,
            awaiting: node.status == NodeStatus::AwaitingUser,
            marker: node.awaiting.as_ref().map(|a| AwaitingMarker {
                cause: a.cause.clone(),
                message: a.message.clone(),
            }),
            iterations: node
                .iterations
                .iter()
                .map(|a| IterationFacts {
                    iter: a.iter,
                    holds_session: a.status.holds_session(),
                })
                .collect(),
        }),
    }
}
