//! Stage 3 (ADR-0080): what the tmux adapter hands the `tmux` binary, observed
//! through a fake `tmux` on `PATH`. Its own test binary: it mutates the
//! process-global `PATH`.

use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::application::messages::{paste_message, send_message};

/// #792: every daemon message to a harness session starts with
/// `[pdo-runtime]`, whether one line (`send_message`) or several
/// (`paste_message`); both are pasted, then submitted by an Enter of their own. A fake `tmux` on `PATH` logs the argv it receives
/// and the stdin it is fed, and the assertion reads the text as the session
/// would.
#[test]
fn daemon_messages_reach_the_session_with_the_runtime_prefix_on_both_paths() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let script = format!(
        "#!/usr/bin/env bash\nprintf 'ARGS:' >> \"{log}\"\nfor a in \"$@\"; do printf ' [%s]' \"$a\" >> \"{log}\"; done\nprintf '\\n' >> \"{log}\"\nif [ \"$3\" = load-buffer ]; then printf 'STDIN:' >> \"{log}\"; cat >> \"{log}\"; printf '\\n' >> \"{log}\"; fi\n",
        log = log.display()
    );
    let fake = dir.path().join("tmux");
    std::fs::write(&fake, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let previous = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![dir.path().to_path_buf()];
    paths.extend(std::env::split_paths(&previous));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

    let tmux = TmuxCli::on_socket("pdo-test");
    send_message(
        &tmux,
        "pdo-r1-solo-iter-1",
        "your output is missing frontmatter",
    );
    paste_message(
        &tmux,
        "pdo-mgr-r1",
        "Review batch\n- comment one\n- comment two",
    );

    std::env::set_var("PATH", previous);
    let calls = std::fs::read_to_string(&log).unwrap();

    // FP of CORE-227 (B1): typed with its Enter in one `send-keys`, the line
    // arrives as a burst that Claude Code takes for a paste, so the Enter turns
    // into a newline and the line sits unsent in the agent's input. The
    // one-line message is pasted too, and submitted by an Enter of its own.
    assert!(
        calls.contains("STDIN:[pdo-runtime] your output is missing frontmatter\n"),
        "one-line path: {calls}"
    );
    assert!(
        calls.contains("[send-keys] [-t] [pdo-r1-solo-iter-1] [Enter]"),
        "one-line path, submitted apart: {calls}"
    );
    assert!(
        !calls.contains("[send-keys] [-t] [pdo-r1-solo-iter-1] [[pdo-runtime]"),
        "never typed with its Enter: {calls}"
    );
    assert!(
        calls.contains("STDIN:[pdo-runtime] Review batch\n- comment one\n- comment two\n"),
        "pasted path: {calls}"
    );
    // The paste is submitted by a bare Enter, which is a keystroke, not a message.
    assert!(
        calls.contains("[send-keys] [-t] [pdo-mgr-r1] [Enter]"),
        "{calls}"
    );
    // Every call is pinned to the daemon's private socket.
    assert!(
        calls.lines().all(|l| l.starts_with("ARGS: [-L] [pdo-test]")
            || l.starts_with("STDIN:")
            || !l.starts_with("ARGS:")),
        "{calls}"
    );
}
