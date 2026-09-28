//! The extensions' surface on HTTP: each extension's routes, nested under
//! `/ext/<id>/`, and `GET /ext`, the index the front reads to fill its UI slots.
//!
//! Every path of an extension sits under its own `/ext/<id>` prefix, so an
//! extension can never hide a route of the core, nor another extension's.
//!
//! ROUTER TRAP: `/ext` is a top-level prefix of the vite dev proxy whitelist
//! (`frontend/vite.config.ts`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
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
/// under `/ext/<id>`. A path of an extension it does not serve answers `404`.
pub(crate) fn mount(router: Router, extensions: &Extensions) -> Router {
    let mut router = router.merge(self::router(extensions.clone()));
    for (manifest, extension) in extensions.iter() {
        let routes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extension.routes()))
            .unwrap_or_else(|_| {
                tracing::error!("extension `{}` panicked in routes", manifest.id);
                None
            });
        if let Some(routes) = routes {
            let prefix = format!("/ext/{}", manifest.id);
            router = router.nest(&prefix, routes.fallback(not_found));
        }
    }
    router
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "not found" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::{Extension, Manifest, ResourceKind, UiSlot};

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
}
