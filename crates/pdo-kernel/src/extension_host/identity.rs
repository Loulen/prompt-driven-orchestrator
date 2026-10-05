//! Who a request comes from, whether the extensions let it in (admission),
//! and how an Actor shows to a person (display name).

use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::{ActorDisplay, Admission, RequestParts};

use super::{panic_message, Extensions};
use crate::Actor;

/// Who a request comes from, and whether the extensions let it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    pub actor: Actor,
    pub admission: Admission,
}

impl Extensions {
    /// The actor the first extension that names it gives; `None` when none
    /// does.
    pub(super) fn identified(&self, request: &RequestParts) -> Option<Actor> {
        for r in &self.registry.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.identify(request))) {
                Ok(Some(actor)) => return Some(actor),
                Ok(None) => {}
                Err(_) => tracing::warn!(
                    "extension `{}` panicked in identify; ignored",
                    r.manifest.id
                ),
            }
        }
        None
    }

    /// What the extensions make of a request none of them identified: the
    /// first that does not admit it wins; a panic refuses.
    pub(super) fn admission(&self, request: &RequestParts) -> Admission {
        for r in &self.registry.registered {
            match catch_unwind(AssertUnwindSafe(|| r.extension.admit(request))) {
                Ok(Admission::Admit) => {}
                Ok(refusal) => return refusal,
                Err(panic) => {
                    let id = &r.manifest.id;
                    tracing::error!(
                        "extension `{id}` panicked in admit ({}); {} refused",
                        panic_message(panic.as_ref()),
                        request.uri.path(),
                    );
                    return Admission::refuse(format!("extension `{id}` failed"));
                }
            }
        }
        Admission::Admit
    }

    /// The first display name an extension gives `actor`; a panic is
    /// ignored.
    pub(super) fn display(&self, actor: &Actor) -> Option<ActorDisplay> {
        self.registry.registered.iter().find_map(|r| {
            catch_unwind(AssertUnwindSafe(|| r.extension.display_name(actor))).unwrap_or_else(
                |_| {
                    tracing::warn!(
                        "extension `{}` panicked in display_name; ignored",
                        r.manifest.id
                    );
                    None
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use pdo_extension_api::axum::http::Request;
    use pdo_extension_api::{Extension, Manifest};

    use super::*;
    use crate::extension_host::{ExtensionHost, NoExtensions};

    fn parts(headers: &[(&str, &str)]) -> RequestParts {
        let mut builder = Request::builder().uri("/runs");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    /// Identifies the caller of the `x-token: good` header as `alice`; refuses
    /// or redirects any other request, as told; names `alice`.
    struct Gate(Admission);
    impl Extension for Gate {
        fn manifest(&self) -> Manifest {
            Manifest::new("gate", "Gate", "0.0.0")
        }
        fn identify(&self, request: &RequestParts) -> Option<Actor> {
            (request.headers.get("x-token")? == "good").then(|| Actor::Identified {
                namespace: "gate".into(),
                id: "alice".into(),
            })
        }
        fn admit(&self, _request: &RequestParts) -> Admission {
            self.0.clone()
        }
        fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
            matches!(actor, Actor::Identified { id, .. } if id == "alice")
                .then(|| ActorDisplay::new("Alice"))
        }
    }

    struct PanicsInAdmit;
    impl Extension for PanicsInAdmit {
        fn manifest(&self) -> Manifest {
            Manifest::new("panics", "Panics", "0.0.0")
        }
        fn admit(&self, _request: &RequestParts) -> Admission {
            panic!("boom")
        }
        fn display_name(&self, _actor: &Actor) -> Option<ActorDisplay> {
            panic!("boom")
        }
    }

    struct Bob;
    impl Extension for Bob {
        fn manifest(&self) -> Manifest {
            Manifest::new("bob", "Bob", "0.0.0")
        }
        fn display_name(&self, _actor: &Actor) -> Option<ActorDisplay> {
            Some(ActorDisplay::new("Bob"))
        }
    }

    #[test]
    fn without_extension_every_request_is_admitted() {
        let identification = NoExtensions.identify_request(&parts(&[]));
        assert_eq!(identification.actor, Actor::LocalOperator);
        assert_eq!(identification.admission, Admission::Admit);
        assert_eq!(NoExtensions.display_name(&Actor::LocalOperator), None);
        let empty = Extensions::register(vec![]).unwrap();
        assert!(empty.identify_request(&parts(&[])).admission.is_admitted());
    }

    #[test]
    fn an_identified_request_is_admitted_without_asking() {
        let host = Extensions::register(vec![Box::new(Gate(Admission::refuse("token")))]).unwrap();
        let identification = host.identify_request(&parts(&[("x-token", "good")]));
        assert_eq!(identification.admission, Admission::Admit);
        assert!(matches!(identification.actor, Actor::Identified { .. }));
    }

    #[test]
    fn an_unidentified_request_gets_the_first_refusal() {
        let host = Extensions::register(vec![
            Box::new(Bob),
            Box::new(Gate(Admission::redirect("/ext/gate/sign-in"))),
            Box::new(PanicsInAdmit),
        ])
        .unwrap();
        let identification = host.identify_request(&parts(&[("x-token", "bad")]));
        assert_eq!(identification.actor, Actor::LocalOperator);
        assert_eq!(
            identification.admission,
            Admission::redirect("/ext/gate/sign-in")
        );
    }

    #[test]
    fn an_extension_that_panics_in_admit_refuses() {
        let host = Extensions::register(vec![Box::new(PanicsInAdmit)]).unwrap();
        assert_eq!(
            host.identify_request(&parts(&[])).admission,
            Admission::refuse("extension `panics` failed")
        );
    }

    #[test]
    fn the_first_extension_that_names_an_actor_wins_and_a_panic_is_skipped() {
        let alice = Actor::Identified {
            namespace: "gate".into(),
            id: "alice".into(),
        };
        let host = Extensions::register(vec![
            Box::new(PanicsInAdmit),
            Box::new(Gate(Admission::Admit)),
            Box::new(Bob),
        ])
        .unwrap();
        assert_eq!(host.display_name(&alice), Some(ActorDisplay::new("Alice")));
        assert_eq!(
            host.display_name(&Actor::LocalOperator),
            Some(ActorDisplay::new("Bob"))
        );
        assert_eq!(
            host.host_of("bob").display_name(&alice),
            Some(ActorDisplay::new("Alice")),
            "an extension's Host names the Actor as the core does"
        );
    }
}
