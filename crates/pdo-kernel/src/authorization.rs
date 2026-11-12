//! The Autorisation d'opération (CONTEXT.md § *Extensions*): the one point where
//! extensions accept or refuse an Operation before it runs.
//!
//! It covers what comes in through the UI, the CLI, the agents and the Triggers,
//! never the scheduler's internal transitions. Without an extension, everything
//! is allowed.
//!
//! A refusal answers `403` with the body every refusal of the daemon carries
//! (ADR-0035 §3): the stable slug [`OPERATION_REFUSED`] in `error`, the sentence
//! the gesture shows in `message`, and the extension's own `reason`.

use pdo_extension_api::Authorization;
pub use pdo_extension_api::{Denied, OPERATION_REFUSED};

use crate::extension_host::ExtensionHost;
use crate::{Actor, Operation};

/// Ask the host whether `actor` may run `operation`. An allowed mutation is
/// recorded, to be announced as applied once its gesture succeeded; a read
/// ([`Verb::Read`](crate::Verb::Read)) never is.
pub fn authorize(
    host: &dyn ExtensionHost,
    actor: &Actor,
    operation: &Operation,
) -> Result<(), Denied> {
    decide(host.authorize(actor, operation), actor, operation)?;
    if !operation.is_read() {
        crate::applied::allowed(actor, operation);
    }
    Ok(())
}

/// Whether `actor` may read what `operation` names (CORE-5): a Run, a Projet,
/// the disk. Never recorded as applied, and a refusal is logged at `debug`
/// only: a list asks once per entity it holds. Without extension, no
/// question at all.
pub fn authorize_read(
    host: &dyn ExtensionHost,
    actor: &Actor,
    operation: &Operation,
) -> Result<(), Denied> {
    if host.allows_everything() {
        return Ok(());
    }
    host.authorize(actor, operation)
        .into_result()
        .inspect_err(|denied| {
            tracing::debug!(
                "read refused: {} {:?} by {}: {}",
                operation.resource,
                operation.target,
                who(actor),
                denied.reason,
            );
        })
}

/// What a read of one entity shows its caller ([`authorize_entity_read`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityRead {
    /// The entity, as stored.
    Shown,
    /// Nothing: a concealed refusal ([`Denied::is_concealed`]). The read
    /// answers exactly as for an entity the core does not know, the same
    /// status and the same body.
    AsUnknown,
}

/// Whether `actor` may read the one entity `operation` names (a Run, a
/// Trigger, a session…), as [`authorize_read`]; a concealed refusal is not
/// an error but [`EntityRead::AsUnknown`]: the caller must not learn that
/// the entity exists.
pub fn authorize_entity_read(
    host: &dyn ExtensionHost,
    actor: &Actor,
    operation: &Operation,
) -> Result<EntityRead, Denied> {
    match authorize_read(host, actor, operation) {
        Ok(()) => Ok(EntityRead::Shown),
        Err(denied) if denied.is_concealed() => Ok(EntityRead::AsUnknown),
        Err(denied) => Err(denied),
    }
}

/// The verdict of `authorization` as a result, the refusal logged.
pub(crate) fn decide(
    authorization: Authorization,
    actor: &Actor,
    operation: &Operation,
) -> Result<(), Denied> {
    authorization.into_result().inspect_err(|denied| {
        tracing::info!(
            "operation refused: {} {:?} {:?} by {}: {}",
            operation.resource,
            operation.verb,
            operation.target,
            who(actor),
            denied.reason,
        );
    })
}

