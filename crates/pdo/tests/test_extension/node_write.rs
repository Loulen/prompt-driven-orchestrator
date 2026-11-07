//! A Node answered through the test extension's `Host` (API 0.23): the
//! core's fixture for `Host::write_to_node` and `node_link`.
//!
//! - `POST /ext/test/runs/<id>/nodes/<node>/write` (`{"text", "iter"?}`)
//!   writes the text into the Node's session on behalf of the caller (the
//!   [`ACTOR_HEADER`](super::ACTOR_HEADER) or the cookie of `act-as`), as if
//!   the caller had typed it and pressed Enter; it answers what was written,
//!   with the Node's deep link (`{"written": NodeWritten, "link"}`), or the
//!   refusal with its status (`{"error", "reason"}`).

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::response::{IntoResponse, Response};
use pdo::extension_api::axum::routing::post;
use pdo::extension_api::axum::{Extension, Json, Router};
use pdo::extension_api::{node_link, Actor, NodeNotWritten, NodeWrite};
use serde_json::{json, Value};

use super::creation::Creation;

fn refused(refusal: NodeNotWritten) -> Response {
    let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({ "error": refusal.message, "reason": refusal.reason })),
    )
        .into_response()
}

pub(super) fn routes(creation: Creation) -> Router {
    Router::new().route(
        "/runs/{id}/nodes/{node}/write",
        post(
            move |Extension(actor): Extension<Actor>,
                  Path((run, node)): Path<(String, String)>,
                  Json(body): Json<Value>| {
                let host = creation.host();
                async move {
                    let Some(host) = host else {
                        return refused(NodeNotWritten::unavailable(
                            "the test extension has no Host yet",
                        ));
                    };
                    let text = body["text"].as_str().unwrap_or_default();
                    let mut write = NodeWrite::new(run, node, text);
                    if let Some(iter) = body["iter"].as_i64() {
                        write = write.at_iter(iter);
                    }
                    match host.write_to_node(&actor, write).await {
                        Ok(written) => {
                            let link =
                                node_link(&written.run_id, &written.node_id, Some(written.iter));
                            Json(json!({ "written": written, "link": link })).into_response()
                        }
                        Err(refusal) => refused(refusal),
                    }
                }
            },
        ),
    )
}
