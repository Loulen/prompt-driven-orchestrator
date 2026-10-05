use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::{
    Actor, Attributes, CliInvocation, CliOutput, CliSpec, EntityRef, ExtensionSkill, Host,
    Manifest, NeutralHost, Operation, UiContributions,
};

/// The head of an HTTP request, as `identify` sees it: method, URI, headers.
/// Never the body, which a hook must not consume.
pub type RequestParts = axum::http::request::Parts;

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

/// What the core gives an extension when the daemon starts.
#[derive(Clone)]
#[non_exhaustive]
pub struct ExtensionContext {
    /// The extension's own data folder, `~/.pdo/extensions/<id>/`, created
    /// before [`Extension::start`] is called: its database, its files. Nothing
    /// else of the core writes there.
    pub data_dir: PathBuf,
    /// The core's mechanisms, for the extension's own mutations: the
    /// Autorisation d'opération of every extension, the events, the
    /// attributes of the other extensions. Keep it: it serves as long as the
    /// daemon runs.
    pub host: Arc<dyn Host>,
}

impl ExtensionContext {
    /// The context of the data folder `data_dir`, with the [`NeutralHost`]
    /// (everything allowed, events go nowhere, no attribute).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            host: Arc::new(NeutralHost),
        }
    }

    /// The same context, with `host`.
    pub fn with_host(mut self, host: Arc<dyn Host>) -> Self {
        self.host = host;
        self
    }
}

impl fmt::Debug for ExtensionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionContext")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

/// Two contexts are equal when they name the same folder and share the same
/// host.
impl PartialEq for ExtensionContext {
    fn eq(&self, other: &Self) -> bool {
        self.data_dir == other.data_dir
            && std::ptr::addr_eq(Arc::as_ptr(&self.host), Arc::as_ptr(&other.host))
    }
}

impl Eq for ExtensionContext {}

