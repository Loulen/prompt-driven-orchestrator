//! Relaunch an ended Session hébergée, and change the configuration it will
//! relaunch with (ADR-0092).

use std::sync::Arc;

use pdo_extension_api::{
    EndReason, HostedConfig, HostedKind, HostedRefusal, HostedSession, SessionKind, SessionRef,
};
use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::open::Agent;
use super::Hosted;
use crate::domain::hosted::{relaunch_of, HostedRecord, Relaunch, RelaunchHarness};

/// The action of the `session` Operation that relaunches a Session hébergée.
pub const RELAUNCH: &str = "relaunch";

/// The action of the `session` Operation that changes the configuration of a
/// Session hébergée.
pub const CONFIGURE: &str = "configure";

impl Hosted {
    /// Relaunch the ended session `key` of `extension` on behalf of `actor`,
    /// who now starts it: the Operation `session` × `relaunch` on it, in its
    /// Projet, its folder and harness resolved again, a slot of the Cap (or a
    /// refusal at once), then `starting` **before** its tmux session exists
    /// and launched as at its opening. An agent resumes the conversation of
    /// its record when its harness allows it ([`relaunch_of`]), and starts a
    /// new one otherwise; a shell starts anew. A launch that fails ends it
    /// again (`exited`); its record stays.
    pub async fn relaunch(
        self: &Arc<Self>,
        extension: &str,
        actor: &Actor,
        key: &str,
    ) -> Result<HostedSession, HostedRefusal> {
        let record = self.recorded(extension, key)?;
        self.authorize(actor, RELAUNCH, &record)?;
        self.relaunch_record(actor, record).await
    }

    /// Relaunch `record` on behalf of `actor`, once allowed: see
    /// [`Self::relaunch`].
    pub(super) async fn relaunch_record(
        self: &Arc<Self>,
        actor: &Actor,
        record: HostedRecord,
    ) -> Result<HostedSession, HostedRefusal> {
        let (extension, key) = (record.session.extension.clone(), record.session.key.clone());
        let (extension, key) = (extension.as_str(), key.as_str());
        if record.is_live() {
            return Err(HostedRefusal::new(
                409,
                format!("the hosted session `{key}` is not ended: terminate it first"),
            ));
        }
        let session = &record.session;
        let cwd = self
            .folder(&session.folder, session.project.as_deref())
            .await?;
        let harness = match session.kind {
            HostedKind::Agent => Some(self.harness(session.config.harness.as_deref()).await?),
            _ => None,
        };
        let relaunch = match &harness {
            Some(harness) => {
                let written = match &record.conversation_id {
                    Some(id) => {
                        let cwd = std::path::Path::new(&session.cwd);
                        self.harnesses
                            .conversation_written(&harness.name, cwd, id)
                            .await
                    }
                    None => None,
                };
                let of = RelaunchHarness::new(
                    &harness.name,
                    harness.can_resume() && !harness.resume_by_id.is_empty(),
                    harness.pins_session_id(),
                );
                Some(relaunch_of(&record, of, written, || {
                    uuid::Uuid::new_v4().to_string()
                }))
            }
            None => None,
        };
        let slot = self.admit("relaunched").await?;
        let now = pdo_kernel::clock::now_iso();
        let relaunched = self.registry.update(record.id(), |r| {
            if !r.relaunch(actor, &now) {
                return false;
            }
            r.session.cwd = cwd.to_string_lossy().into_owned();
            if let Some(harness) = &harness {
                r.harness = Some(harness.name.clone());
            }
            if let Some(Relaunch::Fresh { conversation_id }) = &relaunch {
                r.conversation_id = conversation_id.clone();
            }
            true
        });
        let record = match relaunched {
            Some((record, true)) => record,
            // Relaunched in between by another gesture.
            _ => {
                return Err(HostedRefusal::new(
                    409,
                    format!("the hosted session `{key}` is not ended"),
                ))
            }
        };
        drop(slot);

        let session_ref =
            SessionRef::new(record.id(), SessionKind::Hosted).hosted_by(extension, key);
        let resume = matches!(relaunch, Some(Relaunch::Resume { .. }));
        let launched = {
            let (hosted, record, session_ref, actor) = (
                self.clone(),
                record.clone(),
                session_ref.clone(),
                actor.clone(),
            );
            tokio::task::spawn_blocking(move || {
                // An ended session whose tmux session lingers (a kill that
                // failed) would refuse the new one by its name.
                if hosted.tmux.session_exists(record.id()) {
                    hosted.tmux.kill(record.id());
                }
                let agent = harness.as_ref().map(|harness| Agent { harness, resume });
                hosted.launch(&record, &session_ref, &actor, agent)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("the launch panicked: {e}")))
        };
        let id = record.id().to_string();
        let now = pdo_kernel::clock::now_iso();
        match launched {
            Ok(()) => {
                info!(
                    "Relaunched hosted session {id} of `{extension}`{}",
                    if resume {
                        ", resuming its conversation"
                    } else {
                        ""
                    }
                );
                match self.registry.update(&id, |r| r.run(&now)) {
                    Some((running, true)) => {
                        self.ledger.started(session_ref, Some(actor.clone()));
                        Ok(running.session)
                    }
                    Some((ended, false)) => {
                        self.tmux.kill(&id);
                        Ok(ended.session)
                    }
                    None => Ok(record.session),
                }
            }
            Err(e) => {
                if let Some((_, true)) = self
                    .registry
                    .update(&id, |r| r.end(EndReason::Exited, &now))
                {
                    self.admission.released();
                }
                Err(HostedRefusal::new(
                    500,
                    format!("failed to relaunch the hosted session: {e}"),
                ))
            }
        }
    }

