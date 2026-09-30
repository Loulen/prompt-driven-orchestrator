//! The extension host: what the core calls, and what fans each call out to the
//! registered extensions. Also each extension's [`Host`], through which it
//! runs its own mutations like the core's (amendment #34 of ADR-0081).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};

use pdo_extension_api::{
    Attributes, Authorization, EntityRef, Extension, ExtensionContext, ExtensionEvent,
    ExtensionSkill, Host, Manifest, OperationApplied, RequestParts, EXTENSION_EVENT,
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

    /// Broadcast an event the core makes for the extensions (an
    /// `OperationApplied`): on the bus, hence on `/ws` and to every
    /// extension, once the daemon started with extensions. By default, only
    /// to the extensions.
    fn announce(&self, event: Value) {
        self.publish(&event);
    }

    /// The attributes the extensions stamp on `entity`, which `actor` just
    /// created, each key prefixed with its extension's id. None by default.
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }

    /// Where the entity attributes are stored.
    fn attributes(&self) -> Arc<dyn AttributeStore> {
        Arc::new(NoAttributes)
    }

    /// The skills each extension brings, in registration order, an empty list
    /// included (it has stopped providing its skills); an extension that
    /// panicked saying so is left out. None by default.
    fn skills(&self) -> Vec<ProvidedSkills> {
        Vec::new()
    }
}

/// The skills one extension brings into the Banque de skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSkills {
    pub manifest: Manifest,
    pub skills: Vec<ExtensionSkill>,
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
struct Registered {
    manifest: Manifest,
    extension: Arc<dyn Extension>,
    delivery: Delivery,
}

/// The registered extensions and the services the daemon plugs in.
#[derive(Default)]
struct Registry {
    registered: Vec<Registered>,
    attributes: OnceLock<Arc<dyn AttributeStore>>,
    bus: OnceLock<Arc<dyn EventBus>>,
}

