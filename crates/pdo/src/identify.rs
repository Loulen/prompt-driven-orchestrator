//! The `identify` middleware (D19): the first thing every request goes through.
//!
//! It asks the extension host who the caller is, once per request, and leaves
//! the [`Actor`] in the request's extensions. A handler reads it with axum's
//! `Extension<Actor>` extractor.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
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
    next.run(request).await
}
