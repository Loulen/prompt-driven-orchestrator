//! The core's view of its extensions: the port every context asks.

use std::sync::Arc;

use pdo_extension_api::{
    ActorDisplay, Admission, Attributes, Authorization, EntityRef, ExtensionData, RequestParts,
    Resource, ResourceKind, SessionLaunch, TriggerRef,
};
use serde_json::Value;

use super::creation::{self, CreationRefusal};
use super::{Identification, NodeContribution, ProvidedSkills, TriggerSuspension};
use crate::attributes::{AttributeStore, NoAttributes};
use crate::{Actor, Operation};

/// The core's view of its extensions.
pub trait ExtensionHost: Send + Sync {
    /// The actor behind a request. Never fails: the core has its own default.
    fn identify(&self, request: &RequestParts) -> Actor;

    /// The actor behind a request, and whether to serve it: a request no
    /// extension identified goes through every extension's `admit`. Admitted,
    /// with the actor of [`Self::identify`], by default.
    fn identify_request(&self, request: &RequestParts) -> Identification {
        Identification {
            actor: self.identify(request),
            admission: Admission::Admit,
        }
    }

    /// How `actor` shows to a person, when an extension knows it. None by
    /// default.
    fn display_name(&self, _actor: &Actor) -> Option<ActorDisplay> {
        None
    }

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

    /// [`Self::on_create`], with the data the creation carried for the
    /// extensions (API 0.9): each extension receives its own value. By
    /// default, [`Self::on_create`].
    fn on_create_with_data(
        &self,
        actor: &Actor,
        entity: &EntityRef,
        _data: &ExtensionData,
    ) -> Attributes {
        self.on_create(actor, entity)
    }

    /// The ids of the installed extensions, in registration order. None by
    /// default.
    fn installed(&self) -> Vec<String> {
        Vec::new()
    }

    /// Check the data a creation of a `kind` carries for the extensions (API
    /// 0.9), before anything is created: an id no installed extension has is
    /// refused ([`CreationRefusal::UnknownExtension`]), then each extension
    /// whose id is present checks its value. By default, only the ids are
    /// checked against [`Self::installed`].
    fn validate_create(
        &self,
        _actor: &Actor,
        _kind: &ResourceKind,
        data: &ExtensionData,
    ) -> Result<(), CreationRefusal> {
        creation::unknown_extension(data, &self.installed())
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

    /// The catalogue of the Ressources: the core's, then each extension's
    /// declarations, in registration order. The core's alone by default.
    fn catalogue(&self) -> Vec<Resource> {
        pdo_extension_api::core_resources()
    }

    /// What the extensions add to the launch of a session (API 0.13): a
    /// Node, the Pipeline Manager, the library assistant. Their environment
    /// variables and their preamble ([`NodeContribution`]). Nothing by
    /// default.
    fn session_launch(&self, _launch: &SessionLaunch) -> NodeContribution {
        NodeContribution::default()
    }

    /// The extension that suspends `trigger`, and why, if one does. None by
    /// default.
    fn trigger_suspension(&self, _trigger: &TriggerRef) -> Option<TriggerSuspension> {
        None
    }

    /// Whether no extension can refuse anything: the reads then skip the
    /// question altogether (CORE-5), so a daemon without extension serves its
    /// lists exactly as before. `false` by default.
    fn allows_everything(&self) -> bool {
        false
    }
}
