//! What an extension adds to the interface (ADR-0081, amendment #29): a
//! catalogue of named locations ([`UiLocation`]), a few kinds of contribution
//! ([`UiElement`]), the commands a bundle exports ([`UiCommand`]) and `when`
//! clauses ([`When`]). The reference page is `docs/reference/extension-ui.md`.

mod command;
mod contribution;
mod location;
mod slot;
mod when;

use serde::Serialize;

pub use command::UiCommand;
pub use contribution::{UiContribution, UiElement};
pub use location::UiLocation;
pub use slot::{Slot, UiSlot};
pub use when::When;

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
    /// The slots of version 0.1 the bundle fills, still honoured.
    pub slots: Vec<UiSlot>,
    /// The contributions to the catalogue's locations, in declaration order.
    pub contributions: Vec<UiContribution>,
    /// The commands the bundle exports, for its actions.
    pub commands: Vec<UiCommand>,
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
                "slots": [
                    { "slot": "navigation", "id": "events", "label": "Events", "component": "Events" },
                    {
                        "slot": "list_filter", "resource": "run", "id": "by-author",
                        "label": "Author", "component": "AuthorFilter"
                    },
                ],
                "contributions": [],
                "commands": [],
            })
        );
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
