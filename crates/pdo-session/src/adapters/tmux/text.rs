//! Text into a session: keystrokes, and a bracketed paste.

use std::sync::atomic::{AtomicU64, Ordering};

use super::tmux;

/// Send raw keystrokes to a tmux session. Best-effort — does not fail if the
/// session is absent.
///
/// **Keystrokes, not a message**: a shell command in a `pdo-shell` session, a
/// bare `Enter`. Text meant for a **harness agent** goes through
/// `application::messages`, the one place the `[pdo-runtime]` prefix is posed
/// (#792) — never through here.
pub(super) fn send_keys(socket: &str, session_name: &str, text: &str) {
    let _ = tmux(socket)
        .args(["send-keys", "-t", session_name, text, "Enter"])
        .output();
}

/// Paste `text` verbatim as one bracketed paste, then submit it with Enter.
///
/// `send_keys` types its text as keystrokes, so an embedded newline is a
/// keystroke too — for a chat harness that is a premature submit, and a
/// twenty-comment batch would arrive as twenty half-messages. `load-buffer`
/// from stdin + `paste-buffer -p` hands the whole text over as a paste the
/// harness keeps as one message. Best-effort — a missing session is not an
/// error here (the caller verified it exists). A daemon message to a harness
/// goes through `application::messages::paste_message`, which poses the
/// runtime prefix.
pub(super) fn paste_text(socket: &str, session_name: &str, text: &str) {
    if !paste_without_enter(socket, session_name, text) {
        return;
    }
    let _ = tmux(socket)
        .args(["send-keys", "-t", session_name, "Enter"])
        .output();
}

/// Paste `text` verbatim into a session's input as one bracketed paste and
/// **never** press Enter: the text waits in the agent's input for the user to
/// complete and submit it (« Copy & send to terminal »). `false` when tmux
/// refused the buffer or the paste.
///
/// Each call loads its own named buffer: two pastes sharing a name could swap
/// their texts between the load and the paste.
pub(super) fn paste_without_enter(socket: &str, session_name: &str, text: &str) -> bool {
    use std::io::Write;
    let buffer = format!(
        "pdo-paste-{}-{}",
        std::process::id(),
        PASTE_BUFFER_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let child = tmux(socket)
        .args(["load-buffer", "-b", &buffer, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    if !child.wait().map(|s| s.success()).unwrap_or(false) {
        return false;
    }
    tmux(socket)
        .args([
            "paste-buffer",
            "-p",
            "-d",
            "-b",
            &buffer,
            "-t",
            session_name,
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The sequence that names each paste buffer of this daemon process.
static PASTE_BUFFER_SEQ: AtomicU64 = AtomicU64::new(0);
