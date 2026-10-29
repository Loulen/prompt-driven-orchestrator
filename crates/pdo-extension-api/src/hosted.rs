//! The Sessions hébergées (API 0.17, ADR-0091 of the core): a session of an
//! agent, or a shell, an extension has the core hold outside any Run, through
//! its [`Host`](crate::Host). The core records it, keeps it alive across a
//! restart of the daemon, publishes each change of its state and never writes
//! into its folder.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::Actor;

/// The `type` of the event the core publishes each time the state of a
/// Session hébergée changes ([`HostedSessionChanged`]).
pub const HOSTED_SESSION_CHANGED: &str = "hosted_session_changed";

/// The `type` of the event the core publishes when an extension forgot a
/// Session hébergée ([`HostedSessionForgotten`]).
pub const HOSTED_SESSION_FORGOTTEN: &str = "hosted_session_forgotten";

/// The `type` of the event the core publishes each time the Artefacts live of
/// a Session hébergée change ([`HostedLiveArtifactsChanged`]).
pub const HOSTED_LIVE_ARTIFACTS_CHANGED: &str = "hosted_live_artifacts_changed";

/// What a Session hébergée runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostedKind {
    /// An agent, on a harness: the Contribution au lancement of the
    /// extensions, then its initial prompt, and nothing of the core.
    Agent,
    /// An interactive shell.
    Shell,
}

/// How an agent session is launched. Every field left empty takes the
/// instance's default (the default harness, its default model and effort, no
/// initial prompt). A shell reads none of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HostedConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// What the agent reads after the extensions' text, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl HostedConfig {
    /// The same configuration, on the harness `harness`.
    pub fn on_harness(mut self, harness: impl Into<String>) -> Self {
        self.harness = Some(harness.into());
        self
    }

    /// The same configuration, with the model `model`.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// The same configuration, at the effort `effort`.
    pub fn with_effort(mut self, effort: impl Into<String>) -> Self {
        self.effort = Some(effort.into());
        self
    }

    /// The same configuration, with the initial prompt `prompt`.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }
}

/// Where a Session hébergée works. The core never writes there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostedFolder {
    /// An absolute path to an existing folder.
    Path { path: String },
    /// The primary repository of the session's Projet.
    ProjectRepo,
    /// The home folder of the daemon's user.
    #[default]
    Home,
}

/// The Session hébergée an extension asks its Host to open
/// ([`Host::open_session`](crate::Host::open_session)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenSession {
    /// The extension's own name for the session, unique among its sessions:
    /// what it reads, terminates and lists it by.
    pub key: String,
    pub kind: HostedKind,
    #[serde(default)]
    pub config: HostedConfig,
    #[serde(default)]
    pub folder: HostedFolder,
    /// The Projet the session is in: its Operations are in it, and so is the
    /// Contribution au lancement. `None`: in none.
    #[serde(default)]
    pub project: Option<String>,
}

impl OpenSession {
    /// An agent session `key`, on the instance's defaults, in the home folder,
    /// in no Projet.
    pub fn agent(key: impl Into<String>) -> Self {
        Self::of(key, HostedKind::Agent)
    }

    /// A shell `key`, in the home folder, in no Projet.
    pub fn shell(key: impl Into<String>) -> Self {
        Self::of(key, HostedKind::Shell)
    }

    fn of(key: impl Into<String>, kind: HostedKind) -> Self {
        Self {
            key: key.into(),
            kind,
            config: HostedConfig::default(),
            folder: HostedFolder::default(),
            project: None,
        }
    }

    /// The same session, launched with `config`.
    pub fn with_config(mut self, config: HostedConfig) -> Self {
        self.config = config;
        self
    }

    /// The same session, working in `folder`.
    pub fn in_folder(mut self, folder: HostedFolder) -> Self {
        self.folder = folder;
        self
    }

    /// The same session, in the Projet `project`.
    pub fn in_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }
}

