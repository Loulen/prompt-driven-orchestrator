//! What the composition root plugs into a daemon at boot (see
//! [`DaemonConfig::router_layer`](super::config::DaemonConfig::router_layer)): a
//! transformation of the assembled router, and the wiring of the bounded
//! contexts over the resources the boot opened.

use std::sync::Arc;

use axum::Router;

use crate::contexts::{ContextResources, Contexts};

#[derive(Clone)]
pub struct RouterLayer {
    wrap: Arc<dyn Fn(Router) -> Router + Send + Sync>,
    contexts: Option<WireContexts>,
}

/// Wire the bounded contexts over the daemon's resources.
pub(crate) type WireContexts = Arc<dyn Fn(ContextResources) -> Contexts + Send + Sync>;

impl RouterLayer {
    pub fn new(layer: impl Fn(Router) -> Router + Send + Sync + 'static) -> Self {
        Self {
            wrap: Arc::new(layer),
            contexts: None,
        }
    }

    /// The same layer, wiring the bounded contexts with `wire`.
    pub(crate) fn with_contexts(
        mut self,
        wire: impl Fn(ContextResources) -> Contexts + Send + Sync + 'static,
    ) -> Self {
        self.contexts = Some(Arc::new(wire));
        self
    }

    pub fn apply(&self, router: Router) -> Router {
        (self.wrap)(router)
    }

    pub(crate) fn contexts(&self) -> Option<WireContexts> {
        self.contexts.clone()
    }
}

impl std::fmt::Debug for RouterLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouterLayer(..)")
    }
}
