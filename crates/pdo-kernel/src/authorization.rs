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

/// Ask the host whether `actor` may run `operation`.
pub fn authorize(
    host: &dyn ExtensionHost,
    actor: &Actor,
    operation: &Operation,
) -> Result<(), Denied> {
    decide(host.authorize(actor, operation), actor, operation)?;
    crate::applied::allowed(actor, operation);
    Ok(())
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
        let denied = Denied {
            reason: "projects are read-only".into(),
        };
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
            })
        );
    }
}
