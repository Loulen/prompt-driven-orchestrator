//! Read the Agent de support of an Actor.

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::Actor;

use super::Support;
use crate::domain::names::support_session_name_of;
use crate::domain::support::SupportAgent;

/// The Agent de support of an Actor, as the interface and the CLI read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportStatus {
    /// Its session's name, live or not.
    pub session: String,
    /// Whether its session lives.
    pub alive: bool,
    /// The agent it runs, while it lives.
    pub agent: Option<SupportAgent>,
    /// The choice the Actor last opened one with: the next opening proposes
    /// it.
    pub last_choice: Option<AgentChoice>,
}

impl Support {
    /// The agent of `actor`. A read of its own: no Operation.
    pub fn status(&self, actor: &Actor) -> SupportStatus {
        let session = support_session_name_of(actor);
        let alive = self.tmux.session_exists(&session);
        let record = self.record(&session);
        SupportStatus {
            alive,
            agent: record.agent.filter(|_| alive),
            last_choice: record.last_choice,
            session,
        }
    }
}
