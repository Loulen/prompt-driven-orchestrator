//! The extension host: what the core calls, and what fans each call out to the
//! registered extensions.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pdo_extension_api::{
    Attributes, Authorization, EntityRef, Extension, ExtensionContext, Manifest, RequestParts,
};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::actor::actor_from_headers;
use crate::attributes::{prefixed, AttributeStore, NoAttributes};
use crate::delivery::Delivery;
use crate::event_bus::EventBus;
use crate::registration::{check_manifests, RegistrationError};
use crate::{Actor, Operation};

/// The core's view of its extensions.
pub trait ExtensionHost: Send + Sync {
    /// The actor behind a request. Never fails: the core has its own default.
    fn identify(&self, request: &RequestParts) -> Actor;

    /// Whether `actor` may run `operation`.
    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization;

    /// Hand an event to every extension, without waiting for any.
    fn publish(&self, event: &Value);

    /// The attributes the extensions stamp on `entity`, which `actor` just
    /// created, each key prefixed with its extension's id. None by default.
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }

    /// Where the entity attributes are stored.
    fn attributes(&self) -> Arc<dyn AttributeStore> {
        Arc::new(NoAttributes)
    }
}

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
}

/// What the daemon gives its extension host once its resources are open.
pub struct HostServices {
    /// Where the entity attributes are stored.
    pub attributes: Arc<dyn AttributeStore>,
    /// The bus whose events the extensions observe (the JSON of `/ws`).
    pub bus: Arc<dyn EventBus>,
    /// The folder holding one data folder per extension
    /// (`~/.pdo/extensions`).
    pub data_root: PathBuf,
}

/// One registered extension.
#[derive(Clone)]
struct Registered {
    manifest: Manifest,
    extension: Arc<dyn Extension>,
    delivery: Delivery,
}

/// The host of the extensions registered at startup, in registration order.
///
/// - `identify`: the first extension that names the actor wins; otherwise the
///   core's identification.
/// - `authorize`: every extension must allow; the first refusal wins. An
///   extension that panics refuses.
/// - `publish`: every extension sees the event, on its own thread
///   (`delivery`); a slow or panicking one never delays the others nor the
///   core.
/// - `on_create`: every extension's attributes, prefixed with its id; a panic
///   stamps nothing.
#[derive(Clone, Default)]
pub struct Extensions {
    registered: Vec<Registered>,
    attributes: Arc<OnceLock<Arc<dyn AttributeStore>>>,
}

