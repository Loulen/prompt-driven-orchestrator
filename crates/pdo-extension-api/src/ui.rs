use serde::Serialize;

use crate::ResourceKind;

/// What an extension adds to the interface: one ES bundle it serves, and the
/// UI slots (CONTEXT.md § *Extensions*) that bundle fills. Empty by default.
///
/// The front reads these declarations, then imports the bundle and renders, in
/// each slot, the component the bundle exports under [`UiSlot::component`]. The
/// bundle imports `react` (and `react/jsx-runtime`, `react-dom`) as bare
/// specifiers: the front shares its own React through an import map. A bundle
/// built for another major of React than the front's is never loaded, and a
/// banner says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UiContributions {
    /// Where the daemon serves the extension's UI bundle, if it has one: an
    /// absolute path, usually under the extension's routes (`/ext/<id>/…`).
    pub bundle_url: Option<String>,
    /// The major version of React the bundle was built against.
    pub react_major: Option<u32>,
    /// The slots the bundle fills, in display order.
    pub slots: Vec<UiSlot>,
}

impl UiContributions {
    /// The contributions of the bundle served at `url`, built against React
    /// `react_major`, filling no slot yet.
    pub fn bundle(url: impl Into<String>, react_major: u32) -> Self {
        Self {
            bundle_url: Some(url.into()),
            react_major: Some(react_major),
            slots: Vec::new(),
        }
    }

    /// The same contributions, filling one more slot.
    pub fn with(mut self, slot: UiSlot) -> Self {
        self.slots.push(slot);
        self
    }
}

/// One element an extension adds in one slot of the interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UiSlot {
    /// Which slot, and where.
    #[serde(flatten)]
    pub slot: Slot,
    /// Unique among the extension's slots: it names the element's page for a
    /// navigation entry (`/extensions/<extension id>/<id>` in the front).
    pub id: String,
    /// What the interface shows: the entry, the panel's title, the filter's
    /// name, the tab.
    pub label: String,
    /// The name the bundle exports the component under.
    pub component: String,
}

impl UiSlot {
    pub fn new(
        slot: Slot,
        id: impl Into<String>,
        label: impl Into<String>,
        component: impl Into<String>,
    ) -> Self {
        Self {
            slot,
            id: id.into(),
            label: label.into(),
            component: component.into(),
        }
    }
}

/// The UI slots of version 0.1: where an extension's element appears, and what
/// its component receives.
///
/// Non-exhaustive: adding a slot is a minor evolution of the API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "slot", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Slot {
    /// An entry of the navigation, opening a page that renders the component.
    Navigation,
    /// A panel of the settings page.
    SettingsPanel,
    /// A filter above the list of `resource`. The component receives the
    /// attribute filter in force (prefixed keys: `<id>/<key>` → value) and a
    /// callback to change it; the list then keeps only the entities carrying
    /// every one of those attributes, as `?attr.<id>/<key>=<value>` does on the
    /// list endpoints.
    ListFilter { resource: ResourceKind },
    /// A tab of the detail of one entity of `resource`. The component receives
    /// the entity's kind, id and attributes.
    DetailTab { resource: ResourceKind },
}

#[cfg(test)]
mod tests {
    use super::*;

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
            })
        );
    }
}
