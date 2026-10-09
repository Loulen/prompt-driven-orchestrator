use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

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

    /// `Ok` when allowed, the [`Denied`] refusal otherwise: what a route of an
    /// extension answers with `?`.
    pub fn into_result(self) -> Result<(), Denied> {
        match self {
            Authorization::Allow => Ok(()),
            Authorization::Deny { reason } => Err(Denied { reason }),
        }
    }
}

/// The `error` slug of a refused Operation.
pub const OPERATION_REFUSED: &str = "operation_refused";

/// An Operation an extension refused, with the reason to show the operator.
///
/// As a response, `403` with the body every refusal of the daemon carries:
/// `{"error": "operation_refused", "message": "operation refused: <reason>",
/// "reason": <reason>}`. An extension answers its own refused Operations with
/// it, so a client reads them like the core's.
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

impl Denied {
    /// The JSON body of the refusal.
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({
            "error": OPERATION_REFUSED,
            "message": self.to_string(),
            "reason": self.reason,
        })
    }
}

impl IntoResponse for Denied {
    fn into_response(self) -> Response {
        (StatusCode::FORBIDDEN, axum::Json(self.body())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_its_reason() {
        let deny = Authorization::deny("admins only");
        assert!(!deny.is_allowed());
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            serde_json::json!({ "decision": "deny", "reason": "admins only" })
        );
        assert_eq!(
            deny.into_result(),
            Err(Denied {
                reason: "admins only".into()
            })
        );
        assert_eq!(Authorization::Allow.into_result(), Ok(()));
    }

    #[tokio::test]
    async fn a_refusal_answers_403_with_the_core_body() {
        let response = Denied {
            reason: "projects are read-only".into(),
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
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
