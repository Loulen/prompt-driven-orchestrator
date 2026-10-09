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
    /// The far right of the top bar, **after Settings**: the component of a
    /// [`UiElement::Decoration`] per contribution, rendered as is (an avatar
    /// opening the menu of the signed-in person, say). Since 0.8.
    #[serde(rename = "topbar/account")]
    TopbarAccount,
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
    /// A section of the Info tab of Pipeline info on a template, after Stats:
    /// a [`UiElement::Section`] per contribution.
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
    /// The people of a row of the Runs list, in its left-hand column after
    /// the pipeline's name: a [`UiElement::Decoration`] per contribution,
    /// handed the Actors who have the Run open (its Presences, live). The core
    /// draws no person there itself. Since 0.12.
    #[serde(rename = "run/item/people")]
    RunItemPeople,
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
    /// A mark in the status bar, before the service health: a
    /// [`UiElement::Decoration`] per contribution.
    #[serde(rename = "statusbar")]
    Statusbar,
    /// The empty state of the Runs list, shown while the list holds no Run at
    /// all, under the core's « No runs yet »: the component of a
    /// [`UiElement::Decoration`] per contribution, rendered as is, one under
    /// the other (an explanation of why the list is empty, say). Since 0.11.
    #[serde(rename = "runs/empty")]
    RunsEmpty,
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
    /// The header of a file of the Review page: an icon button per
    /// [`UiElement::Action`], before « Copy path », handing over the file as
    /// `entity` and the pair as `review`; and a mark per
    /// [`UiElement::Decoration`], before its comment counts (amendments #30
    /// and #31).
    #[serde(rename = "review/file")]
    ReviewFile,
    /// The comment editor of the Review page: a [`UiElement::Completion`] per
    /// contribution, a source of suggestions opened by its trigger character
    /// as the comment is typed. Hands over the Run under review and the
    /// compared pair as `review`. Since 0.14.
    #[serde(rename = "review/composer")]
    ReviewComposer,
    /// The « ⋮ » menu of a skill or a folder of the skill bank. Hands over the
    /// skill or the folder as `entity`.
    #[serde(rename = "skill/menu")]
    SkillMenu,
    /// A tab of the Stats surface: a [`UiElement::Tab`] per contribution, on
    /// the rail after the core's tabs (after Performance), rendered in the
    /// same frame (header, period, Refresh). Hands over the period on screen.
    /// Since 0.9.
    #[serde(rename = "stats/tab")]
    StatsTab,
    /// A page of the Settings surface: a [`UiElement::Page`] per
    /// contribution, a rail entry of its own labelled with the contribution's
    /// label, after the core's categories and the Extensions page (the
    /// `settings` location, whose sections it leaves untouched). Since 0.9.
    #[serde(rename = "settings/page")]
    SettingsPage,
    /// A location an extension hosts, by its id `<extension>/<name>`
    /// (`a/panneau`): what it accepts and hands over is the host's
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
        UiLocation::TopbarAccount,
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
        UiLocation::RunItemPeople,
        UiLocation::TriggerItem,
        UiLocation::PipelineItem,
        UiLocation::CanvasNode,
        UiLocation::PortItem,
        UiLocation::Statusbar,
        UiLocation::RunsEmpty,
        UiLocation::TriggersFilter,
        UiLocation::PipelinesFilter,
        UiLocation::ProjectsFilter,
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
        UiLocation::ReviewComposer,
        UiLocation::SkillMenu,
        UiLocation::StatsTab,
        UiLocation::SettingsPage,
    ];

    /// The stable id: what the wire, the deep links (`#/ext/<extension>/<id>/…`)
    /// and the reference page call it.
    pub fn id(self) -> &'static str {
        match self {
            UiLocation::TopbarStart => "topbar/start",
            UiLocation::TopbarEnd => "topbar/end",
            UiLocation::TopbarAccount => "topbar/account",
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
            UiLocation::RunItemPeople => "run/item/people",
            UiLocation::TriggerItem => "trigger/item",
            UiLocation::PipelineItem => "pipeline/item",
            UiLocation::CanvasNode => "canvas/node",
            UiLocation::PortItem => "port/item",
            UiLocation::Statusbar => "statusbar",
            UiLocation::RunsEmpty => "runs/empty",
            UiLocation::TriggersFilter => "triggers/filter",
            UiLocation::PipelinesFilter => "pipelines/filter",
            UiLocation::ProjectsFilter => "projects/filter",
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
            UiLocation::ReviewComposer => "review/composer",
            UiLocation::SkillMenu => "skill/menu",
            UiLocation::StatsTab => "stats/tab",
            UiLocation::SettingsPage => "settings/page",
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
            UiLocation::TopbarStart | UiLocation::SettingsPage => {
                matches!(element, UiElement::Page { .. })
            }
            UiLocation::TopbarEnd => {
                matches!(element, UiElement::Page { .. } | UiElement::Action { .. })
            }
            UiLocation::ReviewFile => {
                matches!(
                    element,
                    UiElement::Decoration { .. } | UiElement::Action { .. }
                )
            }
            UiLocation::ReviewComposer => matches!(element, UiElement::Completion { .. }),
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
            | UiLocation::SkillDetail
            | UiLocation::StatsTab => matches!(element, UiElement::Tab { .. }),
            UiLocation::RunItem
            | UiLocation::RunItemPeople
            | UiLocation::TriggerItem
            | UiLocation::PipelineItem
            | UiLocation::CanvasNode
            | UiLocation::PortItem
            | UiLocation::Statusbar
            | UiLocation::RunsEmpty
            | UiLocation::TopbarAccount => matches!(element, UiElement::Decoration { .. }),
            UiLocation::Hosted(_) => true,
            _ if self.is_action_location() => matches!(element, UiElement::Action { .. }),
            _ => false,
        }
    }

    /// Whether this location is one of the Action locations (amendment #30): a
    /// bar or a menu that accepts actions, and only actions except for
    /// `review/file`, which also takes Decorations (amendment #31).
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
        assert!(UiLocation::TopbarAccount.accepts(&decoration));
        assert!(!UiLocation::TopbarAccount.accepts(&action));
        assert!(!UiLocation::RunItem.accepts(&section));
        assert!(UiLocation::RunItemPeople.accepts(&decoration));
        assert!(!UiLocation::RunItemPeople.accepts(&action));
        assert!(UiLocation::SkillDetail.accepts(&tab));
        assert!(UiLocation::ProjectsFilter.accepts(&filter));
        assert!(!UiLocation::TriggersFilter.accepts(&decoration));
        assert!(UiLocation::ReviewFile.accepts(&decoration));
        assert!(UiLocation::ReviewFile.accepts(&action));
        assert!(!UiLocation::ReviewFile.accepts(&section));
        assert!(UiLocation::StatsTab.accepts(&tab));
        assert!(!UiLocation::StatsTab.accepts(&page));
        assert!(!UiLocation::StatsTab.accepts(&section));
        assert!(UiLocation::SettingsPage.accepts(&page));
        assert!(!UiLocation::SettingsPage.accepts(&section));
        assert!(!UiLocation::SettingsPage.accepts(&action));
    }

    #[test]
    fn the_empty_runs_list_takes_decorations_only() {
        assert_eq!(UiLocation::RunsEmpty.id(), "runs/empty");
        assert!(UiLocation::ALL.contains(&UiLocation::RunsEmpty));
        let decoration = UiElement::Decoration {
            component: "D".into(),
        };
        let section = UiElement::Section {
            component: "S".into(),
        };
        let action = UiElement::Action {
            command: "c".into(),
        };
        assert!(UiLocation::RunsEmpty.accepts(&decoration));
        assert!(!UiLocation::RunsEmpty.accepts(&section));
        assert!(!UiLocation::RunsEmpty.accepts(&action));
        assert!(!UiLocation::RunsEmpty.is_action_location());
    }

    #[test]
    fn the_review_composer_takes_completion_sources_only() {
        assert_eq!(UiLocation::ReviewComposer.id(), "review/composer");
        assert!(UiLocation::ALL.contains(&UiLocation::ReviewComposer));
        let source = UiElement::Completion {
            trigger: "#".into(),
            suggest: "s".into(),
            render: None,
        };
        let decoration = UiElement::Decoration {
            component: "D".into(),
        };
        assert!(UiLocation::ReviewComposer.accepts(&source));
        assert!(!UiLocation::ReviewComposer.accepts(&decoration));
        assert!(!UiLocation::ReviewFile.accepts(&source));
        assert!(!UiLocation::ReviewComposer.is_action_location());
    }

    #[test]
    fn stats_and_settings_take_a_tab_and_a_page() {
        assert_eq!(UiLocation::StatsTab.id(), "stats/tab");
        assert_eq!(UiLocation::SettingsPage.id(), "settings/page");
        assert!(UiLocation::ALL.contains(&UiLocation::StatsTab));
        assert!(UiLocation::ALL.contains(&UiLocation::SettingsPage));
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
            UiElement::Completion {
                trigger: "#".into(),
                suggest: "s".into(),
                render: None,
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
        let hosted = UiLocation::Hosted("a/panneau");
        assert_eq!(hosted.id(), "a/panneau");
        assert_eq!(serde_json::to_value(hosted).unwrap(), "a/panneau");
        assert!(!UiLocation::ALL.contains(&hosted));
    }
}
