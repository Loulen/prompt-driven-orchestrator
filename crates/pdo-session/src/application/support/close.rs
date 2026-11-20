//! Close the Agent de support of an Actor.

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use super::Support;
use crate::domain::names::support_session_name_of;

impl Support {
    /// End the agent of `actor`: the Operation `session` × `delete` on its
    /// session. Its record forgets the agent and keeps the Actor's last
    /// choice. Whether an agent was open; closing none is no error.
    pub fn close(&self, actor: &Actor) -> Result<bool, Denied> {
        let session = support_session_name_of(actor);
        let operation = Operation::new(ResourceKind::Session, Verb::Delete).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation)?;
        let existed = self.tmux.session_exists(&session);
        self.tmux.kill(&session);
        let mut record = self.record(&session);
        if record.agent.take().is_some() {
            if let Err(e) = self.write_record(&session, &record) {
                warn!("Support agent {session}: its record was not written: {e}");
            }
        }
        if existed {
            if self.tmux.session_exists(&session) {
                warn!("Failed to close support agent {session}: the tmux session is still alive");
            } else {
                info!("Closed support agent {session}");
            }
        }
        Ok(existed)
    }
}
