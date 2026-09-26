//! The Autorisation d'opération (CONTEXT.md § *Extensions*): the one point where
//! extensions accept or refuse an Operation before it runs.
//!
//! It covers what comes in through the UI, the CLI, the agents and the Triggers,
//! never the scheduler's internal transitions. Without an extension, everything
//! is allowed.

use std::fmt;

use pdo_extension_api::Authorization;

use crate::extension_host::ExtensionHost;
use crate::{Actor, Operation};

/// An Operation an extension refused, with the reason to show the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub reason: String,
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation refused: {}", self.reason)
    }
}

impl std::error::Error for Denied {}

/// Ask the host whether `actor` may run `operation`.
pub fn authorize(
    host: &dyn ExtensionHost,
    actor: &Actor,
    operation: &Operation,
) -> Result<(), Denied> {
    match host.authorize(actor, operation) {
        Authorization::Allow => Ok(()),
        Authorization::Deny { reason } => Err(Denied { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::NoExtensions;
    use crate::{ResourceKind, Verb};

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
}
