//! A Session hébergée named by its id, as the detail of a session reads and
//! drives it from any Poste (the API's gestures). Each gesture is the
//! extension's on the same session: the same Opération on the Ressource
//! `session`, in its Projet, asked once. A refused caller learns nothing of
//! the session.

use std::sync::Arc;

use pdo_extension_api::{HostedConfig, HostedRefusal, HostedSession, LiveArtifact};
use pdo_kernel::authorization::{authorize_read, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::{Hosted, CONFIGURE, RELAUNCH, TERMINATE};
use crate::domain::hosted::{HostedRecord, HostedSessionEvent};

/// Why a gesture on a Session hébergée named by its id was not done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedByIdError {
    /// Its Opération was refused, before anything was read.
    Refused(Denied),
    /// No such session (`404`), or not in a state for it.
    Refusal(HostedRefusal),
}

impl HostedByIdError {
    /// Its HTTP status.
    pub fn status(&self) -> u16 {
        match self {
            Self::Refused(_) => 403,
            Self::Refusal(refusal) => refusal.status,
        }
    }
}

impl From<HostedRefusal> for HostedByIdError {
    fn from(refusal: HostedRefusal) -> Self {
        Self::Refusal(refusal)
    }
}

impl Hosted {
    /// The session named `id`, for `actor`: the Opération `session` × `read`
    /// on it, in its Projet, first; then `404` for a name no Session
    /// hébergée has.
    pub fn read_by_id(&self, actor: &Actor, id: &str) -> Result<HostedRecord, HostedByIdError> {
        let read = Operation::new(ResourceKind::Session, Verb::Read)
            .on(id)
            .in_projects(self.registry.projects_of(id));
        authorize_read(self.host.as_ref(), actor, &read).map_err(HostedByIdError::Refused)?;
        self.registry.by_id(id).ok_or_else(|| not_found(id).into())
    }

    /// The journal of the session named `id`, oldest first, read as
    /// [`Self::read_by_id`].
    pub fn events_by_id(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<Vec<HostedSessionEvent>, HostedByIdError> {
        Ok(self.read_by_id(actor, id)?.journal)
    }

    /// The Artefacts live of the session named `id`, by path, read as
    /// [`Self::read_by_id`].
    pub fn live_by_id(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<Vec<LiveArtifact>, HostedByIdError> {
        let record = self.read_by_id(actor, id)?;
        Ok(self.live_of(&record))
    }

    /// The content of the Artefact live `path` of the session named `id`,
    /// read as [`Self::read_by_id`], then as [`Self::live_artifact`].
    pub fn live_artifact_by_id(
        &self,
        actor: &Actor,
        id: &str,
        path: &str,
    ) -> Result<Vec<u8>, HostedByIdError> {
        let record = self.read_by_id(actor, id)?;
        Ok(self.live_artifact(&record.session.extension, &record.session.key, path)?)
    }

    /// Terminate the session named `id` on behalf of `actor`: see
    /// [`Self::terminate`].
    pub fn terminate_by_id(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<HostedSession, HostedByIdError> {
        let record = self.allowed_on(actor, TERMINATE, id)?;
        Ok(self.terminate_record(actor, record))
    }

    /// Relaunch the ended session named `id` on behalf of `actor`: see
    /// [`Self::relaunch`].
    pub async fn relaunch_by_id(
        self: &Arc<Self>,
        actor: &Actor,
        id: &str,
    ) -> Result<HostedSession, HostedByIdError> {
        let record = self.allowed_on(actor, RELAUNCH, id)?;
        Ok(self.relaunch_record(actor, record).await?)
    }

    /// Change the configuration of the agent session named `id` on behalf of
    /// `actor`: see [`Self::configure`].
    pub async fn configure_by_id(
        &self,
        actor: &Actor,
        id: &str,
        config: HostedConfig,
    ) -> Result<HostedSession, HostedByIdError> {
        let record = self.allowed_on(actor, CONFIGURE, id)?;
        Ok(self.configure_record(actor, record, config).await?)
    }

    /// The Opération `session` × `action` on the session named `id`, then its
    /// record: a refused caller gets its refusal, not a `404`.
    fn allowed_on(
        &self,
        actor: &Actor,
        action: &str,
        id: &str,
    ) -> Result<HostedRecord, HostedByIdError> {
        self.allowed(actor, action, id)
            .map_err(HostedByIdError::Refused)?;
        self.registry.by_id(id).ok_or_else(|| not_found(id).into())
    }
}

fn not_found(id: &str) -> HostedRefusal {
    HostedRefusal::new(404, format!("no hosted session `{id}`"))
}
