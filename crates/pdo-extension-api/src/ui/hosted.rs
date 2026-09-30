use serde::Serialize;

use super::{UiElementKind, UiLocation};

/// A location an extension hosts in its own interface (an Emplacement hébergé,
/// CONTEXT.md § *Extensions*; ADR-0081, amendment #32): the other extensions
/// contribute to it as to a location of the core, and the host renders their
/// elements through the shared module `@pdo/extension-ui`, with the core's
/// error boundaries and `when` clauses.
///
/// The id is `<extension>/<name>`, prefixed with the host's own id (`pm/onglet`).
/// Like a location of the core, a hosted location is a list, and its id, what
/// it accepts and the context it hands over are part of the host's API.
///
/// The daemon leaves out, with a warning in its log, a declaration whose id
/// does not start with the host's id and `/`, whose name is empty or holds
/// something else than lowercase ASCII letters, digits and `-`, that takes the
/// id of a location of the core or one the host already declared, or that
/// accepts pages (a page belongs to the core's shell).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct HostedLocation {
    /// `<extension>/<name>`: what a contribution names in
    /// [`UiLocation::Hosted`], and the bundle in `useHostedContributions`.
    pub id: &'static str,
    /// The one kind of element it renders.
    pub accepts: UiElementKind,
    /// Where it sits and how it renders its elements, for the authors of the
    /// extensions that contribute to it; empty when undescribed.
    pub description: String,
    /// What it hands its elements, on top of every location's context (the
    /// selected Run, its Projet, `location`), in declaration order.
    pub context: Vec<ContextKey>,
}

/// One key of a [`HostedLocation`]'s context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct ContextKey {
    /// A top-level key of the context (`page`, `task`); a `when` clause reads
    /// what it holds by a dotted path (`task.status`).
    pub key: String,
    /// What it holds.
    pub description: String,
}

impl HostedLocation {
    /// The location `id` (`<extension>/<name>`), rendering elements of kind
    /// `accepts`.
    pub fn new(id: &'static str, accepts: UiElementKind) -> Self {
        Self {
            id,
            accepts,
            description: String::new(),
            context: Vec::new(),
        }
    }

    /// The same location, described for the authors who contribute to it.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// The same location, handing one more key of context to its elements.
    pub fn context_key(mut self, key: impl Into<String>, description: impl Into<String>) -> Self {
        self.context.push(ContextKey {
            key: key.into(),
            description: description.into(),
        });
        self
    }

    /// The location a contribution names to appear here.
    pub fn location(&self) -> UiLocation {
        UiLocation::Hosted(self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UiContribution;
    use serde_json::json;

    #[test]
    fn declares_its_id_kind_and_context_on_the_wire() {
        let tab = HostedLocation::new("pm/onglet", UiElementKind::Tab)
            .describe("A tab of a Page, after History.")
            .context_key("page", "The Page shown: { id, title }");
        assert_eq!(
            serde_json::to_value(&tab).unwrap(),
            json!({
                "id": "pm/onglet",
                "accepts": "tab",
                "description": "A tab of a Page, after History.",
                "context": [{ "key": "page", "description": "The Page shown: { id, title }" }],
            })
        );
        let bare = HostedLocation::new("pm/badge-carte", UiElementKind::Decoration);
        assert_eq!(
            serde_json::to_value(&bare).unwrap(),
            json!({
                "id": "pm/badge-carte", "accepts": "decoration", "description": "", "context": [],
            })
        );
    }

    #[test]
    fn a_contribution_names_it_through_its_location() {
        let tab = HostedLocation::new("pm/onglet", UiElementKind::Tab);
        let audit = UiContribution::tab(tab.location(), "audit", "Audit", "AuditTab");
        assert_eq!(audit.location, UiLocation::Hosted("pm/onglet"));
    }
}
