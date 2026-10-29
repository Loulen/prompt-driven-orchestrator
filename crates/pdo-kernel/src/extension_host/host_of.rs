//! Each extension's [`Host`]: through it, an extension runs its own mutations
//! like the core's (amendment #34 of ADR-0081), and publishes to the others.

use std::sync::Weak;

use std::sync::Arc;

use pdo_extension_api::{
    ActorDisplay, Attributes, Authorization, EntityRef, ExtensionData, ExtensionEvent, Host,
    HostedConfig, HostedGesture, HostedRefusal, HostedSession, LiveArtifact, NewRun, OpenSession,
    OperationApplied, Presence, Resource, RunCostRead, RunCreation, RunNotCreated,
};

use super::{ExtensionHost, Extensions, HostedSessions, Registry};
use crate::event_bus::guarded;
use crate::{Actor, Operation};

/// The announcement of a change of the Triggers on `/ws`; without
/// `trigger_id`, it is about them all.
pub(super) const TRIGGER_UPDATED: &str = "trigger_updated";

/// The [`Host`] of one extension, `id`.
pub(super) struct ExtensionHostOf {
    pub(super) id: String,
    pub(super) registry: Weak<Registry>,
}

impl ExtensionHostOf {
    fn extensions(&self) -> Option<Extensions> {
        self.registry
            .upgrade()
            .map(|registry| Extensions { registry })
    }

    fn hosted(&self) -> Option<Arc<dyn HostedSessions>> {
        self.registry
            .upgrade()
            .and_then(|registry| registry.hosted.get().cloned())
    }
}

/// The answer of a gesture on a Session hébergée before the daemon plugged
/// what holds them.
fn not_hosted() -> HostedGesture {
    Box::pin(std::future::ready(Err(HostedRefusal::unavailable(
        NOT_HOSTED,
    ))))
}

const NOT_HOSTED: &str = "the daemon holds no hosted session yet (starting or stopping)";

impl Host for ExtensionHostOf {
    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        let Some(extensions) = self.extensions() else {
            return Authorization::deny("the daemon is stopping");
        };
        let authorization = ExtensionHost::authorize(&extensions, actor, operation);
        match crate::authorization::decide(authorization, actor, operation) {
            Ok(()) => Authorization::Allow,
            Err(denied) => Authorization::Deny {
                reason: denied.reason,
            },
        }
    }

    /// A guarded event rides the bus with its guard ([`guarded`]): `/ws`
    /// judges each connection by it, the extensions receive it without.
    fn publish(&self, event: ExtensionEvent) {
        if let Some(extensions) = self.extensions() {
            let message = event.to_event(&self.id);
            extensions.broadcast(match &event.guard {
                Some(read) => guarded(message, read),
                None => message,
            });
        }
    }

    fn applied(&self, applied: OperationApplied) {
        if let Some(extensions) = self.extensions() {
            extensions.broadcast(applied.to_event());
        }
    }

    fn on_create(&self, actor: &Actor, entity: &EntityRef) -> Attributes {
        self.extensions()
            .map(|extensions| {
                extensions.stamps(actor, entity, &ExtensionData::new(), Some(&self.id))
            })
            .unwrap_or_default()
    }

    fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
        self.extensions()
            .and_then(|extensions| ExtensionHost::display_name(&extensions, actor))
    }

    fn catalogue(&self) -> Vec<Resource> {
        match self.extensions() {
            Some(extensions) => ExtensionHost::catalogue(&extensions),
            None => pdo_extension_api::core_resources(),
        }
    }

    fn project_name(&self, project_id: &str) -> Option<String> {
        self.registry
            .upgrade()?
            .project_names
            .get()?
            .name(project_id)
    }

    fn triggers_changed(&self) {
        if let Some(extensions) = self.extensions() {
            extensions.broadcast(serde_json::json!({ "type": TRIGGER_UPDATED }));
        }
    }

    fn presences(&self, run_id: &str) -> Vec<Presence> {
        self.registry
            .upgrade()
            .and_then(|registry| registry.run_views.get().cloned())
            .map(|views| views.presences(run_id))
            .unwrap_or_default()
    }

    fn run_cost(&self, run_id: &str) -> RunCostRead {
        let views = self
            .registry
            .upgrade()
            .and_then(|registry| registry.run_views.get().cloned());
        match views {
            Some(views) => views.run_cost(run_id),
            None => Box::pin(std::future::ready(None)),
        }
    }

    fn create_run(&self, actor: &Actor, run: NewRun) -> RunCreation {
        let runs = self
            .registry
            .upgrade()
            .and_then(|registry| registry.runs.get().cloned());
        match runs {
            Some(runs) => runs.create_run(actor.clone(), run),
            None => Box::pin(std::future::ready(Err(RunNotCreated::unavailable(
                "the daemon creates no Run yet (starting or stopping)",
            )))),
        }
    }

    fn open_session(&self, actor: &Actor, open: OpenSession) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.open(&self.id, actor.clone(), open),
            None => not_hosted(),
        }
    }

    fn hosted_session(&self, key: &str) -> Option<HostedSession> {
        self.hosted()?.read(&self.id, key)
    }

    fn hosted_sessions(&self) -> Vec<HostedSession> {
        self.hosted()
            .map(|hosted| hosted.list(&self.id))
            .unwrap_or_default()
    }

    fn terminate_session(&self, actor: &Actor, key: &str) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.terminate(&self.id, actor.clone(), key),
            None => not_hosted(),
        }
    }

    fn relaunch_session(&self, actor: &Actor, key: &str) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.relaunch(&self.id, actor.clone(), key),
            None => not_hosted(),
        }
    }

    fn configure_session(&self, actor: &Actor, key: &str, config: HostedConfig) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.configure(&self.id, actor.clone(), key, config),
            None => not_hosted(),
        }
    }

    fn zoom_session(&self, actor: &Actor, key: &str, zoom: u16) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.zoom(&self.id, actor.clone(), key, zoom),
            None => not_hosted(),
        }
    }

    fn type_in_session(&self, actor: &Actor, key: &str, text: &str) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.type_text(&self.id, actor.clone(), key, text),
            None => not_hosted(),
        }
    }

    fn forget_session(&self, actor: &Actor, key: &str) -> HostedGesture {
        match self.hosted() {
            Some(hosted) => hosted.forget(&self.id, actor.clone(), key),
            None => not_hosted(),
        }
    }

    fn live_artifacts(&self, key: &str) -> Result<Vec<LiveArtifact>, HostedRefusal> {
        match self.hosted() {
            Some(hosted) => hosted.live_artifacts(&self.id, key),
            None => Err(HostedRefusal::unavailable(NOT_HOSTED)),
        }
    }

    fn live_artifact(&self, key: &str, path: &str) -> Result<Vec<u8>, HostedRefusal> {
        match self.hosted() {
            Some(hosted) => hosted.live_artifact(&self.id, key, path),
            None => Err(HostedRefusal::unavailable(NOT_HOSTED)),
        }
    }
}

/// What a panic said, when it said it with a string.
pub(super) fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}
