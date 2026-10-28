//! Open a Session hébergée for an extension (ADR-0091).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdo_extension_api::{
    EndReason, HostedKind, HostedRefusal, HostedSession, OpenSession, SessionKind, SessionLaunch,
    SessionRef,
};
use pdo_kernel::authorization::authorize;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::{Hosted, NotAdmitted};
use crate::domain::hosted::{folder_lookup, primer_path, FolderError, FolderLookup, HostedRecord};
use crate::domain::launch::{build_hosted_script, HostedTail};
use crate::domain::names::hosted_session_name;

/// The action of the `session` Operation that opens a Session hébergée.
pub const OPEN: &str = "open";

impl Hosted {
    /// Open `open` for the extension `extension`, on behalf of `actor`: the
    /// Operation `session` × `open` in its Projet, its folder and harness
    /// resolved, a slot of the Cap taken (or a refusal at once), then
    /// recorded `starting` **before** its tmux session exists (the sweep
    /// keeps a session only by its record), launched without a byte written
    /// into its folder, and `running`. Terminated while it was launched, it
    /// stays ended and its process is killed.
    pub async fn open(
        self: &Arc<Self>,
        extension: &str,
        actor: &Actor,
        open: OpenSession,
    ) -> Result<HostedSession, HostedRefusal> {
        if open.key.trim().is_empty() {
            return Err(HostedRefusal::new(400, "a hosted session needs a key"));
        }
        if self.registry.find(extension, &open.key).is_some() {
            return Err(HostedRefusal::new(
                409,
                format!("a hosted session `{}` exists already", open.key),
            ));
        }
        let operation = Operation::new(ResourceKind::Session, Verb::Action(OPEN.into()))
            .in_projects(open.project.iter().cloned());
        authorize(self.host.as_ref(), actor, &operation)
            .map_err(|denied| HostedRefusal::new(403, denied.to_string()))?;
        let cwd = self.folder(&open).await?;
        let harness = match open.kind {
            HostedKind::Agent => Some(self.harness(open.config.harness.as_deref()).await?),
            _ => None,
        };

        let slot = self.admission.admit().await.map_err(|refused| match refused {
            NotAdmitted::CapReached { live, cap } => HostedRefusal::new(
                409,
                format!("session cap reached ({live}/{cap} live sessions): no hosted session is opened"),
            ),
            NotAdmitted::Unavailable(why) => HostedRefusal::unavailable(why),
        })?;
        let id = hosted_session_name(&uuid::Uuid::new_v4().simple().to_string()[..12]);
        let now = pdo_kernel::clock::now_iso();
        let mut record = HostedRecord::opening(&id, extension, &open, &cwd, &now);
        record.session.started_by = Some(actor.clone());
        record.conversation_id = harness
            .as_ref()
            .filter(|h| h.pins_session_id())
            .map(|_| uuid::Uuid::new_v4().to_string());
        self.registry
            .insert(record.clone())
            .map_err(|e| HostedRefusal::new(409, e))?;
        drop(slot);

        let session_ref = SessionRef::new(&id, SessionKind::Hosted).hosted_by(extension, &open.key);
        // Blocking (the extensions' hooks, the state folder, tmux): off the
        // async runtime's threads.
        let launched = {
            let (hosted, record, session_ref, actor) = (
                self.clone(),
                record.clone(),
                session_ref.clone(),
                actor.clone(),
            );
            tokio::task::spawn_blocking(move || {
                hosted.launch(&record, &session_ref, &actor, harness.as_ref())
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("the launch panicked: {e}")))
        };
        match launched {
            Ok(()) => {
                info!(
                    "Opened hosted session {id} of `{extension}` in {}",
                    cwd.display()
                );
                let now = pdo_kernel::clock::now_iso();
                match self.registry.update(&id, |r| r.run(&now)) {
                    Some((running, true)) => {
                        self.ledger.started(session_ref, Some(actor.clone()));
                        Ok(running.session)
                    }
                    // Terminated while it was launched: its kill came before
                    // its tmux session existed.
                    Some((ended, false)) => {
                        self.tmux.kill(&id);
                        Ok(ended.session)
                    }
                    None => Ok(record.session),
                }
            }
            Err(e) => {
                let now = pdo_kernel::clock::now_iso();
                let ended = self
                    .registry
                    .update(&id, |r| r.end(EndReason::Exited, &now));
                self.registry.forget(&id);
                if let Some((_, true)) = ended {
                    self.admission.released();
                }
                Err(HostedRefusal::new(
                    500,
                    format!("failed to open the hosted session: {e}"),
                ))
            }
        }
    }

