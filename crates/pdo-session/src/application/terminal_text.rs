//! « Copy & send to terminal »: write a text into the input of a node's live
//! session **without** pressing Enter. The user completes and submits it in the
//! terminal; PDO never starts an agent turn on its own.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use crate::application::declared_wait::{node_target, TypedInput};
use crate::application::ports::{NoRunProjects, NodeWaits, RunProjects, Tmux};
use crate::domain::declared_wait::{live_target, WaitRefusal};
use crate::domain::names::node_session_name;

/// The longest text the gesture writes: a few paragraphs, never a document.
pub const TERMINAL_TEXT_MAX_BYTES: usize = 16 * 1024;

/// `POST /runs/{run_id}/nodes/{node_id}/terminal-text`.
pub struct SendTerminalTextCommand {
    pub run_id: String,
    pub node_id: String,
    /// The iteration whose session receives the text; the node's current one
    /// when absent.
    pub iter: Option<i64>,
    pub text: String,
}

/// Why nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendTextError {
    Refused(Denied),
    /// The node holds no session that could receive the text: why, and the
    /// iteration asked about.
    Session(WaitRefusal, i64),
    /// Blank text.
    Empty,
    /// Over [`TERMINAL_TEXT_MAX_BYTES`].
    TooLong,
    /// tmux refused the buffer or the paste into this session.
    PasteFailed(String),
}

pub struct SendTerminalText {
    waits: Arc<dyn NodeWaits>,
    tmux: Arc<dyn Tmux>,
    typed: Arc<TypedInput>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
}

impl SendTerminalText {
    pub fn new(
        waits: Arc<dyn NodeWaits>,
        tmux: Arc<dyn Tmux>,
        typed: Arc<TypedInput>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            waits,
            tmux,
            typed,
            host,
            projects: Arc::new(NoRunProjects),
        }
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.projects = projects;
        self
    }

    /// Paste `text` into the session of `(run, node, iter)` as one bracketed
    /// paste, no Enter, and count it as typed for the declared wait. Returns
    /// the iteration that received it.
    pub async fn execute(
        &self,
        actor: &Actor,
        command: SendTerminalTextCommand,
    ) -> Result<i64, SendTextError> {
        let operation = Operation::new(ResourceKind::Node, Verb::Action("send_text".into()))
            .on(node_target(&command.run_id, &command.node_id))
            .in_projects(self.projects.projects_of_run(&command.run_id).await);
        authorize(self.host.as_ref(), actor, &operation).map_err(SendTextError::Refused)?;
        if command.text.trim().is_empty() {
            return Err(SendTextError::Empty);
        }
        if command.text.len() > TERMINAL_TEXT_MAX_BYTES {
            return Err(SendTextError::TooLong);
        }
        let (iter, session) = live_node_session(
            self.waits.as_ref(),
            self.tmux.as_ref(),
            &command.run_id,
            &command.node_id,
            command.iter,
        )
        .await
        .map_err(|(refusal, iter)| SendTextError::Session(refusal, iter))?;
        // A line break would submit where the program in the pane did not ask
        // for bracketed paste: the text stays on one line.
        let text = one_line(&command.text);
        if !self.tmux.paste_without_enter(&session, &text) {
            return Err(SendTextError::PasteFailed(session));
        }
        self.typed.mark(&session);
        info!(
            "Run {}: wrote {} bytes into the input of {} iter-{iter}",
            command.run_id,
            text.len(),
            command.node_id
        );
        Ok(iter)
    }
}

/// The live session of `(run, node, iter)` — `iter` defaulting to the node's
/// current iteration — and that iteration: the precondition of every gesture
/// that speaks to a node's agent. The projection says the node holds a
/// session; tmux has the last word. A refusal names the iteration it was
/// about.
pub async fn live_node_session(
    waits: &dyn NodeWaits,
    tmux: &dyn Tmux,
    run_id: &str,
    node_id: &str,
    iter: Option<i64>,
) -> Result<(i64, String), (WaitRefusal, i64)> {
    let facts = waits
        .wait_facts(run_id, node_id)
        .await
        .map_err(|_| (WaitRefusal::RunNotFound, iter.unwrap_or(1)))?;
    let iter = live_target(facts.as_ref(), iter)?;
    let session = node_session_name(run_id, node_id, iter);
    if !tmux.session_exists(&session) {
        return Err((WaitRefusal::NodeSessionNotLive, iter));
    }
    Ok((iter, session))
}

/// `text` with every line break turned into a space: a pasted CR or LF is an
/// Enter for a program that did not ask for bracketed paste.
fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}
