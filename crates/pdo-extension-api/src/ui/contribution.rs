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
    /// Where the counter of an icon of `topbar/end` is read (0.19 for a
    /// [`UiElement::Page`], 0.20 for a [`UiElement::Panel`] and a
    /// [`UiElement::Action`]): an absolute path the extension serves, usually
    /// under `/ext/<id>/`, answering a [`PageCounter`](super::PageCounter) for the Actor of the
    /// request. The core reads it when it shows the icon,
    /// and again at each event of the extension on `/ws` (its
    /// `extension_event`s, the events of its Sessions hébergées) and at each
    /// resync. Without one, the icon shows no counter. Since 0.20, a global
    /// [`UiElement::Action`] of `palette` reads its count the same way, shown
    /// at the end of its row.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub counter: Option<String>,
    /// The rank of the extension's group in the command field (0.20; a
    /// contribution at `palette`): the groups of the extensions follow their
    /// rank, lowest first, then those without one, each in registration
    /// order. An extension's group takes the lowest rank of its `palette`
    /// contributions.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub rank: Option<i32>,
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
    /// A panel anchored under its icon (0.20, `topbar/end`): the icon opens
    /// it, the exported component fills it; Escape and a click outside close
    /// it, a click in a toast does not.
    Panel {
        component: String,
        /// An SVG the extension serves, drawn as a [`UiElement::Page`]'s.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        icon: Option<String>,
    },
    /// A tab of a detail.
    Tab { component: String },
    /// A section of a page or a form.
    Section { component: String },
    /// A small mark on an item (a badge on a row).
    Decoration { component: String },
    /// A control that filters a list on attributes.
    ListFilter { component: String },
    /// A source of completion of a text field (0.14): typing `trigger` opens
    /// a list of suggestions the function `suggest` of the bundle answers; the
    /// suggestion picked inserts its text. The core never interprets what is
    /// inserted. Optionally, the `render` of what was inserted in the text
    /// once shown.
    Completion {
        /// The one character that opens the list, typed at the start of the
        /// text or after a space.
        trigger: String,
        /// The function the bundle exports, `(query, context) => suggestions`.
        suggest: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        render: Option<CompletionRender>,
    },
    /// A Source de recherche (0.18): for the query typed in the command
    /// field, the function `search` of the bundle answers results already
    /// filtered by the Actor's rights (a label, facts, an icon, a status, a
    /// deep link). The core groups them under the extension's name, keeps
    /// the first ones of each source, never waits for a slow source and keeps
    /// a failing one to its own group.
    Search {
        /// The function the bundle exports, `(query, context) => results`.
        search: String,
        /// An SVG the extension serves, the icon of its group in the palette;
        /// the extension's first letter, boxed, without one.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        icon: Option<String>,
    },
}

/// How the text a [`UiElement::Completion`] inserted shows once the text is
/// rendered: each match of `pattern` (a JavaScript regular expression) in its
/// plain text is drawn by the exported `component` instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct CompletionRender {
    pub pattern: String,
    pub component: String,
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
    Panel,
    Tab,
    Section,
    Decoration,
    ListFilter,
    Completion,
    Search,
}