    /// The folder `open` works in, which must exist.
    async fn folder(&self, open: &OpenSession) -> Result<PathBuf, HostedRefusal> {
        let bad = |message: String| HostedRefusal::new(400, message);
        let cwd = match folder_lookup(&open.folder, open.project.as_deref()) {
            Ok(FolderLookup::Path(path)) => path,
            Ok(FolderLookup::ProjectRepo(project)) => {
                self.places.project_repo(&project).await.ok_or_else(|| {
                    bad(format!("the Projet `{project}` has no primary repository"))
                })?
            }
            Ok(FolderLookup::Home) => self
                .places
                .home()
                .ok_or_else(|| bad("the daemon has no home folder".into()))?,
            Err(FolderError::Relative(path)) => {
                return Err(bad(format!("the folder `{path}` is not an absolute path")))
            }
            Err(FolderError::NoProject) => {
                return Err(bad(
                    "the primary repository of a Projet needs the session's Projet".into(),
                ))
            }
        };
        if !cwd.is_dir() {
            return Err(bad(format!(
                "the folder `{}` does not exist",
                cwd.display()
            )));
        }
        Ok(cwd)
    }

    async fn harness(
        &self,
        name: Option<&str>,
    ) -> Result<pdo_harness::registry::HarnessDescriptor, HostedRefusal> {
        self.harnesses.resolve(name).await.ok_or_else(|| {
            HostedRefusal::new(
                400,
                format!("unknown harness `{}`", name.unwrap_or_default()),
            )
        })
    }

    /// Ask the extensions for the launch, write the primer into the state
    /// folder (never into the session's folder), and create the tmux session.
    fn launch(
        &self,
        record: &HostedRecord,
        session_ref: &SessionRef,
        actor: &Actor,
        harness: Option<&pdo_harness::registry::HarnessDescriptor>,
    ) -> anyhow::Result<()> {
        let contribution = self.host.session_launch(
            &SessionLaunch::new(session_ref.clone())
                .started_by(actor.clone())
                .in_projects(record.projects()),
        );
        let env: Vec<(String, String)> = contribution.env.clone().into_iter().collect();
        let state_dir = self.registry.store().state_dir(record.id());
        std::fs::create_dir_all(&state_dir)?;
        let config = &record.session.config;
        let primer = match harness {
            Some(_) => write_primer(
                &state_dir,
                contribution.above(config.prompt.clone().unwrap_or_default()),
            )?,
            None => None,
        };
        let tail = match harness {
            Some(harness) => HostedTail::Agent {
                harness,
                primer: primer.as_deref(),
                model: config.model.as_deref(),
                effort: config.effort.as_deref(),
                session_id: record.conversation_id.as_deref(),
            },
            None => HostedTail::Shell,
        };
        let script = build_hosted_script(
            record.id(),
            self.daemon_port,
            &self.tmux.session_path(),
            tail,
            self.tmux_cmd_override.as_deref(),
            &env,
        );
        let cwd = Path::new(&record.session.cwd);
        self.tmux
            .new_session(record.id(), cwd, &script, Some("hosted"))
    }
}

/// Write what the agent reads first, unless there is nothing to read.
fn write_primer(state_dir: &Path, text: String) -> std::io::Result<Option<PathBuf>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let path = primer_path(state_dir);
    std::fs::write(&path, text)?;
    Ok(Some(path))
}
