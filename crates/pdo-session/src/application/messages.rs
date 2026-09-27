//! Text from the daemon to a harness agent: the one place the
//! `[pdo-runtime]` prefix is posed (#792), so no caller (the frontmatter retry of
//! ADR-0035, the review batch of #750…) knows the marker and none can forget it.
//! The prefix is what lets the Steering counter tell PDO's turns from a human's,
//! and what tells the agent itself the line came from the runtime.

use pdo_harness::steering::runtime_message;

use crate::application::ports::Tmux;

/// Type a **one-line message** into a harness session and submit it.
pub fn send_message(tmux: &dyn Tmux, session: &str, text: &str) {
    tmux.send_keys(session, &runtime_message(text));
}

/// Paste a **multi-line message** into a harness session as one bracketed paste
/// and submit it — prefixed at the head of its first line, same rule as
/// [`send_message`].
pub fn paste_message(tmux: &dyn Tmux, session: &str, text: &str) {
    tmux.paste(session, &runtime_message(text));
}
