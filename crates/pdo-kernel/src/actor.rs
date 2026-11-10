//! The Actor of a request, and the core's own rule to identify it when no
//! extension does.

use axum::http::HeaderMap;
use pdo_extension_api::axum;
pub use pdo_extension_api::Actor;

/// Header carrying the Run of the node session a CLI call comes from — the value
/// of `PDO_RUN_ID` in that session (ADR-0064).
pub const SESSION_RUN_HEADER: &str = "X-PDO-Session-Run-Id";
/// Header carrying the node of the session a CLI call comes from — the value of
/// `PDO_NODE_ID` in that session (ADR-0064).
pub const SESSION_NODE_HEADER: &str = "X-PDO-Session-Node-Id";

/// The core's identification, when no extension identified the caller: the
/// agent of a node session when the request carries both session headers (the
/// agent CLI sets them), otherwise the local operator.
///
/// A claim, not a proof: the headers are whatever the caller sends. Whoever
/// relies on the session (the provenance of a child Run, ADR-0064) verifies it.
pub fn actor_from_headers(headers: &HeaderMap) -> Actor {
    match (
        header(headers, SESSION_RUN_HEADER),
        header(headers, SESSION_NODE_HEADER),
    ) {
        (Some(run_id), Some(node_id)) => Actor::Agent {
            run_id: run_id.to_string(),
            node_id: node_id.to_string(),
        },
        _ => Actor::LocalOperator,
    }
}

tokio::task_local! {
    static CURRENT: Actor;
}

/// Run `gesture` as the request of `actor`: [`current`] answers it while the
/// gesture runs on this task. The `identify` middleware does it for every HTTP
/// request.
pub async fn scope<F: std::future::Future>(actor: Actor, gesture: F) -> F::Output {
    CURRENT.scope(actor, gesture).await
}

/// The Actor of the request being served on this task (a secondary adapter
/// recording who did something, the audit journal), `None` outside one (the
/// scheduler, a spawned task).
pub fn current() -> Option<Actor> {
    CURRENT.try_with(Actor::clone).ok()
}

/// A header's trimmed value, `None` when absent, not UTF-8 or blank.
fn header<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[tokio::test]
    async fn the_current_actor_is_the_scope_s_only() {
        assert_eq!(current(), None);
        let alice = Actor::Identified {
            namespace: "x".into(),
            id: "alice".into(),
        };
        assert_eq!(scope(alice.clone(), async { current() }).await, Some(alice));
        assert_eq!(current(), None);
    }

    #[test]
    fn a_request_without_session_comes_from_the_local_operator() {
        assert_eq!(actor_from_headers(&HeaderMap::new()), Actor::LocalOperator);
    }

    #[test]
    fn both_session_headers_make_an_agent() {
        let map = headers(&[(SESSION_RUN_HEADER, "r1"), (SESSION_NODE_HEADER, " n1 ")]);
        assert_eq!(
            actor_from_headers(&map),
            Actor::Agent {
                run_id: "r1".into(),
                node_id: "n1".into()
            }
        );
    }

    #[test]
    fn a_lone_or_blank_session_header_is_no_claim() {
        assert_eq!(
            actor_from_headers(&headers(&[(SESSION_RUN_HEADER, "r1")])),
            Actor::LocalOperator
        );
        assert_eq!(
            actor_from_headers(&headers(&[
                (SESSION_RUN_HEADER, "r1"),
                (SESSION_NODE_HEADER, "  ")
            ])),
            Actor::LocalOperator
        );
    }
}
