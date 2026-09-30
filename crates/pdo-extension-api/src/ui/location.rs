use serde::Serialize;

use super::UiElement;

/// The catalogue of the UI's named locations (Emplacements d'UI, CONTEXT.md §
/// *Extensions*): where an extension's [`UiContribution`](super::UiContribution)
/// appears, on the model of VS Code's contribution points.
///
/// A location has a stable id (`topbar/end`), accepts a few kinds of
/// [`UiElement`] ([`UiLocation::accepts`]) and hands each element a context
/// typed for that place. **A location is a list**: the contributions of every
/// extension add up there, in registration order (extensions first, then each
/// extension's own declaration order); none is exclusive.
///
/// The ids and what each location accepts and hands over are part of the
/// API's semver. The reference page (`docs/reference/extension-ui.md`) documents
/// each of them, and `frontend/src/shared/generated/UiLocation.ts` is this very
/// type in TypeScript.
///
/// Besides the core's locations, an extension may **host** its own
/// ([`UiLocation::Hosted`], declared by a [`HostedLocation`](super::HostedLocation)):
/// the other extensions contribute there exactly as here, and the host renders
/// them through the shared module `@pdo/extension-ui` (ADR-0081, amendment #32).
///
/// Non-exhaustive: adding a location is a minor evolution of the API.
///
/// Since #30 the catalogue also holds the **Action locations**: the bars and
/// menus of the interface ([`UiLocation::is_action_location`]).
///
/// **Adding a location**: a variant here (its id in `serde(rename)`, its line
/// in [`UiLocation::ALL`], [`UiLocation::id`] and [`UiLocation::accepts`]), its
/// section in the reference page, and its rendering in the front (a component
/// that calls `useContributions("<id>", context)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub enum UiLocation {
    /// The start of the top bar, after the PDO mark: a text entry per
    /// [`UiElement::Page`], opening its page full-window, with every page of
    /// the location on the page's rail. The 0.1 `Navigation` slot.
    #[serde(rename = "topbar/start")]
    TopbarStart,
    /// The right-hand group of the top bar, **before Stats**: an icon button per
    /// [`UiElement::Page`] (the page opens full-window, without rail: the page
    /// brings its own navigation) or per [`UiElement::Action`] (the button calls
    /// its command).
    #[serde(rename = "topbar/end")]
    TopbarEnd,
    /// The Extensions page of the Settings: a [`UiElement::Section`] per
    /// contribution. The 0.1 `SettingsPanel` slot.
    #[serde(rename = "settings")]
    Settings,
    /// Above the list of the Runs: a [`UiElement::ListFilter`] per
    /// contribution. The 0.1 `ListFilter { resource: Run }` slot.
    #[serde(rename = "runs/filter")]
    RunsFilter,
    /// A tab of the detail of a Run: a [`UiElement::Tab`] per contribution. The
    /// 0.1 `DetailTab { resource: Run }` slot.
    #[serde(rename = "run/detail")]
    RunDetail,
    // The Action locations (amendment #30): each renders an [`UiElement::Action`]
    // per contribution, as a button of a bar or an entry of a menu, and hands
    // over what is at stake there (an `entity`, a `selection`…).
    /// The title bar of the Runs tab, before « Project » and « New Run »: an
    /// icon button per action.
    #[serde(rename = "runs/header")]
    RunsHeader,
    /// The « ⋯ » menu of a Run's row: a group per extension, before the
    /// destructive entry. Hands over the row's Run as `entity`.
    #[serde(rename = "run/item/menu")]
    RunItemMenu,
    /// The bar of the Runs multi-selection: an entry per action, after the
    /// core's. Hands over the selected Runs as `selection`.
    #[serde(rename = "runs/bulk")]
    RunsBulk,
    /// The toolbar of a Run's canvas, before the info group: an icon button
    /// per action.
    #[serde(rename = "run/toolbar")]
    RunToolbar,
    /// The right-click menu of a node of the canvas. Hands over the node as
    /// `entity` and its `pipeline`.
    #[serde(rename = "canvas/node/menu")]
    CanvasNodeMenu,
    /// The right-click menu of an edge of the canvas. Hands over the edge as
    /// `entity` and its `pipeline`.
    #[serde(rename = "canvas/edge/menu")]
    CanvasEdgeMenu,
    /// The header of a node's detail in a Run: an icon button per action.
    /// Hands over the node as `entity`.
    #[serde(rename = "node/toolbar")]
    NodeToolbar,
    /// The « ⋯ » menu of a Trigger's row. Hands over the Trigger as `entity`.
    #[serde(rename = "trigger/item/menu")]
    TriggerItemMenu,
    /// The « ⋯ » menu of a Pipeline's row. Hands over the Pipeline as `entity`.
    #[serde(rename = "pipeline/item/menu")]
    PipelineItemMenu,
    /// The title bar of the Pipelines tab, before « Import » and « New »: an
    /// icon button per action.
    #[serde(rename = "pipelines/header")]
    PipelinesHeader,
    /// The right-click menu of an editor tab. Hands over the tab's Pipeline as
    /// `entity`.
    #[serde(rename = "tab/menu")]
    TabMenu,
    /// The top bar of the Review page, before « open in a new tab »: an icon
    /// button per action. Hands over the compared pair as `review`.
    #[serde(rename = "review/toolbar")]
    ReviewToolbar,
    /// The header of a file of the Review page, before « Copy path »: an icon
    /// button per action. Hands over the file as `entity` and the pair as
    /// `review`.
    #[serde(rename = "review/file")]
    ReviewFile,
    /// The « ⋮ » menu of a skill or a folder of the skill bank. Hands over the
    /// skill or the folder as `entity`.
    #[serde(rename = "skill/menu")]
    SkillMenu,
    /// A location an extension hosts, by its id `<extension>/<name>`
    /// (`pm/onglet`): what it accepts and hands over is the host's
    /// [`HostedLocation`](super::HostedLocation) declaration. A contribution to
    /// the location of an extension that is not loaded, or that does not
    /// declare it, is left out without error.
    #[serde(untagged)]
    Hosted(#[cfg_attr(feature = "ts", ts(type = "`${string}/${string}`"))] &'static str),
}