/// Where a Session hébergée stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostedState {
    /// Recorded, its process being launched.
    Starting,
    Running,
    /// Its agent declared it waits for a person (`pdo wait-user`).
    AwaitingUser,
    /// Over: see its [`EndReason`].
    Ended,
}

impl HostedState {
    /// Whether it holds a process (and a slot of the Cap).
    pub fn is_live(self) -> bool {
        !matches!(self, HostedState::Ended)
    }
}

/// Why a Session hébergée ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EndReason {
    /// Someone terminated it.
    Terminated,
    /// Its process exited, or was gone when the daemon started.
    Exited,
}

/// A Session hébergée, as the core records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HostedSession {
    /// The session's name: what its terminal is opened by
    /// (`/sessions/<id>/pty`).
    pub id: String,
    /// The extension that opened it.
    pub extension: String,
    pub key: String,
    pub kind: HostedKind,
    pub config: HostedConfig,
    /// The folder as it was asked for.
    pub folder: HostedFolder,
    /// The folder it works in, resolved when it was opened.
    pub cwd: String,
    pub project: Option<String>,
    /// The Actor who started it; `None` when the core does not know it.
    pub started_by: Option<Actor>,
    pub state: HostedState,
    /// Set once it ended.
    pub end_reason: Option<EndReason>,
    /// What its agent said it waits for (`pdo wait-user --message`), while it
    /// is `awaiting_user`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_message: Option<String>,
    /// When it was opened, then last changed (RFC 3339).
    pub created_at: String,
    pub updated_at: String,
}

impl HostedSession {
    /// The session `id` an extension opened under `key`, working in `cwd`,
    /// just recorded: starting, on the defaults, in the home folder, in no
    /// Projet, started by nobody known.
    pub fn new(
        id: impl Into<String>,
        extension: impl Into<String>,
        key: impl Into<String>,
        kind: HostedKind,
        cwd: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            extension: extension.into(),
            key: key.into(),
            kind,
            config: HostedConfig::default(),
            folder: HostedFolder::default(),
            cwd: cwd.into(),
            project: None,
            started_by: None,
            state: HostedState::Starting,
            end_reason: None,
            wait_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

/// Why the core did not do what an extension asked of a Session hébergée: an
/// HTTP status and a message, as a route of the extension would answer them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostedRefusal {
    pub status: u16,
    pub message: String,
}

impl HostedRefusal {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// `503`: this host holds no Session hébergée.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(503, message)
    }
}

impl fmt::Display for HostedRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.status)
    }
}

impl std::error::Error for HostedRefusal {}

/// What a gesture on a Session hébergée answers: the session as it now
/// stands, or the refusal.
pub type HostedGesture =
    Pin<Box<dyn Future<Output = Result<HostedSession, HostedRefusal>> + Send + 'static>>;

/// One Artefact live of a Session hébergée: a file its agent wrote in its
/// `PDO_LIVE_DIR`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LiveArtifact {
    /// Its path under `PDO_LIVE_DIR`, `/`-separated (`notes/plan.md`).
    pub path: String,
    /// Its size, in bytes.
    pub size: u64,
    /// When it was last written (RFC 3339).
    pub modified_at: String,
}

impl LiveArtifact {
    pub fn new(path: impl Into<String>, size: u64, modified_at: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            size,
            modified_at: modified_at.into(),
        }
    }
}

/// The event of a change of state of a Session hébergée: to every
/// extension's `on_event` and, on `/ws`, to the connections whose Actor may
/// read the session (`session` × `read`, in its Projet).
///
/// On the wire: `{"type": "hosted_session_changed", "session": <HostedSession>}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HostedSessionChanged {
    pub session: HostedSession,
}

impl HostedSessionChanged {
    pub fn new(session: HostedSession) -> Self {
        Self { session }
    }

