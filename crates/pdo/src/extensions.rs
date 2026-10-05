//! The extensions' surface on HTTP: each extension's routes, nested under
//! `/ext/<id>/`, `GET /ext`, the index the front reads to fill the
//! catalogue's UI locations (`docs/reference/extension-ui.md`), and
//! `POST /ext-cli/<id>`, the relay of `pdo ext <id>` (`extension_cli.rs`).
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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::extract::{OriginalUri, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use pdo_extension_api::{
    HostedLocation, UiCommand, UiContribution, UiContributions, UiElement, UiElementKind,
    UiLocation,
};
use pdo_kernel::extension_host::{ExtensionHost, Extensions};
use serde::Serialize;
use ts_rs::TS;

use crate::extension_cli::{self, ExtensionCli};

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
    /// Its command line, `pdo ext <id>`; absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cli: Option<ExtensionCli>,
}

/// An extension's bundle, what it adds to the catalogue's locations, the
/// commands it exports and the locations it hosts.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExtensionUi {
    /// Where the bundle is served; no bundle, nothing rendered.
    pub bundle_url: Option<String>,
    /// The major of React the bundle was built against: a bundle whose major
    /// differs from the front's is never loaded.
    pub react_major: Option<u32>,
    /// The major of the shared module `@pdo/extension-ui` the bundle imports,
    /// checked as React's; `null` when it does not import it.
    pub ui_module_major: Option<u32>,
    /// Its contributions, the 0.1 slots already mapped onto their locations,
    /// in declaration order.
    pub contributions: Vec<UiContribution>,
    pub commands: Vec<UiCommand>,
    /// The locations it hosts, in declaration order.
    pub locations: Vec<HostedLocation>,
}

/// Why the daemon leaves out extension `host`'s declaration of `location`, if
/// it does: `declared` holds the ids it already declared.
fn refused_location(
    host: &str,
    location: &HostedLocation,
    declared: &BTreeSet<&str>,
) -> Option<String> {
    let name = location
        .id
        .strip_prefix(host)
        .and_then(|rest| rest.strip_prefix('/'));
    let valid_name = name.is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });
    if !valid_name {
        Some(format!(
            "its id must be `{host}/<name>`, the name in lowercase ASCII letters, digits and `-`"
        ))
    } else if UiLocation::ALL.iter().any(|core| core.id() == location.id) {
        Some("it is a location of the core".to_string())
    } else if declared.contains(location.id) {
        Some("it is already declared".to_string())
    } else if location.accepts == UiElementKind::Page {
        Some("a hosted location cannot accept pages".to_string())
    } else {
        None
    }
}

/// The locations extension `id` hosts, its refused declarations left out
/// with a warning.
fn hosted_locations(id: &str, ui: &UiContributions) -> Vec<HostedLocation> {
    let mut declared = BTreeSet::new();
    ui.locations
        .iter()
        .filter(|location| {
            let refused = refused_location(id, location, &declared);
            match &refused {
                Some(why) => tracing::warn!(
                    "extension `{id}`: hosted location `{}` left out: {why}",
                    location.id
                ),
                None => {
                    declared.insert(location.id);
                }
            }
            refused.is_none()
        })
        .cloned()
        .collect()
}

/// The front's view of extension `id`'s contributions ([`UiContributions::catalogue`]),
/// `hosted` being every location the loaded extensions host.
///
/// Left out, with a warning: an element its location does not accept, an
/// action naming no declared command, an id already taken. Left out without
/// one: a contribution to a hosted location no loaded extension declares (its
/// host is simply not installed).
fn ui_entry(
    id: &str,
    ui: UiContributions,
    locations: Vec<HostedLocation>,
    hosted: &BTreeMap<&'static str, UiElementKind>,
) -> ExtensionUi {
    let mut seen = BTreeSet::new();
    let contributions = ui
        .catalogue()
        .into_iter()
        .filter(|c| {
            let accepted = match c.location {
                UiLocation::Hosted(location) => match hosted.get(location) {
                    None => {
                        tracing::debug!(
                            "extension `{id}`: contribution `{}` ignored: no loaded extension \
                             hosts `{location}`",
                            c.id
                        );
                        return false;
                    }
                    Some(kind) => *kind == c.element.kind(),
                },
                location => location.accepts(&c.element),
            };
            let refused = if !accepted {
                Some(format!("`{}` accepts no such element", c.location.id()))
            } else if !seen.insert(c.id.clone()) {
                Some("its id is already taken".to_string())
            } else if let UiElement::Action { command } = &c.element {
                (!ui.commands.iter().any(|k| &k.id == command)).then(|| {
                    format!("it calls `{command}`, a command the extension does not declare")
                })
            } else {
                None
            };
            if let Some(why) = &refused {
                tracing::warn!("extension `{id}`: contribution `{}` left out: {why}", c.id);
            }
            refused.is_none()
        })
        .collect();
    ExtensionUi {
        bundle_url: ui.bundle_url,
        react_major: ui.react_major,
        ui_module_major: ui.ui_module_major,
        contributions,
        commands: ui.commands,
        locations,
    }
}

