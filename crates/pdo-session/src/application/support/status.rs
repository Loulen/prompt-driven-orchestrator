//! Read the Agent de support of an Actor.

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::Actor;

use super::Support;
use crate::domain::names::support_session_name_of;
use crate::domain::support::SupportAgent;
use crate::domain::support_idle::rfc3339;

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
    /// Its person's last input (RFC 3339), while it lives and the daemon
    /// watches it.
    pub last_input_at: Option<String>,
    /// When it last wrote in its terminal (RFC 3339), while it lives.
    pub last_output_at: Option<String>,
    /// « Still there? », while it is asked.
    pub still_there: Option<StillThere>,
}

/// « Still there? », asked: when the agent closes without an answer, and
/// after how long without input it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillThere {
    /// RFC 3339.
    pub closes_at: String,
    /// The seconds left now: what a countdown reads, whatever its clock says.
    pub closes_in_secs: u32,
    pub idle_timeout_secs: u32,
}

impl Support {
    /// The agent of `actor`. A read of its own: no Operation.
    pub fn status(&self, actor: &Actor) -> SupportStatus {
        let session = support_session_name_of(actor);
        let alive = self.tmux.session_exists(&session);
        let record = self.record(&session);
        let now = self.clock.now();
        let (last_input_at, still_there) = match self.watched().get(&session) {
            Some(watched) if alive => (
                Some(rfc3339(watched.idleness.last_input)),
                watched
                    .idleness
                    .closes_at()
                    .zip(watched.asked_after_secs)
                    .map(|(closes_at, idle_timeout_secs)| StillThere {
                        closes_at: rfc3339(closes_at),
                        closes_in_secs: watched.idleness.closes_in_secs(now).unwrap_or(0),
                        idle_timeout_secs,
                    }),
            ),
            _ => (None, None),
        };
        let last_output_at = alive
            .then(|| self.tmux.last_output_at(&session))
            .flatten()
            .map(|at| rfc3339(at.into()));
        SupportStatus {
            alive,
            agent: record.agent.filter(|_| alive),
            last_choice: record.last_choice,
            session,
            last_input_at,
            last_output_at,
            still_there,
        }
    }
}
