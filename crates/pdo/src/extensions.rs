//! The extensions' surface on HTTP: each extension's routes, nested under
//! `/ext/<id>/`, and `GET /ext`, the index the front reads to fill its UI slots.
//!
//! Every path of an extension sits under its own `/ext/<id>` prefix, so an
//! extension can never hide a route of the core, nor another extension's.
//!
//! The whole `/ext/` prefix belongs to the API and never falls through to the
//! SPA, whatever the method (#22):
//!
//! - `/ext/<id>` and `/ext/<id>/…`, when no extension `<id>` is loaded, or it is
//!   loaded but serves no route: `404`
//!   `{"error": "extension_not_found", "message", "id"}`;
//! - `/ext/<id>/<path>` of a loaded extension that does not serve `<path>`:
//!   `404` `{"error": "not_found", "message", "id"}`.
//!
//! A client tells "extension absent" from a real answer by that slug.
//!
//! ROUTER TRAP: `/ext` is a top-level prefix of the vite dev proxy whitelist
//! (`frontend/vite.config.ts`).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{OriginalUri, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use pdo_extension_api::{Slot, UiContributions};
use pdo_kernel::attributes::wire_kind;
use pdo_kernel::extension_host::Extensions;
use serde::Serialize;
use ts_rs::TS;

/// One registered extension, as the front sees it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// What it adds to the interface; absent when nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ui: Option<ExtensionUi>,
}

/// An extension's bundle and the slots it fills.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExtensionUi {
    /// Where the bundle is served; no bundle, no slot rendered.
    pub bundle_url: Option<String>,
    /// The major of React the bundle was built against: a bundle whose major
    /// differs from the front's is never loaded.
    pub react_major: Option<u32>,
    pub slots: Vec<ExtensionSlot>,
}

/// The UI slots of the extension API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SlotKind {
    Navigation,
    SettingsPanel,
    ListFilter,
    DetailTab,
}

/// One element an extension adds in one slot.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExtensionSlot {
    pub slot: SlotKind,
    /// The kind of entity of a list filter or a detail tab (`run`, `project`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resource: Option<String>,
    pub id: String,
    pub label: String,
    /// The name the bundle exports the component under.
    pub component: String,
}

/// The front's view of an extension's contributions. A slot this build does
/// not know is left out.
fn ui_entry(ui: UiContributions) -> ExtensionUi {
    let slots = ui
        .slots
        .into_iter()
        .filter_map(|slot| {
            let (kind, resource) = match slot.slot {
                Slot::Navigation => (SlotKind::Navigation, None),
                Slot::SettingsPanel => (SlotKind::SettingsPanel, None),
                Slot::ListFilter { resource } => (SlotKind::ListFilter, Some(wire_kind(resource))),
                Slot::DetailTab { resource } => (SlotKind::DetailTab, Some(wire_kind(resource))),
                _ => return None,
            };
            Some(ExtensionSlot {
                slot: kind,
                resource,
                id: slot.id,
                label: slot.label,
                component: slot.component,
            })
        })
        .collect();
    ExtensionUi {
        bundle_url: ui.bundle_url,
        react_major: ui.react_major,
        slots,
    }
}

/// Every extension, in registration order.
fn entries(extensions: &Extensions) -> Vec<ExtensionEntry> {
    extensions
        .iter()
        .map(|(manifest, extension)| ExtensionEntry {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            ui: std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extension.ui()))
                .unwrap_or_else(|_| {
                    tracing::warn!("extension `{}` panicked in ui", manifest.id);
                    None
                })
                .map(ui_entry),
        })
        .collect()
}

/// The core's routes of the extensions: `GET /ext`.
pub fn router(extensions: Extensions) -> Router {
    Router::new()
        .route("/ext", get(index))
        .with_state(extensions)
}

/// `GET /ext`: the registered extensions and their UI contributions; `[]`
/// without extension. Read on every call: an extension may change what it
/// contributes while the daemon runs.
async fn index(State(extensions): State<Extensions>) -> Response {
    Json(entries(&extensions)).into_response()
}

