use serde::Serialize;

/// A command an extension's bundle exports (a Commande d'extension, CONTEXT.md
/// § *Extensions*): what an [`Action`](super::UiElement::Action) calls.
///
/// The bundle exports a function under [`UiCommand::export`]; the core calls it
/// as `fn(context, host)`, where `context` is the one of the location the
/// action sits in (at least the selected Run and its Projet) and `host` lets it
/// answer the operator (`host.notify(message)`) or open a deep link
/// (`host.open("#/ext/…")`). It may return a promise; a throw or a rejection
/// reaches the operator in an error toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct UiCommand {
    /// Unique among the extension's commands; an action names it. A dotted
    /// name prefixed with the extension's id reads best (`pm.new-task`).
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
}

impl UiCommand {
    pub fn new(id: impl Into<String>, title: impl Into<String>, export: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            export: export.into(),
        }
    }

    /// The same command, with the SVG served at `url` as its icon.
    pub fn with_icon(mut self, url: impl Into<String>) -> Self {
        self.icon = Some(url.into());
        self
    }
}
