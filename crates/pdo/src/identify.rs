//! The `identify` middleware (D19): the first thing every request goes through.
//!
//! It asks the extension host who the caller is, once per request, and leaves
//! the [`Actor`] in the request's extensions. A handler reads it with axum's
//! `Extension<Actor>` extractor.
//!
//! It also runs the request in an `OperationApplied` scope: once the response
//! is a success, the extensions observe every Operation the request was
//! allowed to run (D20).

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use pdo_kernel::applied;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;

/// `router`, with `identify` in front of every route, the fallback included.
pub(crate) fn with_identify(router: Router, host: Arc<dyn ExtensionHost>) -> Router {
    router.layer(middleware::from_fn_with_state(host, identify))
}

async fn identify(
    State(host): State<Arc<dyn ExtensionHost>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let actor: Actor = host.identify(&parts);
    let mut request = Request::from_parts(parts, body);
    request.extensions_mut().insert(actor);
    let (response, record) = applied::scope(next.run(request)).await;
    let status = response.status();
    if status.is_success() || status == StatusCode::SWITCHING_PROTOCOLS {
        applied::publish(host.as_ref(), record);
    }
    response
}
