//! Each extension's [`Host`]: through it, an extension runs its own mutations
//! like the core's (amendment #34 of ADR-0081), and publishes to the others.

use std::sync::Weak;

use pdo_extension_api::{
    Attributes, Authorization, EntityRef, ExtensionEvent, Host, OperationApplied,
};

use super::{ExtensionHost, Extensions, Registry};
use crate::{Actor, Operation};

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
}

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

    fn publish(&self, event: ExtensionEvent) {
        if let Some(extensions) = self.extensions() {
            extensions.broadcast(event.to_event(&self.id));
        }
    }

    fn applied(&self, applied: OperationApplied) {
        if let Some(extensions) = self.extensions() {
            extensions.broadcast(applied.to_event());
        }
    }

    fn on_create(&self, actor: &Actor, entity: &EntityRef) -> Attributes {
        self.extensions()
            .map(|extensions| extensions.stamps(actor, entity, Some(&self.id)))
            .unwrap_or_default()
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
