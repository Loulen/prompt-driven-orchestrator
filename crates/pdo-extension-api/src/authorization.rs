use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

/// The sentence a refusal shows a human when the extension that refused gave
/// none: a person never reads the technical reason.
pub const GENERIC_REFUSAL: &str = "You don't have the rights to do this.";

/// The answer of `authorize`.
///
/// On the wire (`POST /authorize`), `{"decision": "allow"}` or `{"decision":
/// "deny", "reason", "sentence"}`: `reason` is the technical cause, for a
/// caller that is not a person (a log, an agent's diagnosis); `sentence` is
/// what a person reads, the extension's or [`GENERIC_REFUSAL`]. A concealed
/// refusal ([`Authorization::concealed`]) adds `"concealed": true`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum Authorization {
    Allow,
    /// Refuse the Operation. The `reason` is the technical cause; `sentence`,
    /// when the extension gives one, the plain-language sentence a person
    /// reads instead (« You can't change Runs in Project P. Ask an
    /// administrator of P. »): what they cannot do and who can grant it, never a Role's or
    /// a Right's name. Without it, a person reads [`GENERIC_REFUSAL`].
    ///
    /// `concealed`: the refusal must not reveal that the entity exists. On a
    /// read of one entity (a Run and its nodes, a Trigger, a session…), the
    /// core then answers exactly as for an id it does not know, the same
    /// status and the same body. Elsewhere (a list, a mutation, a read of the
    /// shared configuration), it is an ordinary refusal.
    Deny {
        reason: String,
        sentence: Option<String>,
        concealed: bool,
    },
}

impl Authorization {
    /// Refuse, with the technical `reason` and no sentence for a person.
    pub fn deny(reason: impl Into<String>) -> Self {
        Authorization::Deny {
            reason: reason.into(),
            sentence: None,
            concealed: false,
        }
    }

    /// This refusal, with `sentence` for a person ([`Authorization::Deny`]);
    /// an allowance stays one.
    pub fn saying(self, sentence: impl Into<String>) -> Self {
        match self {
            Authorization::Allow => Authorization::Allow,
            Authorization::Deny {
                reason, concealed, ..
            } => Authorization::Deny {
                reason,
                sentence: Some(sentence.into()),
                concealed,
            },
        }
    }

    /// This refusal, concealed: on a read of one entity, the core answers as
    /// for an entity it does not know ([`Authorization::Deny`]). An allowance
    /// stays one.
    pub fn concealed(self) -> Self {
        match self {
            Authorization::Allow => Authorization::Allow,
            Authorization::Deny {
                reason, sentence, ..
            } => Authorization::Deny {
                reason,
                sentence,
                concealed: true,
            },
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
            Authorization::Deny {
                reason,
                sentence,
                concealed,
            } => Err(Denied {
                reason,
                sentence,
                concealed,
            }),
        }
    }
}

impl From<Denied> for Authorization {
    fn from(denied: Denied) -> Self {
        Authorization::Deny {
            reason: denied.reason,
            sentence: denied.sentence,
            concealed: denied.concealed,
        }
    }
}

impl Serialize for Authorization {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Authorization::Allow => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("decision", "allow")?;
                map.end()
            }
            Authorization::Deny {
                reason,
                sentence,
                concealed,
            } => {
                let mut map = serializer.serialize_map(Some(3 + usize::from(*concealed)))?;
                map.serialize_entry("decision", "deny")?;
                map.serialize_entry("reason", reason)?;
                map.serialize_entry("sentence", sentence_or_generic(sentence.as_deref()))?;
                if *concealed {
                    map.serialize_entry("concealed", &true)?;
                }
                map.end()
            }
        }
    }
}

fn sentence_or_generic(sentence: Option<&str>) -> &str {
    sentence
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(GENERIC_REFUSAL)
}

/// The `error` slug of a refused Operation.
pub const OPERATION_REFUSED: &str = "operation_refused";

/// An Operation an extension refused, with the reason to show the operator.
///
/// As a response, `403` with the body every refusal of the daemon carries:
/// `{"error": "operation_refused", "message": "operation refused: <reason>",
/// "reason": <reason>, "sentence": <sentence>}`. An extension answers its own
/// refused Operations with it, so a client reads them like the core's: a
/// person reads `sentence` (the extension's, else [`GENERIC_REFUSAL`]), a
/// program or an agent `reason`.
///
/// A concealed refusal ([`Denied::concealed`]) answers the same `403`: only
/// the core, on a read of one entity, turns it into the answer of an unknown
/// entity ([`Authorization::Deny`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub reason: String,
    /// The sentence for a person, when the extension gave one.
    pub sentence: Option<String>,
    /// Whether the refusal must not reveal that the entity exists.
    pub concealed: bool,
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation refused: {}", self.reason)
    }
}

impl std::error::Error for Denied {}

