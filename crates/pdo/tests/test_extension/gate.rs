//! The test extension's identity (0.8): it can **require a token**, then
//! identifies its holder, refuses (or redirects to its sign-in page) every
//! request without it, and **names** the Actors it identifies.
//!
//! - **Token**: `Authorization: Bearer <token>` (the CLI's `PDO_TOKEN`) or the
//!   cookie [`TOKEN_COOKIE`] (set by its sign-in page) identifies
//!   `Actor::Identified { namespace: "test", id: `[`TOKEN_HOLDER`]` }`.
//! - **Admission**: with a token required, a request no extension identified
//!   is refused (`401`), or redirected to [`SIGN_IN`] in redirect mode; the
//!   sign-in page itself is admitted. Without a token required, everything is.
//! - **Display name**: an Actor it identifies, `id` capitalised (`alice` →
//!   `Alice`), without an image: the core draws its initials in its hue.
//! - Routes: `GET`/`PUT /ext/test/gate` (`{token, redirect}`), `GET
//!   /ext/test/sign-in` (a form setting the cookie, then back to `?next=`).

use std::sync::{Arc, Mutex};

use pdo::extension_api::axum::http::header;
use pdo::extension_api::Actor;
use pdo::extension_api::{ActorDisplay, Admission, RequestParts};
use serde::{Deserialize, Serialize};

use super::ID;

/// The Actor id a valid token identifies.
pub const TOKEN_HOLDER: &str = "alice";

/// The cookie the sign-in page sets.
pub const TOKEN_COOKIE: &str = "pdo-test-token";

/// The page a redirection names.
pub const SIGN_IN: &str = "/ext/test/sign-in";

/// What the test extension requires: a token (none: everything is admitted),
/// and whether a refusal redirects to its sign-in page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub redirect: bool,
}

impl Gate {
    /// Requiring `token`, refusing without it.
    pub fn token(token: &str) -> Self {
        Self {
            token: Some(token.to_string()),
            redirect: false,
        }
    }

    /// The same gate, redirecting to the sign-in page instead of refusing.
    pub fn redirecting(mut self) -> Self {
        self.redirect = true;
        self
    }

    /// Read from the demo daemon's `PDO_TEST_EXTENSION_TOKEN` (and
    /// `PDO_TEST_EXTENSION_REDIRECT=1`).
    pub fn from_env() -> Self {
        Self {
            token: std::env::var("PDO_TEST_EXTENSION_TOKEN")
                .ok()
                .filter(|t| !t.trim().is_empty()),
            redirect: std::env::var("PDO_TEST_EXTENSION_REDIRECT").as_deref() == Ok("1"),
        }
    }
}

/// The gate shared by the hooks and the routes.
pub type SharedGate = Arc<Mutex<Gate>>;

/// The token the request carries: its bearer, else its cookie.
fn presented(request: &RequestParts) -> Option<String> {
    let bearer = request
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string());
    bearer.or_else(|| {
        request
            .headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(name, _)| *name == TOKEN_COOKIE)
            .map(|(_, value)| value.to_string())
    })
}

/// The holder of the required token, when the request presents it.
pub fn identify(gate: &Gate, request: &RequestParts) -> Option<Actor> {
    let token = gate.token.as_deref()?;
    (presented(request).as_deref() == Some(token)).then(|| Actor::Identified {
        namespace: ID.to_string(),
        id: TOKEN_HOLDER.to_string(),
    })
}

/// What becomes of a request nobody identified.
pub fn admit(gate: &Gate, request: &RequestParts) -> Admission {
    if gate.token.is_none() || request.uri.path() == SIGN_IN {
        Admission::Admit
    } else if gate.redirect {
        Admission::redirect(SIGN_IN)
    } else {
        Admission::refuse("the test extension requires a token")
    }
}

/// The name of an Actor it identified: its id, capitalised.
pub fn display_name(actor: &Actor) -> Option<ActorDisplay> {
    match actor {
        Actor::Identified { namespace, id } if namespace == ID => {
            let mut chars = id.chars();
            let name: String = chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default();
            Some(ActorDisplay::new(name))
        }
        _ => None,
    }
}

/// The sign-in page: one field, the token, kept in a cookie; then back to
/// `next`. A page outside the interface: it links the core's tokens and loads
/// its theme script (`/shared/theme.js`), so it opens in the Theme and the mode
/// the browser chose.
pub const SIGN_IN_PAGE: &str = include_str!("sign_in.html");

#[cfg(test)]
mod tests {
    use super::*;
    use pdo::extension_api::axum::http::Request;

    fn parts(uri: &str, headers: &[(&str, &str)]) -> RequestParts {
        let mut builder = Request::builder().uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn a_bearer_or_the_cookie_identifies_the_holder() {
        let gate = Gate::token("s3cret");
        let alice = Some(Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        });
        assert_eq!(
            identify(
                &gate,
                &parts("/runs", &[("authorization", "Bearer s3cret")])
            ),
            alice
        );
        assert_eq!(
            identify(
                &gate,
                &parts("/runs", &[("cookie", "a=b; pdo-test-token=s3cret")])
            ),
            alice
        );
        assert_eq!(
            identify(&gate, &parts("/runs", &[("authorization", "Bearer no")])),
            None
        );
        assert_eq!(identify(&Gate::default(), &parts("/runs", &[])), None);
    }

    #[test]
    fn without_token_required_everything_is_admitted() {
        assert!(admit(&Gate::default(), &parts("/runs", &[])).is_admitted());
        assert!(!admit(&Gate::token("t"), &parts("/runs", &[])).is_admitted());
        assert_eq!(
            admit(&Gate::token("t").redirecting(), &parts("/ws", &[])),
            Admission::redirect(SIGN_IN)
        );
        assert!(admit(&Gate::token("t"), &parts(SIGN_IN, &[])).is_admitted());
    }

    #[test]
    fn it_names_its_own_actors_only() {
        let alice = Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        };
        assert_eq!(display_name(&alice).unwrap().name, "Alice");
        assert_eq!(display_name(&Actor::LocalOperator), None);
    }
}