impl Extensions {
    /// Register `extensions`, in this order, once their manifests pass the
    /// checks (a valid and unique id, a compatible API version). The daemon
    /// refuses to start otherwise.
    pub fn register(extensions: Vec<Box<dyn Extension>>) -> Result<Self, RegistrationError> {
        let extensions: Vec<Arc<dyn Extension>> = extensions.into_iter().map(Arc::from).collect();
        let manifests: Vec<_> = extensions.iter().map(|ext| ext.manifest()).collect();
        check_manifests(&manifests)?;
        let registered = manifests
            .into_iter()
            .zip(extensions)
            .map(|(manifest, extension)| {
                tracing::info!(
                    "extension `{}` {} registered (extension API {})",
                    manifest.id,
                    manifest.version,
                    manifest.api_version
                );
                Registered {
                    delivery: Delivery::start(manifest.id.clone(), extension.clone()),
                    manifest,
                    extension,
                }
            })
            .collect();
        Ok(Self {
            registered,
            attributes: Arc::default(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }

    pub fn len(&self) -> usize {
        self.registered.len()
    }

    /// Each extension with its manifest, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&Manifest, &Arc<dyn Extension>)> {
        self.registered.iter().map(|r| (&r.manifest, &r.extension))
    }

    /// Plug the daemon's services in, once, when the daemon starts: the
    /// attribute store, each extension's data folder (created, then handed to
    /// its `start`), and the relay of the bus to the extensions. Must be called
    /// from within the daemon's tokio runtime.
    pub fn start(&self, services: HostServices) {
        let HostServices {
            attributes,
            bus,
            data_root,
        } = services;
        if self.attributes.set(attributes).is_err() {
            tracing::warn!("the extension host was started twice; the second start is ignored");
            return;
        }
        for registered in &self.registered {
            let id = &registered.manifest.id;
            let data_dir = data_root.join(id);
            if let Err(e) = std::fs::create_dir_all(&data_dir) {
                tracing::error!(
                    "failed to create the data folder {} of extension `{id}`: {e}",
                    data_dir.display()
                );
            }
            let context = ExtensionContext::new(data_dir);
            if catch_unwind(AssertUnwindSafe(|| registered.extension.start(&context))).is_err() {
                tracing::error!("extension `{id}` panicked in start");
            }
        }
        if !self.is_empty() {
            self.relay(bus);
        }
    }

    /// Hand every event of `bus` to the extensions.
    fn relay(&self, bus: Arc<dyn EventBus>) {
        let host = self.clone();
        let mut events = bus.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => host.publish(&event),
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!("the extensions missed {n} events of the bus")
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}

impl ExtensionHost for Extensions {
    fn identify(&self, request: &RequestParts) -> Actor {
        for r in &self.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.identify(request))) {
                Ok(Some(actor)) => return actor,
                Ok(None) => {}
                Err(_) => tracing::warn!(
                    "extension `{}` panicked in identify; ignored",
                    r.manifest.id
                ),
            }
        }
        actor_from_headers(&request.headers)
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        for r in &self.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.authorize(actor, operation))) {
                Ok(Authorization::Allow) => {}
                Ok(deny) => return deny,
                Err(panic) => {
                    let id = &r.manifest.id;
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
        for r in &self.registered {
            r.delivery.send(event);
        }
    }

    fn on_create(&self, actor: &Actor, entity: &EntityRef) -> Attributes {
        let mut stamped = Attributes::new();
        for r in &self.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.on_create(actor, entity))) {
                Ok(attributes) => stamped.extend(prefixed(&r.manifest.id, attributes)),
                Err(_) => tracing::warn!(
                    "extension `{}` panicked in on_create; it stamps nothing",
                    r.manifest.id
                ),
            }
        }
        stamped
    }

    fn attributes(&self) -> Arc<dyn AttributeStore> {
        match self.attributes.get() {
            Some(store) => store.clone(),
            None => Arc::new(NoAttributes),
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

    struct Stamps(&'static str, &'static str);
    impl Extension for Stamps {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.0, self.0, "0.0.0")
        }
        fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
            if self.1 == "panic" {
                panic!("boom");
            }
            [("started_by".to_string(), self.1.to_string())].into()
        }
    }

    #[test]
    fn two_extensions_stamping_the_same_key_never_overwrite_each_other() {
        let host = Extensions::register(vec![
            Box::new(Stamps("a", "alice")),
            Box::new(Stamps("b", "bob")),
            Box::new(Stamps("c", "panic")),
        ])
        .unwrap();
        let stamped = host.on_create(
            &Actor::LocalOperator,
            &EntityRef::new(ResourceKind::Run, "r1"),
        );
        assert_eq!(
            stamped,
            [
                ("a/started_by".to_string(), "alice".to_string()),
                ("b/started_by".to_string(), "bob".to_string()),
            ]
            .into()
        );
        assert!(NoExtensions
            .on_create(
                &Actor::LocalOperator,
                &EntityRef::new(ResourceKind::Run, "r1")
            )
            .is_empty());
    }

    #[tokio::test]
    async fn start_creates_each_data_folder_and_plugs_the_attribute_store() {
        struct Remembers(Arc<std::sync::Mutex<Option<PathBuf>>>);
        impl Extension for Remembers {
            fn manifest(&self) -> Manifest {
                Manifest::new("remembers", "Remembers", "0.0.0")
            }
            fn start(&self, context: &ExtensionContext) {
                *self.0.lock().unwrap() = Some(context.data_dir.clone());
            }
        }
        let seen = Arc::new(std::sync::Mutex::new(None));
        let host = Extensions::register(vec![Box::new(Remembers(seen.clone()))]).unwrap();
        let root = std::env::temp_dir().join(format!("pdo-kernel-start-{}", std::process::id()));
        let store = Arc::new(crate::attributes::InMemoryAttributes::default());
        host.start(HostServices {
            attributes: store,
            bus: Arc::new(crate::event_bus::BroadcastEventBus::new(4)),
            data_root: root.clone(),
        });
        let dir = seen.lock().unwrap().clone().unwrap();
        assert_eq!(dir, root.join("remembers"));
        assert!(dir.is_dir());
        let run = EntityRef::new(ResourceKind::Run, "r1");
        host.attributes()
            .put(&run, &[("x/k".to_string(), "v".to_string())].into())
            .await
            .unwrap();
        assert_eq!(
            host.attributes()
                .of_kind(ResourceKind::Run)
                .await
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_dir_all(root).unwrap();
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
