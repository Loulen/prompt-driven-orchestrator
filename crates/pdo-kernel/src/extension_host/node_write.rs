//! The port through which an extension's [`Host`](pdo_extension_api::Host)
//! writes into the session of a Node that waits for a person, on behalf of an
//! Actor (API 0.23).

use std::sync::Arc;

use pdo_extension_api::{NodeWrite, NodeWriting};

use super::Extensions;
use crate::Actor;

/// What writes into the session of a Node (a port: the session context in the
/// daemon).
pub trait NodeWrites: Send + Sync {
    /// Write `write` on behalf of `actor`, as if it typed the text and
    /// pressed Enter in the Node's terminal.
    fn write(&self, actor: Actor, write: NodeWrite) -> NodeWriting;
}

impl Extensions {
    /// Plug in what writes into the sessions of the Nodes, once the daemon
    /// wired its contexts. Until then a `Host::write_to_node` answers `503`.
    pub fn serve_node_writes(&self, writes: Arc<dyn NodeWrites>) {
        if self.registry.node_writes.set(writes).is_err() {
            tracing::warn!("the writes into the Nodes were plugged twice; the second is ignored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::{Extension, Manifest, NodeNotWritten, NodeWritten};

    struct Quiet;
    impl Extension for Quiet {
        fn manifest(&self) -> Manifest {
            Manifest::new("quiet", "Quiet", "0.0.0")
        }
    }

    /// Writes into `r1/n1` alone, naming who wrote in the session.
    struct OneNode;
    impl NodeWrites for OneNode {
        fn write(&self, actor: Actor, write: NodeWrite) -> NodeWriting {
            let answer = if write.run_id == "r1" && write.node_id == "n1" {
                Ok(NodeWritten::new(
                    "r1",
                    "n1",
                    1,
                    format!("by-{}", actor.kind()),
                ))
            } else {
                Err(NodeNotWritten::not_waiting(false))
            };
            Box::pin(std::future::ready(answer))
        }
    }

    #[tokio::test]
    async fn a_host_writes_into_a_node_once_the_daemon_plugs_the_writes() {
        let extensions = Extensions::register(vec![Box::new(Quiet)]).unwrap();
        let host = extensions.host_of("quiet");
        let before = host
            .write_to_node(&Actor::LocalOperator, NodeWrite::new("r1", "n1", "go"))
            .await
            .unwrap_err();
        assert_eq!(
            (before.status, before.reason.as_str()),
            (503, "unavailable")
        );

        extensions.serve_node_writes(Arc::new(OneNode));
        let written = host
            .write_to_node(&Actor::LocalOperator, NodeWrite::new("r1", "n1", "go"))
            .await
            .unwrap();
        assert_eq!(written.session, "by-local_operator");
        let refused = host
            .write_to_node(&Actor::LocalOperator, NodeWrite::new("r1", "n2", "go"))
            .await
            .unwrap_err();
        assert!(refused.is_not_waiting());
    }
}
