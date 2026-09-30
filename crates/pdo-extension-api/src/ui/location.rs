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
    }

    #[test]
    fn a_hosted_location_serializes_as_its_id() {
        let hosted = UiLocation::Hosted("pm/onglet");
        assert_eq!(hosted.id(), "pm/onglet");
        assert_eq!(serde_json::to_value(hosted).unwrap(), "pm/onglet");
        assert!(!UiLocation::ALL.contains(&hosted));
    }
}
