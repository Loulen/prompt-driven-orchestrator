//! The events of the Sessions hébergées (API 0.19): a change of their state,
//! their forgetting, a change of their Artefacts live. Each goes to every
//! extension's `on_event` and, on `/ws`, to the connections whose Actor may
//! read the session.

use serde::{Deserialize, Serialize};

use crate::{HostedSession, LiveArtifact};

/// The `type` of the event the core publishes each time the state of a
/// Session hébergée changes ([`HostedSessionChanged`]).
pub const HOSTED_SESSION_CHANGED: &str = "hosted_session_changed";

/// The `type` of the event the core publishes when an extension forgot a
/// Session hébergée ([`HostedSessionForgotten`]).
pub const HOSTED_SESSION_FORGOTTEN: &str = "hosted_session_forgotten";

/// The `type` of the event the core publishes each time the Artefacts live of
/// a Session hébergée change ([`HostedLiveArtifactsChanged`]).
pub const HOSTED_LIVE_ARTIFACTS_CHANGED: &str = "hosted_live_artifacts_changed";

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

/// The event of a Session hébergée its extension forgot (API 0.19): its
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
/// 0.19): a file of its `PDO_LIVE_DIR` was written, added or removed.
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
    use crate::{Actor, EndReason, HostedConfig, HostedFolder, HostedKind, HostedState};

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
            terminal_zoom: 150,
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
}
