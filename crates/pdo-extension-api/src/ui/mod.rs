//! What an extension adds to the interface (ADR-0081, amendment #29): a
//! catalogue of named locations ([`UiLocation`]), a few kinds of contribution
//! ([`UiElement`]), the commands a bundle exports ([`UiCommand`]) and `when`
//! clauses ([`When`]), and the locations an extension hosts itself
//! ([`HostedLocation`], amendment #32). The reference page is
//! `docs/reference/extension-ui.md`.

mod accepts;
mod command;
mod contribution;
mod hosted;
mod location;
mod slot;
mod when;

use serde::Serialize;

pub use command::{CommandOperation, UiCommand};
pub use contribution::{CompletionRender, PageCounter, UiContribution, UiElement, UiElementKind};
pub use hosted::{ContextKey, HostedLocation};
pub use location::UiLocation;
pub use slot::{Slot, UiSlot};
pub use when::When;

/// The bare specifier of the module the front shares with the bundles, next to
/// React in its import map: it lists and renders the contributions to a hosted
/// location (`useHostedContributions`, `HostedContribution`,
/// `HostedContributions`). A bundle imports it, never embeds it.
pub const UI_MODULE: &str = "@pdo/extension-ui";

/// The major of the shared module [`UI_MODULE`]: versioned with this API, whose
/// major it is. A bundle built against another major is never loaded, and a
/// banner says so, as for React. While the API is in 0.x, the daemon's check of
/// the manifest's `api_version` already refuses an incompatible minor.
pub const UI_MODULE_MAJOR: u32 = parse_major(crate::API_VERSION);

const fn parse_major(version: &str) -> u32 {
    let bytes = version.as_bytes();
    let mut major = 0;
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        major = major * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    major
}

/// What an extension adds to the interface: one ES bundle it serves, the
/// contributions that bundle renders and the commands it exports. Empty by
/// default.
///
/// The front reads these declarations, then imports the bundle and renders,
/// at each location, the component the bundle exports under the contribution's
/// name. The bundle imports `react` (and `react/jsx-runtime`, `react-dom`) as
/// bare specifiers: the front shares its own React through an import map. A
/// bundle built for another major of React than the front's is never loaded,
/// and a banner says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UiContributions {
    /// Where the daemon serves the extension's UI bundle, if it has one: an
    /// absolute path, usually under the extension's routes (`/ext/<id>/…`).
    pub bundle_url: Option<String>,
    /// The major version of React the bundle was built against.
    pub react_major: Option<u32>,
    /// The major of the shared module [`UI_MODULE`] the bundle imports; `None`
    /// when it does not import it.
    pub ui_module_major: Option<u32>,
    /// The slots of version 0.1 the bundle fills, still honoured.
    pub slots: Vec<UiSlot>,
    /// The contributions to the catalogue's locations, in declaration order.
    pub contributions: Vec<UiContribution>,
    /// The commands the bundle exports, for its actions.
    pub commands: Vec<UiCommand>,
    /// The locations the extension hosts, in declaration order.
    pub locations: Vec<HostedLocation>,
}

impl UiContributions {
    /// The contributions of the bundle served at `url`, built against React
    /// `react_major`, filling no location yet.
    pub fn bundle(url: impl Into<String>, react_major: u32) -> Self {
        Self {
            bundle_url: Some(url.into()),
            react_major: Some(react_major),
            ..Self::default()
        }
    }

    /// The same contributions, filling one more slot of version 0.1.
    pub fn with(mut self, slot: UiSlot) -> Self {
        self.slots.push(slot);
        self
    }

    /// The same contributions, with one more contribution to the catalogue.
    pub fn contribute(mut self, contribution: UiContribution) -> Self {
        self.contributions.push(contribution);
        self
    }

    /// The same contributions, the bundle exporting one more command.
    pub fn command(mut self, command: UiCommand) -> Self {
        self.commands.push(command);
        self
    }

    /// The same contributions, the extension hosting one more location.
    pub fn host(mut self, location: HostedLocation) -> Self {
        self.locations.push(location);
        self
    }

    /// The same contributions, the bundle importing the shared module
    /// [`UI_MODULE`] of this version of the API ([`UI_MODULE_MAJOR`]).
    pub fn importing_ui_module(self) -> Self {
        self.importing_ui_module_major(UI_MODULE_MAJOR)
    }

    /// The same contributions, the bundle importing major `major` of the
    /// shared module (a bundle built against another version of the API).
    pub fn importing_ui_module_major(mut self, major: u32) -> Self {
        self.ui_module_major = Some(major);
        self
    }

    /// Everything the bundle adds, in the catalogue's terms: the 0.1 slots
    /// mapped onto their locations ([`UiSlot::contribution`]), then the
    /// contributions, each in declaration order.
    pub fn catalogue(&self) -> Vec<UiContribution> {
        self.slots
            .iter()
            .filter_map(UiSlot::contribution)
            .chain(self.contributions.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceKind;

    #[test]
    fn declares_a_bundle_and_its_slots_on_the_wire() {
        let ui = UiContributions::bundle("/ext/demo/ui.js", 19)
            .with(UiSlot::new(Slot::Navigation, "events", "Events", "Events"))
            .with(UiSlot::new(
                Slot::ListFilter {
                    resource: ResourceKind::Run,
                },
                "by-author",
                "Author",
                "AuthorFilter",
            ));
        assert_eq!(
            serde_json::to_value(&ui).unwrap(),
            serde_json::json!({
                "bundle_url": "/ext/demo/ui.js",
                "react_major": 19,
                "ui_module_major": null,
                "slots": [
                    { "slot": "navigation", "id": "events", "label": "Events", "component": "Events" },
                    {
                        "slot": "list_filter", "resource": "run", "id": "by-author",
                        "label": "Author", "component": "AuthorFilter"
                    },
                ],
                "contributions": [],
                "commands": [],
                "locations": [],
            })
        );
    }

    #[test]
    fn declares_its_hosted_locations_and_the_shared_module_it_imports() {
        let ui = UiContributions::bundle("/ext/a/ui.js", 19)
            .importing_ui_module()
            .host(HostedLocation::new("a/panneau", UiElementKind::Tab));
        let wire = serde_json::to_value(&ui).unwrap();
        assert_eq!(wire["ui_module_major"], UI_MODULE_MAJOR);
        assert_eq!(wire["locations"][0]["id"], "a/panneau");
        assert_eq!(wire["locations"][0]["accepts"], "tab");
    }

    #[test]
    fn the_shared_module_s_major_is_the_api_s() {
        let major: u32 = crate::API_VERSION
            .split('.')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(UI_MODULE_MAJOR, major);
        assert_eq!(parse_major("12.3.4"), 12);
    }

    #[test]
    fn the_catalogue_lists_the_0_1_slots_then_the_contributions() {
        let greet = UiCommand::new("demo.greet", "Greet", "greet");
        let ui = UiContributions::bundle("/ext/demo/ui.js", 19)
            .contribute(UiContribution::action(
                UiLocation::TopbarEnd,
                "greet",
                &greet,
            ))
            .command(greet)
            .with(UiSlot::new(
                Slot::SettingsPanel,
                "settings",
                "Demo",
                "Panel",
            ));
        let catalogue: Vec<_> = ui
            .catalogue()
            .into_iter()
            .map(|c| (c.location.id(), c.id))
            .collect();
        assert_eq!(
            catalogue,
            [
                ("settings", "settings".to_string()),
                ("topbar/end", "greet".to_string())
            ]
        );
    }
}
