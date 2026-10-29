use serde::{Deserialize, Serialize};

use crate::{Actor, NodeLaunch};

/// The `type` of the event the core publishes when an Actor starts a session
/// (API 0.13).
pub const SESSION_STARTED: &str = "session_started";

/// The `type` of the event the core publishes when an Actor takes control of
/// the terminal of a session (API 0.13).
pub const SESSION_TAKEN_OVER: &str = "session_taken_over";

/// The `type` of the event the core publishes when a session ended (API
/// 0.12).
pub const SESSION_ENDED: &str = "session_ended";

/// What a session runs (API 0.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionKind {
    /// A Node of a Run: an agent or a script.
    Node,
    /// The Pipeline Manager of a Run, an agent.
    Manager,
    /// The library assistant, an agent outside any Run: one session per
    /// Actor.
    LibraryAssistant,
    /// The shell of a Run, opened by hand: no agent.
    Shell,
    /// A Session hébergée (API 0.19): an agent or a shell an extension has
    /// the core hold outside any Run.
    Hosted,
}

/// A session of the core (API 0.13): the tmux session an agent, a script or a
/// shell runs in, as the terminals name it (`/sessions/<id>/pty`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionRef {
    /// The session's name.
    pub id: String,
    pub kind: SessionKind,
    /// The Run it belongs to; `None` for the library assistant.
    pub run_id: Option<String>,
    /// The Node it runs, for a [`SessionKind::Node`].
    pub node_id: Option<String>,
    /// The iteration of that Node, from 1.
    pub iter: Option<i64>,
    /// For a [`SessionKind::Hosted`], the extension that opened it (API
    /// 0.19); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// For a [`SessionKind::Hosted`], the key its extension gave it (API
    /// 0.19); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl SessionRef {
    /// The session `id`, of a `kind`, outside any Run.
    pub fn new(id: impl Into<String>, kind: SessionKind) -> Self {
        Self {
            id: id.into(),
            kind,
            run_id: None,
            node_id: None,
            iter: None,
            extension: None,
            key: None,
        }
    }

    /// The same session, in the Run `run_id`.
    pub fn in_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    /// The same session, the Session hébergée `key` of the extension
    /// `extension`.
    pub fn hosted_by(mut self, extension: impl Into<String>, key: impl Into<String>) -> Self {
        self.extension = Some(extension.into());
        self.key = Some(key.into());
        self
    }

    /// The same session, running iteration `iter` of the Node `node_id`.
    pub fn of_node(mut self, node_id: impl Into<String>, iter: i64) -> Self {
        self.node_id = Some(node_id.into());
        self.iter = Some(iter);
        self
    }
}

/// The launch of a session that makes an agent act, or of a script Node, as
/// [`Extension::session_env`](crate::Extension::session_env) and
/// [`Extension::session_preamble`](crate::Extension::session_preamble) see it
/// (API 0.13): which session, in which Run and Projets, and who starts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct SessionLaunch {
    pub session: SessionRef,
    /// The Actor who starts the session: for a Node, the creator of its Run
    /// (the Actor of the Run's `run` × `create`); for the Pipeline Manager,
    /// the creator of the Run when it starts with the Run, otherwise the
    /// Actor whose gesture starts it (its Start, the sending of review
    /// comments); for the library assistant and a Session hébergée, the
    /// Actor who opens it. `None`
    /// for a Run created before the core recorded its creator.
    pub started_by: Option<Actor>,
    /// The Projets of the Run, or the Projet of a Session hébergée (empty: in
    /// none, and for the library assistant).
    pub projects: Vec<String>,
    /// For a Node, its launch as [`Extension::node_env`](crate::Extension::node_env)
    /// sees it; `None` for any other session.
    pub node: Option<NodeLaunch>,
}

impl SessionLaunch {
    /// The launch of `session`, started by nobody known, in no Projet.
    pub fn new(session: SessionRef) -> Self {
        Self {
            session,
            started_by: None,
            projects: Vec::new(),
            node: None,
        }
    }

    /// The same launch, started by `actor`.
    pub fn started_by(mut self, actor: Actor) -> Self {
        self.started_by = Some(actor);
        self
    }

    /// The same launch, in the Projets `projects`.
    pub fn in_projects<I, S>(mut self, projects: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.projects = projects.into_iter().map(Into::into).collect();
        self
    }

    /// The same launch, of the Node `node`.
    pub fn of_node(mut self, node: NodeLaunch) -> Self {
        self.node = Some(node);
        self
    }
}

/// What happened to a session (API 0.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionChange {
    /// An Actor started it ([`SESSION_STARTED`]).
    #[serde(rename = "session_started")]
    Started,
    /// An Actor took control of its terminal ([`SESSION_TAKEN_OVER`]): it
    /// attached and drives it alone, or it took the hand from the pilot of a
    /// shared terminal. Watching as a spectator takes nothing.
    #[serde(rename = "session_taken_over")]
    TakenOver,
    /// It ended, however ([`SESSION_ENDED`]): the work is over, it was
    /// stopped, reaped or it died.
    #[serde(rename = "session_ended")]
    Ended,
}

