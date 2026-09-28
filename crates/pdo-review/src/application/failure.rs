//! Why a use case of the review answers something else than its result: the
//! status and the body the route sends, unchanged since the handlers lived in
//! the legacy crate.

use pdo_kernel::authorization::Denied;
use serde_json::{json, Value};

/// A refusal or a breakdown, with the body the route answers.
#[derive(Debug)]
pub enum Failure {
    /// A `text/plain` body.
    Text(u16, String),
    /// A JSON body.
    Json(u16, Value),
    /// An extension refused the Operation: `403 operation_refused`.
    Refused(Denied),
}

impl Failure {
    /// `{"error": message}` with `status`.
    pub fn error(status: u16, message: impl Into<String>) -> Failure {
        Failure::Json(status, json!({ "error": message.into() }))
    }

    /// The status the route answers.
    pub fn status(&self) -> u16 {
        match self {
            Failure::Text(status, _) | Failure::Json(status, _) => *status,
            Failure::Refused(_) => 403,
        }
    }
}

impl From<Denied> for Failure {
    fn from(denied: Denied) -> Self {
        Failure::Refused(denied)
    }
}

/// A use case's answer.
pub type Outcome<T> = Result<T, Failure>;
