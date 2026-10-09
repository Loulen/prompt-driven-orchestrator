//! Open and reap the library assistant of an Actor.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_extension_api::{SessionKind, SessionLaunch, SessionRef};
use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

use super::FocusCell;
use crate::application::launch::spawn_libassist;
use crate::application::ports::{AssistantHarness, Tmux};
use crate::application::session_ledger::SessionLedger;
use crate::domain::launch::host_daemon_url;
use crate::domain::libassist::{
    build_library_assistant_prompt, libassist_cwd, libassist_primer_path,
};
use crate::domain::names::libassist_session_name_of;

/// The action of the `pipeline` Operation that opens the library assistant
/// (CORE-61): an assistant writes templates, so opening it is a gesture on the
/// Pipelines.
pub const OPEN_ASSISTANT: &str = "open_assistant";

/// Where the assistant runs, and how its launch may be stubbed.
#[derive(Debug, Clone)]
pub struct AssistantLaunch {
    /// The daemon's repo root: the assistant works in its templates folder.
    pub repo_root: PathBuf,
    pub daemon_port: u16,
    /// Replaces the harness tail (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
}

/// What `POST /sessions/libassist` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub session: String,
    /// `false` when the session was already there: a re-attach.
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAssistantError {
    Refused(Denied),
    /// The templates folder could not be created.
    Dir(String),
    /// The session did not come up.
    Spawn(String),
}

/// Open (or re-attach) the assistant of an Actor: create-if-absent, **one per
/// Actor** (CORE-61), the Operation `pipeline` × `open_assistant`. Unlike the
/// run shell there is no Run to gate on. Nothing about *which* template is
/// being edited is frozen at spawn time: the `UserPromptSubmit` hook re-injects
/// its Actor's open template on every message.
///
/// The launch hooks of the extensions are asked for it
/// ([`ExtensionHost::session_launch`], the Actor as who starts it): their
/// variables go into its environment, their text at the top of its primer.
///
/// The session persists across a PTY/tab close; [`CloseAssistant`] reaps it when
/// the user leaves every edit view, and the sweep's idle arm is the backstop for
/// the case no close can cover, a browser reload.
pub struct OpenAssistant {
    tmux: Arc<dyn Tmux>,
    harness: Arc<dyn AssistantHarness>,
    host: Arc<dyn ExtensionHost>,
    launch: AssistantLaunch,
    ledger: Arc<SessionLedger>,
}

impl OpenAssistant {
    pub fn new(
        tmux: Arc<dyn Tmux>,
        harness: Arc<dyn AssistantHarness>,
        host: Arc<dyn ExtensionHost>,
        launch: AssistantLaunch,
    ) -> Self {
        Self {
            ledger: Arc::new(SessionLedger::new(tmux.clone(), host.clone())),
            tmux,
            harness,
            host,
            launch,
        }
    }

