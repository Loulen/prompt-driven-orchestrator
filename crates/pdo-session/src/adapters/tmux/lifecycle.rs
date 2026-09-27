//! Session lifecycle and inventory: create, kill, probe, list, capture.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use super::tmux;

/// `new-session -d -s <name> -c <working_dir> <script>`, then mouse mode on so
/// that wheel events are forwarded as mouse-report escape sequences instead of
/// being silently dropped. `what` names the session kind in the error.
pub(super) fn new_session(
    socket: &str,
    name: &str,
    working_dir: &Path,
    script: &str,
    what: Option<&str>,
) -> Result<()> {
    let label = what.map(|w| format!(" ({w})")).unwrap_or_default();
    let output = tmux(socket)
        .args(["new-session", "-d", "-s", name, "-c"])
        .arg(working_dir)
        .arg(script)
        .output()
        .with_context(|| format!("failed to run tmux new-session{label}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session{label} failed: {stderr}");
    }

    let _ = tmux(socket)
        .args(["set-option", "-t", name, "mouse", "on"])
        .output();
    Ok(())
}

/// Capture the visible pane content (with ANSI escapes) for a session.
/// Returns `None` if the session doesn't exist or capture fails.
pub(super) fn capture(socket: &str, session_name: &str) -> Option<String> {
    let output = tmux(socket)
        .args(["capture-pane", "-pe", "-S", "-1000", "-t", session_name])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Kill a tmux session. Best-effort — does not fail if the session is absent.
pub(super) fn kill(socket: &str, session_name: &str) {
    let _ = tmux(socket)
        .args(["kill-session", "-t", session_name])
        .output();
}

/// Check whether a tmux session exists.
pub(super) fn session_exists(socket: &str, session_name: &str) -> bool {
    tmux(socket)
        .args(["has-session", "-t", session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether the tmux *server* for this socket is alive at all (#234).
///
/// `tmux -L <socket> ls` exits non-zero ("no server running on …") once the
/// socket's server is gone. This is the single most discriminating fact when a
/// node's session is found dead: a dead server means the whole socket collapsed
/// and *every* session under it died at once (e.g. an external `kill <pid>` of
/// the server process), not just this one node — see the session-death
/// diagnostics of the stale detector.
///
/// `Some(true)` = server alive, `Some(false)` = server gone, `None` = the
/// `tmux` probe itself could not be run (so absence is never read as a real
/// "server gone").
pub(super) fn server_alive(socket: &str) -> Option<bool> {
    tmux(socket)
        .args(["ls"])
        .output()
        .map(|o| o.status.success())
        .ok()
}

/// List all tmux sessions whose name starts with `pdo-`, on the given socket.
/// Returns a set of session names.
pub(super) fn list_pdo_sessions(socket: &str) -> HashSet<String> {
    let output = match tmux(socket).args(["ls", "-F", "#{session_name}"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return HashSet::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("pdo-"))
        .map(String::from)
        .collect()
}

/// Whether `session` currently has at least one attached tmux client (#594,
/// ADR-0051 §4).
///
/// The daemon's PTY bridge runs a **real `tmux attach`** per open terminal and
/// kills that client when the WebSocket closes, so `#{session_attached}` is an
/// honest server-side answer to "is a human looking at this right now?" — and it
/// falls back to `0` within ~250 ms of a reload, a crash, or a tab close, which
/// no client-side signal manages. It is the sweep's first `Keep` verdict for the
/// library assistant: never yank a terminal out from under someone.
///
/// A separate call rather than a new column on `list_pdo_sessions`: that
/// function's `#{session_name}`-only format has other callers, and only one
/// session in the inventory needs this fact. Any failure (no such session, tmux
/// gone, unparseable output) answers `false` — the conservative direction here,
/// since the two later verdicts still get their say.
pub(super) fn is_attached(socket: &str, session: &str) -> bool {
    let output = match tmux(socket)
        .args([
            "display-message",
            "-p",
            "-t",
            session,
            "#{session_attached}",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i64>()
        .map(|n| n > 0)
        .unwrap_or(false)
}