/// Every extension, in registration order.
fn entries(extensions: &Extensions) -> Vec<ExtensionEntry> {
    // Every extension's UI first: a contribution's host may register after it.
    let uis: Vec<_> = extensions
        .iter()
        .map(|(manifest, extension)| {
            let ui = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extension.ui()))
                .unwrap_or_else(|_| {
                    tracing::warn!("extension `{}` panicked in ui", manifest.id);
                    None
                })
                .map(|ui| {
                    let locations = hosted_locations(&manifest.id, &ui);
                    (ui, locations)
                });
            let cli =
                extension_cli::spec(&manifest.id, extension.as_ref()).map(|spec| ExtensionCli {
                    summary: spec.summary,
                });
            (manifest, ui, cli)
        })
        .collect();
    let hosted: BTreeMap<&'static str, UiElementKind> = uis
        .iter()
        .flat_map(|(_, ui, _)| ui.iter().flat_map(|(_, locations)| locations))
        .map(|location| (location.id, location.accepts))
        .collect();
    uis.into_iter()
        .map(|(manifest, ui, cli)| ExtensionEntry {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            ui: ui.map(|(ui, locations)| ui_entry(&manifest.id, ui, locations, &hosted)),
            cli,
        })
        .collect()
}

/// The core's routes of the extensions: `GET /ext`, `POST /ext-cli/<id>` and
/// `GET /resources`.
pub fn router(extensions: Extensions) -> Router {
    Router::new()
        .route("/ext", get(index))
        .route("/ext-cli/{id}", post(extension_cli::run))
        .route("/resources", get(resources))
        .with_state(extensions)
}

/// `GET /resources`: the catalogue of the Ressources (CORE-3), the core's
/// then each extension's declarations in registration order; the core's alone
/// without extension.
async fn resources(State(extensions): State<Extensions>) -> Response {
    Json(ExtensionHost::catalogue(&extensions)).into_response()
}

