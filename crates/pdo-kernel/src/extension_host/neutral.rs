//! The host of a daemon with no extension.

use pdo_extension_api::{Authorization, RequestParts};
use serde_json::Value;

use super::ExtensionHost;
use crate::actor::actor_from_headers;
use crate::{Actor, Operation};

/// The host of a daemon with no extension: the core's identification, everything
/// allowed, events go nowhere, no attribute.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExtensions;

impl ExtensionHost for NoExtensions {
    fn identify(&self, request: &RequestParts) -> Actor {
        actor_from_headers(&request.headers)
    }

    fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
        Authorization::Allow
    }

    fn publish(&self, _event: &Value) {}

    fn allows_everything(&self) -> bool {
        true
    }
}
