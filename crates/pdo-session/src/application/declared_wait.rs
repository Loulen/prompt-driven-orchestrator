//! The declared wait's two use cases (#588 / ADR-0069 §1): declare it
//! (`pdo wait-user`, or the daemon on a refused unreleased completion), and lift
//! it on the user's Enter across the PTY bridge.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use crate::application::hosted::Hosted;
use crate::application::ports::{NoRunProjects, NodeWaits, RunProjects, TerminalGesture, Tmux};
use crate::domain::declared_wait::{
    check_target, classify_input, liftable, plan_declaration, DeclareOutcome, WaitRefusal,
    CAUSE_COMPLETION_NOT_RELEASED, CAUSE_DECLARED, COMPLETION_NOT_RELEASED_MESSAGE,
};
use crate::domain::names::{node_session_name, parse_session_name, ParsedSession};

pub use crate::application::typed_input::TypedInput;

/// Where a take-over or a turn of speech comes from: the terminal's bridge.
pub const TERMINAL_SOURCE: &str = "pty";

/// `actor` took the hand on the terminal of `session`, from its bridge or by
/// an answer written on its behalf: a node's session whose pilot changes
/// writes `NodeTakenOver`, once per change of pilot. An iteration that already
/// ended (a `script` node done before the browser attached, a Run over) takes
/// nothing: there is no hand left to take.
pub(crate) async fn record_take_over(
    waits: &dyn NodeWaits,
    typed: &TypedInput,
    actor: &Actor,
    session: &str,
) {
    let Some(ParsedSession::NodeRun {
        run_id,
        node_id,
        iter,
    }) = parse_session_name(session)
    else {
        return;
    };
    if !holds_session(waits, &run_id, &node_id, iter).await {
        return;
    }
    if !typed.pilot(session, actor) {
        return;
    }
    if let Err(e) = waits
        .record_terminal_gesture(
            &run_id,
            &node_id,
            iter,
            TerminalGesture::TakenOver,
            serde_json::json!({ "source": TERMINAL_SOURCE, "actor": actor }),
        )
        .await
    {
        warn!("Run {run_id}: the take-over of {node_id} iter-{iter} was not recorded: {e}");
    }
}

/// Whether `(node, iter)` of a live Run still holds its session, as the Run's
/// log says. An unreadable log answers yes: a take-over is never lost to it.
async fn holds_session(waits: &dyn NodeWaits, run_id: &str, node_id: &str, iter: i64) -> bool {
    match waits.wait_facts(run_id, node_id).await {
        Ok(Some(facts)) => {
            facts.run_live
                && facts.node.is_some_and(|node| {
                    node.iterations
                        .iter()
                        .any(|attempt| attempt.iter == iter && attempt.holds_session)
                })
        }
        Ok(None) => false,
        Err(_) => true,
    }
}

/// The node an Operation on a wait targets: `<run>/<node>`.
pub(crate) fn node_target(run_id: &str, node_id: &str) -> String {
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

/// The Enter rule: every binary frame a browser writes into a **node** session,
/// or a Session hébergée, crosses here, after it reached the pane.
pub struct LiftWait {
    waits: Arc<dyn NodeWaits>,
    typed: Arc<TypedInput>,
    host: Arc<dyn ExtensionHost>,
    projects: Arc<dyn RunProjects>,
    hosted: Option<Arc<Hosted>>,
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
            hosted: None,
        }
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.projects = projects;
        self
    }

    /// The same use case, lifting the waits of the Sessions hébergées of
    /// `hosted` too.
    pub fn with_hosted(mut self, hosted: Arc<Hosted>) -> Self {
        self.hosted = Some(hosted);
        self
    }

    /// Remember typed bytes per session; on an Enter that follows at least one
    /// typed byte since the last declaration, lift the wait if the node awaits on
    /// a declared / refused-completion cause. Never touches a `children_pending`
    /// marker nor a derived wait.
    ///
    /// The first input of `actor` after a take-over or a wait takes the floor:
    /// one `NodeTyped` in the Run's log, or, when it answers a wait, the
    /// `NodeResumed` its Enter writes; the keystrokes that follow write nothing.
    pub async fn note_input(&self, actor: &Actor, session: &str, bytes: &[u8]) {
        let (run_id, node_id, iter) = match parse_session_name(session) {
            Some(ParsedSession::NodeRun {
                run_id,
                node_id,
                iter,
            }) => (run_id, node_id, iter),
            Some(ParsedSession::Hosted) => {
                self.note_hosted_input(actor, session, bytes);
                return;
            }
            _ => return,
        };
        let signal = classify_input(bytes);
        if !signal.typed && !signal.enter {
            return;
        }
        if self
            .typed
            .enter_after_typing(session, signal.typed, signal.enter)
        {
            match self.lift(actor, &run_id, &node_id, iter).await {
                // The answer is this turn's line.
                Ok(true) => {
                    self.typed.takes_floor(session, actor);
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    warn!("Run {run_id}: failed to lift the wait of {node_id} iter-{iter}: {e}")
                }
            }
        }
        if signal.typed && self.typed.takes_floor(session, actor) {
            self.record_typed(actor, &run_id, &node_id, iter).await;
        }
    }

    /// `actor` took the hand on the terminal of `session`: a node's session
    /// whose pilot changes writes `NodeTakenOver`.
    pub async fn taken_over(&self, actor: &Actor, session: &str) {
        record_take_over(self.waits.as_ref(), &self.typed, actor, session).await;
    }

    /// `NodeTyped` for `actor`'s turn, unless the node waits for a person:
    /// then the Enter of the answer speaks, with its `NodeResumed`.
    async fn record_typed(&self, actor: &Actor, run_id: &str, node_id: &str, iter: i64) {
        let facts = match self.waits.wait_facts(run_id, node_id).await {
            Ok(facts) => facts,
            Err(e) => {
                warn!("Run {run_id}: its log could not be read: {e}");
                return;
            }
        };
        let Some(facts) = facts.filter(|f| f.run_live && !f.is_script) else {
            return;
        };
        let Some(node) = facts.node else { return };
        if liftable(&node, iter) {
            return;
        }
        if let Err(e) = self
            .waits
            .record_terminal_gesture(
                run_id,
                node_id,
                iter,
                TerminalGesture::Typed,
                serde_json::json!({ "source": TERMINAL_SOURCE, "actor": actor }),
            )
            .await
        {
            warn!("Run {run_id}: the input into {node_id} iter-{iter} was not recorded: {e}");
        }
    }

    /// The Enter rule on a Session hébergée: an Enter after a typed byte
    /// lifts its wait.
    fn note_hosted_input(&self, actor: &Actor, session: &str, bytes: &[u8]) {
        let Some(hosted) = &self.hosted else { return };
        let signal = classify_input(bytes);
        if !self
            .typed
            .enter_after_typing(session, signal.typed, signal.enter)
        {
            return;
        }
        if let Err(e) = hosted.lift_wait(actor, session) {
            warn!("Failed to lift the wait of hosted session {session}: {e}");
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
                // Who answered, for the telemetry's « include the Actor ».
                serde_json::json!({ "source": "pty_enter", "actor": actor }),
            )
            .await?;
        info!("Run {run_id}: node {node_id} iter-{iter} resumed on the user's Enter");
        Ok(true)
    }
}
