//! The `identify` middleware (D19): the first thing every request goes through.
//!
//! It asks the extension host who the caller is, once per request, and leaves
//! the [`Actor`] in the request's extensions. A handler reads it with axum's
//! `Extension<Actor>` extractor.
//!
//! A request no extension identified then goes through the extensions'
//! admission (`Extension::admit`): refused, it answers `401` (with the page to
//! sign in on, for a redirection) before any route runs. The admission guards
//! every route, `/ws`, the terminals' PTY and the extensions' routes included,
//! never the SPA fallback: the interface always loads, and follows the
//! redirection its first call to the API gets.
//!
//! It also runs the request in an `OperationApplied` scope: once the response
//! is a success, the extensions observe every Operation the request was
//! allowed to run (D20). And in the Actor's scope ([`pdo_kernel::actor::current`]),
//! so a secondary adapter (the audit journal) knows who did it.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use pdo_extension_api::Admission;
use pdo_kernel::applied;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;
use pdo_session::adapters::http::ws_log;

/// `router`, with the actors' routes, the admission in front of every route
/// (not the fallback), and `identify` in front of everything.
pub(crate) fn with_identify(router: Router, host: Arc<dyn ExtensionHost>) -> Router {
    let router = router.merge(crate::actors::router(host.clone()));
    router
        .route_layer(middleware::from_fn(admit))
        .layer(middleware::from_fn_with_state(host, identify))
}

async fn identify(
    State(host): State<Arc<dyn ExtensionHost>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let identification = host.identify_request(&parts);
    let actor: Actor = identification.actor;
    let mut request = Request::from_parts(parts, body);
    request.extensions_mut().insert(actor.clone());
    request.extensions_mut().insert(identification.admission);
    let (response, record) =
        applied::scope(pdo_kernel::actor::scope(actor, next.run(request))).await;
    let status = response.status();
    if status.is_success() || status == StatusCode::SWITCHING_PROTOCOLS {
        applied::publish(host.as_ref(), record);
    }
    response
}

/// Refuse a route to a request the extensions did not admit. Also layered on
/// every router nested under `/ext` (`extensions::mount`): `route_layer` does
/// not reach the fallbacks a nested router brings.
///
/// A refused WebSocket upgrade has its line in the journal (ADR-0083): the
/// browser learns nothing of the refusal, the operator does.
pub(crate) async fn admit(request: Request, next: Next) -> Response {
    match request.extensions().get::<Admission>().cloned() {
        Some(admission) if !admission.is_admitted() => {
            let refusal = admission
                .into_refusal()
                .unwrap_or_else(|| StatusCode::UNAUTHORIZED.into_response());
            if ws_log::is_upgrade(request.headers()) {
                ws_log::refused(
                    request.uri().path(),
                    refusal.status(),
                    "not admitted",
                    request.headers(),
                );
            }
            refusal
        }
        _ => next.run(request).await,
    }
}
