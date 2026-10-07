use serde::Serialize;

use super::{UiCommand, UiLocation, When};

/// One element an extension adds at one named [`UiLocation`] (a Contribution
/// d'UI, CONTEXT.md § *Extensions*).
///
/// Build one with the constructor of its kind ([`UiContribution::page`],
/// [`UiContribution::action`]…), then refine it ([`UiContribution::with_icon`],
/// [`UiContribution::when`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct UiContribution {
    /// Where it appears.
    pub location: UiLocation,
    /// Unique among the extension's contributions: the front keys the element
    /// `<extension id>:<id>`.
    pub id: String,
    /// What the interface shows: the entry, the section's title, the tab, the
    /// tooltip of an icon. An action's is its command's title.
    pub label: String,
    /// What it is, and what renders it.
    #[serde(flatten)]
    pub element: UiElement,
    /// The condition under which the element is rendered; always without.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub when: Option<When>,
}

/// The kinds of contribution: few kinds, many locations.
///
/// A component-backed element names the component its bundle exports; the
/// front renders it with the location's context. An action names one of the
/// extension's [`UiCommand`]s: the core renders the entry and calls the command.
///
/// Non-exhaustive: adding a kind is a minor evolution of the API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub enum UiElement {
    /// A page, opened full-window from its entry, reachable by deep link
    /// (`#/ext/<extension>/<location>/<path>`).
    Page {
        component: String,
        /// An SVG the extension serves (an absolute path, usually under
        /// `/ext/<id>/`), drawn in the colour of the text around it. Without
        /// one, an icon location shows the label's first letter, boxed.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        icon: Option<String>,
    },
    /// An entry that calls a command of the extension.
    Action { command: String },
    /// A tab of a detail.
    Tab { component: String },
    /// A section of a page or a form.
    Section { component: String },
    /// A small mark on an item (a badge on a row).
    Decoration { component: String },
    /// A control that filters a list on attributes.
    ListFilter { component: String },
}

/// The kind of a [`UiElement`], without what renders it: what a
/// [`HostedLocation`](super::HostedLocation) accepts. On the wire, the
/// element's `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub enum UiElementKind {
    Page,
    Action,
    Tab,
    Section,
    Decoration,
    ListFilter,
}

impl UiElement {
    /// What kind of element this is.
    pub fn kind(&self) -> UiElementKind {
        match self {
            UiElement::Page { .. } => UiElementKind::Page,
            UiElement::Action { .. } => UiElementKind::Action,
            UiElement::Tab { .. } => UiElementKind::Tab,
            UiElement::Section { .. } => UiElementKind::Section,
            UiElement::Decoration { .. } => UiElementKind::Decoration,
            UiElement::ListFilter { .. } => UiElementKind::ListFilter,
        }
    }
}

impl UiContribution {
    fn new(location: UiLocation, id: &str, label: &str, element: UiElement) -> Self {
        Self {
            location,
            id: id.to_string(),
            label: label.to_string(),
            element,
            when: None,
        }
    }

    /// A page at `location`, rendered by the exported `component`.
    pub fn page(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::Page {
            component: component.into(),
            icon: None,
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// An entry at `location` that calls `command`, labelled with its title.
    pub fn action(location: UiLocation, id: impl AsRef<str>, command: &UiCommand) -> Self {
        let element = UiElement::Action {
            command: command.id.clone(),
        };
        Self::new(location, id.as_ref(), &command.title, element)
    }

    /// A tab at `location`, rendered by the exported `component`.
    pub fn tab(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::Tab {
            component: component.into(),
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// A section at `location`, rendered by the exported `component`.
    pub fn section(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::Section {
            component: component.into(),
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// A decoration at `location`, rendered by the exported `component`.
    pub fn decoration(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::Decoration {
            component: component.into(),
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// A list filter at `location`, rendered by the exported `component`.
    pub fn list_filter(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::ListFilter {
            component: component.into(),
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// The same page, with the SVG served at `url` as its icon. Only a page
    /// has its own icon (an action shows its command's): elsewhere, unchanged.
    pub fn with_icon(mut self, url: impl Into<String>) -> Self {
        if let UiElement::Page { icon, .. } = &mut self.element {
            *icon = Some(url.into());
        }
        self
    }

    /// The same contribution, rendered only while `condition` holds.
    pub fn when(mut self, condition: When) -> Self {
        self.when = Some(condition);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_page_declares_its_location_component_and_icon_on_the_wire() {
        let page = UiContribution::page(UiLocation::TopbarEnd, "home", "Notes", "HomePage")
            .with_icon("/ext/test/icon.svg");
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            json!({
                "location": "topbar/end", "id": "home", "label": "Notes",
                "type": "page", "component": "HomePage", "icon": "/ext/test/icon.svg",
            })
        );
    }

    #[test]
    fn an_element_kind_is_its_type_on_the_wire() {
        let tab = UiContribution::tab(UiLocation::Hosted("a/panneau"), "audit", "Audit", "Audit");
        assert_eq!(tab.element.kind(), UiElementKind::Tab);
        let wire = serde_json::to_value(&tab).unwrap();
        assert_eq!(wire["location"], "a/panneau");
        assert_eq!(
            wire["type"],
            serde_json::to_value(UiElementKind::Tab).unwrap()
        );
        assert_eq!(
            serde_json::to_value(UiElementKind::ListFilter).unwrap(),
            "list_filter"
        );
    }

    #[test]
    fn an_action_names_its_command_and_takes_its_title() {
        let command = UiCommand::new("test.new-note", "New note", "newNote");
        let action = UiContribution::action(UiLocation::TopbarEnd, "new-note", &command)
            .when(When::has("run"))
            .with_icon("/ignored.svg");
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({
                "location": "topbar/end", "id": "new-note", "label": "New note",
                "type": "action", "command": "test.new-note", "when": { "has": "run" },
            })
        );
    }
}
