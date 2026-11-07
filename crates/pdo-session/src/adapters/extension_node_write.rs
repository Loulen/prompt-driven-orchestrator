//! The writes into the sessions of the Nodes as the extensions' [`Host`]
//! reaches them (API 0.23): the kernel's port, over the session context.
//!
//! [`Host`]: pdo_extension_api::Host

use std::sync::Arc;

use pdo_extension_api::{NodeWrite, NodeWriting};
use pdo_kernel::extension_host::NodeWrites;
use pdo_kernel::Actor;

use crate::application::write_to_node::WriteToNode;

/// The writes into the Nodes of the session context, for the extensions.
pub struct ExtensionNodeWrites(pub Arc<WriteToNode>);

impl NodeWrites for ExtensionNodeWrites {
    fn write(&self, actor: Actor, write: NodeWrite) -> NodeWriting {
        let writes = self.0.clone();
        Box::pin(async move { writes.execute(&actor, write).await })
    }
}
