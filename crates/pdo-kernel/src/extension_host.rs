//! The extension host: what the core calls, and what fans each call out to the
//! registered extensions.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use pdo_extension_api::{Authorization, Extension, RequestParts};
use serde_json::Value;

use crate::actor::actor_from_headers;
use crate::{Actor, Operation};

/// The core's view of its extensions.
pub trait ExtensionHost: Send + Sync {
    /// The actor behind a request. Never fails: the core has its own default.
    fn identify(&self, request: &RequestParts) -> Actor;

    /// Whether `actor` may run `operation`.
    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization;

    /// Hand an event to every extension.
    fn publish(&self, event: &Value);
}

/// The host of a daemon with no extension: the core's identification, everything
/// allowed, events go nowhere.
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
}

/// The host of the extensions registered at startup, in registration order.
///
/// - `identify`: the first extension that names the actor wins; otherwise the
///   core's identification.
/// - `authorize`: every extension must allow; the first refusal wins. An
///   extension that panics refuses.
/// - `publish`: every extension sees the event; a panic in one is logged and
///   does not reach the others.
#[derive(Clone, Default)]
pub struct Extensions {
    extensions: Vec<Arc<dyn Extension>>,
}

impl Extensions {
    pub fn new(extensions: Vec<Box<dyn Extension>>) -> Self {
        Self {
            extensions: extensions.into_iter().map(Arc::from).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.extensions.len()
    }
}

impl ExtensionHost for Extensions {
    fn identify(&self, request: &RequestParts) -> Actor {
        for ext in &self.extensions {
            match catch_unwind(AssertUnwindSafe(|| ext.identify(request))) {
                Ok(Some(actor)) => return actor,
                Ok(None) => {}
                Err(_) => tracing::warn!(
                    "extension `{}` panicked in identify; ignored",
                    ext.manifest().id
                ),
            }
        }
        actor_from_headers(&request.headers)
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        for ext in &self.extensions {
            match catch_unwind(AssertUnwindSafe(|| ext.authorize(actor, operation))) {
                Ok(Authorization::Allow) => {}
                Ok(deny) => return deny,
                Err(_) => {
                    let id = ext.manifest().id;
                    tracing::warn!("extension `{id}` panicked in authorize; refused");
                    return Authorization::deny(format!("extension `{id}` failed"));
                }
            }
        }
        Authorization::Allow
    }

    fn publish(&self, event: &Value) {
        for ext in &self.extensions {
            if catch_unwind(AssertUnwindSafe(|| ext.on_event(event))).is_err() {
                tracing::warn!("extension `{}` panicked in on_event", ext.manifest().id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
    use crate::{ResourceKind, Verb};
    use pdo_extension_api::axum::http::Request;
    use pdo_extension_api::Manifest;

    fn parts(headers: &[(&str, &str)]) -> RequestParts {
        let mut builder = Request::builder().uri("/runs");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    struct Named(&'static str);
    impl Extension for Named {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.0, self.0, "0.0.0")
        }
        fn identify(&self, _request: &RequestParts) -> Option<Actor> {
            Some(Actor::Identified {
                namespace: self.0.into(),
                id: "alice".into(),
            })
        }
    }

    struct Refuses;
    impl Extension for Refuses {
        fn manifest(&self) -> Manifest {
            Manifest::new("refuses", "Refuses", "0.0.0")
        }
        fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
            Authorization::deny("admins only")
        }
    }

    struct Panics;
    impl Extension for Panics {
        fn manifest(&self) -> Manifest {
            Manifest::new("panics", "Panics", "0.0.0")
        }
        fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
            panic!("boom")
        }
    }

    fn op() -> Operation {
        Operation::new(ResourceKind::Project, Verb::Update).on("p1")
    }

    #[test]
    fn no_extensions_uses_the_core_identification() {
        assert_eq!(NoExtensions.identify(&parts(&[])), Actor::LocalOperator);
        assert_eq!(
            NoExtensions.identify(&parts(&[
                (SESSION_RUN_HEADER, "r"),
                (SESSION_NODE_HEADER, "n")
            ])),
            Actor::Agent {
                run_id: "r".into(),
                node_id: "n".into()
            }
        );
        assert!(NoExtensions
            .authorize(&Actor::LocalOperator, &op())
            .is_allowed());
    }

    #[test]
    fn an_empty_registry_behaves_like_no_extensions() {
        let host = Extensions::new(vec![]);
        assert!(host.is_empty());
        assert_eq!(host.identify(&parts(&[])), Actor::LocalOperator);
        assert!(host.authorize(&Actor::LocalOperator, &op()).is_allowed());
    }

    #[test]
    fn the_first_extension_that_identifies_wins() {
        let host = Extensions::new(vec![Box::new(Named("a")), Box::new(Named("b"))]);
        assert_eq!(
            host.identify(&parts(&[])),
            Actor::Identified {
                namespace: "a".into(),
                id: "alice".into()
            }
        );
    }

    #[test]
    fn one_refusal_refuses_and_a_panic_refuses_too() {
        let host = Extensions::new(vec![Box::new(Named("a")), Box::new(Refuses)]);
        assert_eq!(
            host.authorize(&Actor::LocalOperator, &op()),
            Authorization::deny("admins only")
        );
        let host = Extensions::new(vec![Box::new(Panics)]);
        assert!(!host.authorize(&Actor::LocalOperator, &op()).is_allowed());
    }
}
