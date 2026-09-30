//! The Attributs d'entité (CONTEXT.md § *Extensions*, D16): key/value pairs an
//! extension stamps on an entity it sees created, prefixed with the
//! extension's id. The core stores them outside the event log, returns them
//! with the entity and filters the lists on them, without ever interpreting
//! them.
//!
//! The entities that carry attributes are the Runs, the Projets, the agentic
//! profiles, the Triggers and the pipelines of the library. A list endpoint of
//! one of them reads `?attr.<id>/<key>=<value>` ([`AttributeFilter`]).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use crate::applied;
use crate::extension_host::ExtensionHost;
use crate::Actor;
pub use pdo_extension_api::{Attributes, EntityRef, ResourceKind};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The prefix of an attribute filter in a list's query string.
pub const FILTER_PREFIX: &str = "attr.";

/// Where the attributes live (a port: SQLite in the daemon).
pub trait AttributeStore: Send + Sync {
    /// Add `attributes` to `entity`, replacing the value of a key it already
    /// carries.
    fn put<'a>(
        &'a self,
        entity: &'a EntityRef,
        attributes: &'a Attributes,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// The attributes of every entity of `kind` that carries some, by id.
    fn of_kind(
        &self,
        kind: ResourceKind,
    ) -> BoxFuture<'_, Result<BTreeMap<String, Attributes>, String>>;

    /// Drop every attribute of `entity` (it was deleted).
    fn forget<'a>(&'a self, entity: &'a EntityRef) -> BoxFuture<'a, Result<(), String>>;
}

/// The store of a daemon with no extension: nothing is ever stamped, so
/// nothing is stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoAttributes;

impl AttributeStore for NoAttributes {
    fn put<'a>(&'a self, _: &'a EntityRef, _: &'a Attributes) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn of_kind(
        &self,
        _: ResourceKind,
    ) -> BoxFuture<'_, Result<BTreeMap<String, Attributes>, String>> {
        Box::pin(async { Ok(BTreeMap::new()) })
    }

    fn forget<'a>(&'a self, _: &'a EntityRef) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

/// The attributes an extension answered, each key prefixed with its id:
/// `started_by` from the extension `acme` is stored as `acme/started_by`. Two
/// extensions setting the same key never overwrite each other.
pub fn prefixed(extension_id: &str, attributes: Attributes) -> Attributes {
    attributes
        .into_iter()
        .map(|(key, value)| (format!("{extension_id}/{key}"), value))
        .collect()
}

/// Ask the extensions which attributes to stamp on `entity`, which `actor`
/// just created, and store them. Returns what was stamped. A storage failure
/// is logged, never fatal: the creation already happened.
pub async fn stamp(host: &dyn ExtensionHost, actor: &Actor, entity: EntityRef) -> Attributes {
    applied::created(&entity);
    let attributes = host.on_create(actor, &entity);
    if !attributes.is_empty() {
        if let Err(e) = host.attributes().put(&entity, &attributes).await {
            tracing::error!(
                "failed to store the attributes of {:?} {}: {e}",
                entity.kind,
                entity.id
            );
        }
    }
    attributes
}

/// The attributes of every entity of `kind`, by id; none when the store
/// cannot be read (logged): a list still answers.
pub async fn of_kind(host: &dyn ExtensionHost, kind: ResourceKind) -> BTreeMap<String, Attributes> {
    host.attributes()
        .of_kind(kind.clone())
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to read the attributes of the {kind:?} entities: {e}");
            BTreeMap::new()
        })
}

/// The attributes of one entity (none when it carries none).
pub async fn of_entity(host: &dyn ExtensionHost, kind: ResourceKind, id: &str) -> Attributes {
    of_kind(host, kind).await.remove(id).unwrap_or_default()
}

/// The attribute filter of a list: every `attr.<key>=<value>` of its query
/// string, `<key>` being the prefixed key (`acme/started_by`). An entity passes
/// when it carries every one of them. No filter lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeFilter {
    required: Attributes,
}

impl AttributeFilter {
    /// The filter of a query string's decoded pairs; the pairs that are not
    /// attribute filters are ignored.
    pub fn from_pairs<K: AsRef<str>, V: AsRef<str>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let required = pairs
            .into_iter()
            .filter_map(|(key, value)| {
                key.as_ref()
                    .strip_prefix(FILTER_PREFIX)
                    .filter(|key| !key.is_empty())
                    .map(|key| (key.to_string(), value.as_ref().to_string()))
            })
            .collect();
        Self { required }
    }

    /// The filter of a raw query string (`None` without one). What does not
    /// decode is ignored, as a list always ignored its unknown parameters.
    pub fn from_query(query: Option<&str>) -> Self {
        Self::from_pairs(form_urlencoded::parse(query.unwrap_or_default().as_bytes()))
    }

    pub fn is_empty(&self) -> bool {
        self.required.is_empty()
    }

    /// Whether an entity carrying `attributes` passes.
    pub fn matches(&self, attributes: Option<&Attributes>) -> bool {
        self.required
            .iter()
            .all(|(key, value)| attributes.and_then(|carried| carried.get(key)) == Some(value))
    }
}

