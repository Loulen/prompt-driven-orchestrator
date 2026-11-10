//! The tmux adapter: the one implementation of the [`Tmux`] port, pinned to
//! the daemon's private socket. Nothing else in PDO shells out to `tmux`.

mod clients;
mod launch_file;
mod lifecycle;
mod text;

use std::collections::HashSet;
use std::path::Path;

use pdo_harness::adapters::binary_path::harness_probe_path;

use crate::application::ports::Tmux;
use crate::domain::names::tmux_socket_name;

/// Build a `Command` for `tmux -L <socket>`. Use this everywhere we shell
/// out — never `Command::new("tmux")` directly.
fn tmux(socket: &str) -> std::process::Command {
    let mut c = std::process::Command::new("tmux");
    c.args(["-L", socket]);
    c
}

/// The `tmux` binary on the daemon's private socket (`tmux -L pdo-<port>`).
#[derive(Debug, Clone)]
pub struct TmuxCli {
    socket: String,
}

impl TmuxCli {
    /// The socket of the daemon listening on `daemon_port`.
    pub fn for_daemon(daemon_port: u16) -> Self {
        Self::on_socket(tmux_socket_name(daemon_port))
    }

    /// An explicit socket name.
    pub fn on_socket(socket: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
        }
    }
}

impl Tmux for TmuxCli {
    fn socket(&self) -> &str {
        &self.socket
    }

    fn session_path(&self) -> String {
        // Cached `OnceLock`: a cheap clone after the first probe.
        harness_probe_path()
    }

    fn new_session(
        &self,
        name: &str,
        working_dir: &Path,
        script: &str,
        what: Option<&str>,
    ) -> anyhow::Result<()> {
        lifecycle::new_session(&self.socket, name, working_dir, script, what)
    }

    fn kill(&self, name: &str) {
        lifecycle::kill(&self.socket, name)
    }

    fn session_exists(&self, name: &str) -> bool {
        lifecycle::session_exists(&self.socket, name)
    }

    fn session_closed_at(&self, name: &str) -> Option<std::time::SystemTime> {
        lifecycle::session_closed_at(&self.socket, name)
    }

    fn server_alive(&self) -> Option<bool> {
        lifecycle::server_alive(&self.socket)
    }

    fn list_pdo_sessions(&self) -> HashSet<String> {
        lifecycle::list_pdo_sessions(&self.socket)
    }

    fn is_attached(&self, name: &str) -> bool {
        lifecycle::is_attached(&self.socket, name)
    }

    fn capture(&self, name: &str) -> Option<String> {
        lifecycle::capture(&self.socket, name)
    }

    fn send_keys(&self, name: &str, text: &str) {
        text::send_keys(&self.socket, name, text)
    }

    fn paste(&self, name: &str, text: &str) {
        text::paste_text(&self.socket, name, text)
    }

    fn paste_without_enter(&self, name: &str, text: &str) -> bool {
        text::paste_without_enter(&self.socket, name, text)
    }

    fn press_enter(&self, name: &str) -> bool {
        text::press_enter(&self.socket, name)
    }

    fn attach_argv(&self, name: &str, ignore_size: bool) -> Vec<String> {
        clients::attach_argv(&self.socket, name, ignore_size)
    }

    fn attach_command(&self, name: &str) -> String {
        clients::attach_command(&self.socket, name)
    }

    fn set_client_ignore_size(&self, client_pid: u32, ignore_size: bool) -> bool {
        clients::set_ignore_size(&self.socket, client_pid, ignore_size)
    }
}
