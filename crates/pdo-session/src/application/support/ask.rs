//! Ask the live Agent de support of an Actor a question.

use pdo_kernel::authorization::authorize;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::{Support, SupportError};
use crate::domain::names::support_session_name_of;
use crate::domain::support::question_text;

/// The action of the `session` Operation that types a question into an Agent
/// de support.
pub const ASK: &str = "ask";

impl Support {
    /// Type `question` into the live agent of `actor` and submit it, as the
    /// person would: one paste, then Enter, never prefixed as the runtime's.
    /// The Operation `session` × `ask` on its session. Answers the session.
    pub fn ask(&self, actor: &Actor, question: &str) -> Result<String, SupportError> {
        let session = support_session_name_of(actor);
        let operation =
            Operation::new(ResourceKind::Session, Verb::Action(ASK.into())).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(SupportError::Refused)?;
        let question = question_text(question).ok_or(SupportError::EmptyQuestion)?;
        if !self.tmux.session_exists(&session) {
            return Err(SupportError::NotOpen);
        }
        // The paste, then its Enter: either refused, the question did not land.
        if !self.tmux.paste_without_enter(&session, question) || !self.tmux.press_enter(&session) {
            return Err(SupportError::NotDelivered);
        }
        // A question is its person's input: the agent's clock starts over.
        self.input(&session);
        Ok(session)
    }
}