/// The attributes of a list's entities, with the filter the list was asked
/// for: what the list endpoints of the entities that carry attributes share.
#[derive(Debug, Clone, Default)]
pub struct AttributedList {
    filter: AttributeFilter,
    attributes: BTreeMap<String, Attributes>,
}

impl AttributedList {
    /// Read the attributes of the entities of `kind`.
    pub async fn read(
        host: &dyn ExtensionHost,
        kind: ResourceKind,
        filter: AttributeFilter,
    ) -> Self {
        Self {
            attributes: of_kind(host, kind).await,
            filter,
        }
    }

    /// Whether the entity `id` passes the filter.
    pub fn keeps(&self, id: &str) -> bool {
        self.filter.matches(self.attributes.get(id))
    }

    /// The attributes of the entity `id`, to return with it.
    pub fn of(&self, id: &str) -> Attributes {
        self.attributes.get(id).cloned().unwrap_or_default()
    }
}

/// An in-memory store, for the use-case tests of the contexts.
#[derive(Debug, Default)]
pub struct InMemoryAttributes {
    stored: std::sync::Mutex<BTreeMap<(String, String), Attributes>>,
}

impl AttributeStore for InMemoryAttributes {
    fn put<'a>(
        &'a self,
        entity: &'a EntityRef,
        attributes: &'a Attributes,
    ) -> BoxFuture<'a, Result<(), String>> {
        let key = (wire_kind(&entity.kind), entity.id.clone());
        let mut stored = self.stored.lock().unwrap();
        stored.entry(key).or_default().extend(attributes.clone());
        Box::pin(async { Ok(()) })
    }

    fn of_kind(
        &self,
        kind: ResourceKind,
    ) -> BoxFuture<'_, Result<BTreeMap<String, Attributes>, String>> {
        let kind = wire_kind(&kind);
        let found = self
            .stored
            .lock()
            .unwrap()
            .iter()
            .filter(|((k, _), _)| *k == kind)
            .map(|((_, id), attributes)| (id.clone(), attributes.clone()))
            .collect();
        Box::pin(async move { Ok(found) })
    }

    fn forget<'a>(&'a self, entity: &'a EntityRef) -> BoxFuture<'a, Result<(), String>> {
        let key = (wire_kind(&entity.kind), entity.id.clone());
        self.stored.lock().unwrap().remove(&key);
        Box::pin(async { Ok(()) })
    }
}

/// The wire name of a kind of entity, as the stores key it (`agent_profile`).
pub fn wire_kind(kind: &ResourceKind) -> String {
    kind.wire_name()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> Attributes {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn keys_are_prefixed_with_the_extension_id() {
        assert_eq!(
            prefixed("acme", attrs(&[("started_by", "alice")])),
            attrs(&[("acme/started_by", "alice")])
        );
    }

    #[test]
    fn a_filter_reads_the_attr_pairs_and_requires_them_all() {
        let filter = AttributeFilter::from_pairs([
            ("attr.acme/started_by", "alice"),
            ("attr.acme/team", "red"),
            ("root_only", "true"),
            ("attr.", "ignored"),
        ]);
        assert!(!filter.is_empty());
        assert!(filter.matches(Some(&attrs(&[
            ("acme/started_by", "alice"),
            ("acme/team", "red"),
            ("other/x", "y"),
        ]))));
        assert!(!filter.matches(Some(&attrs(&[("acme/started_by", "alice")]))));
        assert!(!filter.matches(None));
        assert!(AttributeFilter::default().matches(None));
        assert!(AttributeFilter::from_pairs([("root_only", "true")]).is_empty());
        assert_eq!(
            AttributeFilter::from_query(Some("attr.acme%2Fstarted_by=alice&x=1")),
            AttributeFilter::from_pairs([("attr.acme/started_by", "alice")])
        );
        assert!(AttributeFilter::from_query(None).is_empty());
    }

    #[tokio::test]
    async fn the_in_memory_store_merges_lists_and_forgets() {
        let store = InMemoryAttributes::default();
        let run = EntityRef::new(ResourceKind::Run, "r1");
        store.put(&run, &attrs(&[("a/k", "1")])).await.unwrap();
        store.put(&run, &attrs(&[("b/k", "2")])).await.unwrap();
        store
            .put(
                &EntityRef::new(ResourceKind::Project, "r1"),
                &attrs(&[("a/k", "3")]),
            )
            .await
            .unwrap();
        let runs = store.of_kind(ResourceKind::Run).await.unwrap();
        assert_eq!(runs["r1"], attrs(&[("a/k", "1"), ("b/k", "2")]));
        store.forget(&run).await.unwrap();
        assert!(store.of_kind(ResourceKind::Run).await.unwrap().is_empty());
        assert_eq!(store.of_kind(ResourceKind::Project).await.unwrap().len(), 1);
    }
}