impl UiElement {
    /// What kind of element this is.
    pub fn kind(&self) -> UiElementKind {
        match self {
            UiElement::Page { .. } => UiElementKind::Page,
            UiElement::Action { .. } => UiElementKind::Action,
            UiElement::Panel { .. } => UiElementKind::Panel,
            UiElement::Tab { .. } => UiElementKind::Tab,
            UiElement::Section { .. } => UiElementKind::Section,
            UiElement::Decoration { .. } => UiElementKind::Decoration,
            UiElement::ListFilter { .. } => UiElementKind::ListFilter,
            UiElement::Completion { .. } => UiElementKind::Completion,
            UiElement::Search { .. } => UiElementKind::Search,
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
            counter: None,
            rank: None,
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

    /// A panel at `location`, anchored under its icon and filled by the
    /// exported `component` (0.20).
    pub fn panel(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        component: impl Into<String>,
    ) -> Self {
        let element = UiElement::Panel {
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

    /// A source of completion at `location`: typing `trigger` opens the
    /// suggestions the bundle's exported function `suggest` answers (0.14).
    pub fn completion(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        trigger: char,
        suggest: impl Into<String>,
    ) -> Self {
        let element = UiElement::Completion {
            trigger: trigger.to_string(),
            suggest: suggest.into(),
            render: None,
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// A Source de recherche at `location` (`palette`): the query typed in
    /// the command field goes to the bundle's exported function `search`
    /// (0.18).
    pub fn search(
        location: UiLocation,
        id: impl AsRef<str>,
        label: impl AsRef<str>,
        search: impl Into<String>,
    ) -> Self {
        let element = UiElement::Search {
            search: search.into(),
            icon: None,
        };
        Self::new(location, id.as_ref(), label.as_ref(), element)
    }

    /// The same source of completion, each match of `pattern` (a JavaScript
    /// regular expression) in a rendered text drawn by the exported
    /// `component`. Elsewhere, unchanged.
    pub fn rendering(mut self, pattern: impl Into<String>, component: impl Into<String>) -> Self {
        if let UiElement::Completion { render, .. } = &mut self.element {
            *render = Some(CompletionRender {
                pattern: pattern.into(),
                component: component.into(),
            });
        }
        self
    }

    /// The same page, panel or Source de recherche, with the SVG served at
    /// `url` as its icon. Only those have their own icon (an action shows its
    /// command's): elsewhere, unchanged.
    pub fn with_icon(mut self, url: impl Into<String>) -> Self {
        if let UiElement::Page { icon, .. }
        | UiElement::Panel { icon, .. }
        | UiElement::Search { icon, .. } = &mut self.element
        {
            *icon = Some(url.into());
        }
        self
    }

    /// The same page or panel, its icon showing the counter `path` answers
    /// (0.19, a panel 0.20); the same Action of `topbar/end`, its icon showing
    /// it, or global Action of `palette`, its row showing it (0.20): see
    /// [`UiContribution::counter`]. Ignored on any other element.
    pub fn with_counter(mut self, path: impl Into<String>) -> Self {
        let counted = match self.element {
            UiElement::Page { .. } | UiElement::Panel { .. } => true,
            UiElement::Action { .. } => {
                matches!(self.location, UiLocation::Palette | UiLocation::TopbarEnd)
            }
            _ => false,
        };
        if counted {
            self.counter = Some(path.into());
        }
        self
    }

    /// The same contribution of `palette`, its extension's group ranked
    /// `rank` in the command field (0.20): see [`UiContribution::rank`].
    /// Ignored at any other location.
    pub fn with_rank(mut self, rank: i32) -> Self {
        if self.location == UiLocation::Palette {
            self.rank = Some(rank);
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
    use crate::ui::PageCounter;
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
    fn a_page_may_name_the_counter_of_its_icon() {
        let page = UiContribution::page(UiLocation::TopbarEnd, "home", "Notes", "HomePage")
            .with_counter("/ext/test/counter");
        assert_eq!(
            serde_json::to_value(&page).unwrap()["counter"],
            "/ext/test/counter"
        );
        let tab = UiContribution::tab(UiLocation::RunDetail, "t", "T", "T").with_counter("/x");
        assert!(serde_json::to_value(&tab).unwrap().get("counter").is_none());
        let plain = UiContribution::page(UiLocation::TopbarEnd, "p", "P", "P");
        assert!(serde_json::to_value(&plain)
            .unwrap()
            .get("counter")
            .is_none());

        let counter = PageCounter::new(3).accented(true).with_detail("1 waiting");
        let wire = serde_json::to_value(&counter).unwrap();
        assert_eq!(
            wire,
            json!({ "count": 3, "accent": true, "detail": "1 waiting" })
        );
        assert_eq!(
            serde_json::from_value::<PageCounter>(json!({ "count": 2 })).unwrap(),
            PageCounter::new(2)
        );
    }

    #[test]
    fn a_panel_declares_its_component_its_icon_and_its_counter() {
        let panel = UiContribution::panel(UiLocation::TopbarEnd, "inbox", "Inbox", "InboxPanel")
            .with_icon("/ext/test/inbox.svg")
            .with_counter("/ext/test/inbox/counter");
        assert_eq!(panel.element.kind(), UiElementKind::Panel);
        assert_eq!(
            serde_json::to_value(&panel).unwrap(),
            json!({
                "location": "topbar/end", "id": "inbox", "label": "Inbox",
                "type": "panel", "component": "InboxPanel", "icon": "/ext/test/inbox.svg",
                "counter": "/ext/test/inbox/counter",
            })
        );
        let bare = UiContribution::panel(UiLocation::TopbarEnd, "p", "P", "P");
        let wire = serde_json::to_value(&bare).unwrap();
        assert!(wire.get("icon").is_none());
        assert!(wire.get("counter").is_none());
    }

    #[test]
    fn an_action_may_name_the_counter_of_its_icon() {
        let command = UiCommand::new("test.sync", "Sync", "sync");
        let action = UiContribution::action(UiLocation::TopbarEnd, "sync", &command)
            .with_counter("/ext/test/pending");
        assert_eq!(
            serde_json::to_value(&action).unwrap()["counter"],
            "/ext/test/pending"
        );
    }

    #[test]
    fn a_global_action_of_the_palette_may_name_its_counter() {
        let command = UiCommand::new("test.inbox", "Inbox", "openInbox");
        let inbox =
            UiContribution::action(UiLocation::Palette, "inbox", &command).with_counter("/ext/t/n");
        assert_eq!(
            serde_json::to_value(&inbox).unwrap(),
            json!({
                "location": "palette", "id": "inbox", "label": "Inbox",
                "type": "action", "command": "test.inbox", "counter": "/ext/t/n",
            })
        );
        let elsewhere =
            UiContribution::action(UiLocation::RunsHeader, "inbox", &command).with_counter("/x");
        assert!(serde_json::to_value(&elsewhere)
            .unwrap()
            .get("counter")
            .is_none());
        let source = UiContribution::search(UiLocation::Palette, "n", "N", "s").with_counter("/x");
        assert!(serde_json::to_value(&source)
            .unwrap()
            .get("counter")
            .is_none());
    }

    #[test]
    fn a_palette_contribution_may_rank_its_group() {
        let source = UiContribution::search(UiLocation::Palette, "notes", "Notes", "searchNotes")
            .with_rank(-2);
        assert_eq!(serde_json::to_value(&source).unwrap()["rank"], -2);
        let plain = UiContribution::search(UiLocation::Palette, "notes", "Notes", "searchNotes");
        assert!(serde_json::to_value(&plain).unwrap().get("rank").is_none());
        let page = UiContribution::page(UiLocation::TopbarEnd, "p", "P", "P").with_rank(1);
        assert!(serde_json::to_value(&page).unwrap().get("rank").is_none());
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
    fn a_completion_declares_its_trigger_its_function_and_its_render() {
        let source = UiContribution::completion(
            UiLocation::ReviewComposer,
            "tags",
            "Tags",
            '#',
            "suggestTags",
        )
        .rendering("#[a-z]+", "Tag");
        assert_eq!(source.element.kind(), UiElementKind::Completion);
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            json!({
                "location": "review/composer", "id": "tags", "label": "Tags",
                "type": "completion", "trigger": "#", "suggest": "suggestTags",
                "render": { "pattern": "#[a-z]+", "component": "Tag" },
            })
        );
        let bare = UiContribution::completion(UiLocation::ReviewComposer, "t", "T", '#', "s")
            .with_icon("/ignored.svg");
        assert!(serde_json::to_value(&bare).unwrap().get("render").is_none());
        let page = UiContribution::page(UiLocation::TopbarEnd, "p", "P", "P").rendering("x", "X");
        assert!(serde_json::to_value(&page).unwrap().get("render").is_none());
    }

    #[test]
    fn a_search_source_declares_its_function_and_its_icon() {
        let source = UiContribution::search(UiLocation::Palette, "notes", "Notes", "searchNotes")
            .with_icon("/ext/test/icon.svg");
        assert_eq!(source.element.kind(), UiElementKind::Search);
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            json!({
                "location": "palette", "id": "notes", "label": "Notes",
                "type": "search", "search": "searchNotes", "icon": "/ext/test/icon.svg",
            })
        );
        let bare = UiContribution::search(UiLocation::Palette, "n", "N", "s").rendering("x", "X");
        let wire = serde_json::to_value(&bare).unwrap();
        assert!(wire.get("icon").is_none());
        assert!(wire.get("render").is_none());
        assert_eq!(
            serde_json::to_value(UiElementKind::Search).unwrap(),
            "search"
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
