//! How a Session hébergée ends (on « terminate », or when its process exits)
//! and how a daemon takes back those a previous one held.

use std::sync::Arc;
use std::time::Duration;

use pdo_extension_api::{
    EndReason, HostedRefusal, HostedSession, HostedState, SessionKind, SessionRef,
};
use pdo_kernel::authorization::authorize;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use super::Hosted;
use crate::domain::hosted::HostedRecord;

/// The action of the `session` Operation that terminates a Session hébergée.
pub const TERMINATE: &str = "terminate";

impl Hosted {
    /// Terminate the session `key` of `extension` on behalf of `actor`: the
    /// Operation `session` × `terminate` on it, in its Projet. It is ended
    /// (`terminated`) **before** its tmux session is killed, so the
    /// observation of its exit never reads it as `exited`. An ended session is
    /// answered as it is.
    pub async fn terminate(
        &self,
        extension: &str,
        actor: &Actor,
        key: &str,
    ) -> Result<HostedSession, HostedRefusal> {
        let record = self
            .registry
            .find(extension, key)
            .ok_or_else(|| HostedRefusal::new(404, format!("no hosted session `{key}`")))?;
        let operation = Operation::new(ResourceKind::Session, Verb::Action(TERMINATE.into()))
            .on(record.id())
            .in_projects(record.projects());
        authorize(self.host.as_ref(), actor, &operation)
            .map_err(|denied| HostedRefusal::new(403, denied.to_string()))?;
        // Ended in between (its process exited): it keeps its reason.
        let (ended, changed) = self
            .registry
            .update(record.id(), |r| {
                r.end(EndReason::Terminated, &pdo_kernel::clock::now_iso())
            })
            .unwrap_or((record, false));
        if changed {
            self.tmux.kill(ended.id());
            info!("Terminated hosted session {}", ended.id());
            self.admission.released();
        }
        Ok(ended.session)
    }

    /// At boot, before the router serves: a live record whose session is
    /// still in tmux is taken back with who started it (`running`, or still
    /// `awaiting_user`); one whose session is gone ended while no daemon
    /// watched (`exited`). The live sessions of an extension this daemon does
    /// not have are terminated (`terminated`), their record kept: nothing runs
    /// without the extension that governs it. Blocking: it asks tmux.
    pub fn restore(&self) {
        let installed = self.host.installed();
        for record in self.registry.all() {
            if !record.is_live() {
                continue;
            }
            let now = pdo_kernel::clock::now_iso();
            if !installed.contains(&record.session.extension) {
                warn!(
                    "hosted session {} of the absent extension `{}` is terminated",
                    record.id(),
                    record.session.extension
                );
                self.registry
                    .update(record.id(), |r| r.end(EndReason::Terminated, &now));
                self.tmux.kill(record.id());
                continue;
            }
            if self.tmux.session_exists(record.id()) {
                self.registry.update(record.id(), |r| r.run(&now));
                self.ledger
                    .adopt(session_ref(&record), record.session.started_by.clone());
            } else {
                warn!(
                    "hosted session {} was gone when the daemon started",
                    record.id()
                );
                self.registry
                    .update(record.id(), |r| r.end(EndReason::Exited, &now));
            }
        }
    }

    /// Publish the Artefacts live that changed, then end (`exited`) every
    /// running session whose tmux session is gone: its process exited. A
    /// session still `starting` is being launched, and is left alone.
    /// Blocking: it asks tmux and reads the disk.
    pub fn reconcile(&self) {
        self.scan_live();
        let mut released = false;
        for record in self.registry.all() {
            let running = matches!(
                record.session.state,
                HostedState::Running | HostedState::AwaitingUser
            );
            if !running || self.tmux.session_exists(record.id()) {
                continue;
            }
            let now = pdo_kernel::clock::now_iso();
            // Terminated in between: its reason stands.
            let exited = self
                .registry
                .update(record.id(), |r| r.end(EndReason::Exited, &now));
            if let Some((_, true)) = exited {
                info!("Hosted session {} exited", record.id());
                released = true;
            }
        }
        if released {
            self.admission.released();
        }
    }

    /// [`Self::reconcile`] every `every`, off the async runtime's threads,
    /// for as long as the daemon runs.
    pub async fn watch(self: Arc<Self>, every: Duration) {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let hosted = self.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || hosted.reconcile()).await {
                warn!("the reconciliation of the hosted sessions panicked: {e}");
            }
        }
    }
}

/// The session `record` is, as the extensions see it.
fn session_ref(record: &HostedRecord) -> SessionRef {
    SessionRef::new(record.id(), SessionKind::Hosted)
        .hosted_by(&record.session.extension, &record.session.key)
}
