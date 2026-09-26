use serde::Serialize;

use crate::{Actor, Attributes, EntityRef, Manifest, Operation};

/// The head of an HTTP request, as `identify` sees it: method, URI, headers.
/// Never the body, which a hook must not consume.
pub type RequestParts = axum::http::request::Parts;

/// The answer of `authorize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[must_use]
pub enum Authorization {
    Allow,
    /// Refuse the Operation. The `reason` reaches the operator in the error the
    /// gesture already shows.
    Deny {
        reason: String,
    },
}

impl Authorization {
    pub fn deny(reason: impl Into<String>) -> Self {
        Authorization::Deny {
            reason: reason.into(),
        }
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self, Authorization::Allow)
    }
}

/// What an extension adds to the interface (the UI slots of CONTEXT.md §
/// *Extensions*). Empty by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UiContributions {
    /// Where the daemon serves the extension's UI bundle, if it has one.
    pub bundle_url: Option<String>,
}

/// A third-party module registered when the daemon starts (ADR-0081).
///
/// Only [`Extension::manifest`] is required. Every hook defaults to the neutral
/// answer: an extension that overrides nothing changes nothing.
///
/// Hooks are called from the daemon's request path, so they must be quick. A
/// panic in `authorize` is a refusal; `on_event` is called off the request path
/// and can never slow the core down.
pub trait Extension: Send + Sync + 'static {
    /// Who this extension is.
    fn manifest(&self) -> Manifest;

    /// Identify the actor behind a request. `None` (the default) lets the next
    /// extension, then the core, decide: the local operator, or the agent of the
    /// node session the request comes from.
    fn identify(&self, _request: &RequestParts) -> Option<Actor> {
        None
    }

    /// Allow or refuse an Operation before it runs. Allows by default. Never
    /// consulted for the scheduler's internal transitions: an extension cannot
    /// freeze a Run in flight.
    fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
        Authorization::Allow
    }

    /// The attributes to stamp on an entity the actor just created. None by
    /// default.
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }

    /// Observe an event: exactly the JSON the daemon broadcasts on `/ws`, plus
    /// `OperationApplied` after each authorized and successful mutation. Ignores
    /// everything by default.
    fn on_event(&self, _event: &serde_json::Value) {}

    /// Routes served under `/ext/<id>/`. None by default.
    fn routes(&self) -> Option<axum::Router> {
        None
    }

    /// Contributions to the interface. None by default.
    fn ui(&self) -> Option<UiContributions> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceKind, Verb};

    /// An extension that overrides nothing but its manifest.
    struct Bare;

    impl Extension for Bare {
        fn manifest(&self) -> Manifest {
            Manifest::new("bare", "Bare", "0.0.0")
        }
    }

    #[test]
    fn every_hook_defaults_to_the_neutral_answer() {
        let ext = Bare;
        let (parts, ()) = axum::http::Request::builder()
            .uri("/runs")
            .body(())
            .unwrap()
            .into_parts();
        let actor = Actor::LocalOperator;
        let op = Operation::new(ResourceKind::Project, Verb::Update).on("p1");

        assert_eq!(ext.identify(&parts), None);
        assert_eq!(ext.authorize(&actor, &op), Authorization::Allow);
        assert!(ext
            .on_create(&actor, &EntityRef::new(ResourceKind::Project, "p1"))
            .is_empty());
        ext.on_event(&serde_json::json!({ "kind": "RunCreated" }));
        assert!(ext.routes().is_none());
        assert!(ext.ui().is_none());
        assert_eq!(ext.manifest().id, "bare");
    }

    #[test]
    fn extensions_are_object_safe() {
        let extensions: Vec<Box<dyn Extension>> = vec![Box::new(Bare)];
        assert_eq!(extensions.len(), 1);
    }

    #[test]
    fn a_refusal_carries_its_reason() {
        let deny = Authorization::deny("admins only");
        assert!(!deny.is_allowed());
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            serde_json::json!({ "decision": "deny", "reason": "admins only" })
        );
    }
}