    /// The same use case, announcing the sessions it starts in `ledger`.
    pub fn with_ledger(mut self, ledger: Arc<SessionLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    pub async fn execute(&self, actor: &Actor) -> Result<Opened, OpenAssistantError> {
        // No target: the Pipelines of the instance, none in particular.
        let operation = Operation::new(ResourceKind::Pipeline, Verb::Action(OPEN_ASSISTANT.into()));
        authorize(self.host.as_ref(), actor, &operation).map_err(OpenAssistantError::Refused)?;
        let repo_root = &self.launch.repo_root;
        let cwd = libassist_cwd(repo_root);
        // The cwd must exist for `tmux new-session -c` to succeed, and a repo with
        // no templates yet has no `pipelines/` dir.
        std::fs::create_dir_all(&cwd)
            .map_err(|e| OpenAssistantError::Dir(format!("failed to create pipelines dir: {e}")))?;

        let session = libassist_session_name_of(actor);
        // Create-then-verify-on-failure, mirroring the run shell.
        if self.tmux.session_exists(&session) {
            return Ok(Opened {
                session,
                created: false,
            });
        }

        // Never sandboxed: authoring is design-time work on the host.
        let daemon_url = host_daemon_url(self.launch.daemon_port);
        let static_prompt = std::fs::read_to_string(
            repo_root
                .join("prompts")
                .join("builtin")
                .join("library-assistant.md"),
        )
        .unwrap_or_default();
        let session_ref = SessionRef::new(&session, SessionKind::LibraryAssistant);
        let contribution = self
            .host
            .session_launch(&SessionLaunch::new(session_ref.clone()).started_by(actor.clone()));
        let full_prompt =
            contribution.above(build_library_assistant_prompt(&daemon_url, &static_prompt));
        let extension_env: Vec<(String, String)> = contribution.env.into_iter().collect();

        let harness = self.harness.resolve().await;
        // The per-message focus hook rides on `--settings`, a hole only the
        // `claude` launch template has. On another harness the token is dropped
        // silently and the primer's fetch-the-focus instruction becomes the only
        // mechanism — say so once, at spawn, rather than let it be discovered from
        // an assistant guessing wrong.
        if !harness.can_inject_hooks()
            || !pdo_harness::probes::settings_hole_takes_claude_file(&harness.name)
        {
            warn!(
                "library assistant on harness '{}': its launch template has no {{settings}} hole \
                 that takes the claude-format hook file, so the UserPromptSubmit focus hook is NOT armed. The assistant will know the open \
                 pipeline only if it follows its primer's instruction to fetch \
                 GET /sessions/libassist/focus before acting (ADR-0051 §3).",
                harness.name
            );
        }

        // Keep the primer OUT of the user-facing templates dir: it and the hook
        // settings live in a sibling `.libassist/`, one primer per session.
        let prompt_path = libassist_primer_path(repo_root, &session);
        let tmux = self.tmux.clone();
        let spawned = spawn_libassist(
            tmux.as_ref(),
            &session,
            &full_prompt,
            &cwd,
            &prompt_path,
            self.launch.daemon_port,
            &harness,
            self.launch.tmux_cmd_override.as_deref(),
            &extension_env,
        );
        match spawned {
            Ok(()) => {
                info!("Opened library assistant {session} in {}", cwd.display());
                self.ledger.started(session_ref, Some(actor.clone()));
                Ok(Opened {
                    session,
                    created: true,
                })
            }
            // A concurrent open won the race — re-attach, not an error.
            Err(_) if self.tmux.session_exists(&session) => Ok(Opened {
                session,
                created: false,
            }),
            Err(e) => Err(OpenAssistantError::Spawn(format!(
                "failed to open library assistant: {e}"
            ))),
        }
    }
}

/// Reap the assistant of an Actor, fired when it leaves **every** pipeline edit view —
/// NOT when they leave the Assistant *tab*: the info panel closes by itself on
/// each edit-tab switch, so tab-leave would reap the conversation on every round
/// trip between two templates.
///
/// Best-effort — killing an absent session is a no-op. **Clears the focus by the
/// same gesture.** "No edit view is open" is one fact, and splitting it across two
/// calls made them diverge: the `pagehide` path can only afford a single
/// `keepalive` request, so it reaped the session and left the focus behind.
pub struct CloseAssistant {
    tmux: Arc<dyn Tmux>,
    focus: Arc<FocusCell>,
    host: Arc<dyn ExtensionHost>,
}

impl CloseAssistant {
    pub fn new(tmux: Arc<dyn Tmux>, focus: Arc<FocusCell>, host: Arc<dyn ExtensionHost>) -> Self {
        Self { tmux, focus, host }
    }

    /// Whether a session existed to reap.
    pub fn execute(&self, actor: &Actor) -> Result<bool, Denied> {
        let session = libassist_session_name_of(actor);
        let operation = Operation::new(ResourceKind::Session, Verb::Delete).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation)?;
        let session = session.as_str();
        self.focus.set(session, None);
        let existed = self.tmux.session_exists(session);
        self.tmux.kill(session);
        if existed {
            // `kill` swallows tmux's exit code, so corroborate rather than claim: a
            // "Reaped" line for a session still alive misdirects the next
            // investigation.
            if self.tmux.session_exists(session) {
                warn!(
                    "Failed to reap library assistant {session}: the tmux session is still alive"
                );
            } else {
                info!("Reaped library assistant {session}");
            }
        }
        Ok(existed)
    }
}
