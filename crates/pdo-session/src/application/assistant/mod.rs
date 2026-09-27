//! The library pipeline authoring assistant (ADR-0048, ADR-0051): **one**
//! assistant for the whole daemon, opened on demand, reaped when the user leaves
//! every edit view, and told which template is open through the focus.

mod focus;
mod lifecycle;
mod save;

use std::sync::Mutex;

pub use focus::DeclareFocus;
pub use lifecycle::{AssistantLaunch, CloseAssistant, OpenAssistant, OpenAssistantError, Opened};
pub use save::{SaveFocused, SaveFocusedError, Saved};

use crate::domain::libassist::LibassistFocus;
use crate::domain::names::LIBASSIST_SESSION_NAME;

/// The target of every Operation on the assistant: its one session.
fn session_target() -> String {
    LIBASSIST_SESSION_NAME.to_string()
}

/// The focus, in memory only (see [`LibassistFocus`]). Lock-poisoning-tolerant:
/// a panic while holding the lock must not make the assistant permanently
/// un-reapable.
#[derive(Debug, Default)]
pub struct FocusCell(Mutex<Option<LibassistFocus>>);

impl FocusCell {
    pub fn get(&self) -> Option<LibassistFocus> {
        match self.0.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    pub fn set(&self, focus: Option<LibassistFocus>) {
        let mut guard = match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = focus;
    }
}
