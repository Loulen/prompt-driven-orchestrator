//! What each location of the catalogue renders: the kinds of
//! [`UiElement`] it accepts, and whether it is an Action location.

use super::{UiElement, UiLocation};

impl UiLocation {
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
                matches!(
                    element,
                    UiElement::Page { .. } | UiElement::Panel { .. } | UiElement::Action { .. }
                )
            }
            UiLocation::ReviewFile => {
                matches!(
                    element,
                    UiElement::Decoration { .. } | UiElement::Action { .. }
                )
            }
            UiLocation::ReviewComposer => matches!(element, UiElement::Completion { .. }),
            UiLocation::Palette => {
                matches!(element, UiElement::Action { .. } | UiElement::Search { .. })
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
            | UiLocation::SkillDetail
            | UiLocation::SessionDetail
            | UiLocation::StatsTab => matches!(element, UiElement::Tab { .. }),
            UiLocation::RunItem
            | UiLocation::RunItemPeople
            | UiLocation::TriggerItem
            | UiLocation::PipelineItem
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
    /// `review/file`, which also takes Decorations (amendment #31), and
    /// `palette`, which also takes Sources de recherche (0.18).
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
                | UiLocation::Palette
        )
    }
}