impl UiLocation {
    /// Every location of the core in this version of the API, in the
    /// catalogue's order (the hosted ones are the extensions').
    pub const ALL: &'static [UiLocation] = &[
        UiLocation::TopbarStart,
        UiLocation::TopbarEnd,
        UiLocation::Settings,
        UiLocation::RunsFilter,
        UiLocation::RunDetail,
        UiLocation::RunsHeader,
        UiLocation::RunItemMenu,
        UiLocation::RunsBulk,
        UiLocation::RunToolbar,
        UiLocation::CanvasNodeMenu,
        UiLocation::CanvasEdgeMenu,
        UiLocation::NodeToolbar,
        UiLocation::TriggerItemMenu,
        UiLocation::PipelineItemMenu,
        UiLocation::PipelinesHeader,
        UiLocation::TabMenu,
        UiLocation::ReviewToolbar,
        UiLocation::ReviewFile,
        UiLocation::SkillMenu,
    ];

    /// The stable id: what the wire, the deep links (`#/ext/<extension>/<id>/…`)
    /// and the reference page call it.
    pub fn id(self) -> &'static str {
        match self {
            UiLocation::TopbarStart => "topbar/start",
            UiLocation::TopbarEnd => "topbar/end",
            UiLocation::Settings => "settings",
            UiLocation::RunsFilter => "runs/filter",
            UiLocation::RunDetail => "run/detail",
            UiLocation::RunsHeader => "runs/header",
            UiLocation::RunItemMenu => "run/item/menu",
            UiLocation::RunsBulk => "runs/bulk",
            UiLocation::RunToolbar => "run/toolbar",
            UiLocation::CanvasNodeMenu => "canvas/node/menu",
            UiLocation::CanvasEdgeMenu => "canvas/edge/menu",
            UiLocation::NodeToolbar => "node/toolbar",
            UiLocation::TriggerItemMenu => "trigger/item/menu",
            UiLocation::PipelineItemMenu => "pipeline/item/menu",
            UiLocation::PipelinesHeader => "pipelines/header",
            UiLocation::TabMenu => "tab/menu",
            UiLocation::ReviewToolbar => "review/toolbar",
            UiLocation::ReviewFile => "review/file",
            UiLocation::SkillMenu => "skill/menu",
            UiLocation::Hosted(id) => id,
        }
    }

    /// Whether this location renders `element`. The core leaves out, with a
    /// warning in its log, a contribution a location does not accept. A
    /// hosted location accepts the kind its host declares
    /// ([`HostedLocation::accepts`](super::HostedLocation::accepts)): the core
    /// checks it against the declaration, this answers `true`.
    pub fn accepts(self, element: &UiElement) -> bool {
        match self {
            UiLocation::TopbarStart => matches!(element, UiElement::Page { .. }),
            UiLocation::TopbarEnd => {
                matches!(element, UiElement::Page { .. } | UiElement::Action { .. })
            }
            UiLocation::Settings => matches!(element, UiElement::Section { .. }),
            UiLocation::RunsFilter => matches!(element, UiElement::ListFilter { .. }),
            UiLocation::RunDetail => matches!(element, UiElement::Tab { .. }),
            UiLocation::Hosted(_) => true,
            _ if self.is_action_location() => matches!(element, UiElement::Action { .. }),
            _ => false,
        }
    }

    /// Whether this location is one of the Action locations (amendment #30): a
    /// bar or a menu that accepts only actions.
    pub fn is_action_location(self) -> bool {
        matches!(
            self,
            UiLocation::RunsHeader
                | UiLocation::RunItemMenu
                | UiLocation::RunsBulk
                | UiLocation::RunToolbar
                | UiLocation::CanvasNodeMenu
                | UiLocation::CanvasEdgeMenu
                | UiLocation::NodeToolbar
                | UiLocation::TriggerItemMenu
                | UiLocation::PipelineItemMenu
                | UiLocation::PipelinesHeader
                | UiLocation::TabMenu
                | UiLocation::ReviewToolbar
                | UiLocation::ReviewFile
                | UiLocation::SkillMenu
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_location_serializes_as_its_id() {
        for location in UiLocation::ALL {
            assert_eq!(
                serde_json::to_value(location).unwrap(),
                serde_json::Value::String(location.id().to_string())
            );
        }
    }

    #[test]
    fn a_location_accepts_only_its_kinds_of_element() {
        let page = UiElement::Page {
            component: "P".into(),
            icon: None,
        };
        let action = UiElement::Action {
            command: "c".into(),
        };
        assert!(UiLocation::TopbarEnd.accepts(&page));
        assert!(UiLocation::TopbarEnd.accepts(&action));
        assert!(UiLocation::TopbarStart.accepts(&page));
        assert!(!UiLocation::TopbarStart.accepts(&action));
        assert!(!UiLocation::Settings.accepts(&page));
        assert!(!UiLocation::RunDetail.accepts(&action));
    }

    #[test]
    fn the_action_locations_accept_only_actions() {
        let page = UiElement::Page {
            component: "P".into(),
            icon: None,
        };
        let tab = UiElement::Tab {
            component: "T".into(),
        };
        let action = UiElement::Action {
            command: "c".into(),
        };
        let actions: Vec<_> = UiLocation::ALL
            .iter()
            .filter(|l| l.is_action_location())
            .map(|l| l.id())
            .collect();
        assert_eq!(
            actions,
            [
                "runs/header",
                "run/item/menu",
                "runs/bulk",
                "run/toolbar",
                "canvas/node/menu",
                "canvas/edge/menu",
                "node/toolbar",
                "trigger/item/menu",
                "pipeline/item/menu",
                "pipelines/header",
                "tab/menu",
                "review/toolbar",
                "review/file",
                "skill/menu",
            ]
        );
        for location in UiLocation::ALL.iter().filter(|l| l.is_action_location()) {
            assert!(location.accepts(&action), "{}", location.id());
            assert!(!location.accepts(&page), "{}", location.id());
            assert!(!location.accepts(&tab), "{}", location.id());
        }
    }

    #[test]
    fn a_hosted_location_serializes_as_its_id() {
        let hosted = UiLocation::Hosted("pm/onglet");
        assert_eq!(hosted.id(), "pm/onglet");
        assert_eq!(serde_json::to_value(hosted).unwrap(), "pm/onglet");
        assert!(!UiLocation::ALL.contains(&hosted));
    }
}
