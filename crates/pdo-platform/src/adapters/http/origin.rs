//! The declared origin of a request, `X-PDO-Actor`, as an axum extractor.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::domain::audit::ActorHint;

/// The [`ActorHint`] a request declares. Never rejects (`Infallible`): an absent
/// or unknown header is `Unknown`, not a 4xx. Place it before the body
/// extractor (`Json<T>`) in a handler's signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginHint(pub ActorHint);

impl OriginHint {
    /// The byte-exact `actor_hint` column value.
    pub fn as_hint(self) -> &'static str {
        self.0.as_hint()
    }
}

impl<S> FromRequestParts<S> for OriginHint
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get("X-PDO-Actor")
            .and_then(|v| v.to_str().ok());
        Ok(OriginHint(ActorHint::from_header(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn hint(value: Option<&str>) -> ActorHint {
        let mut builder = axum::http::Request::builder();
        if let Some(v) = value {
            builder = builder.header("X-PDO-Actor", v);
        }
        let (mut parts, ()) = builder.body(()).unwrap().into_parts();
        OriginHint::from_request_parts(&mut parts, &())
            .await
            .expect("the extraction is infallible")
            .0
    }

    #[tokio::test]
    async fn the_extractor_maps_the_header_and_never_rejects() {
        assert_eq!(hint(Some("ui")).await, ActorHint::Ui);
        assert_eq!(hint(Some("cli")).await, ActorHint::Cli);
        assert_eq!(hint(Some("bogus")).await, ActorHint::Unknown);
        assert_eq!(hint(None).await, ActorHint::Unknown);
    }
}