/// An event of a session, as the core publishes it (API 0.13): to every
/// extension's `on_event` and, for a session of a Run, on `/ws` to the
/// connections that may read the Run.
///
/// On the wire: `{"type": "session_started" | "session_taken_over" |
/// "session_ended", "actor": <Actor> | null, "session": {"id", "kind",
/// "run_id", "node_id", "iter"}}`. `actor` is who started the session (for
/// `session_started` and `session_ended`) or who took control of it
/// (`session_taken_over`); `null` when the core does not know it (a Run
/// created before the core recorded its creator, a session started before
/// the daemon did).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionEvent {
    #[serde(rename = "type")]
    pub change: SessionChange,
    pub actor: Option<Actor>,
    pub session: SessionRef,
}

impl SessionEvent {
    pub fn new(change: SessionChange, actor: Option<Actor>, session: SessionRef) -> Self {
        Self {
            change,
            actor,
            session,
        }
    }

    /// The event as `on_event` receives it.
    pub fn to_event(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// The session event `event` is, if it is one: what an extension's
    /// `on_event` reads it with.
    pub fn from_event(event: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(event.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    #[test]
    fn an_event_carries_its_type_its_actor_and_its_session() {
        let event = SessionEvent::new(
            SessionChange::TakenOver,
            Some(Actor::Identified {
                namespace: "test".into(),
                id: "bob".into(),
            }),
            SessionRef::new("pdo-r1-n1-iter-1", SessionKind::Node)
                .in_run("r1")
                .of_node("n1", 1),
        );
        let json = event.to_event();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "session_taken_over",
                "actor": { "kind": "identified", "namespace": "test", "id": "bob" },
                "session": {
                    "id": "pdo-r1-n1-iter-1", "kind": "node", "run_id": "r1",
                    "node_id": "n1", "iter": 1,
                },
            })
        );
        assert_eq!(SessionEvent::from_event(&json), Some(event));
        assert_eq!(
            SessionEvent::from_event(&serde_json::json!({ "type": "event" })),
            None
        );
    }

    #[test]
    fn an_ended_session_of_nobody_known_reads_back() {
        let event = SessionEvent::new(
            SessionChange::Ended,
            None,
            SessionRef::new("pdo-libassist-shared", SessionKind::LibraryAssistant),
        );
        let json = event.to_event();
        assert_eq!(json["type"], SESSION_ENDED);
        assert_eq!(json["actor"], serde_json::Value::Null);
        assert_eq!(json["session"]["kind"], "library_assistant");
        assert_eq!(SessionEvent::from_event(&json), Some(event));
        assert_eq!(
            serde_json::to_value(SessionChange::Started).unwrap(),
            SESSION_STARTED
        );
        assert_eq!(
            serde_json::to_value(SessionChange::TakenOver).unwrap(),
            SESSION_TAKEN_OVER
        );
    }

    #[test]
    fn a_hosted_session_names_its_extension_and_key_and_the_others_do_not() {
        let hosted =
            SessionRef::new("pdo-hosted-1a2b", SessionKind::Hosted).hosted_by("test", "k1");
        assert_eq!(
            serde_json::to_value(&hosted).unwrap(),
            serde_json::json!({
                "id": "pdo-hosted-1a2b", "kind": "hosted", "run_id": null, "node_id": null,
                "iter": null, "extension": "test", "key": "k1",
            })
        );
        let event = SessionEvent::new(SessionChange::Started, None, hosted);
        assert_eq!(SessionEvent::from_event(&event.to_event()), Some(event));
    }

    #[test]
    fn a_launch_serializes_what_it_knows() {
        let launch =
            SessionLaunch::new(SessionRef::new("pdo-mgr-r1", SessionKind::Manager).in_run("r1"))
                .started_by(Actor::LocalOperator)
                .in_projects(["p1"]);
        assert_eq!(
            serde_json::to_value(&launch).unwrap(),
            serde_json::json!({
                "session": {
                    "id": "pdo-mgr-r1", "kind": "manager", "run_id": "r1",
                    "node_id": null, "iter": null,
                },
                "started_by": { "kind": "local_operator" },
                "projects": ["p1"],
                "node": null,
            })
        );
        let node = NodeLaunch::new("r1", "n1", 1, NodeKind::Script, "/repo");
        let launch =
            SessionLaunch::new(SessionRef::new("s", SessionKind::Node)).of_node(node.clone());
        assert_eq!(launch.node, Some(node));
        assert_eq!(launch.started_by, None);
        assert!(launch.projects.is_empty());
    }
}