/// The actor as a refusal's log line names it: its kind, and for an identified
/// one the extension's namespace and id, so the refusal can be audited.
fn who(actor: &Actor) -> String {
    match actor {
        Actor::Identified { namespace, id } => format!("{namespace}:{id}"),
        Actor::Agent { run_id, node_id } => format!("agent {run_id}/{node_id}"),
        Actor::Trigger { trigger_id } => format!("trigger {trigger_id}"),
        other => other.kind().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::NoExtensions;
    use crate::{ResourceKind, Verb};
    use pdo_extension_api::axum::http::StatusCode;
    use pdo_extension_api::axum::response::IntoResponse;

    #[test]
    fn without_extension_everything_is_allowed() {
        let op = Operation::new(ResourceKind::Run, Verb::Delete).on("r1");
        assert_eq!(authorize(&NoExtensions, &Actor::LocalOperator, &op), Ok(()));
        let agent = Actor::Agent {
            run_id: "r1".into(),
            node_id: "n1".into(),
        };
        assert_eq!(authorize(&NoExtensions, &agent, &op), Ok(()));
    }

    #[test]
    fn a_read_is_asked_but_never_recorded_as_applied() {
        struct RefusesReads;
        impl ExtensionHost for RefusesReads {
            fn identify(&self, _: &pdo_extension_api::RequestParts) -> Actor {
                Actor::LocalOperator
            }
            fn authorize(&self, _: &Actor, operation: &Operation) -> Authorization {
                if operation.is_read() {
                    Authorization::deny("no reading")
                } else {
                    Authorization::Allow
                }
            }
            fn publish(&self, _: &serde_json::Value) {}
        }
        let read = Operation::new(ResourceKind::Run, Verb::Read).on("r1");
        assert_eq!(
            authorize_read(&RefusesReads, &Actor::LocalOperator, &read),
            Err(Denied::new("no reading"))
        );
        assert_eq!(
            authorize_read(&NoExtensions, &Actor::LocalOperator, &read),
            Ok(()),
            "without extension, nothing is asked"
        );
        let allows_reads = Operation::new(ResourceKind::Project, Verb::Read).on("p1");
        let (result, record) = block_on(async {
            crate::applied::scope(async {
                authorize(&NoExtensions, &Actor::LocalOperator, &allows_reads)
            })
            .await
        });
        assert_eq!(result, Ok(()));
        assert!(record.is_empty(), "a read is never applied: {record:?}");
    }

    #[test]
    fn a_concealed_refusal_reads_as_an_unknown_entity() {
        struct Conceals;
        impl ExtensionHost for Conceals {
            fn identify(&self, _: &pdo_extension_api::RequestParts) -> Actor {
                Actor::LocalOperator
            }
            fn authorize(&self, _: &Actor, operation: &Operation) -> Authorization {
                match operation.target.as_deref() {
                    Some("hidden") => Authorization::deny("not a member").concealed(),
                    Some("refused") => Authorization::deny("no reading"),
                    _ => Authorization::Allow,
                }
            }
            fn publish(&self, _: &serde_json::Value) {}
        }
        let read = |id: &str| Operation::new(ResourceKind::Trigger, Verb::Read).on(id);
        let actor = Actor::LocalOperator;
        assert_eq!(
            authorize_entity_read(&Conceals, &actor, &read("shown")),
            Ok(EntityRead::Shown)
        );
        assert_eq!(
            authorize_entity_read(&Conceals, &actor, &read("hidden")),
            Ok(EntityRead::AsUnknown)
        );
        assert_eq!(
            authorize_entity_read(&Conceals, &actor, &read("refused")),
            Err(Denied::new("no reading"))
        );
        assert_eq!(
            authorize_entity_read(&NoExtensions, &actor, &read("hidden")),
            Ok(EntityRead::Shown)
        );
    }

    /// Drive `future` on a fresh current-thread runtime.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn a_refusal_log_names_who_asked() {
        assert_eq!(who(&Actor::LocalOperator), "local_operator");
        assert_eq!(
            who(&Actor::Identified {
                namespace: "acme".into(),
                id: "alice".into()
            }),
            "acme:alice"
        );
        assert_eq!(
            who(&Actor::Agent {
                run_id: "r1".into(),
                node_id: "n1".into()
            }),
            "agent r1/n1"
        );
    }

    #[tokio::test]
    async fn a_refusal_is_a_403_with_the_slug_and_the_reason() {
        let denied = Denied::new("projects are read-only");
        let response = denied.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = pdo_extension_api::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({
                "error": "operation_refused",
                "message": "operation refused: projects are read-only",
                "reason": "projects are read-only",
                "sentence": "You don't have the rights to do this.",
            })
        );
    }
}