/// A third-party module registered when the daemon starts (ADR-0081).
///
/// Only [`Extension::manifest`] is required. Every hook defaults to the neutral
/// answer: an extension that overrides nothing changes nothing.
///
/// Hooks are called from the daemon's request path, so they must be quick. A
/// panic in `authorize` is a refusal; `on_event` is called off the request path
/// and can never slow the core down.
///
/// When the daemon starts, it calls [`Extension::skills`] once (to seed the
/// Banque de skills, before `start`), then [`Extension::start`] once, then
/// [`Extension::routes`] and [`Extension::ui`], before it serves anything.
pub trait Extension: Send + Sync + 'static {
    /// Who this extension is.
    fn manifest(&self) -> Manifest;

    /// Receive what the core gives the extension (its data folder, its
    /// [`Host`]), once, when the daemon starts. Does nothing by default.
    fn start(&self, _context: &ExtensionContext) {}

    /// Identify the actor behind a request. `None` (the default) lets the next
    /// extension, then the core, decide: the local operator, or the agent of the
    /// node session the request comes from.
    fn identify(&self, _request: &RequestParts) -> Option<Actor> {
        None
    }

    /// Allow or refuse an Operation before it runs. Allows by default. Never
    /// consulted for the scheduler's internal transitions: an extension cannot
    /// freeze a Run in flight.
    ///
    /// Also asked about the reads ([`Verb::Read`](crate::Verb::Read), CORE-5):
    /// once per Run a list holds, per Run event `/ws` relays to a connection
    /// (the answer kept a few seconds), per Projet listed, per disk browse,
    /// and for the question of authorization (`POST /authorize`), which asks
    /// without running anything. A refused read hides the entity from a list
    /// and from `/ws`, and answers `403` for one entity: refuse a read on its
    /// resource and verb, never on a verb you do not know.
    fn authorize(&self, _actor: &Actor, _operation: &Operation) -> Authorization {
        Authorization::Allow
    }

    /// The attributes to stamp on an entity the actor just created: a Run, a
    /// Projet, an agentic profile, a Trigger, a pipeline of the library. The
    /// core prefixes each key with the extension's id (`<id>/<key>`), stores
    /// them outside the event log, returns them with the entity and filters
    /// the entity lists on them (`?attr.<id>/<key>=<value>`). None by default.
    ///
    /// Also called for the entities another extension creates
    /// ([`ResourceKind::Extension`](crate::ResourceKind::Extension), through
    /// its [`Host::on_create`]): that extension stores the prefixed attributes
    /// and returns them with its entities.
    fn on_create(&self, _actor: &Actor, _entity: &EntityRef) -> Attributes {
        Attributes::new()
    }

    /// Observe an event: exactly the JSON the daemon broadcasts on `/ws`, which
    /// carries an [`OperationApplied`](crate::OperationApplied) after each
    /// authorized and successful mutation, and the events the other extensions
    /// publish ([`ExtensionEvent`](crate::ExtensionEvent); never the
    /// extension's own). Ignores everything by default.
    ///
    /// Called on the extension's own thread, in the order the events came: a
    /// slow `on_event` delays only this extension's next events, never the
    /// core nor another extension. A panic is logged and the next event is
    /// delivered as usual.
    fn on_event(&self, _event: &serde_json::Value) {}

    /// Routes served under `/ext/<id>/`: the router is nested there, so it
    /// declares its paths from `/` (its `/` answers `/ext/<id>`, its `/events`
    /// `/ext/<id>/events`). It can never hide a route of the core. None by
    /// default.
    ///
    /// The whole `/ext/` prefix is the API's, never the SPA's, whatever the
    /// method: a path the router does not serve answers `404`
    /// `{"error": "not_found", "message", "id"}`; without routes (`None`, or a
    /// panic here), and for any id no loaded extension has, `/ext/<id>` and
    /// everything under it answer `404`
    /// `{"error": "extension_not_found", "message", "id"}`.
    fn routes(&self) -> Option<axum::Router> {
        None
    }

    /// Contributions to the interface. None by default.
    fn ui(&self) -> Option<UiContributions> {
        None
    }

    /// The command line of the extension, `pdo ext <id> <args…>`. `None` (the
    /// default): no command line, and `pdo ext <id>` exits
    /// [`EXIT_NO_CLI`](crate::EXIT_NO_CLI).
    fn cli(&self) -> Option<CliSpec> {
        None
    }

    /// Run one `pdo ext <id> <args…>`, when [`Extension::cli`] declares a
    /// command line. The CLI of the core relays the arguments, the standard
    /// input and the caller's Actor (the agent of a node session, the local
    /// operator otherwise), then writes the answer's stdout and stderr byte
    /// for byte and exits with its code.
    ///
    /// Called on a blocking thread of the daemon's tokio runtime, never on its
    /// request path: it may take its time, and drive an async call with
    /// `tokio::runtime::Handle::current().block_on`. A panic answers exit code
    /// `1`, like a daemon that gives no verdict. By default, exit `1` with a
    /// message: declaring a command line without running it is a bug.
    fn run_cli(&self, _invocation: &CliInvocation) -> CliOutput {
        CliOutput::failure(
            1,
            "this extension declares a command line but does not run it\n",
        )
    }

    /// The skills this extension brings, seeded into the Banque de skills
    /// once, when the daemon starts: called **before** [`Extension::start`],
    /// so the list must not depend on it (embed the files). A skill the list
    /// no longer holds is removed from the bank. None by default.
    fn skills(&self) -> Vec<ExtensionSkill> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceKind, Verb};

    /// An extension that overrides nothing but its manifest.
    struct Bare;

    impl Extension for Bare {
        fn manifest(&self) -> Manifest {
            Manifest::new("bare", "Bare", "0.0.0")
        }
    }

    #[test]
    fn every_hook_defaults_to_the_neutral_answer() {
        let ext = Bare;
        let (parts, ()) = axum::http::Request::builder()
            .uri("/runs")
            .body(())
            .unwrap()
            .into_parts();
        let actor = Actor::LocalOperator;
        let op = Operation::new(ResourceKind::Project, Verb::Update).on("p1");

        assert_eq!(ext.identify(&parts), None);
        assert_eq!(ext.authorize(&actor, &op), Authorization::Allow);
        assert!(ext
            .on_create(&actor, &EntityRef::new(ResourceKind::Project, "p1"))
            .is_empty());
        ext.start(&ExtensionContext::new("/tmp/bare"));
        assert!(ExtensionContext::new("/tmp/bare")
            .host
            .authorize(&actor, &op)
            .is_allowed());
        ext.on_event(&serde_json::json!({ "kind": "RunCreated" }));
        assert!(ext.routes().is_none());
        assert!(ext.ui().is_none());
        assert!(ext.cli().is_none());
        assert_eq!(
            ext.run_cli(&CliInvocation::new(vec![], "", actor.clone()))
                .code,
            1
        );
        assert!(ext.skills().is_empty());
        assert_eq!(ext.manifest().id, "bare");
    }

    #[test]
    fn extensions_are_object_safe() {
        let extensions: Vec<Box<dyn Extension>> = vec![Box::new(Bare)];
        assert_eq!(extensions.len(), 1);
    }

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

    #[test]
    fn a_context_carries_the_neutral_host_until_given_one() {
        let context = ExtensionContext::new("/tmp/x");
        assert_eq!(context, context.clone());
        let other = ExtensionContext::new("/tmp/x").with_host(Arc::new(NeutralHost));
        assert_ne!(context, other, "another host");
        assert!(format!("{context:?}").contains("/tmp/x"));
    }
}