    /// The event as `on_event` receives it.
    pub fn to_event(&self) -> serde_json::Value {
        serde_json::json!({ "type": HOSTED_SESSION_CHANGED, "session": self.session })
    }

    /// The change `event` is, if it is one.
    pub fn from_event(event: &serde_json::Value) -> Option<Self> {
        if event.get("type")? != HOSTED_SESSION_CHANGED {
            return None;
        }
        serde_json::from_value(event.get("session")?.clone())
            .ok()
            .map(Self::new)
    }
}

/// The event of a Session hébergée its extension forgot (API 0.17): its
/// record and its state folder are gone. Published like
/// [`HostedSessionChanged`].
///
/// On the wire: `{"type": "hosted_session_forgotten", "session": <HostedSession>}`,
/// the session as it was last recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HostedSessionForgotten {
    pub session: HostedSession,
}

impl HostedSessionForgotten {
    pub fn new(session: HostedSession) -> Self {
        Self { session }
    }

    /// The event as `on_event` receives it.
    pub fn to_event(&self) -> serde_json::Value {
        serde_json::json!({ "type": HOSTED_SESSION_FORGOTTEN, "session": self.session })
    }

    /// The forgetting `event` is, if it is one.
    pub fn from_event(event: &serde_json::Value) -> Option<Self> {
        if event.get("type")? != HOSTED_SESSION_FORGOTTEN {
            return None;
        }
        serde_json::from_value(event.get("session")?.clone())
            .ok()
            .map(Self::new)
    }
}

/// The event of a change of the Artefacts live of a Session hébergée (API
/// 0.17): a file of its `PDO_LIVE_DIR` was written, added or removed.
/// Published like [`HostedSessionChanged`], with every Artefact live it now
/// has.
///
/// On the wire: `{"type": "hosted_live_artifacts_changed", "id", "extension",
/// "key", "artifacts": [<LiveArtifact>…]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HostedLiveArtifactsChanged {
    /// The session's name.
    pub id: String,
    pub extension: String,
    pub key: String,
    pub artifacts: Vec<LiveArtifact>,
}

impl HostedLiveArtifactsChanged {
    pub fn new(session: &HostedSession, artifacts: Vec<LiveArtifact>) -> Self {
        Self {
            id: session.id.clone(),
            extension: session.extension.clone(),
            key: session.key.clone(),
            artifacts,
        }
    }

    /// The event as `on_event` receives it.
    pub fn to_event(&self) -> serde_json::Value {
        let mut event = serde_json::to_value(self).unwrap_or_default();
        event["type"] = serde_json::json!(HOSTED_LIVE_ARTIFACTS_CHANGED);
        event
    }

