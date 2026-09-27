//! The extension host: what the core calls, and what fans each call out to the
//! registered extensions.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use pdo_extension_api::{Authorization, Extension, RequestParts};
use serde_json::Value;

use crate::actor::actor_from_headers;
use crate::registration::{check_manifests, RegistrationError};
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
    /// Register `extensions`, in this order, once their manifests pass the
    /// checks (a valid and unique id, a compatible API version). The daemon
    /// refuses to start otherwise.
    pub fn register(extensions: Vec<Box<dyn Extension>>) -> Result<Self, RegistrationError> {
        let extensions: Vec<Arc<dyn Extension>> = extensions.into_iter().map(Arc::from).collect();
        let manifests: Vec<_> = extensions.iter().map(|ext| ext.manifest()).collect();
        check_manifests(&manifests)?;
        for manifest in &manifests {
            tracing::info!(
                "extension `{}` {} registered (extension API {})",
                manifest.id,
                manifest.version,
                manifest.api_version
            );
        }
        Ok(Self { extensions })
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
                Err(panic) => {
                    let id = ext.manifest().id;
                    tracing::error!(
                        "extension `{id}` panicked in authorize ({}); {:?} {:?} refused",
                        panic_message(panic.as_ref()),
                        operation.resource,
                        operation.verb,
                    );
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

/// What a panic said, when it said it with a string.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
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
        let host = Extensions::register(vec![]).unwrap();
        assert!(host.is_empty());
        assert_eq!(host.identify(&parts(&[])), Actor::LocalOperator);
        assert!(host.authorize(&Actor::LocalOperator, &op()).is_allowed());
    }

    #[test]
    fn the_first_extension_that_identifies_wins() {
        let host = Extensions::register(vec![Box::new(Named("a")), Box::new(Named("b"))]).unwrap();
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
        let host = Extensions::register(vec![Box::new(Named("a")), Box::new(Refuses)]).unwrap();
        assert_eq!(
            host.authorize(&Actor::LocalOperator, &op()),
            Authorization::deny("admins only")
        );
        let host = Extensions::register(vec![Box::new(Panics)]).unwrap();
        assert!(!host.authorize(&Actor::LocalOperator, &op()).is_allowed());
    }

    #[test]
    fn registration_refuses_an_extension_this_build_cannot_honour() {
        struct Future;
        impl Extension for Future {
            fn manifest(&self) -> Manifest {
                Manifest::new("future", "Future", "0.0.0").requiring_api("99.0.0")
            }
        }
        let err = Extensions::register(vec![Box::new(Future)]).err().unwrap();
        assert!(
            matches!(err, RegistrationError::IncompatibleApi { .. }),
            "{err}"
        );
    }

    #[test]
    fn a_panic_in_authorize_is_a_logged_refusal() {
        let log = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish();
        let host = Extensions::register(vec![Box::new(Panics)]).unwrap();
        let answer = tracing::subscriber::with_default(subscriber, || {
            host.authorize(&Actor::LocalOperator, &op())
        });
        assert_eq!(answer, Authorization::deny("extension `panics` failed"));
        let log = log.text();
        assert_eq!(
            log.lines()
                .filter(|l| l.contains("extension `panics` panicked in authorize (boom)"))
                .count(),
            1,
            "{log}"
        );
    }

    /// A `tracing` writer into memory.
    #[derive(Clone, Default)]
    struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

    impl LogBuffer {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    impl std::io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = LogBuffer;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }
}
