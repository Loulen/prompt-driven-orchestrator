use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// The `error` slug of a request refused by [`Extension::admit`](crate::Extension::admit).
pub const UNAUTHENTICATED: &str = "unauthenticated";

/// The answer of [`Extension::admit`](crate::Extension::admit): what becomes of
/// a request **no extension identified**.
///
/// The core asks only for the routes of its API (the extensions' routes under
/// `/ext/<id>/` included, `/ws` and the terminals' PTY too), never for the
/// SPA's own files: the interface always loads, and follows the redirection
/// its first call to the API gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "admission", rename_all = "snake_case")]
#[must_use]
pub enum Admission {
    /// Serve the request, as without extension.
    Admit,
    /// Answer `401`, with the `reason` to show the caller.
    Refuse { reason: String },
    /// Answer `401` naming `to`, the page to sign in on (one of the
    /// extension's: `/ext/<id>/sign-in`). The core's interface goes there,
    /// appending `next=<the URL to come back to>` to its query.
    Redirect { to: String },
}

impl Admission {
    pub fn refuse(reason: impl Into<String>) -> Self {
        Admission::Refuse {
            reason: reason.into(),
        }
    }

    pub fn redirect(to: impl Into<String>) -> Self {
        Admission::Redirect { to: to.into() }
    }

    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admit)
    }

    /// The `401` body of a refusal or a redirection, `None` when admitted:
    /// `{"error": "unauthenticated", "message", "reason"}`, plus `"redirect":
    /// <to>` for a redirection.
    pub fn body(&self) -> Option<serde_json::Value> {
        let (reason, redirect) = match self {
            Admission::Admit => return None,
            Admission::Refuse { reason } => (reason.as_str(), None),
            Admission::Redirect { to } => ("sign in first", Some(to)),
        };
        let mut body = serde_json::json!({
            "error": UNAUTHENTICATED,
            "message": format!("authentication required: {reason}"),
            "reason": reason,
        });
        if let Some(to) = redirect {
            body["redirect"] = serde_json::Value::String(to.clone());
        }
        Some(body)
    }

    /// `401` with [`Admission::body`] and `WWW-Authenticate: Bearer`, or
    /// `None` when admitted.
    pub fn into_refusal(self) -> Option<Response> {
        let body = self.body()?;
        Some(
            (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
                axum::Json(body),
            )
                .into_response(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitting_has_no_refusal() {
        assert!(Admission::Admit.is_admitted());
        assert_eq!(Admission::Admit.body(), None);
        assert!(Admission::Admit.into_refusal().is_none());
    }

    #[test]
    fn a_refusal_carries_its_reason() {
        assert_eq!(
            Admission::refuse("a token is required").body(),
            Some(serde_json::json!({
                "error": "unauthenticated",
                "message": "authentication required: a token is required",
                "reason": "a token is required",
            }))
        );
    }

    #[tokio::test]
    async fn a_redirection_answers_401_naming_the_page() {
        let response = Admission::redirect("/ext/gate/sign-in")
            .into_refusal()
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "unauthenticated");
        assert_eq!(body["redirect"], "/ext/gate/sign-in");
    }
}