/// The host of the extensions registered at startup, in registration order.
///
/// - `identify`: the first extension that names the actor wins; otherwise the
///   core's identification.
/// - `authorize`: every extension must allow; the first refusal wins. An
///   extension that panics refuses.
/// - `publish`: every extension sees the event, on its own thread
///   (`delivery`); a slow or panicking one never delays the others nor the
///   core. An extension never sees the events it published itself.
/// - `on_create`: every extension's attributes, prefixed with its id; a panic
///   stamps nothing.
///
/// Each extension gets its own [`Host`] at `start` ([`ExtensionHostOf`]).
#[derive(Clone, Default)]
pub struct Extensions {
    registry: Arc<Registry>,
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
            registry: Arc::new(Registry {
                registered,
                ..Registry::default()
            }),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.registry.registered.is_empty()
    }

    pub fn len(&self) -> usize {
        self.registry.registered.len()
    }

    /// Each extension with its manifest, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&Manifest, &Arc<dyn Extension>)> {
        self.registry
            .registered
            .iter()
            .map(|r| (&r.manifest, &r.extension))
    }

    /// The [`Host`] of the extension `id`: what it runs its own mutations
    /// through. It holds the extensions weakly: once the daemon is gone, it
    /// refuses everything and publishes nothing.
    pub fn host_of(&self, id: &str) -> Arc<dyn Host> {
        Arc::new(ExtensionHostOf {
            id: id.to_string(),
            registry: Arc::downgrade(&self.registry),
        })
    }

    /// Plug the daemon's services in, once, when the daemon starts: the
    /// attribute store, the bus, each extension's data folder (created, then
    /// handed to its `start` with its [`Host`]), and the relay of the bus to
    /// the extensions. Must be called from within the daemon's tokio runtime.
    pub fn start(&self, services: HostServices) {
        let HostServices {
            attributes,
            bus,
            data_root,
        } = services;
        if self.registry.attributes.set(attributes).is_err() {
            tracing::warn!("the extension host was started twice; the second start is ignored");
            return;
        }
        let _ = self.registry.bus.set(bus.clone());
        // Subscribed before the `start`s: what an extension publishes there
        // reaches the others too.
        let events = (!self.is_empty()).then(|| bus.subscribe());
        for registered in &self.registry.registered {
            let id = &registered.manifest.id;
            let data_dir = data_root.join(id);
            if let Err(e) = std::fs::create_dir_all(&data_dir) {
                tracing::error!(
                    "failed to create the data folder {} of extension `{id}`: {e}",
                    data_dir.display()
                );
            }
            let context = ExtensionContext::new(data_dir).with_host(self.host_of(id));
            if catch_unwind(AssertUnwindSafe(|| registered.extension.start(&context))).is_err() {
                tracing::error!("extension `{id}` panicked in start");
            }
        }
        if let Some(events) = events {
            self.relay(events);
        }
    }

    /// Hand every event of the bus subscription `events` to the extensions.
    fn relay(&self, mut events: tokio::sync::broadcast::Receiver<Value>) {
        // Weak: the relay must not keep the extensions (and their delivery
        // threads) alive once the daemon dropped them.
        let registry = Arc::downgrade(&self.registry);
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => match registry.upgrade() {
                        Some(registry) => Extensions { registry }.publish(&event),
                        None => break,
                    },
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!("the extensions missed {n} events of the bus")
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    /// Broadcast `event` on the bus once started (hence `/ws` and the relay to
    /// the extensions), straight to the extensions before.
    fn broadcast(&self, event: Value) {
        match self.registry.bus.get() {
            Some(bus) => bus.publish(event),
            None => self.publish(&event),
        }
    }

    /// Every extension's attributes for `entity`, but those of `except`.
    fn stamps(&self, actor: &Actor, entity: &EntityRef, except: Option<&str>) -> Attributes {
        let mut stamped = Attributes::new();
        for r in &self.registry.registered {
            if Some(r.manifest.id.as_str()) == except {
                continue;
            }
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
}

impl ExtensionHost for Extensions {
    fn identify(&self, request: &RequestParts) -> Actor {
        for r in &self.registry.registered {
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
        for r in &self.registry.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.authorize(actor, operation))) {
                Ok(Authorization::Allow) => {}
                Ok(deny) => return deny,
                Err(panic) => {
                    let id = &r.manifest.id;
                    tracing::error!(
                        "extension `{id}` panicked in authorize ({}); {} {:?} refused",
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
        let publisher = (event["type"] == EXTENSION_EVENT)
            .then(|| event["extension"].as_str())
            .flatten();
        for r in &self.registry.registered {
            if publisher != Some(r.manifest.id.as_str()) {
                r.delivery.send(event);
            }
        }
    }

    /// Without extension, nothing: `/ws` carries exactly what it did before.
    fn announce(&self, event: Value) {
        if !self.is_empty() {
            self.broadcast(event);
        }
    }

    fn on_create(&self, actor: &Actor, entity: &EntityRef) -> Attributes {
        self.stamps(actor, entity, None)
    }

    fn attributes(&self) -> Arc<dyn AttributeStore> {
        match self.registry.attributes.get() {
            Some(store) => store.clone(),
            None => Arc::new(NoAttributes),
        }
    }

    fn skills(&self) -> Vec<ProvidedSkills> {
        self.registry.registered
            .iter()
            .filter_map(
                // An empty list is kept: the seed removes what the extension
                // provided before. A panic is no list at all: it removes nothing.
                |r| match catch_unwind(AssertUnwindSafe(|| r.extension.skills())) {
                    Ok(skills) => Some(ProvidedSkills {
                        manifest: r.manifest.clone(),
                        skills,
                    }),
                    Err(_) => {
                        tracing::error!(
                            "extension `{}` panicked in skills; it brings none",
                            r.manifest.id
                        );
                        None
                    }
                },
            )
            .collect()
    }
}

/// The [`Host`] of one extension, `id`.
struct ExtensionHostOf {
    id: String,
    registry: Weak<Registry>,
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
    fn skills_come_with_the_manifest_of_their_extension() {
        struct Brings;
        impl Extension for Brings {
            fn manifest(&self) -> Manifest {
                Manifest::new("brings", "Brings", "1.2.0")
            }
            fn skills(&self) -> Vec<ExtensionSkill> {
                vec![ExtensionSkill::new("demo", "---\nname: demo\n---\n")]
            }
        }
        struct PanicsInSkills;
        impl Extension for PanicsInSkills {
            fn manifest(&self) -> Manifest {
                Manifest::new("broken", "Broken", "0.0.0")
            }
            fn skills(&self) -> Vec<ExtensionSkill> {
                panic!("no skills today")
            }
        }
        let host = Extensions::register(vec![
            Box::new(Named("a")),
            Box::new(Brings),
            Box::new(PanicsInSkills),
        ])
        .unwrap();
        let provided = host.skills();
        assert_eq!(provided.len(), 2, "the panicking extension is left out");
        assert_eq!(provided[0].manifest.id, "a");
        assert!(provided[0].skills.is_empty(), "an empty list is kept");
        assert_eq!(provided[1].manifest.id, "brings");
        assert_eq!(provided[1].manifest.version, "1.2.0");
        assert_eq!(provided[1].skills[0].name, "demo");
        assert!(NoExtensions.skills().is_empty());
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

    /// Records what `on_event` receives, refuses the extension resources when
    /// told to, and stamps `witness` on what it sees created.
    struct Peer {
        id: &'static str,
        refuses: bool,
        seen: Arc<std::sync::Mutex<Vec<Value>>>,
    }

    impl Peer {
        fn new(id: &'static str, refuses: bool) -> (Self, Arc<std::sync::Mutex<Vec<Value>>>) {
            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let peer = Peer {
                id,
                refuses,
                seen: seen.clone(),
            };
            (peer, seen)
        }
    }

    impl Extension for Peer {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.id, self.id, "0.0.0")
        }
        fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
            match &operation.resource {
                ResourceKind::Extension { .. } if self.refuses => {
                    Authorization::deny(format!("{} says no", self.id))
                }
                _ => Authorization::Allow,
            }
        }
        fn on_create(&self, _actor: &Actor, entity: &EntityRef) -> Attributes {
            [("witness".to_string(), entity.kind.wire_name())].into()
        }
        fn on_event(&self, event: &Value) {
            self.seen.lock().unwrap().push(event.clone());
        }
    }

    fn item() -> ResourceKind {
        ResourceKind::extension("a", "item")
    }

    async fn wait_for(seen: &Arc<std::sync::Mutex<Vec<Value>>>, count: usize) -> Vec<Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let events = seen.lock().unwrap().clone();
            if events.len() >= count {
                return events;
            }
            assert!(std::time::Instant::now() < deadline, "only {events:?}");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[test]
    fn an_extension_operation_is_refusable_by_any_extension_even_its_own() {
        let (a, _) = Peer::new("a", false);
        let (b, _) = Peer::new("b", true);
        let host = Extensions::register(vec![Box::new(a), Box::new(b)]).unwrap();
        let create = Operation::new(item(), Verb::Create);
        assert_eq!(
            host.host_of("a").authorize(&Actor::LocalOperator, &create),
            Authorization::deny("b says no")
        );
        // A core Operation is untouched by B's rule.
        assert!(host
            .host_of("a")
            .authorize(&Actor::LocalOperator, &op())
            .is_allowed());

        let (a, _) = Peer::new("a", true);
        let host = Extensions::register(vec![Box::new(a), Box::new(Panics)]).unwrap();
        assert_eq!(
            host.host_of("a").authorize(&Actor::LocalOperator, &create),
            Authorization::deny("a says no"),
            "the emitter itself refuses"
        );
        let (a, _) = Peer::new("a", false);
        let host = Extensions::register(vec![Box::new(a), Box::new(Panics)]).unwrap();
        assert_eq!(
            host.host_of("a").authorize(&Actor::LocalOperator, &create),
            Authorization::deny("extension `panics` failed"),
            "a panic refuses"
        );
    }

    #[test]
    fn the_other_extensions_stamp_an_extension_entity_under_their_prefix() {
        let (a, _) = Peer::new("a", false);
        let (c, _) = Peer::new("c", false);
        let host = Extensions::register(vec![
            Box::new(a),
            Box::new(c),
            Box::new(Stamps("p", "panic")),
        ])
        .unwrap();
        let stamped = host
            .host_of("a")
            .on_create(&Actor::LocalOperator, &EntityRef::new(item(), "i1"));
        assert_eq!(
            stamped,
            [("c/witness".to_string(), "a/item".to_string())].into(),
            "never the emitter's own, never the panicking one's"
        );
    }

    #[tokio::test]
    async fn published_events_and_applied_operations_reach_the_bus_and_the_others() {
        let (a, seen_by_a) = Peer::new("a", false);
        let (b, seen_by_b) = Peer::new("b", false);
        let extensions = Extensions::register(vec![Box::new(a), Box::new(b)]).unwrap();
        let bus = Arc::new(crate::event_bus::BroadcastEventBus::new(16));
        let mut ws = bus.subscribe();
        let root = std::env::temp_dir().join(format!("pdo-kernel-host-{}", std::process::id()));
        extensions.start(HostServices {
            attributes: Arc::new(crate::attributes::InMemoryAttributes::default()),
            bus,
            data_root: root.clone(),
        });
        let host = extensions.host_of("a");
        host.publish(ExtensionEvent::new(
            "item_created",
            serde_json::json!({ "id": "i1" }),
        ));
        let entity = EntityRef::new(item(), "i1");
        host.applied(OperationApplied::new(
            Actor::LocalOperator,
            Operation::new(item(), Verb::Create),
            Some(entity),
        ));

        let event = ws.recv().await.unwrap();
        assert_eq!(
            event,
            serde_json::json!({
                "type": "extension_event", "extension": "a", "name": "item_created",
                "payload": { "id": "i1" },
            })
        );
        let applied = ws.recv().await.unwrap();
        assert_eq!(
            applied,
            serde_json::json!({
                "type": "operation_applied",
                "actor": { "kind": "local_operator" },
                "operation": { "resource": "a/item", "verb": "create", "target": null },
                "entity": { "kind": "a/item", "id": "i1" },
            })
        );
        assert_eq!(wait_for(&seen_by_b, 2).await, vec![event, applied.clone()]);
        assert_eq!(
            wait_for(&seen_by_a, 1).await,
            vec![applied],
            "the emitter never receives its own event"
        );

        // The core's own OperationApplied rides the same bus.
        extensions.announce(serde_json::json!({ "type": "operation_applied" }));
        assert_eq!(ws.recv().await.unwrap()["type"], "operation_applied");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn what_an_extension_publishes_in_start_reaches_the_others() {
        struct Greets;
        impl Extension for Greets {
            fn manifest(&self) -> Manifest {
                Manifest::new("greets", "Greets", "0.0.0")
            }
            fn start(&self, context: &ExtensionContext) {
                context
                    .host
                    .publish(ExtensionEvent::new("hello", Value::Null));
            }
        }
        let (b, seen_by_b) = Peer::new("b", false);
        let extensions = Extensions::register(vec![Box::new(Greets), Box::new(b)]).unwrap();
        extensions.start(HostServices {
            attributes: Arc::new(NoAttributes),
            bus: Arc::new(crate::event_bus::BroadcastEventBus::new(4)),
            data_root: std::env::temp_dir()
                .join(format!("pdo-kernel-greets-{}", std::process::id())),
        });
        assert_eq!(wait_for(&seen_by_b, 1).await[0]["name"], "hello");
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("pdo-kernel-greets-{}", std::process::id())),
        );
    }

    #[test]
    fn a_host_outliving_its_daemon_refuses_everything() {
        let (a, _) = Peer::new("a", false);
        let host = Extensions::register(vec![Box::new(a)])
            .unwrap()
            .host_of("a");
        assert_eq!(
            host.authorize(&Actor::LocalOperator, &op()),
            Authorization::deny("the daemon is stopping")
        );
        assert!(host
            .on_create(&Actor::LocalOperator, &EntityRef::new(item(), "i1"))
            .is_empty());
    }

    #[tokio::test]
    async fn without_extension_nothing_is_announced() {
        let extensions = Extensions::register(vec![]).unwrap();
        let bus = Arc::new(crate::event_bus::BroadcastEventBus::new(4));
        let mut ws = bus.subscribe();
        extensions.start(HostServices {
            attributes: Arc::new(NoAttributes),
            bus,
            data_root: std::env::temp_dir(),
        });
        extensions.announce(serde_json::json!({ "type": "operation_applied" }));
        assert!(ws.try_recv().is_err());
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
