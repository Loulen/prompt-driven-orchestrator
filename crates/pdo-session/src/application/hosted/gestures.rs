//! Lay a text in the input of a Session hébergée, and forget an ended one
//! (ADR-0091).

use pdo_extension_api::{HostedRefusal, HostedSession, HostedSessionForgotten, HostedState};
use pdo_kernel::Actor;
use tracing::info;

use super::Hosted;

/// The action of the `session` Operation that lays a text in the input of a
/// Session hébergée.
pub const TYPE: &str = "type";

/// The action of the `session` Operation that forgets a Session hébergée.
pub const FORGET: &str = "forget";

impl Hosted {
    /// Lay `text` in the input of the live session `key` of `extension` on
    /// behalf of `actor`, as one bracketed paste and **without** Enter: it
    /// waits for a person to complete and submit it. The Operation `session`
    /// × `type` on it, in its Projet. The text counts as typed: the Enter
    /// that submits it lifts a declared wait.
    pub async fn type_text(
        &self,
        extension: &str,
        actor: &Actor,
        key: &str,
        text: &str,
    ) -> Result<HostedSession, HostedRefusal> {
        if text.is_empty() {
            return Err(HostedRefusal::new(400, "there is no text to lay"));
        }
        let record = self.recorded(extension, key)?;
        self.authorize(actor, TYPE, &record)?;
        if !matches!(
            record.session.state,
            HostedState::Running | HostedState::AwaitingUser
        ) {
            return Err(HostedRefusal::new(
                409,
                format!("the hosted session `{key}` is not running"),
            ));
        }
        let (tmux, id, text) = (self.tmux.clone(), record.id().to_string(), text.to_string());
        let pasted = tokio::task::spawn_blocking(move || tmux.paste_without_enter(&id, &text))
            .await
            .unwrap_or(false);
        if !pasted {
            return Err(HostedRefusal::new(
                409,
                format!("the text could not be laid in the hosted session `{key}`"),
            ));
        }
        self.typed.mark(record.id());
        Ok(record.session)
    }

    /// Forget the ended session `key` of `extension` on behalf of `actor`:
    /// the Operation `session` × `forget` on it, in its Projet, then its
    /// record and its state folder (its primer, its Artefacts live) are
    /// erased, and the forgetting published. Its key is free again.
    pub async fn forget(
        &self,
        extension: &str,
        actor: &Actor,
        key: &str,
    ) -> Result<HostedSession, HostedRefusal> {
        let record = self.recorded(extension, key)?;
        self.authorize(actor, FORGET, &record)?;
        // Ended under the registry's lock: a relaunch in between keeps it.
        if !self.registry.forget_ended(record.id()) {
            return Err(HostedRefusal::new(
                409,
                format!("the hosted session `{key}` is not ended: terminate it first"),
            ));
        }
        self.forget_live(record.id());
        self.registry.announce(
            &record,
            HostedSessionForgotten::new(record.session.clone()).to_event(),
        );
        info!("Forgot hosted session {}", record.id());
        Ok(record.session)
    }
}
