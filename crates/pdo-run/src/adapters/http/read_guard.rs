//! The guard of a Run's reads (CORE-5): every `GET /runs/{run_id}…` of the
//! daemon (the Run's detail, its events, its nodes, its artifacts, its diff,
//! its review comments, a node's pane…) answers `403` when the caller may not
//! read the Run.
//!
//! A layer of the whole router, not a check in each handler: the routes under
//! `/runs/{run_id}` belong to several contexts (run, session, review), and a
//! read added tomorrow is guarded without anyone remembering to. It runs
//! inside `identify`, so the Actor is known. The lists (`GET /runs`,
//! `/runs/reapable`, `/runs/presences`) are filtered by their use case
//! instead.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use pdo_kernel::Actor;

use crate::Runs;

/// `router`, each read of one Run guarded.
pub fn guard_run_reads(router: Router, runs: Arc<Runs>) -> Router {
    router.layer(middleware::from_fn_with_state(runs, guard))
}

/// The lists under `/runs/`, which no Run id names.
const LISTS: [&str; 2] = ["reapable", "presences"];

async fn guard(State(runs): State<Arc<Runs>>, request: Request, next: Next) -> Response {
    if let Some(run_id) = read_of_run(request.method(), request.uri().path()) {
        let actor = request
            .extensions()
            .get::<Actor>()
            .cloned()
            .unwrap_or(Actor::LocalOperator);
        if let Err(denied) = runs.authorize_read(&actor, run_id).await {
            return denied.into_response();
        }
    }
    next.run(request).await
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
    fn a_list_a_mutation_or_another_path_is_not_guarded_here() {
        assert_eq!(read_of_run(&Method::GET, "/runs"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/reapable"), None);
        assert_eq!(read_of_run(&Method::GET, "/runs/presences"), None);
        assert_eq!(read_of_run(&Method::POST, "/runs/r1/commands"), None);
        assert_eq!(read_of_run(&Method::DELETE, "/runs/r1"), None);
        assert_eq!(read_of_run(&Method::GET, "/runsx/r1"), None);
        assert_eq!(read_of_run(&Method::GET, "/projects/p1"), None);
    }
}
