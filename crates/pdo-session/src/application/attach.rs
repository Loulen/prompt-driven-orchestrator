//! Pop a session out into an OS terminal window (`POST /sessions/{id}/attach`,
//! `POST /sessions/{run_id}/manager/attach`). The inline xterm.js terminal is
//! primary (ADR-0005); this is the escape hatch.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use crate::application::ports::{TerminalWindow, Tmux};
use crate::domain::names::manager_session_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    Refused(Denied),
    /// The Run has no manager session.
    NoManager {
        session: String,
    },
    Terminal(String),
}

/// The session attached, and the terminal program used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub session: String,
    pub terminal: String,
}

pub struct AttachTerminal {
    tmux: Arc<dyn Tmux>,
    window: Arc<dyn TerminalWindow>,
    host: Arc<dyn ExtensionHost>,
}

impl AttachTerminal {
    pub fn new(
        tmux: Arc<dyn Tmux>,
        window: Arc<dyn TerminalWindow>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self { tmux, window, host }
    }

    /// Attach a terminal window to `session`.
    pub fn session(&self, actor: &Actor, session: &str) -> Result<Attached, AttachError> {
        self.authorize(actor, session)?;
        let terminal = self.open(session)?;
        info!("Attached terminal {terminal} to session {session}");
        Ok(Attached {
            session: session.to_string(),
            terminal,
        })
    }

    /// Attach a terminal window to the Run's manager, which must be running.
    pub fn manager(&self, actor: &Actor, run_id: &str) -> Result<Attached, AttachError> {
        let session = manager_session_name(run_id);
        self.authorize(actor, &session)?;
        if !self.tmux.session_exists(&session) {
            return Err(AttachError::NoManager { session });
        }
        let terminal = self.open(&session)?;
        info!("Attached terminal {terminal} to manager session {session}");
        Ok(Attached { session, terminal })
    }

    fn authorize(&self, actor: &Actor, session: &str) -> Result<(), AttachError> {
        let operation =
            Operation::new(ResourceKind::Session, Verb::Action("attach".into())).on(session);
        authorize(self.host.as_ref(), actor, &operation).map_err(AttachError::Refused)
    }

    fn open(&self, session: &str) -> Result<String, AttachError> {
        let attach = self.tmux.attach_command(session);
        self.window
            .open(session, &attach)
            .map_err(|e| AttachError::Terminal(format!("failed to attach: {e}")))
    }
}
