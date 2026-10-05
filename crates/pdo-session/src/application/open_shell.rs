//! Open (or re-attach) the ad-hoc bash shell of a terminal Run, in its
//! pipeline worktree (#316, ADR-0021).

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use crate::application::launch::spawn_shell;
use crate::application::ports::{NoRunProjects, RunLookup, RunProjects, RunShells, Tmux};
use crate::domain::launch::SandboxWrap;
use crate::domain::names::shell_session_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenShellError {
    Refused(Denied),
    Run(RunLookup),
    /// Not terminal, archived, or its worktree is gone.
    NotEligible,
    /// The Run is sandboxed and its container could not be brought up.
    Sandbox(String),
    Spawn(String),
}

/// The shell session, and whether this call created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedShell {
    pub session: String,
    pub created: bool,
}

/// Create-if-absent, one shell per Run. The daemon is the source of truth for
/// eligibility, because the client cannot see the worktree path: a live Run is
/// refused because a stray edit in its worktree would break the daemon's
/// `git merge` on node completion; an archived Run has no worktree left.
pub struct OpenShell {
    runs: Arc<dyn RunShells>,
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
    daemon_port: u16,
    projects: Arc<dyn RunProjects>,
}

impl OpenShell {
    pub fn new(
        runs: Arc<dyn RunShells>,
        tmux: Arc<dyn Tmux>,
        host: Arc<dyn ExtensionHost>,
        daemon_port: u16,
    ) -> Self {
        Self {
            runs,
            tmux,
            host,
            daemon_port,
            projects: Arc::new(NoRunProjects),
        }
    }

    /// The same use case, its Operations in the Projets `projects` gives.
    pub fn with_projects(mut self, projects: Arc<dyn RunProjects>) -> Self {
        self.projects = projects;
        self
    }

    pub async fn execute(
        &self,
        actor: &Actor,
        run_id: &str,
    ) -> Result<OpenedShell, OpenShellError> {
        let session = shell_session_name(run_id);
        // A creation carries no target (the session does not exist yet).
        let operation = Operation::new(ResourceKind::Session, Verb::Create)
            .in_projects(self.projects.projects_of_run(run_id).await);
        authorize(self.host.as_ref(), actor, &operation).map_err(OpenShellError::Refused)?;
        let target = self
            .runs
            .shell_target(run_id)
            .await
            .map_err(OpenShellError::Run)?;
        if !target.eligible {
            return Err(OpenShellError::NotEligible);
        }

        // Create-then-verify-on-failure: a concurrent POST may win the
        // `new-session` (tmux rejects the duplicate), so on a spawn failure
        // re-test existence — if the session is now there, this is a benign
        // re-attach.
        if self.tmux.session_exists(&session) {
            return Ok(OpenedShell {
                session,
                created: false,
            });
        }

        // A sandboxed run's shell must enter its container. The container's
        // lifetime is coextensive with shell eligibility EXCEPT after a host
        // reboot, where boot recovery skips terminal runs. Resurrect it here, or
        // fail EXPLICITLY — never fall back to a silent host bash.
        if target.sandbox.is_some() {
            self.runs
                .ensure_sandbox_ready(run_id)
                .await
                .map_err(OpenShellError::Sandbox)?;
        }

        // A run shell runs bash, not a harness tail: no staging-set env to
        // forward. A hand-typed `pi` inside it still reads the staged `.pi/agent`
        // home (mounted), and can export the two vars itself.
        let wrap = target
            .sandbox
            .as_deref()
            .map(|exec| SandboxWrap { exec, set_env: &[] });
        match spawn_shell(
            self.tmux.as_ref(),
            &session,
            &target.worktree,
            run_id,
            self.daemon_port,
            wrap.as_ref(),
        ) {
            Ok(()) => {
                info!(
                    "Opened run shell {session} in {}",
                    target.worktree.display()
                );
                Ok(OpenedShell {
                    session,
                    created: true,
                })
            }
            // A concurrent POST won the race — re-attach, not an error.
            Err(_) if self.tmux.session_exists(&session) => Ok(OpenedShell {
                session,
                created: false,
            }),
            Err(e) => Err(OpenShellError::Spawn(format!("failed to open shell: {e}"))),
        }
    }
}
