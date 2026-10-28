//! The Sessions hébergées as the extensions' [`Host`](pdo_extension_api::Host)
//! reaches them (API 0.17): the kernel's port, over the session context.

use std::sync::Arc;

use pdo_extension_api::{HostedGesture, HostedSession, OpenSession};
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
}
