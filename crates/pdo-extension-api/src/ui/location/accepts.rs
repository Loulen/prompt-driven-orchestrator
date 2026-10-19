//! What each location of the catalogue accepts: the kinds of element it
//! renders, and which locations are the Action locations.

use super::super::UiElement;
use super::UiLocation;

impl UiLocation {
    /// Whether this location renders `element`. The core leaves out, with a
    /// warning in its log, a contribution a location does not accept. A
    /// hosted location accepts the kind its host declares
    /// ([`HostedLocation::accepts`](crate::ui::HostedLocation::accepts)): the core
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
