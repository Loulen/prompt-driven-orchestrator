//! The clients attached to a session: the argv the PTY bridge attaches with,
//! and the live `ignore-size` flag of a spectator's client (#867, ADR-0075).

use super::tmux;
use crate::domain::launch::sh_single_quote;

/// `tmux -L <socket> attach -t <name> [-f ignore-size]`.
pub(super) fn attach_argv(socket: &str, name: &str, ignore_size: bool) -> Vec<String> {
    let mut argv: Vec<String> = ["tmux", "-L", socket, "attach", "-t", name]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if ignore_size {
        argv.push("-f".to_string());
        argv.push("ignore-size".to_string());
    }
    argv
}

/// `tmux -L '<socket>' attach -t '<name>'`, quoted for a shell.
pub(super) fn attach_command(socket: &str, name: &str) -> String {
    format!(
        "tmux -L {} attach -t {}",
        sh_single_quote(socket),
        sh_single_quote(name)
    )
}

/// `refresh-client -f ignore-size | !ignore-size` on the client whose process is
/// `client_pid`. `false` when that client is not listed or tmux refused.
pub(super) fn set_ignore_size(socket: &str, client_pid: u32, flag: bool) -> bool {
    let Some(tty) = client_tty(socket, client_pid) else {
        return false;
    };
    let value = if flag { "ignore-size" } else { "!ignore-size" };
    tmux(socket)
        .args(["refresh-client", "-t", &tty, "-f", value])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The tty of the tmux client run by `client_pid`, as `refresh-client -t` wants it.
fn client_tty(socket: &str, client_pid: u32) -> Option<String> {
    let out = tmux(socket)
        .args(["list-clients", "-F", "#{client_pid} #{client_tty}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let pid = client_pid.to_string();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| {
            let (p, tty) = line.split_once(' ')?;
            (p == pid).then(|| tty.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spectator_attaches_ignoring_its_size() {
        assert_eq!(
            attach_argv("pdo-6172", "pdo-mgr-r1", false),
            ["tmux", "-L", "pdo-6172", "attach", "-t", "pdo-mgr-r1"]
        );
        assert_eq!(
            attach_argv("pdo-6172", "pdo-mgr-r1", true)[6..],
            ["-f", "ignore-size"]
        );
        assert_eq!(
            attach_command("pdo-6172", "pdo-it's"),
            "tmux -L 'pdo-6172' attach -t 'pdo-it'\\''s'"
        );
    }
}