    /// Change the configuration of the agent session `key` of `extension` on
    /// behalf of `actor`: the Operation `session` × `configure` on it, in its
    /// Projet. `config` replaces the whole configuration. Its process keeps
    /// the one it was launched with; the new one applies at its next relaunch
    /// (an initial prompt, only when that relaunch starts a new
    /// conversation).
    pub async fn configure(
        &self,
        extension: &str,
        actor: &Actor,
        key: &str,
        config: HostedConfig,
    ) -> Result<HostedSession, HostedRefusal> {
        let record = self.recorded(extension, key)?;
        self.authorize(actor, CONFIGURE, &record)?;
        self.configure_record(actor, record, config).await
    }

    /// Change the configuration of `record` on behalf of `actor`, once
    /// allowed: see [`Self::configure`].
    pub(super) async fn configure_record(
        &self,
        actor: &Actor,
        record: HostedRecord,
        config: HostedConfig,
    ) -> Result<HostedSession, HostedRefusal> {
        let key = record.session.key.clone();
        if record.session.kind != HostedKind::Agent {
            return Err(HostedRefusal::new(
                400,
                format!("the hosted session `{key}` is a shell: it has no configuration"),
            ));
        }
        if config.harness.is_some() {
            self.harness(config.harness.as_deref()).await?;
        }
        let now = pdo_kernel::clock::now_iso();
        let (record, _) = self
            .registry
            .update(record.id(), |r| r.configure(config, actor, &now))
            .unwrap_or((record, false));
        Ok(record.session)
    }

    /// The session `key` of `extension`, or `404`.
    pub(super) fn recorded(
        &self,
        extension: &str,
        key: &str,
    ) -> Result<HostedRecord, HostedRefusal> {
        self.registry
            .find(extension, key)
            .ok_or_else(|| HostedRefusal::new(404, format!("no hosted session `{key}`")))
    }

    /// The Operation `session` × `action` on `record`, in its Projet, for
    /// `actor`; `403` when it is refused.
    pub(super) fn authorize(
        &self,
        actor: &Actor,
        action: &str,
        record: &HostedRecord,
    ) -> Result<(), HostedRefusal> {
        self.allowed(actor, action, record.id())
            .map_err(|denied| HostedRefusal::new(403, denied.to_string()))
    }

    /// The Operation `session` × `action` on the session named `id`, in the
    /// Projet of its record (in none for a name not recorded), for `actor`.
    pub(super) fn allowed(&self, actor: &Actor, action: &str, id: &str) -> Result<(), Denied> {
        let operation = Operation::new(ResourceKind::Session, Verb::Action(action.into()))
            .on(id)
            .in_projects(self.registry.projects_of(id));
        authorize(self.host.as_ref(), actor, &operation)
    }
}
