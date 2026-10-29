//! The Sessions hébergées as the extensions' [`Host`](pdo_extension_api::Host)
//! reaches them (API 0.19): the kernel's port, over the session context.

use std::sync::Arc;

use pdo_extension_api::{
    HostedConfig, HostedGesture, HostedRefusal, HostedSession, LiveArtifact, OpenSession,
};
use pdo_kernel::extension_host::HostedSessions;
use pdo_kernel::Actor;

use crate::application::hosted::Hosted;

/// The Sessions hébergées of the session context, for the extensions.
pub struct ExtensionHostedSessions(pub Arc<Hosted>);

impl HostedSessions for ExtensionHostedSessions {
    fn open(&self, extension: &str, actor: Actor, open: OpenSession) -> HostedGesture {
        let (hosted, extension) = (self.0.clone(), extension.to_string());
        Box::pin(async move { hosted.open(&extension, &actor, open).await })
    }

    fn read(&self, extension: &str, key: &str) -> Option<HostedSession> {
        self.0
            .registry()
            .find(extension, key)
            .map(|record| record.session)
    }

    fn list(&self, extension: &str) -> Vec<HostedSession> {
        self.0.registry().list(extension)
    }

    fn terminate(&self, extension: &str, actor: Actor, key: &str) -> HostedGesture {
        let (hosted, extension, key) = (self.0.clone(), extension.to_string(), key.to_string());
        Box::pin(async move { hosted.terminate(&extension, &actor, &key).await })
    }

    fn relaunch(&self, extension: &str, actor: Actor, key: &str) -> HostedGesture {
        let (hosted, extension, key) = (self.0.clone(), extension.to_string(), key.to_string());
        Box::pin(async move { hosted.relaunch(&extension, &actor, &key).await })
    }

    fn configure(
        &self,
        extension: &str,
        actor: Actor,
        key: &str,
        config: HostedConfig,
    ) -> HostedGesture {
        let (hosted, extension, key) = (self.0.clone(), extension.to_string(), key.to_string());
        Box::pin(async move { hosted.configure(&extension, &actor, &key, config).await })
    }

    fn zoom(&self, extension: &str, actor: Actor, key: &str, zoom: u16) -> HostedGesture {
        let (hosted, extension, key) = (self.0.clone(), extension.to_string(), key.to_string());
        Box::pin(async move { hosted.zoom(&extension, &actor, &key, zoom).await })
    }

    fn type_text(&self, extension: &str, actor: Actor, key: &str, text: &str) -> HostedGesture {
        let (hosted, extension, key, text) = (
            self.0.clone(),
            extension.to_string(),
            key.to_string(),
            text.to_string(),
        );
        Box::pin(async move { hosted.type_text(&extension, &actor, &key, &text).await })
    }

    fn forget(&self, extension: &str, actor: Actor, key: &str) -> HostedGesture {
        let (hosted, extension, key) = (self.0.clone(), extension.to_string(), key.to_string());
        Box::pin(async move { hosted.forget(&extension, &actor, &key).await })
    }

    fn live_artifacts(&self, extension: &str, key: &str) -> Vec<LiveArtifact> {
        self.0.live_artifacts(extension, key)
    }

    fn live_artifact(
        &self,
        extension: &str,
        key: &str,
        path: &str,
    ) -> Result<Vec<u8>, HostedRefusal> {
        self.0.live_artifact(extension, key, path)
    }
}
