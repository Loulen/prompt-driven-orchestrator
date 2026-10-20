//! The library pipeline authoring assistant (ADR-0048, ADR-0051): **one**
//! assistant per Actor (CORE-61), opened on demand, reaped when its Actor
//! leaves every edit view, and told which template is open through its
//! Actor's focus.

mod focus;
mod lifecycle;
mod save;

use std::collections::HashMap;
use std::sync::Mutex;

pub use focus::DeclareFocus;
pub use lifecycle::{AssistantLaunch, CloseAssistant, OpenAssistant, OpenAssistantError, Opened};
pub use save::{Read, SaveFocused, SaveFocusedError, Saved};

use crate::domain::libassist::LibassistFocus;

/// The focus of each assistant session, in memory only (see
/// [`LibassistFocus`]), by session name: each Actor's assistant follows what
/// its own Actor has open. Lock-poisoning-tolerant: a panic while holding the
/// lock must not make an assistant permanently un-reapable.
#[derive(Debug, Default)]
pub struct FocusCell(Mutex<HashMap<String, LibassistFocus>>);

impl FocusCell {
    /// The focus of the assistant session `session`.
    pub fn get(&self, session: &str) -> Option<LibassistFocus> {
        self.lock().get(session).cloned()
    }

    /// Declare (or clear, `None`) the focus of the assistant session `session`.
    pub fn set(&self, session: &str, focus: Option<LibassistFocus>) {
        let mut guard = self.lock();
        match focus {
            Some(focus) => {
                guard.insert(session.to_string(), focus);
            }
            None => {
                guard.remove(session);
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, LibassistFocus>> {
        match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
}
