//! The presence of the participants of a draft: who is there, what each one
//! selected on the canvas, where each one types. Nothing of it is saved: it
//! ends with the connection.

use pdo_extension_api::ActorDisplay;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest id a selection or a typing position may name.
const MAX_ID: usize = 200;

/// A participant of a draft, as every other one sees it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DraftParticipant {
    /// Its connection: one person in two windows is two participants.
    pub connection: String,
    pub actor: Actor,
    /// Its name and avatar, when an extension names the Actor.
    pub display: Option<ActorDisplay>,
    /// Whether it may change the draft; otherwise it only reads.
    pub can_write: bool,
    /// The library assistant of `actor`, seated by its saves.
    pub assistant: bool,
    #[serde(flatten)]
    pub presence: PresenceState,
}

/// What a participant says of itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresenceState {
    /// What it selected on the canvas.
    #[serde(default)]
    pub selection: Option<PresenceSelection>,
    /// Where it types.
    #[serde(default)]
    pub typing: Option<PresenceTyping>,
}

/// A canvas element: a node, an edge (by index), a note or a loop region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresenceSelection {
    pub kind: SelectionKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SelectionKind {
    Node,
    Edge,
    Note,
    Region,
}

/// A text field of a node someone types in, and the caret's position in it
/// (UTF-16 code units).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresenceTyping {
    pub node_id: String,
    pub field: TypingField,
    pub caret: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TypingField {
    Prompt,
}

impl PresenceState {
    /// The state as kept: an id too long to be one is dropped.
    pub fn bounded(self) -> Self {
        Self {
            selection: self.selection.filter(|s| s.id.len() <= MAX_ID),
            typing: self.typing.filter(|t| t.node_id.len() <= MAX_ID),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_reads_with_its_missing_fields_empty() {
        let state: PresenceState =
            serde_json::from_str(r#"{"selection":{"kind":"node","id":"a"}}"#).unwrap();
        assert_eq!(
            state,
            PresenceState {
                selection: Some(PresenceSelection {
                    kind: SelectionKind::Node,
                    id: "a".into()
                }),
                typing: None,
            }
        );
    }

    #[test]
    fn an_overlong_id_is_dropped() {
        let state = PresenceState {
            selection: Some(PresenceSelection {
                kind: SelectionKind::Node,
                id: "x".repeat(300),
            }),
            typing: Some(PresenceTyping {
                node_id: "a".into(),
                field: TypingField::Prompt,
                caret: 3,
            }),
        };
        let bounded = state.bounded();
        assert_eq!(bounded.selection, None);
        assert!(bounded.typing.is_some());
    }
}
