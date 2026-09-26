use std::collections::BTreeMap;

use serde::Serialize;

use crate::ResourceKind;

/// Entity attributes (CONTEXT.md § *Extensions*): key/value pairs an extension
/// stamps on an entity. The core prefixes each key with the extension's id,
/// stores it outside the event log and never interprets it.
pub type Attributes = BTreeMap<String, String>;

/// The entity a hook is about: its kind and its id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub struct EntityRef {
    pub kind: ResourceKind,
    pub id: String,
}

impl EntityRef {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}
