//! The declared wait of a Session hébergée (ADR-0091 §4): its agent says it
//! waits for a person (`pdo wait-user`), and the Enter of a person on its
//! terminal lifts it, as for a Node (ADR-0069 §1).

use pdo_extension_api::HostedState;
use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::Hosted;
use crate::domain::declared_wait::{truncate_message, DeclareOutcome};
use crate::domain::hosted::WaitDeclared;

/// The action of the `session` Operation that declares a wait.
pub const WAIT_USER: &str = "wait_user";

/// The action of the `session` Operation that lifts a wait (the Enter of a
/// person on its terminal).
const RESUME: &str = "resume";

/// Why a wait was not declared on a Session hébergée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedWaitError {
    /// No Session hébergée is recorded under that name.
    NotFound,
    /// It holds no running agent: it is starting, ended, or its tmux session
    /// is gone.
    NotRunning,
    /// The Operation `session` × `wait_user` was refused.
    Refused(Denied),
}

impl HostedWaitError {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::NotFound => "hosted_session_not_found",
            Self::NotRunning => "hosted_session_not_running",
            Self::Refused(_) => "forbidden",
        }
    }

    pub fn message(&self, id: &str) -> String {
        match self {
            Self::NotFound => format!("no hosted session {id}"),
            Self::NotRunning => format!("the hosted session {id} is not running"),
            Self::Refused(denied) => denied.to_string(),
        }
    }
}

impl Hosted {
    /// The agent of the session `id` declares it waits for a person, saying
    /// `message`: the Operation `session` × `wait_user` on it, in its Projet,
    /// then `awaiting_user`, published. A repeat with the same message is a
    /// no-op; a new message refreshes the wait. The Enter rule counts from
    /// here.
    pub fn declare_wait(
        &self,
        actor: &Actor,
        id: &str,
        message: Option<&str>,
    ) -> Result<DeclareOutcome, HostedWaitError> {
        // Asked before the session is looked up: a refused caller learns
        // nothing of it.
        let operation = Operation::new(ResourceKind::Session, Verb::Action(WAIT_USER.into()))
            .on(id)
            .in_projects(self.registry.projects_of(id));
        authorize(self.host.as_ref(), actor, &operation).map_err(HostedWaitError::Refused)?;
        let record = self.registry.by_id(id).ok_or(HostedWaitError::NotFound)?;
        let holds_agent = matches!(
            record.session.state,
            HostedState::Running | HostedState::AwaitingUser
        );
        if !holds_agent || !self.tmux.session_exists(id) {
            return Err(HostedWaitError::NotRunning);
        }
        let message = message.map(truncate_message).filter(|m| !m.is_empty());
        let now = pdo_kernel::clock::now_iso();
        let mut declared = None;
        let updated = self.registry.update(id, |r| {
            declared = r.await_user(message.clone(), &now);
            declared.is_some()
        });
        match (updated, declared) {
            (None, _) => Err(HostedWaitError::NotFound),
            (Some((record, _)), None) if record.session.state == HostedState::AwaitingUser => {
                Ok(DeclareOutcome::Noop)
            }
            (Some(_), None) => Err(HostedWaitError::NotRunning),
            (Some(_), Some(declared)) => {
                self.typed.forget(id);
                info!(
                    "Hosted session {id} declared a wait{}",
                    message.map(|m| format!(": {m}")).unwrap_or_default()
                );
                Ok(match declared {
                    WaitDeclared::Declared => DeclareOutcome::Declared,
                    WaitDeclared::Refreshed => DeclareOutcome::Refreshed,
                })
            }
        }
    }

    /// A person's Enter reached the session `id`: if it waits, the Operation
    /// `session` × `resume` on it is asked for `actor`, and it runs again,
    /// published. Whether the wait was lifted.
    pub fn lift_wait(&self, actor: &Actor, id: &str) -> Result<bool, String> {
        let Some(record) = self.registry.by_id(id) else {
            return Ok(false);
        };
        if record.session.state != HostedState::AwaitingUser {
            return Ok(false);
        }
        self.authorize(actor, RESUME, &record)
            .map_err(|refusal| refusal.message)?;
        let now = pdo_kernel::clock::now_iso();
        let lifted = matches!(
            self.registry.update(id, |r| r.resume(&now)),
            Some((_, true))
        );
        if lifted {
            info!("Hosted session {id} resumed on the user's Enter");
        }
        Ok(lifted)
    }
}
