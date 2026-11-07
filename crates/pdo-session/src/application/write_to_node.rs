//! Write into the session of a Node that waits for a person, on behalf of an
//! Actor (API 0.23): what an extension's `Host::write_to_node` asks, to answer
//! an agent's question from elsewhere than its terminal.
//!
//! The text is typed as the Actor would type it in the terminal, then
//! submitted: one bracketed paste on one line, then Enter. The Node resumes,
//! with the Actor as who answered, and the Actor is announced as taking
//! control of the session (CORE-61), as when it takes the hand. Only a Node
//! that waits for a person receives it: running, ended, waiting on its
//! children or unknown, it is refused with the stable reason `not_waiting`,
//! and nothing is written.

use std::sync::Arc;

use pdo_extension_api::{
    NodeNotWritten, NodeWrite, NodeWritten, NODE_WRITE_FAILED, NODE_WRITE_INVALID,
    NODE_WRITE_MAX_BYTES,
};
use pdo_kernel::authorization::authorize;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::application::ports::{NoRunProjects, NodeWaits, RunProjects, Tmux};
use crate::application::session_ledger::SessionLedger;
use crate::application::terminal_text::one_line;
use crate::domain::declared_wait::{liftable, WaitFacts};
use crate::domain::names::node_session_name;

/// Where the resumption of a Node comes from when an extension wrote its
/// answer (`NodeResumed.source`).
pub const SOURCE: &str = "extension";

pub struct WriteToNode {
    waits: Arc<dyn NodeWaits>,
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
    ledger: Arc<SessionLedger>,
    /// One write at a time: of two answers to the same wait, the second finds
    /// the Node running.
    one_at_a_time: Mutex<()>,
}

impl WriteToNode {
    pub fn new(
        waits: Arc<dyn NodeWaits>,
        tmux: Arc<dyn Tmux>,
        host: Arc<dyn ExtensionHost>,
        ledger: Arc<SessionLedger>,
    ) -> Self {
        Self {
            waits,
            tmux,
            host,
            projects: Arc::new(NoRunProjects),
            ledger,
            one_at_a_time: Mutex::new(()),
        }
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.projects = projects;
        self
    }

    /// Type `write.text` into the session of the Node and press Enter, on
    /// behalf of `actor`, if `actor` may take control of that session and the
    /// Node waits for a person at that iteration.
    pub async fn execute(
        &self,
        actor: &Actor,
        write: NodeWrite,
    ) -> Result<NodeWritten, NodeNotWritten> {
        let _turn = self.one_at_a_time.lock().await;
        let facts = self
            .waits
            .wait_facts(&write.run_id, &write.node_id)
            .await
            .map_err(|e| {
                warn!("Run {}: its log could not be read: {e}", write.run_id);
                NodeNotWritten::unavailable(format!("run {} could not be read", write.run_id))
            })?;
        // The right first: a refused Actor learns nothing of which Nodes exist.
        let iter = write
            .iter
            .or(facts
                .as_ref()
                .and_then(|f| f.node.as_ref())
                .map(|node| node.iter))
            .unwrap_or(1);
        let session = node_session_name(&write.run_id, &write.node_id, iter);
        let operation = Operation::new(ResourceKind::Session, Verb::Action("take_control".into()))
            .on(session.as_str())
            .in_projects(self.projects.projects_of_run(&write.run_id).await);
        authorize(self.host.as_ref(), actor, &operation)
            .map_err(|denied| NodeNotWritten::refused(denied.reason))?;
        let Some(facts) = facts.filter(|f| f.defined || f.node.is_some()) else {
            return Err(NodeNotWritten::not_waiting(false));
        };

        if write.text.trim().is_empty() || write.text.len() > NODE_WRITE_MAX_BYTES {
            return Err(NodeNotWritten::new(
                400,
                NODE_WRITE_INVALID,
                format!(
                    "the text is blank or over {} KB; nothing written",
                    NODE_WRITE_MAX_BYTES / 1024
                ),
            ));
        }
        if !waits_for_a_person(&facts, iter) || !self.tmux.session_exists(&session) {
            return Err(NodeNotWritten::not_waiting(true));
        }

        let text = one_line(&write.text);
        if !self.tmux.paste_without_enter(&session, &text) {
            return Err(write_failed(&session, "paste"));
        }
        if !self.tmux.press_enter(&session) {
            return Err(write_failed(&session, "Enter"));
        }
        let resumed = self
            .waits
            .record_resumed(
                &write.run_id,
                &write.node_id,
                iter,
                serde_json::json!({ "source": SOURCE, "actor": actor }),
            )
            .await;
        if let Err(e) = resumed {
            warn!(
                "Run {}: {} iter-{iter} received the text but its resumption was not recorded: {e}",
                write.run_id, write.node_id
            );
        }
        self.ledger.taken_over(actor, &session);
        info!(
            "Run {}: wrote {} bytes into {} iter-{iter} on behalf of {}, and submitted them",
            write.run_id,
            text.len(),
            write.node_id,
            actor.kind()
        );
        Ok(NodeWritten::new(write.run_id, write.node_id, iter, session))
    }
}

/// Whether the Node of `facts` waits for a person at `iter`: its Run is live,
/// it runs an agent, and it waits on a cause a person's answer lifts (a
/// declared wait, a completion not released), never on its children.
fn waits_for_a_person(facts: &WaitFacts, iter: i64) -> bool {
    facts.run_live
        && !facts.is_script
        && facts.node.as_ref().is_some_and(|node| liftable(node, iter))
}

fn write_failed(session: &str, what: &str) -> NodeNotWritten {
    NodeNotWritten::new(
        502,
        NODE_WRITE_FAILED,
        format!("tmux refused the {what} into {session}"),
    )
}
