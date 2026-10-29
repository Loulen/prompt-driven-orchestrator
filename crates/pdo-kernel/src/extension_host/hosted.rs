//! The port through which an extension's [`Host`](pdo_extension_api::Host)
//! has the core hold its Sessions hébergées (API 0.17, ADR-0091).

use std::sync::Arc;

use pdo_extension_api::{HostedGesture, HostedSession, OpenSession};

use super::Extensions;
use crate::Actor;

/// What holds the Sessions hébergées of the extensions (a port: the session
/// context in the daemon). Every call names the extension the session belongs
/// to: an extension only ever reaches its own.
pub trait HostedSessions: Send + Sync {
    /// Open `open` for the extension `extension`, on behalf of `actor`.
    fn open(&self, extension: &str, actor: Actor, open: OpenSession) -> HostedGesture;

    /// The session `key` of `extension`, if it gave that key.
    fn read(&self, extension: &str, key: &str) -> Option<HostedSession>;

    /// Every session of `extension`, in the order they were opened.
    fn list(&self, extension: &str) -> Vec<HostedSession>;

    /// Terminate the session `key` of `extension`, on behalf of `actor`.
    fn terminate(&self, extension: &str, actor: Actor, key: &str) -> HostedGesture;
}

impl Extensions {
    /// Plug in what holds the extensions' Sessions hébergées, once the daemon
    /// wired its contexts. Until then a `Host::open_session` answers `503`.
    pub fn serve_hosted_sessions(&self, sessions: Arc<dyn HostedSessions>) {
        if self.registry.hosted.set(sessions).is_err() {
            tracing::warn!("the hosted sessions were plugged twice; the second is ignored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::{Extension, HostedKind, HostedRefusal, Manifest};
    use std::sync::Mutex;

    struct Quiet;
    impl Extension for Quiet {
        fn manifest(&self) -> Manifest {
            Manifest::new("quiet", "Quiet", "0.0.0")
        }
    }

    /// Records who asked what, holds one session per extension and key.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<HostedSession>>);
    impl HostedSessions for Recorder {
        fn open(&self, extension: &str, actor: Actor, open: OpenSession) -> HostedGesture {
            let mut session = HostedSession::new(
                format!("pdo-hosted-{}", open.key),
                extension,
                open.key,
                open.kind,
                "/tmp",
            );
            session.started_by = Some(actor);
            self.0.lock().unwrap().push(session.clone());
            Box::pin(std::future::ready(Ok(session)))
        }
        fn read(&self, extension: &str, key: &str) -> Option<HostedSession> {
            self.list(extension).into_iter().find(|s| s.key == key)
        }
        fn list(&self, extension: &str) -> Vec<HostedSession> {
            let sessions = self.0.lock().unwrap();
            sessions
                .iter()
                .filter(|s| s.extension == extension)
                .cloned()
                .collect()
        }
        fn terminate(&self, extension: &str, _actor: Actor, key: &str) -> HostedGesture {
            let found = self.read(extension, key);
            Box::pin(std::future::ready(
                found.ok_or_else(|| HostedRefusal::new(404, "no such session")),
            ))
        }
    }

    #[tokio::test]
    async fn a_host_holds_its_own_sessions_once_the_daemon_plugs_them() {
        let extensions = Extensions::register(vec![Box::new(Quiet)]).unwrap();
        let host = extensions.host_of("quiet");
        let before = host
            .open_session(&Actor::LocalOperator, OpenSession::shell("s1"))
            .await
            .unwrap_err();
        assert_eq!(before.status, 503);
        assert!(host.hosted_sessions().is_empty());

        extensions.serve_hosted_sessions(Arc::new(Recorder::default()));
        let opened = host
            .open_session(&Actor::LocalOperator, OpenSession::agent("s1"))
            .await
            .unwrap();
        assert_eq!(opened.extension, "quiet");
        assert_eq!(opened.kind, HostedKind::Agent);
        assert_eq!(opened.started_by, Some(Actor::LocalOperator));
        assert_eq!(host.hosted_session("s1"), Some(opened.clone()));
        assert_eq!(host.hosted_sessions(), vec![opened]);
        assert_eq!(
            extensions.host_of("other").hosted_session("s1"),
            None,
            "an extension reaches only its own"
        );
        assert_eq!(
            host.terminate_session(&Actor::LocalOperator, "nope")
                .await
                .unwrap_err()
                .status,
            404
        );
    }
}