impl Denied {
    /// The refusal of technical `reason`, without a sentence for a person.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            sentence: None,
            concealed: false,
        }
    }

    /// This refusal, with `sentence` for a person.
    pub fn saying(mut self, sentence: impl Into<String>) -> Self {
        self.sentence = Some(sentence.into());
        self
    }

    /// This refusal, concealed ([`Authorization::concealed`]).
    pub fn concealed(mut self) -> Self {
        self.concealed = true;
        self
    }

    /// Whether the refusal must not reveal that the entity exists: on a read
    /// of one entity, the core answers as for an id it does not know.
    pub fn is_concealed(&self) -> bool {
        self.concealed
    }

    /// What a person reads: the extension's sentence, else [`GENERIC_REFUSAL`].
    pub fn sentence(&self) -> &str {
        sentence_or_generic(self.sentence.as_deref())
    }

    /// What an agent reads: the sentence, then the technical cause in
    /// parentheses, so it can diagnose.
    pub fn for_an_agent(&self) -> String {
        format!("{} ({})", self.sentence(), self.reason)
    }

    /// The refusal a `403` body holds (`{"error": "operation_refused",
    /// "reason", "sentence"?}`), `None` for any other body: what a client of
    /// the daemon reads back, to show the sentence to a person or
    /// [`Denied::for_an_agent`] to an agent.
    pub fn from_body(body: &serde_json::Value) -> Option<Self> {
        if body.get("error")?.as_str()? != OPERATION_REFUSED {
            return None;
        }
        let reason = body
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let sentence = body
            .get("sentence")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty());
        Some(Self {
            reason: reason.to_string(),
            sentence: sentence.map(str::to_string),
            concealed: false,
        })
    }

    /// The JSON body of the refusal.
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({
            "error": OPERATION_REFUSED,
            "message": self.to_string(),
            "reason": self.reason,
            "sentence": self.sentence(),
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
            serde_json::json!({
                "decision": "deny",
                "reason": "admins only",
                "sentence": GENERIC_REFUSAL,
            })
        );
        assert_eq!(deny.into_result(), Err(Denied::new("admins only")));
        assert_eq!(Authorization::Allow.into_result(), Ok(()));
        assert_eq!(
            serde_json::to_value(Authorization::Allow).unwrap(),
            serde_json::json!({ "decision": "allow" })
        );
    }

    #[test]
    fn a_refusal_may_carry_a_sentence_for_a_person() {
        let deny = Authorization::deny("the test rules refuse this gesture")
            .saying("You can't change Runs in Project P. Ask an administrator of P.");
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            serde_json::json!({
                "decision": "deny",
                "reason": "the test rules refuse this gesture",
                "sentence": "You can't change Runs in Project P. Ask an administrator of P.",
            })
        );
        let denied = deny.into_result().unwrap_err();
        assert_eq!(
            denied.sentence(),
            "You can't change Runs in Project P. Ask an administrator of P."
        );
        assert_eq!(
            denied.for_an_agent(),
            "You can't change Runs in Project P. Ask an administrator of P. (the test rules refuse this gesture)"
        );
        assert_eq!(
            Authorization::from(denied.clone()),
            Authorization::Deny {
                reason: denied.reason.clone(),
                sentence: denied.sentence.clone(),
                concealed: false,
            }
        );
        assert_eq!(
            Authorization::Allow.saying("ignored"),
            Authorization::Allow,
            "an allowance has no sentence"
        );
    }

    #[test]
    fn a_refusal_may_be_concealed() {
        let deny = Authorization::deny("not a member of the Run's Projects")
            .saying("You can't see this Run.")
            .concealed();
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            serde_json::json!({
                "decision": "deny",
                "reason": "not a member of the Run's Projects",
                "sentence": "You can't see this Run.",
                "concealed": true,
            })
        );
        let denied = deny.clone().into_result().unwrap_err();
        assert!(denied.is_concealed());
        assert_eq!(Authorization::from(denied.clone()), deny);
        assert_eq!(
            Authorization::deny("x").concealed().saying("Ask Bob."),
            Authorization::deny("x").saying("Ask Bob.").concealed(),
            "the order of the builders does not matter"
        );
        assert_eq!(Authorization::Allow.concealed(), Authorization::Allow);
        assert!(!Denied::new("x").is_concealed());
        assert!(Denied::new("x").concealed().is_concealed());
        assert_eq!(
            denied.body(),
            Denied::new("not a member of the Run's Projects")
                .saying("You can't see this Run.")
                .body(),
            "as a 403, a concealed refusal reads like any other"
        );
    }

    #[test]
    fn a_client_reads_the_refusal_back_from_its_body() {
        let denied = Denied::new("the test rules refuse it").saying("You can't do this. Ask Bob.");
        assert_eq!(Denied::from_body(&denied.body()), Some(denied));
        let older = serde_json::json!({
            "error": "operation_refused",
            "message": "operation refused: no",
            "reason": "no",
        });
        assert_eq!(Denied::from_body(&older), Some(Denied::new("no")));
        assert_eq!(
            Denied::from_body(&older).unwrap().for_an_agent(),
            format!("{GENERIC_REFUSAL} (no)")
        );
        assert_eq!(
            Denied::from_body(&serde_json::json!({ "error": "not_found" })),
            None
        );
        assert_eq!(Denied::from_body(&serde_json::json!("text")), None);
    }

    #[test]
    fn a_blank_sentence_is_the_generic_one() {
        assert_eq!(Denied::new("x").saying("  ").sentence(), GENERIC_REFUSAL);
        assert_eq!(Denied::new("x").sentence(), GENERIC_REFUSAL);
    }

    #[tokio::test]
    async fn a_refusal_answers_403_with_the_core_body() {
        let response = Denied::new("projects are read-only")
            .saying("You can't edit Projects.")
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
                "sentence": "You can't edit Projects.",
            })
        );
    }
}
