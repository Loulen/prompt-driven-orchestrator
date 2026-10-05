//! The declared wait's two use cases (#588 / ADR-0069 §1): declare it
//! (`pdo wait-user`, or the daemon on a refused unreleased completion), and lift
//! it on the user's Enter across the PTY bridge.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use crate::application::ports::{NoRunProjects, NodeWaits, RunProjects, Tmux};
use crate::domain::declared_wait::{
    check_target, classify_input, liftable, plan_declaration, DeclareOutcome, WaitRefusal,
    CAUSE_COMPLETION_NOT_RELEASED, CAUSE_DECLARED, COMPLETION_NOT_RELEASED_MESSAGE,
};
use crate::domain::names::{node_session_name, parse_session_name, ParsedSession};

/// The sessions a human typed into since their last declaration: the Enter rule
/// lifts a wait only after at least one typed byte.
#[derive(Default)]
pub struct TypedInput(Mutex<HashSet<String>>);

impl TypedInput {
    fn forget(&self, session: &str) {
        self.0.lock().unwrap().remove(session);
    }

    /// Record the frame's signal; `true` when it is an Enter that follows a
    /// typed byte (and the count starts over).
    fn enter_after_typing(&self, session: &str, typed: bool, enter: bool) -> bool {
        let mut sessions = self.0.lock().unwrap();
        if typed {
            sessions.insert(session.to_string());
        }
        enter && sessions.remove(session)
    }
}

/// The node an Operation on a wait targets: `<run>/<node>`.
fn node_target(run_id: &str, node_id: &str) -> String {
    format!("{run_id}/{node_id}")
}

/// Why a wait was not declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclareWaitError {
    Refused(Denied),
    Wait(WaitRefusal),
}

/// `POST /runs/{run_id}/nodes/{node_id}/wait-user`.
pub struct DeclareWaitCommand {
    pub run_id: String,
    pub node_id: String,
    pub iter: i64,
    pub message: Option<String>,
}

pub struct DeclareWait {
    waits: Arc<dyn NodeWaits>,
    tmux: Arc<dyn Tmux>,
    typed: Arc<TypedInput>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
}

impl DeclareWait {
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

    /// The agent (or the operator) declares the wait. Accepted on any node
    /// holding a live session; refused by name otherwise; a no-op on a repeat.
    pub async fn execute(
        &self,
        actor: &Actor,
        command: DeclareWaitCommand,
    ) -> Result<DeclareOutcome, DeclareWaitError> {
        let operation = Operation::new(ResourceKind::Node, Verb::Action("wait_user".into()))
            .on(node_target(&command.run_id, &command.node_id))
            .in_projects(self.projects.projects_of_run(&command.run_id).await);
        authorize(self.host.as_ref(), actor, &operation).map_err(DeclareWaitError::Refused)?;
        self.declare(
            &command.run_id,
            &command.node_id,
            command.iter,
            CAUSE_DECLARED,
            command.message.as_deref(),
        )
        .await
        .map_err(DeclareWaitError::Wait)
    }

    /// The daemon's own declaration on a refused unreleased completion
    /// (ADR-0069 §1): an internal transition, never authorized, best-effort —
    /// it never changes the refusal itself.
    pub async fn on_completion_not_released(&self, run_id: &str, node_id: &str, iter: i64) {
        if let Err(refusal) = self
            .declare(
                run_id,
                node_id,
                iter,
                CAUSE_COMPLETION_NOT_RELEASED,
                Some(COMPLETION_NOT_RELEASED_MESSAGE),
            )
            .await
        {
            warn!(
                "Run {run_id}: refused completion of {node_id} could not declare the wait ({})",
                refusal.slug()
            );
        }
    }

