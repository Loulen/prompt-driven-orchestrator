//! The two internal test extensions of the hosted locations (ADR-0081,
//! amendment #32): **A** hosts locations in its own page, **B** contributes to
//! them, as a third-party extension contributes to `pm`'s.
//!
//! - **A** (`a`): a `topbar/end` page, `HostPage`, listing two items. It hosts
//!   [`PANEL`] (sections, rendered under each item, context `item`) and
//!   [`ACTIONS`] (actions, in the page's header, context `item`: the first
//!   one). Its bundle (`host_a.js`) renders both through the shared module
//!   `@pdo/extension-ui`, imported as a bare specifier, never embedded.
//! - **B** (`b`): a section at [`PANEL`] showing the context A handed it, a
//!   second one while [`ContributorControl::set_panels`] says 2, an action at
//!   [`ACTIONS`] calling `b.ping`, and a section at `absent/panel`, which no
//!   extension hosts (ignored). `POST /ext/b/panels {"count": n}` switches the
//!   number of sections while the daemon runs (reload the page to see it).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pdo::extension_api::axum::http::header;
use pdo::extension_api::axum::response::IntoResponse;
use pdo::extension_api::axum::routing::{get, post};
use pdo::extension_api::axum::{Json, Router};
use pdo::extension_api::{
    Extension, HostedLocation, Manifest, UiCommand, UiContribution, UiContributions, UiElementKind,
    UiLocation,
};
use serde_json::{json, Value};

use super::REACT_MAJOR;

/// The id of the host.
pub const HOST_ID: &str = "a";
/// The id of the contributor.
pub const CONTRIBUTOR_ID: &str = "b";
/// A's location for sections, under each of its items.
pub const PANEL: &str = "a/panneau";
/// A's location for actions, in its page's header.
pub const ACTIONS: &str = "a/actions";

const HOST_BUNDLE: &str = include_str!("host_a.js");
const CONTRIBUTOR_BUNDLE: &str = include_str!("contributor_b.js");

fn javascript(source: &'static str) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript")], source)
}

/// Extension A: hosts [`PANEL`] and [`ACTIONS`].
pub struct HostA;

impl Extension for HostA {
    fn manifest(&self) -> Manifest {
        Manifest::new(HOST_ID, "Test host A", "0.0.0")
    }

    fn routes(&self) -> Option<Router> {
        Some(Router::new().route("/ui.js", get(|| async { javascript(HOST_BUNDLE) })))
    }

    fn ui(&self) -> Option<UiContributions> {
        Some(
            UiContributions::bundle(format!("/ext/{HOST_ID}/ui.js"), REACT_MAJOR)
                .importing_ui_module()
                .host(
                    HostedLocation::new(PANEL, UiElementKind::Section)
                        .describe("Under each item of A's page, one section per contribution.")
                        .context_key("item", "The item the section sits under: { id, title }"),
                )
                .host(
                    HostedLocation::new(ACTIONS, UiElementKind::Action)
                        .describe("A button per contribution in the header of A's page.")
                        .context_key("item", "The first item of the page: { id, title }"),
                )
                .contribute(UiContribution::page(
                    UiLocation::TopbarEnd,
                    "host",
                    "Host A",
                    "HostPage",
                )),
        )
    }
}

/// What a test changes in extension B while the daemon runs.
#[derive(Clone)]
pub struct ContributorControl {
    panels: Arc<AtomicUsize>,
}

impl ContributorControl {
    /// How many sections B contributes to [`PANEL`]: 1 or 2 (0 counts as 1).
    pub fn set_panels(&self, count: usize) {
        self.panels.store(count, Ordering::Relaxed);
    }

    fn panels(&self) -> usize {
        self.panels.load(Ordering::Relaxed)
    }
}

/// Extension B: contributes to A's locations.
pub struct ContributorB {
    control: ContributorControl,
}

impl ContributorB {
    /// The contributor, with one section at [`PANEL`], and its control.
    pub fn new() -> (Self, ContributorControl) {
        let control = ContributorControl {
            panels: Arc::new(AtomicUsize::new(1)),
        };
        (
            Self {
                control: control.clone(),
            },
            control,
        )
    }
}

impl Extension for ContributorB {
    fn manifest(&self) -> Manifest {
        Manifest::new(CONTRIBUTOR_ID, "Test contributor B", "0.0.0")
    }

    fn routes(&self) -> Option<Router> {
        let control = self.control.clone();
        Some(
            Router::new()
                .route("/ui.js", get(|| async { javascript(CONTRIBUTOR_BUNDLE) }))
                .route(
                    "/panels",
                    post(move |Json(body): Json<Value>| {
                        let count = body["count"].as_u64().unwrap_or(1) as usize;
                        control.set_panels(count);
                        async move { Json(json!({ "panels": count })) }
                    }),
                ),
        )
    }

    fn ui(&self) -> Option<UiContributions> {
        let ping = UiCommand::new("b.ping", "Ping from B", "ping");
        let panel = UiLocation::Hosted(PANEL);
        let mut ui = UiContributions::bundle(format!("/ext/{CONTRIBUTOR_ID}/ui.js"), REACT_MAJOR)
            .contribute(UiContribution::section(panel, "panel", "B panel", "BPanel"));
        if self.control.panels() >= 2 {
            ui = ui.contribute(UiContribution::section(
                panel,
                "second-panel",
                "B second panel",
                "BSecondPanel",
            ));
        }
        Some(
            ui.contribute(UiContribution::action(
                UiLocation::Hosted(ACTIONS),
                "ping",
                &ping,
            ))
            .contribute(UiContribution::section(
                UiLocation::Hosted("absent/panel"),
                "nowhere",
                "Nowhere",
                "BPanel",
            ))
            .command(ping),
        )
    }
}
