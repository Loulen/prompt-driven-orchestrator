//! Lay a text in the input of a Session hébergée, set its Zoom du terminal,
//! and forget an ended one (ADR-0092).

use pdo_extension_api::{HostedRefusal, HostedSession, HostedSessionForgotten, HostedState};
use pdo_kernel::authorization::{authorize, authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::Hosted;
use crate::domain::hosted::{terminal_zoom, ZoomOutOfRange};

/// The action of the `session` Operation that lays a text in the input of a
/// Session hébergée.
pub const TYPE: &str = "type";

/// The action of the `session` Operation that forgets a Session hébergée.
pub const FORGET: &str = "forget";

/// The action of the `session` Operation that sets the Zoom du terminal of a
/// Session hébergée.
pub const ZOOM: &str = "zoom";

/// Why the Zoom du terminal of a session was not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedZoomError {
    /// No Session hébergée has that name.
    NotFound(String),
    OutOfRange(ZoomOutOfRange),
    /// The Operation `session` × `zoom` was refused.
    Refused(Denied),
}

impl HostedZoomError {
    /// Its HTTP status.
    pub fn status(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::OutOfRange(_) => 400,
            Self::Refused(_) => 403,
        }
    }
}

impl std::fmt::Display for HostedZoomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no hosted session `{id}`"),
            Self::OutOfRange(e) => e.fmt(f),
            Self::Refused(denied) => denied.fmt(f),
        }
    }
}

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

    /// Set the Zoom du terminal of the session `key` of `extension` on
    /// behalf of `actor` (the Host's gesture): see [`Self::zoom_by_id`].
    pub async fn zoom(
        &self,
        extension: &str,
        actor: &Actor,
        key: &str,
        zoom: u16,
    ) -> Result<HostedSession, HostedRefusal> {
        let record = self.recorded(extension, key)?;
        self.zoom_by_id(actor, record.id(), zoom)
            .map_err(|e| match e {
                HostedZoomError::Refused(denied) => HostedRefusal::new(403, denied.to_string()),
                other => HostedRefusal::new(other.status(), other.to_string()),
            })
    }

    /// Set the Zoom du terminal of the session named `id` on behalf of
    /// `actor` (the API's gesture, from any Poste). The Operation `session`
    /// × `zoom` on it, in its Projet, is asked first: a refused caller learns
    /// nothing of the session. Then a name no Session hébergée has is
    /// `NotFound` and a zoom out of range `OutOfRange`; otherwise its Zoom du
    /// terminal is `zoom` percent, recorded and published: every terminal on
    /// it follows. Live or ended, agent or shell.
    pub fn zoom_by_id(
        &self,
        actor: &Actor,
        id: &str,
        zoom: u16,
    ) -> Result<HostedSession, HostedZoomError> {
        let operation = Operation::new(ResourceKind::Session, Verb::Action(ZOOM.into()))
            .on(id)
            .in_projects(self.registry.projects_of(id));
        authorize(self.host.as_ref(), actor, &operation).map_err(HostedZoomError::Refused)?;
        let not_found = || HostedZoomError::NotFound(id.to_string());
        self.registry.by_id(id).ok_or_else(not_found)?;
        terminal_zoom(zoom).map_err(HostedZoomError::OutOfRange)?;
        let now = pdo_kernel::clock::now_iso();
        // Forgotten in between: there is nothing left to zoom.
        let (record, _) = self
            .registry
            .update(id, |r| r.zoom(zoom, &now).unwrap_or(false))
            .ok_or_else(not_found)?;
        Ok(record.session)
    }

    /// The Zoom du terminal of the session named `id`, for `actor`: `None`
    /// for a name no Session hébergée has (its terminal is at 100 %), the
    /// refusal when `actor` may not run `session` × `read` on it.
    pub fn zoom_of(&self, actor: &Actor, id: &str) -> Result<Option<u16>, Denied> {
        let Some(record) = self.registry.by_id(id) else {
            return Ok(None);
        };
        let read = Operation::new(ResourceKind::Session, Verb::Read)
            .on(record.id())
            .in_projects(record.projects());
        authorize_read(self.host.as_ref(), actor, &read)?;
        Ok(Some(record.session.terminal_zoom))
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
