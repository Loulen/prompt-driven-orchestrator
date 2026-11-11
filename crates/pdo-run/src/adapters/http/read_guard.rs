//! The guard of a Run's reads (CORE-5): every `GET /runs/{run_id}…` of the
//! daemon (the Run's detail, its events, its nodes, its artifacts, its diff,
//! its review comments, a node's pane…) answers `403` when the caller may not
//! read the Run.
//!
//! A concealed refusal ([`Denied::is_concealed`]) answers instead exactly what
//! the route answers for a Run that does not exist: the request is routed
//! again, unguarded, under an id no Run has, and that id is written back as
//! the one asked in the answer. Same status, same body, whatever the route:
//! the caller cannot tell the Run exists.
//!
//! A layer of the whole router, not a check in each handler: the routes under
//! `/runs/{run_id}` belong to several contexts (run, session, review), and a
//! read added tomorrow is guarded without anyone remembering to. It runs
//! inside `identify`, so the Actor is known. The lists (`GET /runs`,
//! `/runs/reapable`, `/runs/presences`) are filtered by their use case
//! instead.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, Method, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use pdo_kernel::authorization::Denied;
use pdo_kernel::Actor;
use tower::ServiceExt;

use crate::Runs;

/// What the guard reads: the Runs, and the router it guards, to route a
/// concealed read again (a layer runs after the routing: the route already
/// holds the id asked).
#[derive(Clone)]
struct Guard {
    runs: Arc<Runs>,
    unguarded: Router,
}

/// `router`, each read of one Run guarded.
pub fn guard_run_reads(router: Router, runs: Arc<Runs>) -> Router {
    let guard_state = Guard {
        runs,
        unguarded: router.clone(),
    };
    router.layer(middleware::from_fn_with_state(guard_state, guard))
}

/// The lists under `/runs/`, which no Run id names.
const LISTS: [&str; 3] = ["reapable", "presences", "costs"];

async fn guard(State(guard): State<Guard>, request: Request, next: Next) -> Response {
    let Guard { runs, unguarded } = guard;
    if let Some(segment) = read_of_run(request.method(), request.uri().path()) {
        let actor = request
            .extensions()
            .get::<Actor>()
            .cloned()
            .unwrap_or(Actor::LocalOperator);
        // The id the route reads: the segment percent-decoded, as `Path` does.
        let run_id = decoded(segment);
        if let Err(denied) = runs.authorize_read(&actor, &run_id).await {
            let segment = segment.to_string();
            return refused(denied, (&segment, &run_id), &actor, request, unguarded).await;
        }
    }
    next.run(request).await
}

/// The largest answer about an unknown Run the guard rewrites: such answers
/// are a few bytes (`run not found`, `[]`).
const UNKNOWN_ANSWER_LIMIT: usize = 1 << 20;

/// `segment` percent-decoded; as it is when it is not UTF-8 once decoded
/// (the route then refuses it).
fn decoded(segment: &str) -> String {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .map_or_else(|_| segment.to_string(), |id| id.into_owned())
}

/// The answer to `actor`'s read of the Run `(segment, run_id)` (as the path
/// writes it, as the route reads it) that `denied` refuses: the refusal, or
/// when it is concealed, what `unguarded` answers about a Run that does not
/// exist.
async fn refused(
    denied: Denied,
    (segment, run_id): (&str, &str),
    actor: &Actor,
    request: Request,
    unguarded: Router,
) -> Response {
    if !denied.is_concealed() {
        return denied.into_response();
    }
    let suffix = format!("-{}", uuid::Uuid::new_v4().simple());
    let Some(uri) = with_run_id(request.uri(), segment, &format!("{segment}{suffix}")) else {
        return denied.into_response();
    };
    let unknown = format!("{run_id}{suffix}");
    // A new request: the first routing left its path's parameters in the
    // extensions, which a second routing would add to. Only the Actor goes on.
    let (parts, body) = request.into_parts();
    let mut again = Request::new(body);
    *again.method_mut() = parts.method;
    *again.uri_mut() = uri;
    *again.version_mut() = parts.version;
    *again.headers_mut() = parts.headers;
    again.extensions_mut().insert(actor.clone());
    let answer = match unguarded.oneshot(again).await {
        Ok(answer) => answer,
        Err(never) => match never {},
    };
    named_back(answer, &unknown, run_id).await
}