/// `GET /ext`: the registered extensions and their UI contributions, in
/// registration order (the order every location lists them in); `[]`
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
            // `admit` on the nested router itself: its fallbacks (`unserved`,
            // and any the extension declares in a sub-router) land in the
            // parent's fallback, which `identify`'s `route_layer` never wraps.
            let routes = routes
                .fallback(unserved)
                .layer(axum::middleware::from_fn(crate::identify::admit));
            router = router.nest(&prefix, routes);
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
    router.route("/ext/", any(absent.clone())).nest(
        "/ext",
        Router::new()
            .fallback(absent)
            .layer(axum::middleware::from_fn(crate::identify::admit)),
    )
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
    use pdo_extension_api::{Extension, Manifest, ResourceKind, Slot, UiSlot, When};
    use serde_json::Value;
    use tower::util::ServiceExt;

    struct Contributes;
    impl Extension for Contributes {
        fn manifest(&self) -> Manifest {
            Manifest::new("demo", "Demo", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            let greet = UiCommand::new("demo.greet", "Greet", "greet");
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
                    ))
                    .contribute(
                        UiContribution::page(UiLocation::TopbarEnd, "home", "Demo", "Home")
                            .with_icon("/ext/demo/icon.svg"),
                    )
                    .contribute(
                        UiContribution::action(UiLocation::TopbarEnd, "greet", &greet)
                            .when(When::has("run")),
                    )
                    .command(greet),
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
    fn the_index_lists_each_extension_and_its_contributions() {
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
                        "ui_module_major": null,
                        // The 0.1 detail tab of an agentic profile maps onto no location.
                        "contributions": [
                            {
                                "location": "topbar/start", "id": "events", "label": "Events",
                                "type": "page", "component": "Events",
                            },
                            {
                                "location": "topbar/end", "id": "home", "label": "Demo",
                                "type": "page", "component": "Home", "icon": "/ext/demo/icon.svg",
                            },
                            {
                                "location": "topbar/end", "id": "greet", "label": "Greet",
                                "type": "action", "command": "demo.greet", "when": { "has": "run" },
                            },
                        ],
                        "commands": [{ "id": "demo.greet", "title": "Greet", "export": "greet" }],
                        "locations": [],
                    },
                },
                { "id": "silent", "name": "Silent", "version": "0.1.0" },
            ])
        );
    }

    /// Contributes a page to `topbar/end`, and what no location renders.
    struct Second;
    impl Extension for Second {
        fn manifest(&self) -> Manifest {
            Manifest::new("second", "Second", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            let orphan = UiCommand::new("second.orphan", "Orphan", "orphan");
            Some(
                UiContributions::bundle("/ext/second/ui.js", 19)
                    .contribute(UiContribution::page(UiLocation::TopbarEnd, "p", "P", "P"))
                    // An action where only sections go, an action without its
                    // command, an id already taken.
                    .contribute(UiContribution::action(UiLocation::Settings, "a", &orphan))
                    .contribute(UiContribution::action(UiLocation::TopbarEnd, "b", &orphan))
                    .contribute(UiContribution::page(UiLocation::TopbarEnd, "p", "Q", "Q")),
            )
        }
    }

    #[test]
    fn a_location_lists_every_extension_in_registration_order() {
        let extensions =
            Extensions::register(vec![Box::new(Contributes), Box::new(Second)]).unwrap();
        let at_topbar_end: Vec<(String, String)> = entries(&extensions)
            .into_iter()
            .flat_map(|entry| {
                let id = entry.id;
                entry
                    .ui
                    .into_iter()
                    .flat_map(|ui| ui.contributions)
                    .filter(|c| c.location == UiLocation::TopbarEnd)
                    .map(move |c| (id.clone(), c.label))
            })
            .collect();
        assert_eq!(
            at_topbar_end,
            [
                ("demo".to_string(), "Demo".to_string()),
                ("demo".to_string(), "Greet".to_string()),
                ("second".to_string(), "P".to_string()),
            ]
        );
    }

    /// Hosts `host/panel` (sections) and `host/actions` (actions), and declares
    /// what the daemon refuses to host.
    struct Host;
    impl Extension for Host {
        fn manifest(&self) -> Manifest {
            Manifest::new("host", "Host", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            Some(
                UiContributions::bundle("/ext/host/ui.js", 19)
                    .importing_ui_module()
                    .host(
                        HostedLocation::new("host/panel", UiElementKind::Section)
                            .describe("Under each item of the host's page.")
                            .context_key("item", "The item: { id, title }"),
                    )
                    .host(HostedLocation::new("host/actions", UiElementKind::Action))
                    // Another extension's prefix, a core location, a bad name,
                    // a page, a second declaration.
                    .host(HostedLocation::new("other/panel", UiElementKind::Section))
                    .host(HostedLocation::new("host/", UiElementKind::Section))
                    .host(HostedLocation::new("host/Panel", UiElementKind::Section))
                    .host(HostedLocation::new("host/page", UiElementKind::Page))
                    .host(HostedLocation::new("host/panel", UiElementKind::Tab)),
            )
        }
    }

    /// Contributes to `host/panel` twice, to `host/actions`, and to what no
    /// one hosts.
    struct Contributor;
    impl Extension for Contributor {
        fn manifest(&self) -> Manifest {
            Manifest::new("contributor", "Contributor", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            let ping = UiCommand::new("contributor.ping", "Ping", "ping");
            let panel = UiLocation::Hosted("host/panel");
            Some(
                UiContributions::bundle("/ext/contributor/ui.js", 19)
                    .contribute(UiContribution::section(panel, "first", "First", "First"))
                    .contribute(UiContribution::section(panel, "second", "Second", "Second"))
                    .contribute(UiContribution::action(
                        UiLocation::Hosted("host/actions"),
                        "ping",
                        &ping,
                    ))
                    // Absent host, a kind the host does not accept.
                    .contribute(UiContribution::section(
                        UiLocation::Hosted("absent/panel"),
                        "nowhere",
                        "Nowhere",
                        "Nowhere",
                    ))
                    .contribute(UiContribution::tab(panel, "tab", "Tab", "Tab"))
                    .command(ping),
            )
        }
    }

    /// A core location's id, claimed by an extension named `topbar`.
    struct Topbar;
    impl Extension for Topbar {
        fn manifest(&self) -> Manifest {
            Manifest::new("topbar", "Topbar", "1.0.0")
        }
        fn ui(&self) -> Option<UiContributions> {
            Some(
                UiContributions::bundle("/ext/topbar/ui.js", 19)
                    .host(HostedLocation::new("topbar/end", UiElementKind::Action)),
            )
        }
    }

    fn contribution_ids(entry: &ExtensionEntry) -> Vec<(&str, &str)> {
        entry
            .ui
            .iter()
            .flat_map(|ui| &ui.contributions)
            .map(|c| (c.location.id(), c.id.as_str()))
            .collect()
    }

    #[test]
    fn the_index_exposes_the_locations_an_extension_hosts() {
        // The contributor registers first: its host is known all the same.
        let extensions = Extensions::register(vec![
            Box::new(Contributor),
            Box::new(Host),
            Box::new(Topbar),
        ])
        .unwrap();
        let entries = entries(&extensions);
        let host = serde_json::to_value(&entries[1]).unwrap();
        assert_eq!(
            host["ui"]["ui_module_major"],
            pdo_extension_api::UI_MODULE_MAJOR
        );
        assert_eq!(
            host["ui"]["locations"],
            serde_json::json!([
                {
                    "id": "host/panel", "accepts": "section",
                    "description": "Under each item of the host's page.",
                    "context": [{ "key": "item", "description": "The item: { id, title }" }],
                },
                { "id": "host/actions", "accepts": "action", "description": "", "context": [] },
            ])
        );
        let topbar = serde_json::to_value(&entries[2]).unwrap();
        assert_eq!(topbar["ui"]["locations"], serde_json::json!([]));
        // Both contributions to the same hosted location, and the action; not
        // the one to an absent host, nor the tab the host does not accept.
        assert_eq!(
            contribution_ids(&entries[0]),
            [
                ("host/panel", "first"),
                ("host/panel", "second"),
                ("host/actions", "ping"),
            ]
        );
    }

    #[test]
    fn a_contribution_to_an_absent_host_is_ignored() {
        let extensions = Extensions::register(vec![Box::new(Contributor)]).unwrap();
        let entries = entries(&extensions);
        assert_eq!(contribution_ids(&entries[0]), []);
        assert_eq!(entries[0].ui.as_ref().unwrap().commands.len(), 1);
    }

    /// The version of the shared module the front declares.
    const FRONT_MODULE: &str =
        include_str!("../../../frontend/src/shared/extensions/uiModuleVersion.ts");

    #[test]
    fn the_front_s_shared_module_is_versioned_with_the_api() {
        let version = format!(
            "export const UI_MODULE_VERSION = \"{}\";",
            pdo_extension_api::API_VERSION
        );
        assert!(
            FRONT_MODULE.lines().any(|line| line == version),
            "uiModuleVersion.ts must declare `{version}`"
        );
        let major = format!(
            "export const UI_MODULE_MAJOR = {};",
            pdo_extension_api::UI_MODULE_MAJOR
        );
        assert!(
            FRONT_MODULE.lines().any(|line| line == major),
            "uiModuleVersion.ts must declare `{major}`"
        );
    }

    /// The reference page of the catalogue, which extension authors read.
    const REFERENCE: &str = include_str!("../../../docs/reference/extension-ui.md");

    #[test]
    fn the_reference_page_documents_every_location_of_the_catalogue() {
        for location in UiLocation::ALL {
            let heading = format!("### `{}`", location.id());
            assert!(
                REFERENCE.lines().any(|line| line == heading),
                "docs/reference/extension-ui.md has no `{heading}` section"
            );
        }
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