/// `router`, with the core's `/ext` routes and each extension's routes nested
/// under `/ext/<id>`. The rest of the prefix answers `404` JSON: a path a loaded
/// extension does not serve (`not_found`), and any `/ext/<id>…` of an id no
/// loaded extension serves routes for (`extension_not_found`).
pub(crate) fn mount(router: Router, extensions: &Extensions) -> Router {
    let mut router = router.merge(self::router(extensions.clone()));
    // Each loaded extension's id, and whether it serves routes.
    let mut serves = BTreeMap::new();
    for (manifest, extension) in extensions.iter() {
        let routes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extension.routes()))
            .unwrap_or_else(|_| {
                tracing::error!("extension `{}` panicked in routes", manifest.id);
                None
            });
        let id = manifest.id.clone();
        serves.insert(id.clone(), routes.is_some());
        if let Some(routes) = routes {
            let prefix = format!("/ext/{id}");
            let unserved =
                move |OriginalUri(uri): OriginalUri| async move { not_found(&id, uri.path()) };
            router = router.nest(&prefix, routes.fallback(unserved));
        }
    }
    // What no route under `/ext` claims. A fallback, not a `/ext/{id}/{*rest}`
    // route: axum keeps nested fallbacks in their own router, consulted only
    // once no route matched, so a route here would win over every extension's
    // `not_found` above. `/ext/` matches no fallback's `{*rest}`, hence its route.
    let serves = Arc::new(serves);
    let absent = move |OriginalUri(uri): OriginalUri| async move {
        let path = uri.path();
        let id = ext_id(path);
        match serves.get(id) {
            Some(true) => not_found(id, path),
            Some(false) => extension_not_found(id, true),
            None => extension_not_found(id, false),
        }
    };
    router
        .route("/ext/", any(absent.clone()))
        .nest("/ext", Router::new().fallback(absent))
}

/// The `<id>` of `/ext/<id>…`; empty for `/ext/`.
fn ext_id(path: &str) -> &str {
    let rest = path.strip_prefix("/ext/").unwrap_or_default();
    rest.split('/').next().unwrap_or_default()
}