/// `uri`, its `/runs/{from}` segment renamed `to`.
fn with_run_id(uri: &Uri, from: &str, to: &str) -> Option<Uri> {
    let rest = uri.path().strip_prefix("/runs/")?.strip_prefix(from)?;
    let mut renamed = format!("/runs/{to}{rest}");
    if let Some(query) = uri.query() {
        renamed.push('?');
        renamed.push_str(query);
    }
    renamed.parse().ok()
}

/// `answer`, every `unknown` of its body written `asked`: the answer about
/// an unknown Run named as the caller named it.
async fn named_back(answer: Response, unknown: &str, asked: &str) -> Response {
    let (mut parts, body) = answer.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, UNKNOWN_ANSWER_LIMIT).await else {
        return Denied::new("unreadable answer").into_response();
    };
    let body = match std::str::from_utf8(&bytes) {
        Ok(text) if text.contains(unknown) => Body::from(text.replace(unknown, asked)),
        _ => Body::from(bytes),
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, body)
}

/// The Run a request reads: `{run_id}` of a `GET` or `HEAD` of
/// `/runs/{run_id}` or below. `None` for the lists (`/runs`,
/// `/runs/reapable`, `/runs/presences`) and every other request.
fn read_of_run<'a>(method: &Method, path: &'a str) -> Option<&'a str> {
    if method != Method::GET && method != Method::HEAD {
        return None;
    }
    let rest = path.strip_prefix("/runs/")?;
    let run_id = rest.split('/').next()?;
    (!run_id.is_empty() && !LISTS.contains(&run_id)).then_some(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_of_one_run_names_it() {
        let get = Method::GET;
        assert_eq!(read_of_run(&get, "/runs/r1"), Some("r1"));
        assert_eq!(read_of_run(&get, "/runs/r1/nodes/n1/pane"), Some("r1"));
        assert_eq!(read_of_run(&get, "/runs/r1/diff/structured"), Some("r1"));
        assert_eq!(read_of_run(&Method::HEAD, "/runs/r1/artifact"), Some("r1"));
    }

    #[test]
    fn the_guard_reads_the_id_the_route_reads() {
        assert_eq!(decoded("r1"), "r1");
        assert_eq!(decoded("%32026-r1"), "2026-r1");
        assert_eq!(decoded("%FF"), "%FF", "not UTF-8: the route refuses it");
    }

    #[test]
    fn a_concealed_read_goes_on_under_another_id() {
        let uri: Uri = "/runs/r1/nodes/n1/pane?lines=5".parse().unwrap();
        assert_eq!(
            with_run_id(&uri, "r1", "r1-x").unwrap(),
            "/runs/r1-x/nodes/n1/pane?lines=5"
        );
        let uri: Uri = "/runs/r1".parse().unwrap();
        assert_eq!(with_run_id(&uri, "r1", "r1-x").unwrap(), "/runs/r1-x");
    }

    #[tokio::test]
    async fn the_answer_names_the_run_as_the_caller_did() {
        let answer = (
            axum::http::StatusCode::NOT_FOUND,
            "run r1-x not found (r1-x)",
        )
            .into_response();
        let answer = named_back(answer, "r1-x", "r1").await;
        assert_eq!(answer.status(), axum::http::StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(answer.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"run r1 not found (r1)");
    }

    #[test]
    fn a_list_a_mutation_or_another_path_is_not_guarded_here() {
        assert_eq!(read_of_run(&Method::GET, "/runs"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/reapable"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/presences"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/costs"), None);
        assert_eq!(read_of_run(&Method::POST, "/runs/r1/commands"), None);
        assert_eq!(read_of_run(&Method::DELETE, "/runs/r1"), None);
        assert_eq!(read_of_run(&Method::GET, "/runsx/r1"), None);
        assert_eq!(read_of_run(&Method::GET, "/projects/p1"), None);
    }
}
