//! Piloting a terminal: what the PTY bridge asks of the context for each
//! browser connection — may it attach or only watch, how, who pilots, and what
//! its keystrokes do to a declared wait.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, authorize_read, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tokio::sync::watch;
use tracing::warn;

use crate::application::declared_wait::LiftWait;
use crate::application::ignore_size::follow_ignore_size;
use crate::application::ports::{projects_of_session, NoRunProjects, RunProjects, Tmux};
use crate::application::session_ledger::SessionLedger;
use crate::application::shared_terminal::SharedTerminalRegistry;
use crate::domain::shared_terminal::ConnId;

/// How a connection is let into a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// It may drive the terminal: type into it, take control of it.
    Drives,
    /// It may only watch; the sentence says why it cannot type.
    ReadOnly(String),
}

pub struct Terminals {
    registry: Arc<SharedTerminalRegistry>,
    tmux: Arc<dyn Tmux>,
    lift: LiftWait,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
    ledger: Arc<SessionLedger>,
}

impl Terminals {
    pub fn new(tmux: Arc<dyn Tmux>, lift: LiftWait, host: Arc<dyn ExtensionHost>) -> Self {
        Self {
            registry: Arc::default(),
            ledger: Arc::new(SessionLedger::new(tmux.clone(), host.clone())),
            tmux,
            lift,
            host,
            projects: Arc::new(NoRunProjects),
        }
    }

    /// The same use case, announcing who takes control of a terminal in
    /// `ledger` (CORE-61).
    pub fn with_ledger(mut self, ledger: Arc<SessionLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    /// The same use case over `registry`, shared with the gestures that read
    /// who pilots a terminal.
    pub fn with_registry(mut self, registry: Arc<SharedTerminalRegistry>) -> Self {
        self.registry = registry;
        self
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.lift = self.lift.with_projects(projects.clone());
        self.projects = projects;
        self
    }

    /// How `actor` may open a terminal on `session`: to drive it with
    /// `session` × `attach`; else read-only with `session` × `read`, the
    /// sentence of the attach's refusal saying why; else not at all, with the
    /// read's refusal.
    pub async fn admit(&self, actor: &Actor, session: &str) -> Result<Admission, Denied> {
        let attach = self.operation(session, "attach").await;
        let Err(refused) = authorize(self.host.as_ref(), actor, &attach) else {
            return Ok(Admission::Drives);
        };
        let read = Operation::new(ResourceKind::Session, Verb::Read)
            .on(session)
            .in_projects(attach.projects);
        authorize_read(self.host.as_ref(), actor, &read)?;
        Ok(Admission::ReadOnly(refused.sentence().to_string()))
    }

    /// Who is connected to which session, and who pilots.
    pub fn registry(&self) -> &SharedTerminalRegistry {
        &self.registry
    }

    /// The argv of the `tmux attach` the bridge runs in its PTY.
    pub fn attach_argv(&self, session: &str, ignore_size: bool) -> Vec<String> {
        self.tmux.attach_argv(session, ignore_size)
    }

    /// Keep the client of `client_pid` in line with its role, until the
    /// connection leaves.
    pub fn follow_ignore_size(
        &self,
        client_pid: u32,
        wanted: watch::Receiver<bool>,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        follow_ignore_size(self.tmux.clone(), client_pid, wanted)
    }

    /// A spectator takes control (#870). Only a spectator's request does
    /// anything; the registry flips the roles, the `ignore-size` of both postes'
    /// clients and the read-only filter. A take-over that changed the pilot is
    /// announced (CORE-61) and kept in the Run's log; a refused one, or one
    /// with no effect, is not.
    pub async fn take_control(&self, actor: &Actor, session: &str, id: ConnId) -> bool {
        let operation = self.operation(session, "take_control").await;
        if let Err(denied) = authorize(self.host.as_ref(), actor, &operation) {
            warn!("Take control of {session} refused: {denied}");
            return false;
        }
        let taken = self.registry.take_control(session, id);
        if taken {
            self.ledger.taken_over(actor, session);
            self.lift.taken_over(actor, session).await;
        }
        taken
    }

    /// The connection of `actor` that just joined `session` drives it: it is
    /// the first to attach, or the hand came to its poste (CORE-61). Announced
    /// as a take-over, and kept in the Run's log when the pilot changes; a
    /// spectator who joins announces nothing.
    pub async fn joined_as_driver(&self, actor: &Actor, session: &str) {
        self.ledger.taken_over(actor, session);
        self.lift.taken_over(actor, session).await;
    }

    /// A binary frame reached the pane: the Enter rule of the declared wait
    /// (#588 / ADR-0069 §1). Called after the write, so the bridge never delays
    /// input.
    pub async fn note_input(&self, actor: &Actor, session: &str, bytes: &[u8]) {
        self.lift.note_input(actor, session, bytes).await;
    }

    async fn operation(&self, session: &str, action: &str) -> Operation {
        Operation::new(ResourceKind::Session, Verb::Action(action.into()))
            .on(session)
            .in_projects(projects_of_session(self.projects.as_ref(), session).await)
    }
}
