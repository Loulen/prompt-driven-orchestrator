//! A text an extension has the core write into the session of a Node that
//! waits for a person (API 0.23), on behalf of an Actor, as if that Actor had
//! typed it and pressed Enter; and the deep link toward a Node, which leads a
//! person to that Node's terminal.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// The stable reason of a write refused because the Node does not wait for a
/// person: it runs, it ended, it waits on something else (its children), or
/// the core does not know it. Nothing was written.
pub const NODE_NOT_WAITING: &str = "not_waiting";

/// The message of a [`NODE_NOT_WAITING`] refusal.
pub const NODE_NOT_WAITING_MESSAGE: &str = "the node no longer waits";

/// The stable reason of a write refused by the Autorisation d'opération: the
/// Actor may not take control of the Node's terminal. Nothing was written.
pub const NODE_WRITE_REFUSED: &str = "refused";

/// The stable reason of a write whose text is blank or too long. Nothing was
/// written.
pub const NODE_WRITE_INVALID: &str = "invalid_text";

/// The stable reason of a write the terminal did not take (its paste or its
/// Enter failed).
pub const NODE_WRITE_FAILED: &str = "write_failed";

/// The stable reason of a write this host cannot make (no Node session held,
/// the daemon starting or stopping).
pub const NODE_WRITE_UNAVAILABLE: &str = "unavailable";

/// The longest text a write carries, in bytes: an answer, never a document.
pub const NODE_WRITE_MAX_BYTES: usize = 16 * 1024;

/// What an extension asks its [`Host`](crate::Host) to write
/// ([`Host::write_to_node`](crate::Host::write_to_node)): `text` into the
/// session of the Node `node_id` of the Run `run_id`, at the iteration `iter`
/// (its current one when `None`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NodeWrite {
    pub run_id: String,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iter: Option<i64>,
    pub text: String,
}

impl NodeWrite {
    /// `text` for the current iteration of `node_id` in `run_id`.
    pub fn new(
        run_id: impl Into<String>,
        node_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            node_id: node_id.into(),
            iter: None,
            text: text.into(),
        }
    }

    /// The same write, for the iteration `iter` only: refused as
    /// [`NODE_NOT_WAITING`] when the Node waits at another one.
    pub fn at_iter(mut self, iter: i64) -> Self {
        self.iter = Some(iter);
        self
    }
}

/// A write that reached the Node: the iteration whose session received it,
/// and that session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NodeWritten {
    pub run_id: String,
    pub node_id: String,
    pub iter: i64,
    pub session: String,
}

impl NodeWritten {
    pub fn new(
        run_id: impl Into<String>,
        node_id: impl Into<String>,
        iter: i64,
        session: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            node_id: node_id.into(),
            iter,
            session: session.into(),
        }
    }
}

/// Why nothing was written: an HTTP status, a stable reason (one of the
/// `NODE_*` constants) and a message a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NodeNotWritten {
    pub status: u16,
    pub reason: String,
    pub message: String,
}

impl NodeNotWritten {
    pub fn new(status: u16, reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            reason: reason.into(),
            message: message.into(),
        }
    }

    /// [`NODE_NOT_WAITING`]: `409`, or `404` when the core does not know the
    /// Run or the Node.
    pub fn not_waiting(known: bool) -> Self {
        Self::new(
            if known { 409 } else { 404 },
            NODE_NOT_WAITING,
            NODE_NOT_WAITING_MESSAGE,
        )
    }

    /// [`NODE_WRITE_REFUSED`]: `403`, with the refusal's reason.
    pub fn refused(message: impl Into<String>) -> Self {
        Self::new(403, NODE_WRITE_REFUSED, message)
    }

    /// [`NODE_WRITE_UNAVAILABLE`]: `503`.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(503, NODE_WRITE_UNAVAILABLE, message)
    }

    /// Whether the Node did not wait ([`NODE_NOT_WAITING`]).
    pub fn is_not_waiting(&self) -> bool {
        self.reason == NODE_NOT_WAITING
    }
}

impl fmt::Display for NodeNotWritten {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}, {})", self.message, self.reason, self.status)
    }
}

impl std::error::Error for NodeNotWritten {}

/// What [`Host::write_to_node`](crate::Host::write_to_node) answers.
pub type NodeWriting =
    Pin<Box<dyn Future<Output = Result<NodeWritten, NodeNotWritten>> + Send + 'static>>;

/// The deep link toward the Node `node_id` of the Run `run_id` (API 0.23),
/// at its iteration `iter` when given: `#/runs/<run>/nodes/<node>`, or
/// `#/runs/<run>/nodes/<node>/iter/<n>`. Opened in the interface, it selects
/// the Run, then the Node (at that iteration), and shows its detail with its
/// terminal; a Node the Run does not hold opens the Run alone. Each segment is
/// percent-encoded. Prefix it with the instance's public address to send it.
pub fn node_link(run_id: &str, node_id: &str, iter: Option<i64>) -> String {
    let mut link = format!("{}/nodes/{}", run_link(run_id), encode_segment(node_id));
    if let Some(iter) = iter {
        link.push_str(&format!("/iter/{iter}"));
    }
    link
}

/// The deep link toward the Run `run_id`: `#/runs/<run>`.
pub fn run_link(run_id: &str) -> String {
    format!("#/runs/{}", encode_segment(run_id))
}

/// `segment` with every byte outside the unreserved set percent-encoded, as
/// `encodeURIComponent` does for the ids PDO gives.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_link_names_the_run_the_node_and_its_iteration() {
        assert_eq!(node_link("r1", "worker", None), "#/runs/r1/nodes/worker");
        assert_eq!(
            node_link("20261106-224006-decf160", "XBG5Cxkn", Some(2)),
            "#/runs/20261106-224006-decf160/nodes/XBG5Cxkn/iter/2"
        );
        assert_eq!(run_link("r1"), "#/runs/r1");
        assert_eq!(node_link("r 1", "a/b", None), "#/runs/r%201/nodes/a%2Fb");
    }

    #[test]
    fn a_write_reads_back_from_json_without_its_iteration() {
        let write: NodeWrite = serde_json::from_value(
            serde_json::json!({"run_id": "r1", "node_id": "n1", "text": "go"}),
        )
        .unwrap();
        assert_eq!(write, NodeWrite::new("r1", "n1", "go"));
        assert_eq!(
            serde_json::to_value(NodeWrite::new("r1", "n1", "go").at_iter(2)).unwrap(),
            serde_json::json!({"run_id": "r1", "node_id": "n1", "iter": 2, "text": "go"})
        );
    }

    #[test]
    fn a_node_that_no_longer_waits_is_refused_with_a_stable_reason() {
        let known = NodeNotWritten::not_waiting(true);
        assert_eq!(known.status, 409);
        assert_eq!(known.reason, "not_waiting");
        assert_eq!(known.message, "the node no longer waits");
        assert!(known.is_not_waiting());
        assert_eq!(NodeNotWritten::not_waiting(false).status, 404);
        assert_eq!(NodeNotWritten::refused("no").status, 403);
        assert!(!NodeNotWritten::refused("no").is_not_waiting());
        assert_eq!(
            NodeNotWritten::unavailable("x").to_string(),
            "x (unavailable, 503)"
        );
    }
}
