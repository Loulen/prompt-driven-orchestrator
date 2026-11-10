//! Text from the daemon to a harness agent: the one place the
//! `[pdo-runtime]` prefix is posed (#792), so no caller (the frontmatter retry of
//! ADR-0035, the review batch of #750…) knows the marker and none can forget it.
//! The prefix is what lets the Steering counter tell PDO's turns from a human's,
//! and what tells the agent itself the line came from the runtime.

use pdo_harness::steering::runtime_message;

use crate::application::ports::Tmux;

/// Hand a **one-line message** to a harness session and submit it.
///
/// Pasted, never typed: `send-keys` with the text and its Enter in one call
/// delivers a burst that Claude Code takes for a paste, so the Enter becomes a
/// newline and the line sits unsent in the agent's input (FP of CORE-227: the
/// release nudge never woke the agent). A bracketed paste then an Enter of its
/// own is what submits, the path the operator's « write to node » already uses.
/// It differs from [`paste_message`] only by what its callers hand it.
pub fn send_message(tmux: &dyn Tmux, session: &str, text: &str) {
    paste_message(tmux, session, text);
}

/// Paste a **multi-line message** into a harness session as one bracketed paste
/// and submit it — prefixed at the head of its first line, same rule as
/// [`send_message`].
pub fn paste_message(tmux: &dyn Tmux, session: &str, text: &str) {
    tmux.paste(session, &runtime_message(text));
}
