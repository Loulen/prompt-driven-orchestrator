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
/// Non-exhaustive: adding a location is a minor evolution of the API.
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
    /// A tab of the detail of a node of a Run (its I/O pane): a
    /// [`UiElement::Tab`] per contribution, after I/O and Orchestration.
    #[serde(rename = "node/detail")]
    NodeDetail,
    /// A tab of the detail of a Trigger: a [`UiElement::Tab`] per contribution,
    /// after the core's Details.
    #[serde(rename = "trigger/detail")]
    TriggerDetail,
    /// A tab of the detail of a skill of the bank: a [`UiElement::Tab`] per
    /// contribution, after SKILL.md and Files.
    #[serde(rename = "skill/detail")]
    SkillDetail,
    /// A section of the Projet editor, under the core's fields, at creation as
    /// at edition: a [`UiElement::Section`] per contribution, taking part in the
    /// editor's Save and Cancel.
    #[serde(rename = "project/edit")]
    ProjectEdit,
    /// A section of the New Run form (a Run or a Trigger), after the core's
    /// fields: a [`UiElement::Section`] per contribution, taking part in the
    /// form's Launch (or Create) and Cancel.
    #[serde(rename = "run/new")]
    RunNew,
    /// A section of the inspector of a node of a pipeline, after Outputs: a
    /// [`UiElement::Section`] per contribution.
    #[serde(rename = "node/config")]
    NodeConfig,
    /// A section of the inspector of a pipeline (nothing selected on the
    /// canvas), after Stats: a [`UiElement::Section`] per contribution.
    #[serde(rename = "pipeline/config")]
    PipelineConfig,
    /// A section of the Info tab of a Run, after its stats: a
    /// [`UiElement::Section`] per contribution.
    #[serde(rename = "run/info")]
    RunInfo,
    /// A mark on a row of the Runs list, in its right-hand column, on the line
    /// of the child counters: a [`UiElement::Decoration`] per contribution.
    #[serde(rename = "run/item")]
    RunItem,
    /// A mark on a row of the Triggers list, on its second line: a
    /// [`UiElement::Decoration`] per contribution.
    #[serde(rename = "trigger/item")]
    TriggerItem,
    /// A mark on a row of the Pipelines list, on its second line: a
    /// [`UiElement::Decoration`] per contribution.
    #[serde(rename = "pipeline/item")]
    PipelineItem,
    /// A mark on a node card of the canvas, on a line under its title: a
    /// [`UiElement::Decoration`] per contribution.
    #[serde(rename = "canvas/node")]
    CanvasNode,
    /// A mark on a port row of a node's detail (Inputs and Outputs), next to
    /// the port's name: a [`UiElement::Decoration`] per contribution.
    #[serde(rename = "port/item")]
    PortItem,
    /// A mark on the header of a file of the Review page, before its comment
    /// counts: a [`UiElement::Decoration`] per contribution.
    #[serde(rename = "review/file")]
    ReviewFile,
    /// A mark in the status bar, before the service health: a
    /// [`UiElement::Decoration`] per contribution.
    #[serde(rename = "statusbar")]
    Statusbar,
    /// Above the list of the Triggers: a [`UiElement::ListFilter`] per
    /// contribution. The 0.1 `ListFilter { resource: Trigger }` slot.
    #[serde(rename = "triggers/filter")]
    TriggersFilter,
    /// Above the list of the Pipelines: a [`UiElement::ListFilter`] per
    /// contribution. The 0.1 `ListFilter { resource: Pipeline }` slot.
    #[serde(rename = "pipelines/filter")]
    PipelinesFilter,
    /// Above the Runs and Triggers lists, which group by Projet: a
    /// [`UiElement::ListFilter`] per contribution, keeping the groups of the
    /// Projets that match. The 0.1 `ListFilter { resource: Project }` slot.
    #[serde(rename = "projects/filter")]
    ProjectsFilter,
}