    /// Append one `NodeAwaitingUser` with `{cause, message}` unless the node
    /// already awaits on exactly that, and forget any keystroke typed before the
    /// declaration (the Enter rule counts from here).
    async fn declare(
        &self,
        run_id: &str,
        node_id: &str,
        iter: i64,
        cause: &str,
        message: Option<&str>,
    ) -> Result<DeclareOutcome, WaitRefusal> {
        let facts = self
            .waits
            .wait_facts(run_id, node_id)
            .await
            .map_err(|_| WaitRefusal::RunNotFound)?;
        check_target(facts.as_ref(), iter)?;
        // The projection says the node holds a session; tmux has the last word. A
        // pane that died before the sweep noticed must not park the run on a
        // question nobody can answer in it (FP #793, finding 3).
        let session = node_session_name(run_id, node_id, iter);
        if !self.tmux.session_exists(&session) {
            return Err(WaitRefusal::NodeSessionNotLive);
        }
        let node = facts
            .and_then(|f| f.node)
            .ok_or(WaitRefusal::NodeSessionNotLive)?;
        let Some((payload, outcome)) = plan_declaration(&node, cause, message) else {
            return Ok(DeclareOutcome::Noop);
        };
        let shown = payload["message"].as_str().map(str::to_string);
        if let Err(e) = self
            .waits
            .record_awaiting(run_id, node_id, iter, payload)
            .await
        {
            warn!(
                "declared wait: failed to append NodeAwaitingUser for {node_id} in {run_id}: {e}"
            );
            return Err(WaitRefusal::RunNotLive);
        }
        self.typed.forget(&session);
        info!(
            "Run {run_id}: node {node_id} iter-{iter} declared a wait ({cause}{})",
            shown.map(|m| format!(": {m}")).unwrap_or_default()
        );
        Ok(outcome)
    }
}

/// The Enter rule: every binary frame a browser writes into a **node** session
/// crosses here, after it reached the pane.
pub struct LiftWait {
    waits: Arc<dyn NodeWaits>,
    typed: Arc<TypedInput>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
}

impl LiftWait {
    pub fn new(
        waits: Arc<dyn NodeWaits>,
        typed: Arc<TypedInput>,
        host: Arc<dyn ExtensionHost>,
    ) -> Self {
        Self {
            waits,
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

    /// Remember typed bytes per session; on an Enter that follows at least one
    /// typed byte since the last declaration, lift the wait if the node awaits on
    /// a declared / refused-completion cause. Never touches a `children_pending`
    /// marker nor a derived wait.
    pub async fn note_input(&self, actor: &Actor, session: &str, bytes: &[u8]) {
        let Some(ParsedSession::NodeRun {
            run_id,
            node_id,
            iter,
        }) = parse_session_name(session)
        else {
            return;
        };
        let signal = classify_input(bytes);
        if !signal.typed && !signal.enter {
            return;
        }
        if !self
            .typed
            .enter_after_typing(session, signal.typed, signal.enter)
        {
            return;
        }
        match self.lift(actor, &run_id, &node_id, iter).await {
            Ok(_) => {}
            Err(e) => warn!("Run {run_id}: failed to lift the wait of {node_id} iter-{iter}: {e}"),
        }
    }

    /// Append `NodeResumed` if the node awaits on a cause a keystroke may lift.
    /// Returns whether anything was written.
    pub async fn lift(
        &self,
        actor: &Actor,
        run_id: &str,
        node_id: &str,
        iter: i64,
    ) -> Result<bool, String> {
        let Some(node) = self
            .waits
            .wait_facts(run_id, node_id)
            .await?
            .and_then(|f| f.node)
        else {
            return Ok(false);
        };
        if !liftable(&node, iter) {
            return Ok(false);
        }
        let operation = Operation::new(ResourceKind::Node, Verb::Action("resume".into()))
            .on(node_target(run_id, node_id))
            .in_projects(self.projects.projects_of_run(run_id).await);
        authorize(self.host.as_ref(), actor, &operation).map_err(|d| d.to_string())?;
        self.waits
            .record_resumed(
                run_id,
                node_id,
                iter,
                serde_json::json!({ "source": "pty_enter" }),
            )
            .await?;
        info!("Run {run_id}: node {node_id} iter-{iter} resumed on the user's Enter");
        Ok(true)
    }
}
