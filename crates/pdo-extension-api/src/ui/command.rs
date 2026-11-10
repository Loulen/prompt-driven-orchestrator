use serde::Serialize;

use crate::{ResourceKind, Verb};

/// A command an extension's bundle exports (a Commande d'extension, CONTEXT.md
/// § *Extensions*): what an [`Action`](super::UiElement::Action) calls.
///
/// The bundle exports a function under [`UiCommand::export`]; the core calls it
/// as `fn(context, host)`, where `context` is the one of the location the
/// action sits in (at least the selected Run and its Projet) and `host` lets it
/// answer the operator (`host.notify(message)`), open a deep link
/// (`host.open("#/ext/…")`) or open New Run prefilled (0.22,
/// `host.openNewRun(prefill)`). It may return a promise; a throw or a
/// rejection reaches the operator in an error toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct UiCommand {
    /// Unique among the extension's commands; an action names it. A dotted
    /// name prefixed with the extension's id reads best (`test.new-note`).
    pub id: String,
    /// What the operator reads on the entry that calls it.
    pub title: String,
    /// An SVG the extension serves, as for a page's icon; the title's first
    /// letter, boxed, without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<String>,
    /// The name the bundle exports the function under.
    pub export: String,
    /// The Operation the command makes (0.18): before rendering an action
    /// that calls it, the core asks the Autorisation d'opération about it, on
    /// the entity of the action's location when it hands one over, and leaves
    /// the action out when it is refused. Without one, the action always
    /// shows; the extension's route still refuses what it must.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub operation: Option<CommandOperation>,
}

/// The Operation a [`UiCommand`] makes, without its target: the core aims it
/// at the entity the action acts on (`entity.id` of its location's context),
/// and places it in the Projets as the question of authorization does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct CommandOperation {
    /// `run`, `project`…, or `<extension>/<kind>`.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub resource: ResourceKind,
    /// `read`, `create`, `update`, `delete`, or `{ "action": <name> }`.
    #[cfg_attr(
        feature = "ts",
        ts(type = "\"read\" | \"create\" | \"update\" | \"delete\" | { action: string }")
    )]
    pub verb: Verb,
}

impl UiCommand {
    pub fn new(id: impl Into<String>, title: impl Into<String>, export: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            export: export.into(),
            operation: None,
        }
    }

    /// The same command, with the SVG served at `url` as its icon.
    pub fn with_icon(mut self, url: impl Into<String>) -> Self {
        self.icon = Some(url.into());
        self
    }

    /// The same command, making the Operation `verb` on `resource`: an action
    /// calling it shows only to whom the Autorisation d'opération allows it.
    pub fn guarded_by(mut self, resource: ResourceKind, verb: Verb) -> Self {
        self.operation = Some(CommandOperation { resource, verb });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_command_declares_the_operation_it_makes() {
        let bare = UiCommand::new("test.greet", "Greet", "greet");
        assert!(serde_json::to_value(&bare)
            .unwrap()
            .get("operation")
            .is_none());
        let guarded = bare.guarded_by(
            ResourceKind::extension("test", "note"),
            Verb::Action("archive".into()),
        );
        assert_eq!(
            serde_json::to_value(&guarded).unwrap(),
            json!({
                "id": "test.greet", "title": "Greet", "export": "greet",
                "operation": { "resource": "test/note", "verb": { "action": "archive" } },
            })
        );
        let read = UiCommand::new("c", "C", "c").guarded_by(ResourceKind::Run, Verb::Read);
        assert_eq!(
            serde_json::to_value(&read).unwrap()["operation"],
            json!({ "resource": "run", "verb": "read" })
        );
    }
}