/// `404` for `/ext/<id>…` when no loaded extension `id` serves routes.
fn extension_not_found(id: &str, loaded: bool) -> Response {
    let message = if id.is_empty() {
        "`/ext/` names no extension".to_string()
    } else if loaded {
        format!("extension `{id}` serves no route")
    } else {
        format!("no extension `{id}` in this daemon")
    };
    let body = serde_json::json!({ "error": "extension_not_found", "message": message, "id": id });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

/// `404` for a `path` the loaded extension `id` does not serve.
fn not_found(id: &str, path: &str) -> Response {
    let message = format!("extension `{id}` serves no `{path}`");
    let body = serde_json::json!({ "error": "not_found", "message": message, "id": id });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Method, Request};
    use pdo_extension_api::{Extension, Manifest, ResourceKind, UiSlot};
    use serde_json::Value;
    use tower::util::ServiceExt;

    struct Contributes;
    impl Extension for Contributes {
        fn manifest(&self) -> Manifest {
            Manifest::new("demo", "Demo", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            Some(
                UiContributions::bundle("/ext/demo/ui.js", 19)
                    .with(UiSlot::new(Slot::Navigation, "events", "Events", "Events"))
                    .with(UiSlot::new(
                        Slot::DetailTab {
                            resource: ResourceKind::AgentProfile,
                        },
                        "tab",
                        "Tab",
                        "Tab",
                    )),
            )
        }
    }

    struct Silent;
    impl Extension for Silent {
        fn manifest(&self) -> Manifest {
            Manifest::new("silent", "Silent", "0.1.0")
        }
    }

    #[test]
    fn the_index_lists_each_extension_and_its_slots() {
        let extensions =
            Extensions::register(vec![Box::new(Contributes), Box::new(Silent)]).unwrap();
        assert_eq!(
            serde_json::to_value(entries(&extensions)).unwrap(),
            serde_json::json!([
                {
                    "id": "demo", "name": "Demo", "version": "1.0.0",
                    "ui": {
                        "bundle_url": "/ext/demo/ui.js",
                        "react_major": 19,
                        "slots": [
                            { "slot": "navigation", "id": "events", "label": "Events", "component": "Events" },
                            {
                                "slot": "detail_tab", "resource": "agent_profile", "id": "tab",
                                "label": "Tab", "component": "Tab"
                            },
                        ],
                    },
                },
                { "id": "silent", "name": "Silent", "version": "0.1.0" },
            ])
        );
    }

    /// Serves `/` and `/items` under `/ext/serves/`.
    struct Serves;
    impl Extension for Serves {
        fn manifest(&self) -> Manifest {
            Manifest::new("serves", "Serves", "1.0.0")
        }
        fn routes(&self) -> Option<Router> {
            Some(
                Router::new()
                    .route("/", get(|| async { "root" }))
                    .route("/items", get(|| async { "items" })),
            )
        }
    }

    /// Panics when asked for its routes.
    struct PanicsInRoutes;
    impl Extension for PanicsInRoutes {
        fn manifest(&self) -> Manifest {
            Manifest::new("broken", "Broken", "1.0.0")
        }
        fn routes(&self) -> Option<Router> {
            panic!("no routes today")
        }
    }

    /// The router as the daemon assembles it: the SPA fallback, then `mount`.
    fn app(extensions: Vec<Box<dyn Extension>>) -> Router {
        let extensions = Extensions::register(extensions).unwrap();
        mount(
            Router::new().fallback(crate::spa::static_handler),
            &extensions,
        )
    }

    /// `(status, content-type, body)`; the body is `Value::Null` when not JSON.
    async fn call(app: &Router, method: Method, uri: &str) -> (u16, String, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(request).await.unwrap();
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, content_type, body)
    }

    /// Asserts `uri` answers `404` JSON with `slug`, naming `id`.
    async fn assert_404(app: &Router, method: Method, uri: &str, slug: &str, id: &str) -> Value {
        let (status, content_type, body) = call(app, method.clone(), uri).await;
        assert_eq!(status, 404, "{method} {uri}: {body}");
        assert_eq!(content_type, "application/json", "{method} {uri}");
        assert_eq!(body["error"], slug, "{method} {uri}: {body}");
        assert_eq!(body["id"], id, "{method} {uri}: {body}");
        assert!(body["message"].is_string(), "{method} {uri}: {body}");
        body
    }

    #[tokio::test]
    async fn an_extension_without_routes_answers_404_json_under_its_prefix() {
        let app = app(vec![Box::new(Silent), Box::new(PanicsInRoutes)]);
        for uri in [
            "/ext/silent",
            "/ext/silent/",
            "/ext/silent/x",
            "/ext/silent/a/b",
        ] {
            let body = assert_404(&app, Method::GET, uri, "extension_not_found", "silent").await;
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("serves no route"),
                "{body}"
            );
        }
        assert_404(
            &app,
            Method::POST,
            "/ext/silent/x",
            "extension_not_found",
            "silent",
        )
        .await;
        assert_404(
            &app,
            Method::GET,
            "/ext/broken/x",
            "extension_not_found",
            "broken",
        )
        .await;
    }

    #[tokio::test]
    async fn an_unknown_id_answers_404_json_for_every_method() {
        let app = app(vec![Box::new(Serves)]);
        for method in [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::DELETE,
        ] {
            for uri in ["/ext/hello", "/ext/hello/", "/ext/hello/api/x"] {
                let (status, content_type, _) = call(&app, method.clone(), uri).await;
                assert_eq!(status, 404, "{method} {uri}");
                assert_eq!(content_type, "application/json", "{method} {uri}");
            }
        }
        let body = assert_404(
            &app,
            Method::GET,
            "/ext/hello",
            "extension_not_found",
            "hello",
        )
        .await;
        assert_eq!(body["message"], "no extension `hello` in this daemon");
        // An id sharing a prefix with a loaded one is still unknown.
        assert_404(
            &app,
            Method::GET,
            "/ext/servesx/items",
            "extension_not_found",
            "servesx",
        )
        .await;
        assert_404(&app, Method::GET, "/ext/", "extension_not_found", "").await;
    }

    #[tokio::test]
    async fn a_loaded_extension_keeps_its_routes_and_its_404() {
        let app = app(vec![Box::new(Serves)]);
        assert_eq!(call(&app, Method::GET, "/ext/serves").await.0, 200);
        assert_eq!(call(&app, Method::GET, "/ext/serves/items").await.0, 200);
        let body = assert_404(&app, Method::GET, "/ext/serves/nope", "not_found", "serves").await;
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("/ext/serves/nope"),
            "{body}"
        );
        assert_404(
            &app,
            Method::POST,
            "/ext/serves/nope/deeper",
            "not_found",
            "serves",
        )
        .await;
        // No `{*rest}` matches an empty tail: the core's catch-all still knows it.
        assert_404(&app, Method::GET, "/ext/serves/", "not_found", "serves").await;
    }

    #[tokio::test]
    async fn the_index_and_the_spa_are_untouched() {
        let app = app(vec![]);
        let (status, content_type, body) = call(&app, Method::GET, "/ext").await;
        assert_eq!((status, content_type.as_str()), (200, "application/json"));
        assert_eq!(body, serde_json::json!([]));
        for uri in ["/", "/runs/abc/review", "/definitely-not-a-route"] {
            let (status, content_type, _) = call(&app, Method::GET, uri).await;
            assert_eq!(status, 200, "{uri}");
            assert!(
                content_type.starts_with("text/html"),
                "{uri}: {content_type}"
            );
        }
    }
}