    /// The change `event` is, if it is one.
    pub fn from_event(event: &serde_json::Value) -> Option<Self> {
        if event.get("type")? != HOSTED_LIVE_ARTIFACTS_CHANGED {
            return None;
        }
        serde_json::from_value(event.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> HostedSession {
        HostedSession {
            id: "pdo-hosted-1a2b3c4d5e6f".into(),
            extension: "test".into(),
            key: "k1".into(),
            kind: HostedKind::Agent,
            config: HostedConfig::default().with_model("opus"),
            folder: HostedFolder::Path {
                path: "/tmp/x".into(),
            },
            cwd: "/tmp/x".into(),
            project: Some("p1".into()),
            started_by: Some(Actor::LocalOperator),
            state: HostedState::Ended,
            end_reason: Some(EndReason::Terminated),
            wait_message: None,
            created_at: "2026-10-28T10:00:00Z".into(),
            updated_at: "2026-10-28T10:05:00Z".into(),
        }
    }

    #[test]
    fn a_change_carries_its_type_and_the_session_and_reads_back() {
        let change = HostedSessionChanged::new(session());
        let json = change.to_event();
        assert_eq!(json["type"], HOSTED_SESSION_CHANGED);
        assert_eq!(json["session"]["state"], "ended");
        assert_eq!(json["session"]["end_reason"], "terminated");
        assert_eq!(json["session"]["kind"], "agent");
        assert_eq!(
            json["session"]["folder"],
            serde_json::json!({ "kind": "path", "path": "/tmp/x" })
        );
        assert_eq!(
            json["session"]["config"],
            serde_json::json!({ "model": "opus" })
        );
        assert_eq!(HostedSessionChanged::from_event(&json), Some(change));
        assert_eq!(
            HostedSessionChanged::from_event(&serde_json::json!({ "type": "session_ended" })),
            None
        );
    }

    #[test]
    fn a_forgotten_session_and_a_change_of_its_artefacts_live_read_back() {
        let forgotten = HostedSessionForgotten::new(session());
        let json = forgotten.to_event();
        assert_eq!(json["type"], HOSTED_SESSION_FORGOTTEN);
        assert_eq!(json["session"]["key"], "k1");
        assert_eq!(HostedSessionForgotten::from_event(&json), Some(forgotten));
        assert_eq!(
            HostedSessionChanged::from_event(&json),
            None,
            "a forgetting is no change"
        );

        let live = HostedLiveArtifactsChanged::new(
            &session(),
            vec![LiveArtifact::new(
                "notes/plan.md",
                12,
                "2026-10-28T10:06:00Z",
            )],
        );
        let json = live.to_event();
        assert_eq!(
            json,
            serde_json::json!({
                "type": HOSTED_LIVE_ARTIFACTS_CHANGED, "id": "pdo-hosted-1a2b3c4d5e6f",
                "extension": "test", "key": "k1",
                "artifacts": [{
                    "path": "notes/plan.md", "size": 12, "modified_at": "2026-10-28T10:06:00Z",
                }],
            })
        );
        assert_eq!(HostedLiveArtifactsChanged::from_event(&json), Some(live));
        assert_eq!(
            HostedLiveArtifactsChanged::from_event(
                &HostedSessionChanged::new(session()).to_event()
            ),
            None
        );
    }

    #[test]
    fn a_wait_message_travels_only_when_there_is_one() {
        let mut waiting = session();
        assert!(serde_json::to_value(&waiting)
            .unwrap()
            .get("wait_message")
            .is_none());
        waiting.state = HostedState::AwaitingUser;
        waiting.wait_message = Some("Which DB?".into());
        let json = serde_json::to_value(&waiting).unwrap();
        assert_eq!(json["state"], "awaiting_user");
        assert_eq!(json["wait_message"], "Which DB?");
        assert_eq!(
            serde_json::from_value::<HostedSession>(json).unwrap(),
            waiting
        );
    }

    #[test]
    fn an_open_defaults_to_the_home_folder_in_no_projet() {
        let open = OpenSession::shell("s");
        assert_eq!(open.folder, HostedFolder::Home);
        assert_eq!(open.project, None);
        assert_eq!(open.config, HostedConfig::default());
        let open: OpenSession =
            serde_json::from_value(serde_json::json!({ "key": "a", "kind": "agent" })).unwrap();
        assert_eq!(open, OpenSession::agent("a"));
        let open = OpenSession::agent("b")
            .in_project("p1")
            .in_folder(HostedFolder::ProjectRepo)
            .with_config(HostedConfig::default().with_prompt("hi"));
        assert_eq!(
            serde_json::to_value(&open).unwrap(),
            serde_json::json!({
                "key": "b", "kind": "agent", "config": { "prompt": "hi" },
                "folder": { "kind": "project_repo" }, "project": "p1",
            })
        );
    }

    #[test]
    fn only_an_ended_session_holds_no_process() {
        assert!(HostedState::Starting.is_live());
        assert!(HostedState::Running.is_live());
        assert!(HostedState::AwaitingUser.is_live());
        assert!(!HostedState::Ended.is_live());
        assert_eq!(
            HostedRefusal::new(409, "session cap reached").to_string(),
            "session cap reached (409)"
        );
    }
}