impl UiLocation {
    /// Every location of this version of the API, in the catalogue's order.
    pub const ALL: &'static [UiLocation] = &[
        UiLocation::TopbarStart,
        UiLocation::TopbarEnd,
        UiLocation::Settings,
        UiLocation::RunsFilter,
        UiLocation::RunDetail,
        UiLocation::NodeDetail,
        UiLocation::TriggerDetail,
        UiLocation::SkillDetail,
        UiLocation::ProjectEdit,
        UiLocation::RunNew,
        UiLocation::NodeConfig,
        UiLocation::PipelineConfig,
        UiLocation::RunInfo,
        UiLocation::RunItem,
        UiLocation::TriggerItem,
        UiLocation::PipelineItem,
        UiLocation::CanvasNode,
        UiLocation::PortItem,
        UiLocation::ReviewFile,
        UiLocation::Statusbar,
        UiLocation::TriggersFilter,
        UiLocation::PipelinesFilter,
        UiLocation::ProjectsFilter,
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
            UiLocation::NodeDetail => "node/detail",
            UiLocation::TriggerDetail => "trigger/detail",
            UiLocation::SkillDetail => "skill/detail",
            UiLocation::ProjectEdit => "project/edit",
            UiLocation::RunNew => "run/new",
            UiLocation::NodeConfig => "node/config",
            UiLocation::PipelineConfig => "pipeline/config",
            UiLocation::RunInfo => "run/info",
            UiLocation::RunItem => "run/item",
            UiLocation::TriggerItem => "trigger/item",
            UiLocation::PipelineItem => "pipeline/item",
            UiLocation::CanvasNode => "canvas/node",
            UiLocation::PortItem => "port/item",
            UiLocation::ReviewFile => "review/file",
            UiLocation::Statusbar => "statusbar",
            UiLocation::TriggersFilter => "triggers/filter",
            UiLocation::PipelinesFilter => "pipelines/filter",
            UiLocation::ProjectsFilter => "projects/filter",
        }
    }

    /// Whether this location renders `element`. The core leaves out, with a
    /// warning in its log, a contribution a location does not accept.
    pub fn accepts(self, element: &UiElement) -> bool {
        match self {
            UiLocation::TopbarStart => matches!(element, UiElement::Page { .. }),
            UiLocation::TopbarEnd => {
                matches!(element, UiElement::Page { .. } | UiElement::Action { .. })
            }
            UiLocation::Settings
            | UiLocation::ProjectEdit
            | UiLocation::RunNew
            | UiLocation::NodeConfig
            | UiLocation::PipelineConfig
            | UiLocation::RunInfo => matches!(element, UiElement::Section { .. }),
            UiLocation::RunsFilter
            | UiLocation::TriggersFilter
            | UiLocation::PipelinesFilter
            | UiLocation::ProjectsFilter => matches!(element, UiElement::ListFilter { .. }),
            UiLocation::RunDetail
            | UiLocation::NodeDetail
            | UiLocation::TriggerDetail
            | UiLocation::SkillDetail => matches!(element, UiElement::Tab { .. }),
            UiLocation::RunItem
            | UiLocation::TriggerItem
            | UiLocation::PipelineItem
            | UiLocation::CanvasNode
            | UiLocation::PortItem
            | UiLocation::ReviewFile
            | UiLocation::Statusbar => matches!(element, UiElement::Decoration { .. }),
        }
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
        let section = UiElement::Section {
            component: "S".into(),
        };
        let decoration = UiElement::Decoration {
            component: "D".into(),
        };
        let tab = UiElement::Tab {
            component: "T".into(),
        };
        let filter = UiElement::ListFilter {
            component: "F".into(),
        };
        assert!(UiLocation::ProjectEdit.accepts(&section));
        assert!(!UiLocation::ProjectEdit.accepts(&tab));
        assert!(UiLocation::RunItem.accepts(&decoration));
        assert!(UiLocation::Statusbar.accepts(&decoration));
        assert!(!UiLocation::RunItem.accepts(&section));
        assert!(UiLocation::SkillDetail.accepts(&tab));
        assert!(UiLocation::ProjectsFilter.accepts(&filter));
        assert!(!UiLocation::TriggersFilter.accepts(&decoration));
    }

    #[test]
    fn every_location_accepts_one_kind_of_element_at_least() {
        let kinds = [
            UiElement::Page {
                component: "P".into(),
                icon: None,
            },
            UiElement::Action {
                command: "c".into(),
            },
            UiElement::Tab {
                component: "T".into(),
            },
            UiElement::Section {
                component: "S".into(),
            },
            UiElement::Decoration {
                component: "D".into(),
            },
            UiElement::ListFilter {
                component: "F".into(),
            },
        ];
        for location in UiLocation::ALL {
            assert!(
                kinds.iter().any(|k| location.accepts(k)),
                "{}",
                location.id()
            );
        }
    }
}
