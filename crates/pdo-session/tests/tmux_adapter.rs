//! Stage 3 (ADR-0080): the tmux adapter against a real tmux server, on a
//! private socket of its own.

use std::path::Path;
use std::time::{Duration, Instant};

use pdo_session::adapters::tmux::TmuxCli;
use pdo_session::application::ports::Tmux;

/// A private socket, killed with its server when the test ends.
struct Server(TmuxCli);

impl Server {
    fn new(tag: &str) -> Self {
        Self(TmuxCli::on_socket(format!(
            "pdo-session-it-{tag}-{}",
            std::process::id()
        )))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", self.0.socket(), "kill-server"])
            .output();
    }
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_session_lives_is_listed_captured_and_killed_on_its_own_socket() {
    let server = Server::new("life");
    let tmux = &server.0;
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(tmux.server_alive(), Some(false), "no server yet");
    tmux.new_session("pdo-it-one", dir.path(), "sleep 30", None)
        .unwrap();
    assert!(tmux.session_exists("pdo-it-one"));
    assert_eq!(tmux.server_alive(), Some(true));
    assert!(tmux.list_pdo_sessions().contains("pdo-it-one"));
    assert!(!tmux.is_attached("pdo-it-one"), "nobody attached");
    assert!(tmux.capture("pdo-it-one").is_some());

    let err = tmux
        .new_session("pdo-it-one", dir.path(), "sleep 30", Some("shell"))
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("tmux new-session (shell) failed:") && err.contains("duplicate session"),
        "{err}"
    );

    tmux.kill("pdo-it-one");
    assert!(!tmux.session_exists("pdo-it-one"));
    assert_eq!(tmux.capture("pdo-it-one"), None);
    tmux.kill("pdo-it-one");
}

#[test]
fn keystrokes_and_a_paste_reach_the_pane() {
    let server = Server::new("text");
    let tmux = &server.0;
    let dir = tempfile::tempdir().unwrap();
    tmux.new_session("pdo-it-text", dir.path(), "cat", None)
        .unwrap();
    tmux.send_keys("pdo-it-text", "typed line");
    tmux.paste("pdo-it-text", "pasted one\npasted two");
    assert!(
        wait_until(|| {
            let pane = tmux.capture("pdo-it-text").unwrap_or_default();
            pane.contains("typed line") && pane.contains("pasted two")
        }),
        "{:?}",
        tmux.capture("pdo-it-text")
    );
}

#[test]
fn another_socket_sees_nothing_of_this_one() {
    let a = Server::new("a");
    let b = Server::new("b");
    a.0.new_session("pdo-it-mine", Path::new("/"), "sleep 30", None)
        .unwrap();
    assert!(a.0.session_exists("pdo-it-mine"));
    assert!(!b.0.session_exists("pdo-it-mine"));
    assert!(b.0.list_pdo_sessions().is_empty());
}

#[test]
fn a_client_that_is_not_attached_cannot_have_its_size_flag_flipped() {
    let server = Server::new("clients");
    let tmux = &server.0;
    tmux.new_session("pdo-it-clients", Path::new("/"), "sleep 30", None)
        .unwrap();
    assert!(!tmux.set_client_ignore_size(u32::MAX, true));
    assert_eq!(
        tmux.attach_argv("pdo-it-clients", true),
        [
            "tmux",
            "-L",
            tmux.socket(),
            "attach",
            "-t",
            "pdo-it-clients",
            "-f",
            "ignore-size"
        ]
    );
}
