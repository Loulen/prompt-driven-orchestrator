//! Close the Agent de support of an Actor.

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::Support;
use crate::domain::names::support_session_name_of;
use crate::domain::support_idle::SupportChange;

impl Support {
    /// End the agent of `actor`: the Operation `session` × `delete` on its
    /// session. Its record forgets the agent and keeps the Actor's last
    /// choice; every tab of the Actor hears it closed. Whether an agent was
    /// open; closing none is no error.
    pub fn close(&self, actor: &Actor) -> Result<bool, Denied> {
        let session = support_session_name_of(actor);
        let operation = Operation::new(ResourceKind::Session, Verb::Delete).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation)?;
        // A harness that exited on its own since the last pass of the watch
        // left its clock: its end is told all the same.
        let watched = self.watched().contains_key(&session);
        let existed = self.end(&session);
        if existed || watched {
            info!("Closed support agent {session}");
            self.tell(&session, SupportChange::Closed { idle: false });
        }
        Ok(existed)
    }
}
