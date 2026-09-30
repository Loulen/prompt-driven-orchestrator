use serde::Serialize;

use super::{UiContribution, UiLocation};
use crate::ResourceKind;

/// One element an extension adds in one slot of the interface: the form of
/// version 0.1, still honoured. Each slot maps onto its named location
/// ([`UiSlot::contribution`]); a new extension declares a [`UiContribution`]
/// instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UiSlot {
    /// Which slot, and where.
    #[serde(flatten)]
    pub slot: Slot,
    /// Unique among the extension's slots and contributions.
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

    /// The contribution this 0.1 slot stands for in the catalogue:
    ///
    /// | 0.1 slot                         | location       | element     |
    /// |----------------------------------|----------------|-------------|
    /// | `Navigation`                     | `topbar/start` | page        |
    /// | `SettingsPanel`                  | `settings`     | section     |
    /// | `ListFilter { resource: Run }`   | `runs/filter`  | list filter |
    /// | `DetailTab { resource: Run }`    | `run/detail`   | tab         |
    ///
    /// `None` for a list filter or a detail tab of another resource: version
    /// 0.1 never rendered them, and no location of the catalogue stands for
    /// them yet.
    pub fn contribution(&self) -> Option<UiContribution> {
        let (id, label, component) = (&self.id, &self.label, self.component.clone());
        Some(match self.slot {
            Slot::Navigation => UiContribution::page(UiLocation::TopbarStart, id, label, component),
            Slot::SettingsPanel => {
                UiContribution::section(UiLocation::Settings, id, label, component)
            }
            Slot::ListFilter {
                resource: ResourceKind::Run,
            } => UiContribution::list_filter(UiLocation::RunsFilter, id, label, component),
            Slot::DetailTab {
                resource: ResourceKind::Run,
            } => UiContribution::tab(UiLocation::RunDetail, id, label, component),
            _ => return None,
        })
    }
}

/// The UI slots of version 0.1: where an extension's element appears, and what
/// its component receives. Superseded by the catalogue of named locations
/// ([`UiLocation`]), onto which each slot maps.
///
/// Non-exhaustive: adding a slot is a minor evolution of the API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "slot", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Slot {
    /// An entry of the navigation, opening a page that renders the component
    /// (`topbar/start`).
    Navigation,
    /// A panel of the settings page (`settings`).
    SettingsPanel,
    /// A filter above the list of `resource`. The component receives the
    /// attribute filter in force (prefixed keys: `<id>/<key>` → value) and a
    /// callback to change it; the list then keeps only the entities carrying
    /// every one of those attributes, as `?attr.<id>/<key>=<value>` does on the
    /// list endpoints (`runs/filter` for the Runs).
    ListFilter { resource: ResourceKind },
    /// A tab of the detail of one entity of `resource`. The component receives
    /// the entity's kind, id and attributes (`run/detail` for a Run).
    DetailTab { resource: ResourceKind },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_slot_of_0_1_maps_onto_its_location() {
        let run = ResourceKind::Run;
        let cases = [
            (Slot::Navigation, "topbar/start", "page"),
            (Slot::SettingsPanel, "settings", "section"),
            (
                Slot::ListFilter {
                    resource: run.clone(),
                },
                "runs/filter",
                "list_filter",
            ),
            (Slot::DetailTab { resource: run }, "run/detail", "tab"),
        ];
        for (slot, location, kind) in cases {
            let contribution = UiSlot::new(slot, "x", "X", "X").contribution().unwrap();
            let wire = serde_json::to_value(&contribution).unwrap();
            assert_eq!(wire["location"], location);
            assert_eq!(wire["type"], kind);
            assert_eq!(wire["component"], "X");
            assert!(contribution.location.accepts(&contribution.element));
        }
        let project_tab = Slot::DetailTab {
            resource: ResourceKind::Project,
        };
        assert_eq!(UiSlot::new(project_tab, "x", "X", "X").contribution(), None);
    }
}
